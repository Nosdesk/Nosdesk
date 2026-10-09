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
import { sessionGone } from '@nosdesk/core/services/session'
import { discardCollabToken, getCollabToken, peekCollabToken } from '@/services/collabToken'

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

/** A doc's connection state, from the provider's live state. */
export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected'

/**
 * What the editor shows about the connection, if anything. The note renders
 * from its local copy straight away, so only trouble shows: a first connect
 * that is slow, a drop that lasts, or no connection at all.
 */
export type ConnectionBadge = 'connecting' | 'reconnecting' | 'disconnected' | null

/** Why the server closed a document's connection for good. */
export type ConnectionRefusal = 'signed-out' | 'no-access' | 'gone'

/**
 * Read the server's close code (`CollabRefusal` in
 * `handlers/collaboration.rs`): 4401 the token wasn't accepted, 4403 no
 * access, 4404 the document is gone. Any other 44xx close reads as no access.
 */
function refusalFor(code: number | undefined): ConnectionRefusal {
  if (code === 4401) return 'signed-out'
  if (code === 4404) return 'gone'
  return 'no-access'
}

/** A first connect shows "Connecting..." only once it takes this long,
 *  timed from when the note is opened (a hover prewarm doesn't count). */
const SLOW_CONNECT_MS = 4000
/** A drop shows "Reconnecting..." only once it lasts this long; y-websocket
 *  usually reconnects a blip sooner. */
const RECONNECT_GRACE_MS = 1000

/** What the socket flags don't say about a provider's connection. */
interface LinkState {
  /** A token for the next connect is being fetched (or the fetch is
   *  backing off after a failure): connecting, not disconnected. */
  tokenPending: boolean
  /** Bumped by every connect started and every deliberate stop, so a token
   *  fetch that finishes after either knows it no longer applies. */
  attempt: number
  /** The next try at fetching a token, after a failed one. */
  retryTimer: ReturnType<typeof setTimeout> | null
  /** Failed or refused tokens since the server last served the doc; spaces
   *  out the retries. */
  tokenFailures: number
  /** The server closed the socket for good (a 44xx close). Cleared when a
   *  connect is started again. */
  terminal: boolean
  /** Why, for a terminal close. */
  refusal: ConnectionRefusal | null
  /** A refused token was already replaced once since the server last served
   *  this connection, so another refusal means the person is signed out. */
  retriedToken: boolean
  /** Connected since the last deliberate disconnect, so not connecting
   *  again means the connection dropped. */
  everConnected: boolean
  /** Recompute the doc's status; set by the store. */
  changed: () => void
}

const linkStates = new WeakMap<WebsocketProvider, LinkState>()

function linkState(provider: WebsocketProvider): LinkState {
  let state = linkStates.get(provider)
  if (!state) {
    state = {
      tokenPending: false,
      attempt: 0,
      retryTimer: null,
      tokenFailures: 0,
      terminal: false,
      refusal: null,
      retriedToken: false,
      everConnected: false,
      changed: () => {},
    }
    linkStates.set(provider, state)
  }
  return state
}

function isOffline(): boolean {
  return typeof navigator !== 'undefined' && navigator.onLine === false
}

/**
 * Derive the connection state from the provider's live state. This is the
 * single source of truth: recomputed on every change rather than
 * reconstructed from a history of transitions, so it can never drift (the
 * old per-editor event juggling could latch `disconnected` on a reused,
 * actually-connected provider). A token on its way, or a reconnect y-websocket
 * has scheduled, is connecting; only a terminal close, being offline or a
 * deliberate disconnect is disconnected.
 */
function deriveConnectionStatus(provider: WebsocketProvider): ConnectionStatus {
  const link = linkState(provider)
  if (provider.wsconnected) return 'connected'
  if (link.terminal || isOffline()) return 'disconnected'
  if (provider.wsconnecting || link.tokenPending || provider.shouldConnect) return 'connecting'
  return 'disconnected'
}

/** Backoff between token fetches after a failed one: the first retry soon
 *  (an API blip), then further apart, so an outage can't become a tight loop. */
const TOKEN_RETRY_FIRST_MS = 2000
const TOKEN_RETRY_MAX_MS = 30_000

/** Providers torn down by `evict`; a token fetch that finishes afterwards
 *  must not reconnect them. */
const retiredProviders = new WeakSet<WebsocketProvider>()

/** Stop any connect in progress: a pending token fetch or retry no longer
 *  connects when it finishes. */
