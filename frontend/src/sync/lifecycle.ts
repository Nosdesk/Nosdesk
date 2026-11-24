/**
 * Sync engine lifecycle.
 *
 * - `hydrate(userUuid, schemaHash)` is called once per workspace
 *   shell. It opens the IndexedDB, rehydrates the pool from cached
 *   rows, then either runs cold-bootstrap or warm-delta against the
 *   server and finally opens the SSE stream.
 * - `subscribe(group)` adds to the subscription set and returns a
 *   promise that resolves when bootstrap-of-this-group completes.
 *   Used by router-driven group expansion (entering a project view).
 * - `unsubscribe(group)` drops a group and the rows only it owned.
 *   Views reach both through `useGroupSubscription`.
 * - `tearDown()` releases the IndexedDB handle and resets the pool;
 *   called on sign-out.
 */
import { watch } from 'vue'
import { logger } from '@nosdesk/core/utils/logger'
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import * as queue from './queue'
import { setReferenceFetcher } from '@nosdesk/core/sync/composables'
import { notifySyncActions } from '@nosdesk/core/sync/observers'
import { applyWorkspaceCapabilities } from '@/composables/useWorkspaceCapabilities'
import { purgeAllCollabDocs } from '@/utils/collabLocalCache'
import { authFetch } from '@/services/authFetch'
import { workspaceReadyRef } from '@/services/activeWorkspace'
import { canFetchWorkspace, cursorHeld, isHydrated, setCaughtUp, state } from './runtimeState'
import { MODEL_VERSIONS_TAG, SCHEMA_VERSIONS } from './schemaVersions'
import {
  dropGroupWatermarks,
  noteSnapshotMaxAge,
  partitionWarmGroups,
  pruneTornWatermarks,
  recordGroupWatermarks,
  currentRecordTick,
  recordedSince,
  resetSessionMarks,
  resetWatermarkPrune,
  staleWatermarkedGroups,
} from './warm'
import { followAssetVisibility, reconcileByOwner } from './reconcile'
import { isWorkspaceGroup, ownersOnHydrate, SNAPSHOT_BACKED, stripGroupsAndCache } from './ownership'
import { cancelGroupReleases } from './useGroupSubscription'
import { applyActions, cachedOwners, rowKey } from './apply'
import { referenceFetcher } from './referenceFetchers'
import type {
  BootstrapLine,
  BootstrapMeta,
  DeltaResponse,
  SyncAggregate,
} from '@nosdesk/core/sync/types'

export { caughtUp } from './runtimeState'
export { followAssetVisibility, reconcileByOwner } from './reconcile'
export { applySseFrame } from './apply'

/**
 * Bring every subscribed group current. Called after hydrate() and whenever
 * the workspace becomes ready, to drain groups whose fetch subscribe()
 * deferred (it adds the group to the pool's set but defers the fetch when the
 * runtime isn't hydrated or no workspace is selected), so none is left empty
 * until the next tearDown. Warm-cached groups catch up via delta; the rest
 * bootstrap (see bringGroupsCurrent).
 */
function bootstrapDeferredGroups(): void {
  const groups = Array.from(pool.getSubscribedGroups())
  if (groups.length > 0) void bringGroupsCurrent(groups)
}

/**
 * Bring groups current: delta catch-up for warm-cached groups, full
 * bootstrap for the rest. Delta runs FIRST — a bootstrap's `__end__`
 * advances the global cursor to now, so catching the warm groups up from
 * the cached cursor must happen before that or their missed events are
 * skipped forever.
 */
async function bringGroupsCurrent(groups: string[]): Promise<void> {
  if (groups.length === 0 || !canFetchWorkspace()) return
  // Read before any await: a group let go while the partition and catch-up
  // run is no longer the bootstrap's to stream (see runBootstrap).
  const subscribedAt = pool.currentSubscriptionTick()
  const { warm, cold, stale } = await partitionWarmGroups(groups)
  // Cached rows the cursor tracks: the bootstrap below must not jump the
  // cursor past their events, whether or not the delta catch-up got through.
  const cached = warm.length > 0 || stale.length > 0
  // Marked pending through the catch-up too, so a refresh in that window
  // doesn't snapshot them a second time.
  await whileBootstrapping(cold, async () => {
    if (cached) {
      logger.info('sync warm launch: delta catch-up instead of snapshot re-stream', {
        groups: warm,
        stale,
      })
      // A stale group's cached rows catch up too, so its fresh snapshot runs
      // the `keepCursor` path: the cursor holds for any group brought current
      // by delta elsewhere, and the strip drops what lingered.
      await catchUpViaDelta()
    }
    if (cold.length > 0) {
      await runBootstrap(cold, subscribedAt, cached)
    }
  })
}

/** Run `work` with `groups` marked as being bootstrapped (see
 * LifecycleState.bootstrapping). */
async function whileBootstrapping<T>(groups: readonly string[], work: () => Promise<T>): Promise<T> {
  const pending = state.bootstrapping
  for (const g of groups) pending.set(g, (pending.get(g) ?? 0) + 1)
  try {
    return await work()
  } finally {
    for (const g of groups) {
      const left = (pending.get(g) ?? 1) - 1
      if (left > 0) pending.set(g, left)
      else pending.delete(g)
    }
  }
}

/**
 * Run `work` holding the cursor (see LifecycleState.cursorHolds). The last
 * hold to go runs the delta that replays, for every subscribed group, what
 * the holds kept back and what the snapshots' horizons passed: rows come back
 * to their latest state, since events carry whole rows, in order. That holds
 * whether `work` finished, failed or was cut short. A tear-down or resync
 * replaces the holds, and the replay is theirs to run then.
 */
async function holdCursorWhile<T>(work: () => Promise<T>): Promise<T> {
  const holds = state.cursorHolds
  const epoch = pool.currentEpoch()
  const token = {}
  holds.add(token)
  try {
    return await work()
  } finally {
    holds.delete(token)
    if (holds.size === 0 && holds === state.cursorHolds && pool.currentEpoch() === epoch) {
      await catchUpViaDelta()
    }
  }
}

