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
import { defineStore } from 'pinia'
import { ref } from 'vue'

import { logger } from '@/utils/logger'
import { SafePermanentUserData } from '@/utils/safePermanentUserData'

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

interface SessionEntry {
  docId: string
  ydoc: Y.Doc
  provider: WebsocketProvider
  /** PermanentUserData has no destroy and registers cumulative
   *  observers per construction (Y source: `PermanentUserData.js`).
   *  Living once per `Y.Doc` lifetime here is the only safe shape;
   *  consumers must not new it up themselves. */
  permanentUserData: SafePermanentUserData
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
   *  whether to apply one-time setup (`ydoc.gc = false`, the
   *  initial `setUserMapping` for the local clientID) vs
   *  leaving the existing session intact. */
  isNew: boolean
}

export const useCollabSessionStore = defineStore('collabSession', () => {
  /**
   * Reactive snapshot of currently held docIds + their refcounts.
   * Useful for diagnostics UIs ("You have 3 collaborative docs
   * open"). Updated whenever sessions are added/removed.
   */
  const sessionSnapshot = ref<Array<{ docId: string; refCount: number }>>([])

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
    // awareness listeners and avoid memory leaks.
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
    const provider = new WebsocketProvider(
      options.baseWsUrl,
      docId,
      ydoc,
      options.providerParams,
    )
    const permanentUserData = new SafePermanentUserData(ydoc)
    const entry: SessionEntry = {
      docId,
      ydoc,
      provider,
      permanentUserData,
      refCount: 1,
      lastReleasedAt: null,
      graceTimer: null,
    }
    sessions.set(docId, entry)
    enforceLruCap()
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

  /** Forced eviction. For tests, debug menus, and the
   *  "ticket deleted" SSE handler in Phase 5. */
  function destroy(docId: string): void {
    evict(docId)
  }

  /** Test helper: wipe all sessions. Used by integration
   *  tests; never call in production code paths. */
  function destroyAll(): void {
    for (const docId of [...sessions.keys()]) evict(docId)
  }

  return {
    sessionSnapshot,
    acquire,
    release,
    destroy,
    destroyAll,
  }
})