function stopConnecting(provider: WebsocketProvider): void {
  const link = linkState(provider)
  link.attempt++
  link.tokenPending = false
  if (link.retryTimer) clearTimeout(link.retryTimer)
  link.retryTimer = null
}

/**
 * Connect with a token that is valid at handshake time. y-websocket rebuilds
 * the URL from `params` on every (re)connect, so the token must be in place
 * before that happens:
 * - a still-valid cached token is set synchronously, ahead of y-websocket's
 *   own reconnect timer (at least 200ms), and the reconnect proceeds;
 * - an expired one parks the reconnect, fetches a fresh token, then connects.
 *   Without the pause the first retry after the ~2 minute TTL went out with
 *   the old token and was refused.
 * A failed fetch never connects with the token the provider already holds
 * (the server may have refused it): the doc stays reconnecting and the fetch
 * is retried with backoff, or at once when the device comes back online. If
 * the session is gone (`sessionGone`: the shared refresh was rejected, or no
 * session is held), the doc is signed out instead.
 */
async function connectWithValidToken(provider: WebsocketProvider): Promise<void> {
  stopConnecting(provider)
  const link = linkState(provider)
  const attempt = link.attempt
  // A connect started again: an earlier terminal close no longer stands.
  link.terminal = false
  link.refusal = null
  const cached = peekCollabToken()
  if (cached) {
    provider.params = { token: cached }
    // Inside a `connection-close` the socket is still attached, so this only
    // re-arms `shouldConnect`; y-websocket's backoff timer does the reconnect.
    provider.connect()
    link.changed()
    return
  }
  // Park the reconnect y-websocket may have scheduled (its timer checks
  // `shouldConnect`). Not `disconnect()`: inside `connection-close` that
  // re-enters the close path.
  provider.shouldConnect = false
  link.tokenPending = true
  link.changed()
  let token: string | null = null
  try {
    token = await getCollabToken()
  } catch (err) {
    logger.warn('Collab session: token fetch failed', { err })
  }
  // Superseded by a newer connect or a deliberate stop. A terminal close
  // (the server said not to reconnect), emitted after `connection-close`,
  // wins over the reconnect too.
  if (attempt !== link.attempt || retiredProviders.has(provider)) return
  if (link.terminal) {
    link.tokenPending = false
    link.changed()
    return
  }
  if (token !== null) {
    link.tokenPending = false
    provider.params = { token }
    provider.connect()
    link.changed()
    return
  }
  if (sessionGone()) {
    link.tokenPending = false
    link.terminal = true
    link.refusal = 'signed-out'
    link.changed()
    return
  }
  retryTokenLater(provider)
}

/** Fetch a fresh token and connect after a backoff; reconnecting meanwhile. */
function retryTokenLater(provider: WebsocketProvider): void {
  const link = linkState(provider)
  if (link.retryTimer) clearTimeout(link.retryTimer)
  provider.shouldConnect = false
  link.tokenPending = true
  const delay = Math.min(TOKEN_RETRY_MAX_MS, TOKEN_RETRY_FIRST_MS * 2 ** link.tokenFailures)
  link.tokenFailures++
  link.retryTimer = setTimeout(() => {
    link.retryTimer = null
    void connectWithValidToken(provider)
  }, delay)
  link.changed()
}

/**
 * Connect the provider and keep its token fresh across reconnects. Owned here
 * so callers (editor, prewarm) never deal with the token.
 */
async function attachCollabToken(provider: WebsocketProvider): Promise<void> {
  await connectWithValidToken(provider)
  provider.on('connection-close', () => {
    // An explicit disconnect (evict, the pause above) is not a reconnect.
    if (provider.shouldConnect) void connectWithValidToken(provider)
  })
}