/**
 * Page through the delta feed until the cursor is current. Bounded so a
 * pathological backlog can't spin here forever; anything left after the cap
 * keeps arriving via the normal 10s poll / SSE.
 */
const CATCH_UP_MAX_PAGES = 20

async function catchUpViaDelta(): Promise<void> {
  for (let i = 0; i < CATCH_UP_MAX_PAGES; i++) {
    const hasMore = await pullDelta()
    if (!hasMore) return
  }
  logger.warn('sync warm launch: catch-up hit the page cap; poll continues the tail')
}

// Re-fire once a workspace is selected: an early subscribe (or a hydrate that
// finished before the slug was set) deferred its bootstrap and the delta poll
// was skipped. Draining here makes arriving at a ready workspace immediate
// rather than waiting up to one poll interval.
watch(workspaceReadyRef, (ready) => {
  if (ready && isHydrated()) {
    bootstrapDeferredGroups()
    void pullDelta()
  }
})

/**
 * Single-flight sync-runtime bootstrap. `fetchServerIdentity` + `hydrate` +
 * `attachSseBridge` are one-time setup, but they can be reached concurrently —
 * two rapid navigations during cold start (or right after a workspace switch)
 * both hit the auth guard before `state.handle` is set. A shared promise makes
 * every caller await ONE run, so `hydrate()`'s IndexedDB open never races itself
 * and the schema round-trip happens once, not per navigation. Cleared by
 * `tearDown()` so it re-arms after a switch / logout.
 */
let runtimePromise: Promise<void> | null = null

export function ensureSyncRuntime(
  userUuid: string,
  workspaceSlug: string | null,
): Promise<void> {
  // A runtime open for another workspace is torn down first. The workspace
  // guard normally does this before the slug changes; this is the backstop
  // that keeps one workspace's rows from ever serving under another's slug.
  if (runtimePromise && state.openArgs && state.openArgs.workspaceSlug !== workspaceSlug) {
    logger.warn('sync runtime open for another workspace; re-bootstrapping', {
      from: state.openArgs.workspaceSlug,
      to: workspaceSlug,
    })
    let rebootstrap: Promise<void> | null = null
    rebootstrap = (async () => {
      // The live-updates stream, its pending retry and its cached token are
      // the other workspace's too; the new runtime opens its own.
      const { detachSseBridge } = await import('@/sync/sseBridge')
      detachSseBridge()
      await tearDown() // clears runtimePromise; restore it so callers share this run
      // A collab token is bound to the workspace it was minted in, and so is
      // a file token.
      const { resetCollabToken } = await import('@/services/collabToken')
      resetCollabToken()
      const { resetFileToken } = await import('@/services/fileToken')
      resetFileToken()
      runtimePromise = rebootstrap
      await bootstrapRuntime(userUuid, workspaceSlug)
    })().catch((e) => {
      runtimePromise = null
      logger.warn('Failed to re-bootstrap sync runtime', { error: e })
    })
    runtimePromise = rebootstrap
    return rebootstrap
  }
  if (!runtimePromise) {
    runtimePromise = bootstrapRuntime(userUuid, workspaceSlug).catch((e) => {
      runtimePromise = null // let a later navigation retry after a failure
      logger.warn('Failed to bootstrap sync runtime', { error: e })
    })
  }
  return runtimePromise
}

async function bootstrapRuntime(
  userUuid: string,
  workspaceSlug: string | null,
): Promise<void> {
  const { schemaHash, instanceId, workspaceEpoch } = await fetchServerIdentity()
  await hydrate(userUuid, schemaHash, instanceId, workspaceSlug, workspaceEpoch)
  const { attachSseBridge } = await import('@/sync/sseBridge')
  attachSseBridge()
  // Have a collab token ready, so opening a note connects without first
  // waiting for one.
  const { keepCollabTokenWarm } = await import('@/services/collabToken')
  keepCollabTokenWarm(workspaceSlug)
}

const POLL_INTERVAL_MS = 10_000

/** Warm the engine for an authenticated user. Idempotent —
 * subsequent calls are no-ops while the handle is live (or while
 * memory-only mode is active). */
