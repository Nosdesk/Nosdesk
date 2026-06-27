/**
 * Refcounted Yjs collaboration session store.
 *
 * Owns the Y.Doc + WebsocketProvider for each open document
 * (`docId`) so the underlying state survives the
 * `CollaborativeEditor` component's mount/unmount cycle.
 *
 *   editor.onMounted  → session.acquire(docId)
 *   editor.onBeforeUnmount → session.release(docId)
 *
 * What this enables:
 *   - Navigating away from a ticket and back is instant: the
 *     ydoc is still in memory, awareness is still live, the
 *     websocket is still connected.
 *   - When refcount hits zero, the websocket disconnects after
 *     a 30s grace period (Linear-style), so a quick "oops"
 *     back-navigation re-uses the same connection.
 *   - When more than `MAX_SESSIONS` documents are held, the
 *     least-recently-released session is destroyed (Y.Doc +
 *     WebsocketProvider both, per y-websocket#142, to avoid
 *     awareness leaks).
 *
 * Y.Doc and WebsocketProvider instances are NEVER stored
 * inside Vue's reactive system, they are EventEmitters with
 * cyclic structure that Pinia/Vue would fail to track and
 * could grow unbounded if proxied. The Map of sessions lives
 * at module scope; only the user-facing "session count" / LRU
 * snapshot is exposed reactively.
 */
import * as Y from 'yjs'
import { WebsocketProvider } from 'y-websocket'
import { IndexeddbPersistence, clearDocument as clearIdbDocument } from 'y-indexeddb'
import { COLLAB_IDB_TOUCH_KEY } from '@/utils/collabLocalCache'
import { defineStore } from 'pinia'
import { ref } from 'vue'

import { logger } from '@nosdesk/core/utils/logger'
import { SafePermanentUserData } from '@nosdesk/core/utils/safePermanentUserData'
import { collabWsBaseUrl } from '@nosdesk/core/transport'

/**
 * How long after refcount hits 0 we keep the websocket open
 * before disconnecting. Tuned so a quick nav-away/nav-back
 * round-trip never re-handshakes; longer than that and the
 * session is released to LRU eviction.
 */
const GRACE_MS = 30_000

/**
 * Soft cap on simultaneously-cached sessions. When exceeded,
 * the oldest session with refCount === 0 is evicted (Y.Doc and
 * WebsocketProvider both destroyed). Tune up if users routinely
 * jump between many tickets within a 30s window; tune down if
 * memory pressure shows up in long sessions.
 */
const MAX_SESSIONS = 8

/**
 * Cap on the number of distinct docs we keep in IndexedDB across
 * sessions, refreshes, and tabs. Crossing this triggers a prune
 * pass that calls `y-indexeddb`'s `clearDocument()` on the
 * least-recently-touched docs until under the cap.
 *
 * 50 is a generous default for IT-helpdesk usage (a power user
 * touching that many tickets in a session is unusual). Storing
 * a typical ticket's Yjs updates is a few KB, so 50 docs is
 * well under any browser's quota.
 */
const MAX_IDB_DOCS = 50

/**
 * localStorage key for the "last touched" timestamp per docId.
 * Persistent across reloads so the prune is meaningful across
 * sessions, not just within one tab's lifetime. Defined in the shared
 * cache util so the epoch wipe (sync lifecycle) reads the same map.
 */
const IDB_TOUCH_KEY = COLLAB_IDB_TOUCH_KEY

/**
 * Feature flag: set `localStorage['nosdesk:disable-idb-collab'] = '1'`
 * to disable local persistence at runtime. Useful when the browser's
 * IndexedDB is sandboxed (private windows on some browsers), throws
 * QuotaExceededError, or for reproducing fresh-fetch debugging.
 *
 * Construction failures are also caught at acquire time so a single
 * broken doc doesn't break the rest of the app.
 */
function isLocalPersistenceEnabled(): boolean {
  if (typeof localStorage === 'undefined') return false
  try {
    return localStorage.getItem('nosdesk:disable-idb-collab') !== '1'
  } catch {
    return false
  }
}

/** User-facing connection state for the editor's status indicator. */
export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected'

/**
 * Derive the connection state from the provider's live socket flags.
 * This is the single source of truth: recomputed on every `status`
 * event rather than reconstructed from a history of transitions, so it
 * can never drift (the old per-editor event juggling could latch
 * `disconnected` on a reused, actually-connected provider).
 */
function deriveConnectionStatus(provider: WebsocketProvider): ConnectionStatus {
  if (provider.wsconnected) return 'connected'
  if (provider.wsconnecting) return 'connecting'
  return 'disconnected'
}