interface SessionEntry {
  docId: string
  ydoc: Y.Doc
  provider: WebsocketProvider
  /** Bound `status` listener, kept so `evict` can `off()` it. */
  statusListener: () => void
  /** Bound `closed` (terminal close) listener, likewise. */
  closedListener: () => void
  /** Bound `sync` listener, likewise. */
  syncListener: (synced: boolean) => void
  /** Pending timer that shows the badge once a connect or drop lasts. */
  badgeTimer: ReturnType<typeof setTimeout> | null
  /** The badge `badgeTimer` will show. */
  badgeTimerFor: ConnectionBadge
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

/** Fetch a token now for every doc waiting to retry one. */
function retryPendingTokens(): void {
  for (const entry of sessions.values()) {
    if (linkState(entry.provider).retryTimer) void connectWithValidToken(entry.provider)
  }
}

// ---- Hidden tabs ------------------------------------------------
// A tab hidden for a while lets its notes' connections go; shown again, the
// notes still open reconnect through `connectWithValidToken`, so the dial
// carries a token that is valid now, not the one held when the tab was hidden
// (by then usually past its ~2 minute life). Owned here, with the provider,
// rather than by each editor: one listener covers every open note, and no
// caller dials the provider directly.

/** How long a tab stays hidden before its notes disconnect. A brief tab
 *  switch keeps them connected. */
const HIDDEN_DISCONNECT_MS = 30_000

/** Providers disconnected because the tab was hidden. */
const hiddenTabProviders = new WeakSet<WebsocketProvider>()
let hiddenTabTimer: ReturnType<typeof setTimeout> | null = null

function disconnectForHiddenTab(): void {
  for (const { provider } of sessions.values()) {
    const link = linkState(provider)
    if (link.terminal) continue
    const live = provider.wsconnected || provider.wsconnecting || provider.shouldConnect || link.tokenPending
    if (!live) continue
    hiddenTabProviders.add(provider)
    // Showing the tab again is a first connect, timed as one.
    link.everConnected = false
    stopConnecting(provider)
    provider.disconnect()
  }
}

function reconnectShownTab(): void {
  for (const entry of sessions.values()) {
    if (!hiddenTabProviders.has(entry.provider)) continue
    hiddenTabProviders.delete(entry.provider)
    // A note closed while the tab was hidden reconnects when it is reopened
    // (`acquire`), not now.
    if (entry.refCount > 0) void connectWithValidToken(entry.provider)
  }
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      hiddenTabTimer ??= setTimeout(() => {
        hiddenTabTimer = null
        if (document.hidden) disconnectForHiddenTab()
      }, HIDDEN_DISCONNECT_MS)
      return
    }
    if (hiddenTabTimer) clearTimeout(hiddenTabTimer)
    hiddenTabTimer = null
    reconnectShownTab()
  })
}

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

  /** Per-doc badge for the editor, timed from `connectionStatus`. */
  const connectionBadge = ref<Record<string, ConnectionBadge>>({})

  /** Per-doc reason the server refused the connection, for the editor to
   *  say instead of a bare "Disconnected". */
  const connectionRefusal = ref<Record<string, ConnectionRefusal | null>>({})

  function clearBadgeTimer(entry: SessionEntry): void {
    if (entry.badgeTimer) clearTimeout(entry.badgeTimer)
    entry.badgeTimer = null
    entry.badgeTimerFor = null
  }

  /** Recompute a doc's status and, from it, its badge. */
  function refreshStatus(entry: SessionEntry): void {
    const { docId, provider } = entry
    const status = deriveConnectionStatus(provider)
    const link = linkState(provider)
    if (status === 'connected') link.everConnected = true
    connectionStatus.value[docId] = status
    connectionRefusal.value[docId] = link.terminal ? link.refusal : null

    if (status !== 'connecting') {
      clearBadgeTimer(entry)
      connectionBadge.value[docId] = status === 'disconnected' ? 'disconnected' : null
      return
    }
    // Nobody shows a note nobody holds (a hover prewarm, or one closed and in
    // its grace period), so its wait isn't timed: the timer starts when the
    // note is opened.
    if (entry.refCount === 0) {
      clearBadgeTimer(entry)
      connectionBadge.value[docId] = null
      return
    }
    const due: ConnectionBadge = link.everConnected ? 'reconnecting' : 'connecting'
    if (connectionBadge.value[docId] === due || entry.badgeTimerFor === due) return
    clearBadgeTimer(entry)
    connectionBadge.value[docId] = null
    entry.badgeTimerFor = due
    entry.badgeTimer = setTimeout(
      () => {
        entry.badgeTimer = null
        entry.badgeTimerFor = null
        connectionBadge.value[docId] = due
      },
      due === 'reconnecting' ? RECONNECT_GRACE_MS : SLOW_CONNECT_MS,
    )
  }

  // Going offline or back online changes every doc's status at once. Back
  // online, a doc waiting to retry its token fetch tries again straight away.
  if (typeof window !== 'undefined') {
    const refreshAll = () => {
      for (const entry of sessions.values()) refreshStatus(entry)
    }
    window.addEventListener('online', () => {
      retryPendingTokens()
      refreshAll()
    })
    window.addEventListener('offline', refreshAll)
  }

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
        // Deliberate: reopening the note later is a first connect again,
        // not a reconnect.
        linkState(entry.provider).everConnected = false
        stopConnecting(entry.provider)
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
      entry.provider.off('closed', entry.closedListener)
      entry.provider.off('sync', entry.syncListener)
    } catch {
      // Provider may already be torn down; nothing to do.
    }
    linkState(entry.provider).changed = () => {}
    stopConnecting(entry.provider)
    clearBadgeTimer(entry)
    delete connectionStatus.value[docId]
    delete connectionBadge.value[docId]
    delete connectionRefusal.value[docId]
    try {
      retiredProviders.add(entry.provider)
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
      // Re-connect if the websocket dropped while idle, with a token that is
      // still valid (a session idle past the TTL holds an expired one).
      if (!existing.provider.wsconnected) {
        try {
          void connectWithValidToken(existing.provider)
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
      refreshStatus(existing)
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

    // Create disconnected: the collab WS authenticates with a connection token
    // in the URL query (a browser WebSocket can't send a header or cross-origin
    // cookie). The store owns fetching it (callers don't), then connects.
    const provider = new WebsocketProvider(options.baseWsUrl, docId, ydoc, {
      ...options.providerParams,
      connect: false,
    })
    const permanentUserData = new SafePermanentUserData(ydoc)
    // One subscription per provider. Derives status from live state on
    // every transition, including the token fetch and a terminal close,
    // which y-websocket reports after its last `status` event.
    const onStatus = () => refreshStatus(entry)
    const onClosed = (event?: { code?: number }) => {
      const link = linkState(provider)
      const refusal = refusalFor(event?.code)
      // A refused token may only be stale (it outlived its short life, or a
      // clock is off): fetch a fresh one and try again at once.
      if (refusal === 'signed-out' && !link.retriedToken) {
        link.retriedToken = true
        discardCollabToken()
        void connectWithValidToken(provider)
        return
      }
      // A fresh token refused too, while the session holds, is a token
      // problem, not the person signed out: keep fetching fresh ones, spaced
      // out, and say so in the log in case the server keeps refusing.
      if (refusal === 'signed-out' && !sessionGone()) {
        logger.warn('Collab session: the server refused a fresh token; fetching another', {
          docId,
          attempts: link.tokenFailures + 1,
        })
        stopConnecting(provider)
        discardCollabToken()
        retryTokenLater(provider)
        return
      }
      link.terminal = true
      link.refusal = refusal
      refreshStatus(entry)
    }
    // The server served the document: a later refused token is a new
    // problem, worth one fresh token again. Not on `connected`: a refused
    // connection is upgraded (and so reports connected) before it is closed.
    const onSync = (synced: boolean) => {
      if (!synced) return
      const link = linkState(provider)
      link.retriedToken = false
      link.tokenFailures = 0
    }
    const entry: SessionEntry = {
      docId,
      ydoc,
      provider,
      statusListener: onStatus,
      closedListener: onClosed,
      syncListener: onSync,
      badgeTimer: null,
      badgeTimerFor: null,
      permanentUserData,
      idb,
      refCount: 1,
      lastReleasedAt: null,
      graceTimer: null,
    }
    linkState(provider).changed = onStatus
    provider.on('status', onStatus)
    provider.on('closed', onClosed)
    provider.on('sync', onSync)
    void attachCollabToken(provider)
    // Seeded synchronously: the token fetch above has already started.
    onStatus()
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
      refreshStatus(entry)
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
    // Same path as `acquire` (which fetches the token + connects); we drop the
    // refcount immediately and start the grace timer, so the session disconnects
    // on its own if the user never actually navigates here.
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

  /**
   * Close every open document's connection and drop its session. On a
   * workspace switch or sign-out: the connections belong to the workspace
   * being left, and would otherwise reconnect with the next one's token
   * and be refused until their grace period ran out.
   */
  function closeAll(): void {
    for (const docId of [...sessions.keys()]) evict(docId)
  }

  /** Test helper: wipe all sessions. */
  function destroyAll(): void {
    closeAll()
  }

  return {
    sessionSnapshot,
    connectionStatus,
    connectionBadge,
    connectionRefusal,
    closeAll,
    acquire,
    release,
    destroy,
    warm,
    purgeData,
    destroyAll,
  }
})