export async function hydrate(
  userUuid: string,
  schemaHash: string,
  instanceId = '',
  workspaceSlug: string | null = null,
  workspaceEpoch = '',
): Promise<void> {
  if (state.handle || state.memoryOnly) return
  pool.setSchemaHash(schemaHash)
  state.schemaHash = schemaHash
  state.openArgs = { userUuid, workspaceSlug }
  state.instanceId = instanceId
  setReferenceFetcher(referenceFetcher)

  // Try to open IDB; degrade gracefully if it's unavailable
  // (private browsing, quota, blocked, deleted-while-open). The
  // runtime stays usable — bootstrap still happens, the pool
  // still holds rows, optimistic writes still apply — just
  // without warm-start persistence.
  try {
    state.handle = await idb.open(userUuid, schemaHash, workspaceSlug)
  } catch (e) {
    logger.warn('IndexedDB unavailable; degrading sync engine to memory-only', { error: e })
    state.memoryOnly = true
    state.workspaceEpoch = workspaceEpoch
    queue.setIdbHandle(null)
    startDeltaPollFallback()
    // Memory-only is still a hydrated runtime: drain any group that subscribed
    // while IDB open was in flight so it bootstraps rather than staying empty.
    bootstrapDeferredGroups()
    return
  }

  queue.setIdbHandle(state.handle)

  const persistedHash = await idb.getSchemaHash(state.handle)
  const persistedInstance = await idb.getInstanceId(state.handle)
  const persistedEpoch = await idb.getWorkspaceEpoch(state.handle)

  // Two reasons to throw away the local cache and re-bootstrap clean:
  //   - schema mismatch: cached rows may not fit the new schema.
  //   - instance mismatch (epoch fence): the cached data belongs to a
  //     different database generation. We only trust an instance id the
  //     server actually reported (non-empty), and an absent persisted
  //     id counts as a mismatch so the first load after this ships
  //     clears caches written before instance ids existed.
  //   - workspace epoch mismatch (data fence): this workspace's data was
  //     replaced (an archive import), so every cached row has an id that is
  //     gone or now names something else. Same trust rule: only an epoch the
  //     server reported counts, and an absent persisted one is a mismatch
  //     (the server has none until the first replacement).
  const schemaChanged = !!persistedHash && persistedHash !== schemaHash
  const instanceChanged = instanceId !== '' && persistedInstance !== instanceId
  const epochChanged = workspaceEpoch !== '' && persistedEpoch !== workspaceEpoch

  if (schemaChanged || instanceChanged || epochChanged) {
    logger.info('Sync cache stale; wiping local cache and rebootstrapping', {
      schemaChanged,
      instanceChanged,
      epochChanged,
      previousInstance: persistedInstance,
      currentInstance: instanceId,
    })
    const oldName = state.handle.name
    state.handle.db.close()
    await idb.wipe(oldName)
    state.handle = await idb.open(userUuid, schemaHash, workspaceSlug)
    queue.setIdbHandle(state.handle)

    // The collaborative-document caches (separate y-indexeddb
    // databases) aren't covered by the sync-pool wipe above, and on an
    // instance change they're exactly the stale data that resurrects
    // notes onto recycled ids. Purge them too, other tabs' open ones
    // included: they are the same stale generation. A replaced workspace
    // recycles ids the same way.
    if (instanceChanged || epochChanged) {
      await purgeAllCollabDocs({ includeOpen: true })
    }
  }
  // A failed /sync/schema reports no epoch: keep the one the cache was
  // written under rather than forgetting it.
  state.workspaceEpoch = workspaceEpoch !== '' ? workspaceEpoch : (persistedEpoch ?? '')

  // Rehydrate the in-memory pool from cached rows. Rows whose
  // schema_version is stale are filtered out by `loadModels`. A row cached
  // without owners (an older build wrote it) is owned `legacy`; `local`
  // belonged to the session that cached it.
  //
  // A snapshot-backed row keeps only the owners whose snapshot the cursor
  // still tracks (see ownersOnHydrate), and goes when none is left: its
  // groups' watermarks were pruned, so a delta would never bring it current.
  // Other aggregates (`cycle`) keep their rows as they are.
  const rows = await idb.loadModels(state.handle, SCHEMA_VERSIONS)
  const watermarks = (await idb.getGroupWatermarks(state.handle)) ?? {}
  const legacy = [pool.LEGACY_OWNER]
  const dropped: Array<[SyncAggregate, string]> = []
  for (const r of rows) {
    let owners = r.owners ? r.owners.filter((o) => o !== pool.LOCAL_OWNER) : legacy
    if (SNAPSHOT_BACKED.has(r.aggregate)) {
      owners = ownersOnHydrate(owners, watermarks)
      if (owners.length === 0) {
        dropped.push([r.aggregate, r.id])
        continue
      }
    }
    pool.upsert(r.aggregate, r.id, r.data, owners)
  }
  if (dropped.length > 0) {
    logger.info('sync hydrate: dropped cached rows no watermarked group owns', {
      dropped: dropped.length,
    })
    await idb.deleteModels(state.handle, dropped)
  }

  // Seed the composite cursor from the cache. A cache written before
  // the commit-safe cursor shipped has no xid8 (defaults to 0); that's
  // fine — pullDelta omits `from_xid8` while xid8 is 0 and the bootstrap
  // that follows every subscribe reseeds a real xid8.
  const lastCachedSyncId = (await idb.getLastSyncId(state.handle)) ?? 0
  const lastCachedXid8 = (await idb.getLastXid8(state.handle)) ?? 0
  pool.setCursor(lastCachedXid8, lastCachedSyncId)

  // The max age the server sent last session judges this launch's
  // watermarks; this session's first bootstrap or delta replaces it.
  const storedMaxAge = await idb.getSnapshotMaxAge(state.handle)
  if (typeof storedMaxAge === 'number' && state.snapshotMaxAgeSecs === null) {
    state.snapshotMaxAgeSecs = storedMaxAge
  }

  // Persist hash + instance id for the next warm start so the next
  // boot can detect a schema or database-generation change.
  await idb.setSchemaHash(state.handle, schemaHash)
  if (instanceId !== '') {
    await idb.setInstanceId(state.handle, instanceId)
  }
  if (state.workspaceEpoch !== '') {
    await idb.setWorkspaceEpoch(state.handle, state.workspaceEpoch)
  }

  // Replay any persisted optimistic transactions (a previous tab
  // crash between persist and flush would have left them in the
  // store).
  void queue.flush()

  // Start the periodic delta-poll fallback. The SSE bridge will
  // also push updates; the poll is belt-and-braces for cases
  // where SSE is wedged behind a corporate proxy or the EventSource
  // is mid-reconnect. Idempotent — only starts a single timer.
  startDeltaPollFallback()
  watchSnapshotAge()

  // Drain groups that subscribed before hydrate() finished (see subscribe()).
  bootstrapDeferredGroups()
}

/**
 * Take a group's snapshot again once it grows past the max age, while the
 * app runs: when the tab becomes visible, and on the poll while it is. A
 * delta never removes a row whose removal it missed (an access change, a
 * cascade with no event); a snapshot's strip does. Runs the `keepCursor` path
 * (the session is caught up), so the cursor holds and a delta replays what
 * the snapshot's horizon passed.
 */
/** The refresh running now, if any. A token rather than a flag, so a
 * tear-down can let the next workspace refresh while this one winds down. */