interface SessionEntry {
  docId: string
  ydoc: Y.Doc
  provider: WebsocketProvider
  /** Bound `status` listener, kept so `evict` can `off()` it. */
  statusListener: () => void
  /** PermanentUserData has no destroy and registers cumulative
   *  observers per construction (Y source: `PermanentUserData.js`).
   *  Living once per `Y.Doc` lifetime here is the only safe shape;
   *  consumers must not new it up themselves. */
  permanentUserData: SafePermanentUserData
  /** IndexedDB persistence layer. `null` when disabled by feature
   *  flag or when construction failed (private window, quota,
   *  sandboxed origin). The provider alone still works without it,
   *  the only loss is the cold-load instant-render UX. */
  idb: IndexeddbPersistence | null
  refCount: number
  /** Wallclock ms of the most recent release (refCount → 0). */
  lastReleasedAt: number | null
  /** Pending grace-period disconnect timer. */
  graceTimer: ReturnType<typeof setTimeout> | null
}

/**
 * Module-scoped registry. Lives outside Pinia's reactive proxy
 * so Y.Doc / WebsocketProvider keep their EventEmitter
 * semantics.
 */
const sessions = new Map<string, SessionEntry>()

// ---- IndexedDB LRU bookkeeping ---------------------------------
// Tracks when each docId was last accessed so we can prune the
// oldest stores when their count exceeds MAX_IDB_DOCS. Survives
// reloads (localStorage) so a long-running browser doesn't
// accumulate hundreds of dead docs on disk.

function loadTouchMap(): Map<string, number> {
  if (typeof localStorage === 'undefined') return new Map()
  try {
    const raw = localStorage.getItem(IDB_TOUCH_KEY)
    if (!raw) return new Map()
    const parsed = JSON.parse(raw) as Record<string, number>
    return new Map(Object.entries(parsed))
  } catch {
    return new Map()
  }
}

function persistTouchMap(map: Map<string, number>): void {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(IDB_TOUCH_KEY, JSON.stringify(Object.fromEntries(map)))
  } catch {
    // Quota / sandboxed origins; tracking degrades to per-tab.
  }
}

function touchDoc(docId: string): void {
  const map = loadTouchMap()
  map.set(docId, Date.now())
  persistTouchMap(map)
}

function untouchDoc(docId: string): void {
  const map = loadTouchMap()
  if (!map.delete(docId)) return
  persistTouchMap(map)
}

/**
 * If we've accumulated more IDB docs than the cap, fire-and-forget
 * `clearDocument()` for the oldest entries until we're back under.
 * Active sessions are excluded so an open editor never has its
 * cache yanked from under it.
 */
function pruneIdbStores(): void {
  const map = loadTouchMap()
  if (map.size <= MAX_IDB_DOCS) return
  const entries = [...map.entries()]
    .filter(([docId]) => !sessions.has(docId))
    .sort((a, b) => a[1] - b[1])
  const toRemove = map.size - MAX_IDB_DOCS
  for (let i = 0; i < toRemove && i < entries.length; i++) {
    const [docId] = entries[i]
    map.delete(docId)
    clearIdbDocument(docId).catch((err) => {
      logger.warn('Collab session: clearIdbDocument during prune failed', {
        docId,
        err,
      })
    })
  }
  persistTouchMap(map)
}

/**
 * On `beforeunload`, broadcast `setLocalState(null)` for every
 * active session so the y-websocket server immediately removes
 * our awareness entries instead of waiting for the ping-cycle
 * GC (~60s). Without this, the *next* tab the same user opens
 * sees a ghost copy of themselves in the viewers list.
 *
 * The call only succeeds if the WS is still open; on hard kill
 * the server falls back to its ping-cycle cleanup. Best-effort
 * either way.
 */
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    for (const entry of sessions.values()) {
      try {
        entry.provider.awareness.setLocalState(null)
      } catch {
        // Awareness may already be torn down; nothing to do.
      }
    }
  })
}

export interface CollabSessionAcquireOptions {
  /** WebSocket URL prefix the WebsocketProvider should connect
   *  to (e.g. `ws://localhost:8080/api/collaboration/ws`). */
  baseWsUrl: string
  /** Optional WebsocketProvider config passed through verbatim. */
  providerParams?: ConstructorParameters<typeof WebsocketProvider>[3]
}

export interface AcquireResult {
  ydoc: Y.Doc
  provider: WebsocketProvider
  permanentUserData: SafePermanentUserData
  /** True on the first `acquire` for this `docId`, false on
   *  subsequent re-acquires. The caller uses this to decide
   *  whether to apply one-time setup (`setUserMapping` for the
   *  local clientID) vs leaving the existing session intact. */
  isNew: boolean
}

