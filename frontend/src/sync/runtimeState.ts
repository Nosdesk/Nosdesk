/**
 * Runtime state the sync engine modules share: the open IndexedDB handle,
 * the identity the cache was opened under, and the readiness gates every
 * workspace-scoped fetch checks.
 */
import { readonly, ref } from 'vue'
import type * as idb from './idb'
import { sessionGone } from '@nosdesk/core/services/session'
import { workspaceReady } from '@/services/activeWorkspace'

export interface LifecycleState {
  handle: idb.IdbHandle | null
  schemaHash: string
  /** Periodic delta-poll timer, only running when SSE is wedged or
   * unavailable. Cleared on tearDown(). */
  pollTimer: ReturnType<typeof setInterval> | null
  /** True when the user's IndexedDB couldn't be opened (private
   * browsing, quota, blocked). The runtime degrades to memory-only
   * — no persistence, no warm-start, but reads/writes still work. */
  memoryOnly: boolean
  /** Set while a retention resync is wiping and re-bootstrapping. Both the
   * 10s poll and the workspace-ready watch call pullDelta(), and a resync
   * re-bootstraps (which can itself trigger a poll), so without this a single
   * aged-out cursor could start several overlapping wipes. */
  resyncing: boolean
  /** The arguments hydrate() opened the database with. A retention resync
   * wipes and reopens, and `IdbHandle` carries only the derived name, so the
   * inputs have to be kept rather than reverse-engineered from it. */
  openArgs: { userUuid: string; workspaceSlug: string | null } | null
  /** False until this session's first successful catch-up (a delta apply or
   * a bootstrap end). Until then an SSE frame may apply its rows but must
   * NOT advance the cursor: the SSE bridge attaches while the launch
   * catch-up is still in flight, and a frame that jumped the cursor to
   * "now" would make the catch-up delta skip the whole offline span. The
   * frame's events are also in `sync_actions`, so the catch-up re-delivers
   * them; holding the cursor back loses nothing. */
  caughtUp: boolean
  /** The workspace data epoch the local caches hold rows from ('' when none
   * is known). The server moves it when the workspace's data is replaced (an
   * archive import gives every row a new id); `/sync/schema` and every delta
   * carry the current one, and a different one wipes every local cache of the
   * workspace. Not `pool.currentEpoch()`, which counts tear-downs. */
  workspaceEpoch: string
  /** The database instance id hydrate() fenced on ('' when the server
   * reported none), kept so a resync can write it into the reopened cache. */
  instanceId: string
}

export const state: LifecycleState = {
  handle: null,
  schemaHash: '',
  pollTimer: null,
  memoryOnly: false,
  resyncing: false,
  openArgs: null,
  caughtUp: false,
  workspaceEpoch: '',
  instanceId: '',
}

/** Memory-only counts as hydrated (it still bootstraps and serves; only
 *  warm-start persistence is lost). Gating subscribe/runBootstrap/pullDelta on
 *  `state.handle` alone stranded memory-only groups empty until a force-quit. */
export function isHydrated(): boolean {
  return state.handle !== null || state.memoryOnly
}

/** A workspace-scoped fetch (bootstrap/delta) may only fire once the runtime is
 *  hydrated AND a workspace is selected. `workspaceReady()` is the same gate the
 *  Colada queries use (true in host mode, or once the path-mode slug is set);
 *  without it the sync engine sends header-less requests that fail 400
 *  NoWorkspaceSelected, the 10s delta poll spamming them all session.
 *  And only while there is a session: once a refresh is refused, each poll
 *  would otherwise 401 and ask the server for a refresh again, every 10s,
 *  until something tore the runtime down. A sign-in resumes it. */
export function canFetchWorkspace(): boolean {
  return isHydrated() && workspaceReady() && !sessionGone()
}

/**
 * Whether the pool reflects the server: a bootstrap or delta catch-up has
 * completed since the runtime opened. False after a network failure, when
 * an empty pool means "unknown", not "nothing there". Reactive, because a
 * `subscribe` before the runtime is hydrated returns at once and the
 * bootstrap it queued completes later.
 */
const caughtUpRef = ref(false)
export const caughtUp = readonly(caughtUpRef)
export function setCaughtUp(v: boolean): void {
  state.caughtUp = v
  caughtUpRef.value = v
}