let refreshRun: object | null = null

/** When each group was last snapshotted again for its age (performance.now).
 * A snapshot that failed waits this long before the next try instead of
 * every poll. */
const staleRefreshTriedAt = new Map<string, number>()
const STALE_REFRESH_RETRY_MS = 10 * 60_000

/** Every client crossing the max age together (a server restart lowering
 * it) would snapshot the workspace in the same poll; each waits a random
 * while first. */
function staleRefreshJitterMs(): number {
  return 2_000 + Math.random() * 28_000
}

/** Subscribed groups this session's snapshot of is stale, leaving out those
 * a bootstrap is already pending for and those tried too recently. */
function staleGroupsToRefresh(groups: readonly string[], now: number): string[] {
  return staleWatermarkedGroups(groups).filter((g) => {
    if (state.bootstrapping.has(g)) return false
    const tried = staleRefreshTriedAt.get(g)
    return tried === undefined || now - tried >= STALE_REFRESH_RETRY_MS
  })
}

export async function refreshStaleSnapshots(jitterMs = staleRefreshJitterMs()): Promise<void> {
  if (refreshRun || state.resyncing || !state.caughtUp || !canFetchWorkspace()) return
  if (staleGroupsToRefresh(Array.from(pool.getSubscribedGroups()), performance.now()).length === 0) {
    return
  }
  const run = {}
  refreshRun = run
  const epoch = pool.currentEpoch()
  try {
    if (jitterMs > 0) await new Promise((resolve) => setTimeout(resolve, jitterMs))
    // Torn down, wiping, or no longer caught up while it waited.
    if (pool.currentEpoch() !== epoch || state.resyncing || refreshRun !== run) return
    if (!state.caughtUp || !canFetchWorkspace()) return
    // Read with the groups, before any await (see runBootstrap's `stillOurs`).
    const subscribedAt = pool.currentSubscriptionTick()
    const now = performance.now()
    const stale = staleGroupsToRefresh(Array.from(pool.getSubscribedGroups()), now)
    if (stale.length === 0) return
    for (const g of stale) staleRefreshTriedAt.set(g, now)
    logger.info('sync: snapshots past their max age; taking them again', { groups: stale })
    await runBootstrap(stale, subscribedAt)
  } catch (error) {
    logger.warn('sync: taking stale snapshots again failed', { error })
  } finally {
    if (refreshRun === run) refreshRun = null
  }
}

function onVisibilityChange(): void {
  if (document.visibilityState === 'visible') void refreshStaleSnapshots()
}

function watchSnapshotAge(): void {
  if (typeof document === 'undefined') return
  document.removeEventListener('visibilitychange', onVisibilityChange)
  document.addEventListener('visibilitychange', onVisibilityChange)
}

function unwatchSnapshotAge(): void {
  if (typeof document === 'undefined') return
  document.removeEventListener('visibilitychange', onVisibilityChange)
}

/** Fetch the server's compiled schema hash, database instance id and
 * the workspace's data epoch. The schema hash names the IndexedDB; the
 * instance id is the epoch fence (a change means a different database
 * generation); the workspace epoch fences this workspace's data (a change
 * means it was replaced). `instanceId` and `workspaceEpoch` fall back to
 * `''` so a fetch/DB hiccup never triggers a wipe. */
export async function fetchServerIdentity(): Promise<{
  schemaHash: string
  instanceId: string
  workspaceEpoch: string
}> {
  const unknown = { schemaHash: 'unknown', instanceId: '', workspaceEpoch: '' }
  try {
    const res = await authFetch('/sync/schema')
    if (!res.ok) return unknown
    const body = (await res.json()) as {
      server_schema?: string
      instance_id?: string
      workspace_epoch?: string | null
    }
    return {
      schemaHash: body.server_schema ?? 'unknown',
      instanceId: body.instance_id ?? '',
      workspaceEpoch: body.workspace_epoch ?? '',
    }
  } catch (e) {
    logger.warn('Failed to fetch /sync/schema; falling back to "unknown"', { error: e })
    return unknown
  }
}

/**
 * Subscribe to a sync group. If the group isn't already in the pool, brings
 * it current: a delta catch-up when its snapshot is warm-cached from a prior
 * session, else a full per-group bootstrap. Idempotent — calling twice with
 * the same group runs no extra work.
 */
export async function subscribe(group: string): Promise<void> {
  if (pool.getSubscribedGroups().has(group)) return
  pool.subscribe(group)
  if (!canFetchWorkspace()) {
    // Runtime not hydrated, or no workspace selected yet: the group stays in the
    // pool set and its bootstrap is drained by bootstrapDeferredGroups once
    // hydrate finishes or the workspace becomes ready (see the watch above).
    logger.warn('subscribe() deferred: runtime not ready', { group })
    return
  }
  await bringGroupsCurrent([group])
}

/**
 * Unsubscribe from a sync group. It leaves the group list every delta sends,
 * its watermark goes, and it comes off the owners of every pooled row; a row
 * it owned alone leaves the pool, and a row another group still owns stays.
 * The cached copy of a row it owned alone goes too, when the cache says the
 * group alone owns it; other cached rows are left for the next hydrate, since
 * other tabs share the cache (see stripGroupsAndCache). A bootstrap for the
 * group still in flight stops stamping it and neither strips nor watermarks
 * it when it ends (see runBootstrap). The workspace group is never
 * unsubscribed: it backs the lists for the session.
 */
export async function unsubscribe(group: string): Promise<void> {
  if (isWorkspaceGroup(group)) {
    logger.warn('unsubscribe() ignored for the workspace group', { group })
    return
  }
  if (!pool.unsubscribe(group)) return
  const handle = state.handle
  // Both issued before any await, so a quick re-subscribe's watermark and
  // snapshot rows are written after these, not undone by them.
  const { cached } = stripGroupsAndCache(handle, new Set([group]))
  if (!handle) return
  try {
    await Promise.all([dropGroupWatermarks(handle, [group]), cached])
  } catch (error) {
    logger.warn('sync: caching an unsubscribe failed', { group, error })
  }
}