export const useCollabSessionStore = defineStore('collabSession', () => {
  /**
   * Reactive snapshot of currently held docIds + their refcounts.
   * Useful for diagnostics UIs ("You have 3 collaborative docs
   * open"). Updated whenever sessions are added/removed.
   */
  const sessionSnapshot = ref<Array<{ docId: string; refCount: number }>>([])

  /**
   * Reactive per-doc connection status. Owned here because the
   * provider lives here and outlives the editor's mount cycle; one
   * subscription per provider keeps it correct across remounts and
   * shared between editors on the same doc. Editors read it; they
   * don't compute it.
   */
  const connectionStatus = ref<Record<string, ConnectionStatus>>({})

  function refreshSnapshot(): void {
    sessionSnapshot.value = [...sessions.values()].map((s) => ({
      docId: s.docId,
      refCount: s.refCount,
    }))
  }

  function cancelGrace(entry: SessionEntry): void {
    if (entry.graceTimer) {
      clearTimeout(entry.graceTimer)
      entry.graceTimer = null
    }
  }

  function scheduleGrace(entry: SessionEntry): void {
    cancelGrace(entry)
    entry.graceTimer = setTimeout(() => {
      // Disconnect (not destroy) so a re-acquire can resume
      // without re-allocating the doc. LRU eviction is what
      // ultimately frees memory.
      if (entry.refCount === 0) {
        try {
          entry.provider.disconnect()
        } catch (err) {
          logger.warn('Collab session: disconnect after grace failed', {
            docId: entry.docId,
            err,
          })
        }
      }
      entry.graceTimer = null
    }, GRACE_MS)
  }

  function evict(docId: string): void {
    const entry = sessions.get(docId)
    if (!entry) return
    cancelGrace(entry)
    // Per y-websocket#142, both must be destroyed to free
    // awareness listeners and avoid memory leaks. IndexedDB
    // persistence layer is destroyed first so it stops listening
    // for ydoc updates before the doc tombstones; `destroy()`
    // here releases the IDB connection but PRESERVES the
    // on-disk data, the `clearData()` wipe is reserved for
    // explicit "ticket deleted" cleanup in Phase 5.
    if (entry.idb) {
      try {
        entry.idb.destroy()
      } catch (err) {
        logger.warn('Collab session: idb.destroy() threw', { docId, err })
      }
    }
    try {
      entry.provider.off('status', entry.statusListener)
    } catch {
      // Provider may already be torn down; nothing to do.
    }
    delete connectionStatus.value[docId]
    try {
      entry.provider.destroy()
    } catch (err) {
      logger.warn('Collab session: provider.destroy() threw', {
        docId,
        err,
      })
    }
    try {
      entry.ydoc.destroy()
    } catch (err) {
      logger.warn('Collab session: ydoc.destroy() threw', {
        docId,
        err,
      })
    }
    sessions.delete(docId)
    refreshSnapshot()
  }

  function enforceLruCap(): void {
    if (sessions.size <= MAX_SESSIONS) return
    const candidates = [...sessions.values()]
      .filter((s) => s.refCount === 0 && s.lastReleasedAt !== null)
      .sort((a, b) => (a.lastReleasedAt ?? 0) - (b.lastReleasedAt ?? 0))
    while (sessions.size > MAX_SESSIONS && candidates.length > 0) {
      const victim = candidates.shift()
      if (victim) evict(victim.docId)
    }
  }

  function acquire(docId: string, options: CollabSessionAcquireOptions): AcquireResult {
    const existing = sessions.get(docId)
    if (existing) {
      cancelGrace(existing)
      // Re-connect if the websocket dropped while idle.
      if (!existing.provider.wsconnected) {
        try {
          existing.provider.connect()
        } catch (err) {
          logger.warn('Collab session: re-connect on acquire failed', {
            docId,
            err,
          })
        }
      }
      existing.refCount++
      // Clear `lastReleasedAt` while at least one consumer holds a
      // reference. Without this, an actively-bounced session (idle
      // → reacquire → idle → reacquire) keeps the stamp from its
      // first release, so `enforceLruCap` sorts it as older than a
      // genuinely-cold session and may evict it ahead of a session
      // the user is mid-interaction with. `enforceLruCap` already
      // gates on `refCount === 0`, so this is just bookkeeping
      // hygiene — `lastReleasedAt` is only meaningful when the
      // session is actually released.
      existing.lastReleasedAt = null
      // Re-seed in case the provider settled while no listener-driven
      // event fired (the listener persists, but this guards the
      // already-connected-during-grace case).
      connectionStatus.value[docId] = deriveConnectionStatus(existing.provider)
      refreshSnapshot()
      return {
        ydoc: existing.ydoc,
        provider: existing.provider,
        permanentUserData: existing.permanentUserData,
        isNew: false,
      }
    }

    const ydoc = new Y.Doc()
    // Disable GC before any update is applied so snapshot
    // history (rendered by `permanentUserData.dss`) survives.
    // Idempotent at the Y.Doc level (it's a plain boolean read
    // by the GC routine; setting it before any merges is the
    // safe path per yjs README "DocOpts").
    ydoc.gc = false

    // IndexedDB persistence: best-effort. Construction can throw
    // (private windows, sandboxed origins, quota exceeded). When
    // it does, the session degrades to provider-only and the
    // editor cold-starts from the websocket like before.
    let idb: IndexeddbPersistence | null = null
    if (isLocalPersistenceEnabled()) {
      try {
        idb = new IndexeddbPersistence(docId, ydoc)
      } catch (err) {
        logger.warn('Collab session: IndexeddbPersistence construction failed, continuing without local cache', {
          docId,
          err,
        })
      }
    }

    const provider = new WebsocketProvider(
      options.baseWsUrl,
      docId,
      ydoc,
      options.providerParams,
    )
    const permanentUserData = new SafePermanentUserData(ydoc)
    // One subscription per provider. Derives status from live socket
    // flags on every transition; seeded synchronously so a provider
    // that's already mid-connect reads correctly.
    const onStatus = () => {
      connectionStatus.value[docId] = deriveConnectionStatus(provider)
    }
    provider.on('status', onStatus)
    onStatus()
    const entry: SessionEntry = {
      docId,
      ydoc,
      provider,
      statusListener: onStatus,
      permanentUserData,
      idb,
      refCount: 1,
      lastReleasedAt: null,
      graceTimer: null,
    }
    sessions.set(docId, entry)
    enforceLruCap()
    if (idb) {
      touchDoc(docId)
      pruneIdbStores()
    }
    refreshSnapshot()
    return { ydoc, provider, permanentUserData, isNew: true }
  }

  function release(docId: string): void {
    const entry = sessions.get(docId)
    if (!entry) return
    entry.refCount = Math.max(0, entry.refCount - 1)
    if (entry.refCount === 0) {
      entry.lastReleasedAt = Date.now()
      scheduleGrace(entry)
    }
    refreshSnapshot()
  }

  /** Forced eviction. For tests and debug menus. The collab
   *  session is destroyed but the on-disk IDB cache is left in
   *  place; use `purgeData()` to wipe both. */
  function destroy(docId: string): void {
    evict(docId)
  }

  /**
   * Pre-load a doc without taking a refcount on it. Intended
   * for hover-prefetch on RouterLink (`@mouseenter="warm(docId)"`)
   * so by the time the user clicks, the WebSocket handshake and
   * IndexedDB load are already in flight or done. If they don't
   * click within the grace window the session disconnects on its
   * own.
   *
   * No-op if a session for this docId already exists (warm or
   * cold) since the data is already in flight or cached. Options
   * default to the derived collab WS URL + the same provider
   * params the editor uses, so callers (like a `<RouterLink>`'s
   * `@mouseenter`) don't need to know connection details.
   */
  function warm(docId: string, options?: CollabSessionAcquireOptions): void {
    if (sessions.has(docId)) return
    const opts: CollabSessionAcquireOptions = options ?? {
      baseWsUrl: collabWsBaseUrl(),
      providerParams: { resyncInterval: 20000, disableBc: true },
    }
    // Same path as `acquire` but we drop the refcount immediately
    // and start the grace timer, so the session disconnects on
    // its own if the user never actually navigates here.
    acquire(docId, opts)
    release(docId)
  }

  /**
   * Forced eviction + IndexedDB wipe + LRU bookkeeping cleanup.
   * For the SSE "ticket deleted" handler: when the server
   * deletes a ticket the local cached doc is stale data the user
   * shouldn't see again, even after a refresh. `clearData()` on
   * an active session if there is one, otherwise `clearDocument`
   * by name.
   */
  async function purgeData(docId: string): Promise<void> {
    const entry = sessions.get(docId)
    if (entry?.idb) {
      try {
        await entry.idb.clearData()
      } catch (err) {
        logger.warn('Collab session: idb.clearData() failed', { docId, err })
      }
    } else {
      try {
        await clearIdbDocument(docId)
      } catch (err) {
        logger.warn('Collab session: clearIdbDocument failed', { docId, err })
      }
    }
    if (entry) evict(docId)
    untouchDoc(docId)
  }

  /** Test helper: wipe all sessions. Used by integration
   *  tests; never call in production code paths. */
  function destroyAll(): void {
    for (const docId of [...sessions.keys()]) evict(docId)
  }

  return {
    sessionSnapshot,
    connectionStatus,
    acquire,
    release,
    destroy,
    warm,
    purgeData,
    destroyAll,
  }
})