/**
 * Pull a delta from the server for the currently-subscribed groups.
 * Called on warm start (after rehydrate), on SSE reconnect, and on
 * a periodic-poll fallback when SSE is wedged.
 */
/**
 * Wipe the local cache and re-bootstrap. Two reasons:
 *
 * - The server reports `resync_required`: our cursor has aged past
 *   `SYNC_ACTIONS_RETENTION_DAYS`, so the deletes we missed have been pruned
 *   and no delta can deliver them, or the workspace's data was replaced after
 *   the cursor was issued.
 * - The server reports a new workspace data epoch (`newWorkspaceEpoch`): the
 *   workspace's data was replaced (an archive import), so every cached row
 *   carries an id that is gone or now names something else.
 *
 * Bootstrap alone cannot repair this: the stream only upserts, so a row deleted
 * while we were away is never removed, it just stops being mentioned. Only
 * dropping the cache first clears those phantoms.
 *
 * Collaborative-document caches are purged only for a new workspace epoch.
 * After a retention gap they are still right: this is our cursor being old,
 * not the documents being different, and purging them would cost every open
 * document a re-download for no correctness gain. After a replacement they are
 * the same stale generation as the rows, keyed by ids that now belong to other
 * documents.
 */
async function resyncFromScratch(newWorkspaceEpoch?: string): Promise<void> {
  if (state.resyncing) return
  state.resyncing = true
  try {
    if (newWorkspaceEpoch) {
      logger.warn('sync: workspace data replaced; wiping every local cache and re-bootstrapping', {
        previousEpoch: state.workspaceEpoch,
        currentEpoch: newWorkspaceEpoch,
      })
    } else {
      logger.warn('sync: cursor predates the server feed; wiping and re-bootstrapping')
    }

    if (state.handle && state.openArgs) {
      const name = state.handle.name
      const { userUuid, workspaceSlug } = state.openArgs
      state.handle.db.close()
      state.handle = null
      queue.setIdbHandle(null)
      await idb.wipe(name)
      resetSessionMarks()
      state.handle = await idb.open(userUuid, state.schemaHash, workspaceSlug)
      queue.setIdbHandle(state.handle)
    }

    if (newWorkspaceEpoch) {
      await purgeAllCollabDocs({ includeOpen: true })
      // Recorded once the old caches are gone, so a later poll doesn't start
      // a second wipe, and a crash before here wipes again on the next launch.
      state.workspaceEpoch = newWorkspaceEpoch
    }
    // The reopened database is empty: write back what the next launch
    // compares against, or it would see none and wipe again.
    if (state.handle) {
      await idb.setSchemaHash(state.handle, state.schemaHash)
      if (state.instanceId !== '') await idb.setInstanceId(state.handle, state.instanceId)
      if (state.workspaceEpoch !== '') {
        await idb.setWorkspaceEpoch(state.handle, state.workspaceEpoch)
      }
      if (state.snapshotMaxAgeSecs !== null) {
        await idb.setSnapshotMaxAge(state.handle, state.snapshotMaxAgeSecs)
      }
    }

    // The groups to bring back, read right before reset() drops them and with
    // no await between: a view may have let a group go, or taken one, during
    // the wipe. Re-subscribing is what makes the bootstrap fetch them.
    const groups = Array.from(pool.getSubscribedGroups())
    // reset() clears rows, cursor, subscriptions AND the schema hash. Restore
    // the hash: runBootstrap compares the server's against it and would
    // otherwise warn on a mismatch against an empty string every resync.
    pool.reset()
    // Bootstraps still streaming belong to the old pool and stop at its
    // epoch; their holds must not keep the cursor back from this one.
    state.cursorHolds = new Set()
    state.bootstrapping = new Map()
    state.notifiedAhead = new Set()
    pool.setSchemaHash(state.schemaHash)
    for (const g of groups) pool.subscribe(g)
    if (groups.length > 0) await runBootstrap(groups)
  } catch (e) {
    // Leave `resyncing` cleared so a later poll can retry. The cache may be
    // half-wiped, which is still better than keeping known-phantom rows.
    logger.error('sync: resync failed; will retry on a later poll', { error: e })
  } finally {
    state.resyncing = false
  }
}

export async function pullDelta(): Promise<boolean> {
  if (!canFetchWorkspace()) return false
  // A `keepCursor` bootstrap is streaming and the cursor holds for it; the
  // delta that closes it replays from there (see LifecycleState.cursorHolds).
  if (cursorHeld()) return false
  const groups = Array.from(pool.getSubscribedGroups())
  if (groups.length === 0) return false
  const from = pool.getLastSyncId()
  const fromXid8 = pool.getLastXid8()
  // Send the commit-safe `from_xid8` only once a real xid8 has seeded
  // the cursor. Until then (fresh client, or a pre-upgrade cache) omit
  // it so the server serves the legacy horizon-gated catch-up rather
  // than re-streaming all of history from xid8 0.
  let url = `/sync/delta?from=${from}&groups=${encodeURIComponent(groups.join(','))}`
  if (fromXid8 > 0) url += `&from_xid8=${fromXid8}`
  const epoch = pool.currentEpoch()
  try {
    const res = await authFetch(url)
    if (!res.ok) {
      logger.warn('sync delta failed', { status: res.status })
      return false
    }
    const body = (await res.json()) as DeltaResponse
    // The workspace was torn down while this was in flight.
    if (pool.currentEpoch() !== epoch) return false
    // A `keepCursor` bootstrap began meanwhile: this page could move the
    // cursor past an event its rows then overwrite. Its closing delta reads
    // the span again.
    if (cursorHeld()) return false
    // The workspace's data was replaced. Checked before `resync_required`
    // and independently of it: SSE frames may already have moved the cursor
    // past the server's resync floor, and only this wipe also clears the
    // collaborative-document caches.
    const workspaceEpoch = body.workspace_epoch ?? ''
    if (workspaceEpoch !== '' && workspaceEpoch !== state.workspaceEpoch) {
      await resyncFromScratch(workspaceEpoch)
      return false
    }
    if (body.resync_required) {
      // Return without applying: this page is a partial view of a span we
      // cannot complete, and the re-bootstrap replaces it wholesale.
      await resyncFromScratch()
      return false
    }
    // Every delta carries current capability flags (absent only from an
    // older backend or a failed server-side probe, where keeping the
    // current flags is right). This is what keeps feature chrome correct
    // on a warm launch that never re-streams the bootstrap `__meta__`.
    if (body.capabilities) {
      applyWorkspaceCapabilities(body.capabilities)
    }
    noteSnapshotMaxAge(body.snapshot_max_age_secs)
    applyActions(body.actions)
    // Torn-cache prune must land before the advanced cursor is persisted
    // (see pruneTornWatermarks).
    await pruneTornWatermarks()
    if (pool.currentEpoch() !== epoch) return false
    setCaughtUp(true)
    pool.setCursor(body.last_xid8, body.last_sync_id)
    if (state.handle) {
      await idb.setLastSyncId(state.handle, body.last_sync_id)
      await idb.setLastXid8(state.handle, body.last_xid8)
    }
    // Same observer fan-out as the SSE path, so imperative consumers
    // (useSyncActions) stay live even when SSE is wedged and this 10s
    // poll is the delivery path. Events SSE already told them about while
    // the cursor stayed behind are applied again here but not told twice.
    const ahead = state.notifiedAhead
    notifySyncActions(
      ahead.size > 0 ? body.actions.filter((a) => !ahead.has(a.sync_id)) : body.actions,
    )
    if (!body.has_more && !cursorHeld()) ahead.clear()
    // After the cursor is stored: bringing assets back re-reads the snapshot,
    // which moves the cursor on itself.
    await followAssetVisibility(body.capabilities?.assets_visible, 'delta')
    return body.has_more
  } catch (e) {
    logger.warn('sync delta network error', { error: e })
    return false
  }
}

/**
 * Fetch and stream a bootstrap for the given groups, applying each
 * row to the pool as it arrives. Used both on cold start (with the
 * full subscription list) and for incremental group expansion.
 * `subscribedAt`: the subscription tick the caller read the groups at, when
 * it awaited something since (see `stillOurs` below); defaults to now.
 */
export async function runBootstrap(
  groups: string[],
  subscribedAt?: number,
  holdCursor = false,
): Promise<void> {
  // Needs a hydrated runtime AND a selected workspace (not an IDB handle). In
  // memory-only mode the persistence writes below are individually skipped, but
  // the fetch + pool.upsert still run so views populate.
  if (!canFetchWorkspace()) return
  // A group added to a session that is caught up (opening a ticket) leaves
  // the cursor where it is. Its snapshot ends at the server's horizon, which
  // can be past events for the other groups that nothing has delivered yet;
  // the delta after it replays that span for every group instead.
  // `holdCursor` asks the same of a launch whose cached groups a delta
  // could not yet bring current: the cursor must not jump past their events.
  const keepCursor = (state.caughtUp || holdCursor) && pool.getLastXid8() > 0
  // A group unsubscribed after `since`, even if subscribed again since, is no
  // longer this bootstrap's: it is not requested, its rows stop being stamped
  // with it, and `__end__` neither strips nor watermarks it. A re-subscribe
  // runs its own bootstrap.
  const since = subscribedAt ?? pool.currentSubscriptionTick()
  const stillOurs = (g: string) => !pool.unsubscribedSince(g, since)
  groups = groups.filter(stillOurs)
  if (groups.length === 0) return
  // While it streams, live events apply their rows but leave the cursor
  // where it is (see holdCursorWhile): the delta after it then replays them
  // over the snapshot's rows, which may be older.
  await whileBootstrapping(groups, () =>
    keepCursor
      ? holdCursorWhile(() => streamBootstrap(groups, keepCursor, since))
      : streamBootstrap(groups, keepCursor, since),
  )
}

/** How long a bootstrap may go without a byte before it is given up. A
 * socket that died quietly (a laptop waking) never ends on its own, and the
 * cursor would stay held behind it for the rest of the session. */
const BOOTSTRAP_IDLE_MS = 60_000

/** An abort signal that fires after `ms` with no `touch()`, and a hook run
 * when it does. */
function idleWatch(ms: number) {
  const controller = new AbortController()
  let onStall: () => void = () => {}
  let timer: ReturnType<typeof setTimeout> | undefined
  const touch = () => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      controller.abort()
      onStall()
    }, ms)
  }
  touch()
  return {
    signal: controller.signal,
    touch,
    stalled: () => controller.signal.aborted,
    onStall: (run: () => void) => {
      onStall = run
    },
    stop: () => clearTimeout(timer),
  }
}

/**
 * Stream one bootstrap (see readBootstrap), giving it up when it goes
 * BOOTSTRAP_IDLE_MS without a byte: it then ends unfinished, as a failed
 * fetch does.
 */
async function streamBootstrap(
  groups: string[],
  keepCursor: boolean,
  since: number,
): Promise<boolean> {
  const idle = idleWatch(BOOTSTRAP_IDLE_MS)
  try {
    return await readBootstrap(groups, keepCursor, since, idle)
  } finally {
    idle.stop()
  }
}

/**
 * A group this bootstrap asked for that the server no longer grants (access
 * taken away, or the ticket gone) has no snapshot behind its rows: drop its
 * watermark and strip it, with its cached rows where the cache says it alone
 * owns them. Otherwise its stale watermark and rows would outlive it. Only
 * when the server sent a granted list at all, and only for groups `ours`
 * keeps: not one let go meanwhile, nor one a newer snapshot has granted.
 */
async function dropDeniedGroups(
  requested: readonly string[],
  granted: readonly string[] | undefined,
  ours: (group: string) => boolean,
): Promise<void> {
  if (!Array.isArray(granted)) return
  const denied = requested.filter((g) => !granted.includes(g) && ours(g))
  if (denied.length === 0) return
  logger.info('sync: groups no longer granted; dropping their snapshot', { groups: denied })
  const handle = state.handle
  const { cached } = stripGroupsAndCache(handle, new Set(denied))
  try {
    await Promise.all([handle ? dropGroupWatermarks(handle, denied) : undefined, cached])
  } catch (error) {
    logger.warn('sync: dropping groups no longer granted failed', { error })
  }
}

/**
 * Read one bootstrap into the pool and the cache, and at its end strip,
 * watermark and (unless `keepCursor`) move the cursor. `since`: see
 * runBootstrap. True when it reached its end in this workspace.
 */
async function readBootstrap(
  groups: string[],
  keepCursor: boolean,
  since: number,
  idle: ReturnType<typeof idleWatch>,
): Promise<boolean> {
  const stillOurs = (g: string) => !pool.unsubscribedSince(g, since)
  // A watermark this session records after now belongs to a newer snapshot.
  const recordTick = currentRecordTick()
  // Rows a live action writes from here on are newer than the snapshot may
  // be, so the strip at `__end__` leaves them alone.
  const startedAt = pool.currentTouch()
  const url = `/sync/bootstrap?groups=${encodeURIComponent(groups.join(','))}&schema=${encodeURIComponent(state.schemaHash)}`
  const epoch = pool.currentEpoch()
  let res: Response
  try {
    res = await authFetch(url, { signal: idle.signal })
  } catch (e) {
    if (idle.stalled()) logger.warn('sync bootstrap stalled before it began; giving it up', { groups })
    else logger.error('sync bootstrap network error', { error: e })
    return false
  }
  if (!res.ok || !res.body) {
    logger.error('sync bootstrap http error', { status: res.status })
    return false
  }

  const reader = res.body.getReader()
  idle.onStall(() => {
    logger.warn('sync bootstrap stalled; giving it up', { groups })
    void reader.cancel().catch(() => {})
  })
  idle.touch()
  let reachedEnd = false
  const decoder = new TextDecoder()
  let buffer = ''
  let bootstrapMeta: BootstrapMeta | null = null
  // Ids this snapshot carried, per aggregate, for the strip on `__end__`
  // (see reconcileByOwner).
  const seen = new Map<SyncAggregate, Set<string>>()
  // What this snapshot's rows are owned by: the groups the server granted,
  // set when `__meta__` arrives, less those unsubscribed since the start. One
  // set shared by every row it creates.
  let snapshotOwners: ReadonlySet<string> = new Set(groups)
  let ownersTick = since
  // A workspace snapshot replaces `legacy` on the rows it carries.
  let stripsLegacy = false
  // Rows to cache, written as the pool holds them at flush time: an
  // unsubscribe while they waited may have taken a group off them, or removed
  // them, and a live action may have written them.
  const persistBatch: Array<[SyncAggregate, string]> = []
  const flushPersistBatch = async () => {
    if (state.handle && persistBatch.length > 0) {
      const rows: idb.ModelRow[] = []
      for (const [aggregate, id] of persistBatch.splice(0, persistBatch.length)) {
        const row = pool.get<Record<string, unknown>>(aggregate, id)
        if (!row) continue
        rows.push({
          aggregate,
          id,
          schema_version: SCHEMA_VERSIONS[aggregate]!,
          data: { ...row },
          owners: cachedOwners(aggregate, id),
        })
      }
      if (rows.length > 0) await idb.putModels(state.handle, rows)
    }
  }

  while (true) {
    let chunk: ReadableStreamReadResult<Uint8Array>
    try {
      chunk = await reader.read()
    } catch (e) {
      // Aborted (see idleWatch) or the connection dropped: unfinished.
      if (!idle.stalled()) logger.warn('sync bootstrap stream failed', { error: e })
      return false
    }
    const { done, value } = chunk
    if (idle.stalled()) return false
    idle.touch()
    // The workspace was torn down mid-stream: the rest is not ours to apply.
    if (pool.currentEpoch() !== epoch) {
      void reader.cancel().catch(() => {})
      return false
    }
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    let nl: number
    while ((nl = buffer.indexOf('\n')) !== -1) {
      const raw = buffer.slice(0, nl).trim()
      buffer = buffer.slice(nl + 1)
      if (raw.length === 0) continue
      let line: BootstrapLine
      try {
        line = JSON.parse(raw) as BootstrapLine
      } catch (e) {
        logger.warn('bootstrap line failed to parse; dropping', { raw, error: e })
        continue
      }
      if ('__meta__' in line) {
        const meta = line.__meta__ as BootstrapMeta
        bootstrapMeta = meta
        const granted = meta.groups_granted ?? []
        snapshotOwners = new Set(granted.filter(stillOurs))
        ownersTick = pool.currentSubscriptionTick()
        // Unsubscribed from every group it grants while the request was out.
        if (granted.length > 0 && snapshotOwners.size === 0) {
          void reader.cancel().catch(() => {})
          return false
        }
        stripsLegacy = [...snapshotOwners].some(isWorkspaceGroup)
        if (meta.server_schema !== state.schemaHash) {
          logger.warn('bootstrap meta carries a different schema_hash than client', {
            server: meta.server_schema,
            client: state.schemaHash,
          })
        }
        // Surface workspace-level capability flags into the
        // shared composable so any UI component can read them.
        applyWorkspaceCapabilities(meta)
        noteSnapshotMaxAge(meta.snapshot_max_age_secs)
        await followAssetVisibility(
          meta.assets_visible,
          groups.some((g) => g.startsWith('workspace:')) ? 'workspace-snapshot' : 'other-snapshot',
        )
      } else if ('__model__' in line) {
        if (ownersTick !== pool.currentSubscriptionTick()) {
          ownersTick = pool.currentSubscriptionTick()
          const before = snapshotOwners.size
          snapshotOwners = new Set([...snapshotOwners].filter(stillOurs))
          // Every group this snapshot was for has been unsubscribed: the rest
          // of it belongs to no one.
          // Its rows still waiting to be cached are dropped: those another
          // group owns are cached as that group's stream left them.
          if (before > 0 && snapshotOwners.size === 0) {
            void reader.cancel().catch(() => {})
            return false
          }
        }
        const { __model__: aggregate, ...payload } = line
        const id = rowKey(aggregate, payload as Record<string, unknown>)
        if (id == null) {
          logger.warn('bootstrap row missing key; dropping', { aggregate, payload })
          continue
        }
        pool.upsert(aggregate, id, payload as Record<string, unknown>, snapshotOwners)
        if (stripsLegacy) pool.stripOwner(aggregate, id, pool.LEGACY_OWNER)
        let ids = seen.get(aggregate)
        if (!ids) seen.set(aggregate, (ids = new Set()))
        ids.add(String(id))
        if (SCHEMA_VERSIONS[aggregate] != null) {
          persistBatch.push([aggregate, String(id)])
          if (persistBatch.length >= 200) await flushPersistBatch()
        }
      } else if ('__end__' in line) {
        // Bootstrap finished successfully — persist any tail and
        // advance the cursor.
        await flushPersistBatch()
        if (bootstrapMeta) {
          reachedEnd = true
          // The end line advances the cursor to now, which tears any cached
          // group not currently subscribed; prune before persisting. The prune
          // also strips those groups from owners, and runs before the strip
          // below so a row they shared with a granted group goes in this pass.
          // A `keepCursor` run comes after a delta that already pruned, so this
          // is a no-op there.
          await pruneTornWatermarks()
          await reconcileByOwner(
            (bootstrapMeta.groups_granted ?? []).filter(stillOurs),
            seen,
            startedAt,
          )
          await dropDeniedGroups(
            groups,
            bootstrapMeta.groups_granted,
            (g) => stillOurs(g) && !recordedSince(g, recordTick),
          )
          if (!keepCursor) {
            setCaughtUp(true)
            // Advance the in-memory cursor always; persist it only when IDB is
            // available (memory-only keeps the cursor in the pool for delta polls).
            pool.setCursor(bootstrapMeta.last_xid8, bootstrapMeta.last_sync_id)
          }
          if (state.handle) {
            if (!keepCursor) {
              await idb.setLastSyncId(state.handle, bootstrapMeta.last_sync_id)
              await idb.setLastXid8(state.handle, bootstrapMeta.last_xid8)
            }
            // Watermark the snapshots the server actually granted (not the
            // request list: a requested-but-denied group must not gain a
            // watermark or the next launch would warm-path it into
            // emptiness) + stamp the aggregate versions they were written
            // under, so the next launch can bring these groups current
            // with a delta instead of re-streaming the snapshot. A group
            // unsubscribed meanwhile gets none. Nothing awaits between this
            // check and queuing the write, so an unsubscribe either came
            // first and is filtered here, or queues its drop after the write.
            const granted = bootstrapMeta.groups_granted ?? []
            const completed = groups.filter((g) => granted.includes(g) && stillOurs(g))
            await recordGroupWatermarks(state.handle, completed, bootstrapMeta.last_xid8)
            await idb.setModelVersionsTag(state.handle, MODEL_VERSIONS_TAG)
          }
        }
      } else if ('__error__' in line) {
        logger.error('bootstrap streamed error envelope', { line })
      }
    }
  }
  await flushPersistBatch()
  return reachedEnd && pool.currentEpoch() === epoch
}

/** Tear-down: release the IDB handle, reset the pool, drop subs. */
export async function tearDown(): Promise<void> {
  stopDeltaPollFallback()
  unwatchSnapshotAge()
  refreshRun = null
  staleRefreshTriedAt.clear()
  resetSessionMarks()
  // A pending grace unsubscribe belongs to this workspace; it must not fire
  // into the next one, where the same group name means another ticket.
  cancelGroupReleases()
  if (state.handle) {
    state.handle.db.close()
    state.handle = null
  }
  queue.setIdbHandle(null)
  setReferenceFetcher(null)
  pool.reset()
  state.schemaHash = ''
  state.memoryOnly = false
  state.openArgs = null
  state.resyncing = false
  state.instanceId = ''
  state.workspaceEpoch = ''
  state.snapshotMaxAgeSecs = null
  state.cursorHolds = new Set()
  state.bootstrapping = new Map()
  state.notifiedAhead = new Set()
  setCaughtUp(false)
  resetWatermarkPrune()
  // Re-arm the single-flight bootstrap so the next navigation rebuilds the
  // runtime for the new session / workspace.
  runtimePromise = null
}

/** Periodic delta poll. Single shared timer driven by setInterval —
 * idempotent start, idempotent stop. The architecture doc places
 * this at 10s as a fallback when SSE is wedged; SSE-driven
 * applySseFrame races with the poll harmlessly because both paths
 * funnel through `applyActions` which is upsert-by-id. */
function startDeltaPollFallback(): void {
  if (state.pollTimer) return
  state.pollTimer = setInterval(() => {
    void pullDelta()
    // A tab that stays visible (a wallboard) never sees a visibility change.
    if (typeof document !== 'undefined' && document.visibilityState === 'visible') {
      void refreshStaleSnapshots()
    }
  }, POLL_INTERVAL_MS)
}

function stopDeltaPollFallback(): void {
  if (state.pollTimer) {
    clearInterval(state.pollTimer)
    state.pollTimer = null
  }
}
