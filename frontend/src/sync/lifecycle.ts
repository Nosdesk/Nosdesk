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
import { canFetchWorkspace, isHydrated, setCaughtUp, state } from './runtimeState'
import { MODEL_VERSIONS_TAG, SCHEMA_VERSIONS } from './schemaVersions'
import { partitionWarmGroups, pruneTornWatermarks, resetWatermarkPrune } from './warm'
import { followAssetVisibility, reconcileWithSnapshot } from './reconcile'
import { applyActions, rowKey } from './apply'
import { referenceFetcher } from './referenceFetchers'
import type {
  BootstrapLine,
  BootstrapMeta,
  DeltaResponse,
  SyncAggregate,
} from '@nosdesk/core/sync/types'

export { caughtUp } from './runtimeState'
export { followAssetVisibility, reconcileWithSnapshot } from './reconcile'
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
  const { warm, cold } = await partitionWarmGroups(groups)
  if (warm.length > 0) {
    logger.info('sync warm launch: delta catch-up instead of snapshot re-stream', {
      groups: warm,
    })
    await catchUpViaDelta()
  }
  if (cold.length > 0) {
    await runBootstrap(cold)
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
      // A collab token is bound to the workspace it was minted in.
      const { resetCollabToken } = await import('@/services/collabToken')
      resetCollabToken()
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
  // schema_version is stale are filtered out by `loadModels`.
  const rows = await idb.loadModels(state.handle, SCHEMA_VERSIONS)
  for (const r of rows) {
    pool.upsert(r.aggregate, r.id, r.data)
  }

  // Seed the composite cursor from the cache. A cache written before
  // the commit-safe cursor shipped has no xid8 (defaults to 0); that's
  // fine — pullDelta omits `from_xid8` while xid8 is 0 and the bootstrap
  // that follows every subscribe reseeds a real xid8.
  const lastCachedSyncId = (await idb.getLastSyncId(state.handle)) ?? 0
  const lastCachedXid8 = (await idb.getLastXid8(state.handle)) ?? 0
  pool.setCursor(lastCachedXid8, lastCachedSyncId)

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

  // Drain groups that subscribed before hydrate() finished (see subscribe()).
  bootstrapDeferredGroups()
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
    // Capture before pool.reset() drops them: these are the groups to bring
    // back, and re-subscribing is what makes the bootstrap fetch them.
    const groups = Array.from(pool.getSubscribedGroups())
    if (newWorkspaceEpoch) {
      logger.warn('sync: workspace data replaced; wiping every local cache and re-bootstrapping', {
        groups,
        previousEpoch: state.workspaceEpoch,
        currentEpoch: newWorkspaceEpoch,
      })
    } else {
      logger.warn('sync: cursor predates the server feed; wiping and re-bootstrapping', {
        groups,
      })
    }

    if (state.handle && state.openArgs) {
      const name = state.handle.name
      const { userUuid, workspaceSlug } = state.openArgs
      state.handle.db.close()
      state.handle = null
      queue.setIdbHandle(null)
      await idb.wipe(name)
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
    }

    // reset() clears rows, cursor, subscriptions AND the schema hash. Restore
    // the hash: runBootstrap compares the server's against it and would
    // otherwise warn on a mismatch against an empty string every resync.
    pool.reset()
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
    // poll is the delivery path.
    notifySyncActions(body.actions)
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
 */
export async function runBootstrap(groups: string[]): Promise<void> {
  // Needs a hydrated runtime AND a selected workspace (not an IDB handle). In
  // memory-only mode the persistence writes below are individually skipped, but
  // the fetch + pool.upsert still run so views populate.
  if (!canFetchWorkspace()) return
  // A group added to a session that is caught up (opening a ticket) leaves
  // the cursor where it is. Its snapshot ends at the server's horizon, which
  // can be past events for the other groups that nothing has delivered yet;
  // the delta after it replays that span for every group instead.
  const keepCursor = state.caughtUp && pool.getLastXid8() > 0
  const url = `/sync/bootstrap?groups=${encodeURIComponent(groups.join(','))}&schema=${encodeURIComponent(state.schemaHash)}`
  const epoch = pool.currentEpoch()
  let res: Response
  try {
    res = await authFetch(url)
  } catch (e) {
    logger.error('sync bootstrap network error', { error: e })
    return
  }
  if (!res.ok || !res.body) {
    logger.error('sync bootstrap http error', { status: res.status })
    return
  }

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let bootstrapMeta: BootstrapMeta | null = null
  // Ids this snapshot carried, per aggregate, for the reconciliation on
  // `__end__` (see SNAPSHOT_COMPLETE_AGGREGATES).
  const seen = new Map<SyncAggregate, Set<string>>()
  const persistBatch: idb.ModelRow[] = []
  const flushPersistBatch = async () => {
    if (state.handle && persistBatch.length > 0) {
      await idb.putModels(state.handle, persistBatch.splice(0, persistBatch.length))
    }
  }

  while (true) {
    const { done, value } = await reader.read()
    // The workspace was torn down mid-stream: the rest is not ours to apply.
    if (pool.currentEpoch() !== epoch) {
      void reader.cancel().catch(() => {})
      return
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
        if (meta.server_schema !== state.schemaHash) {
          logger.warn('bootstrap meta carries a different schema_hash than client', {
            server: meta.server_schema,
            client: state.schemaHash,
          })
        }
        // Surface workspace-level capability flags into the
        // shared composable so any UI component can read them.
        applyWorkspaceCapabilities(meta)
        await followAssetVisibility(
          meta.assets_visible,
          groups.some((g) => g.startsWith('workspace:')) ? 'workspace-snapshot' : 'other-snapshot',
        )
      } else if ('__model__' in line) {
        const { __model__: aggregate, ...payload } = line
        const id = rowKey(aggregate, payload as Record<string, unknown>)
        if (id == null) {
          logger.warn('bootstrap row missing key; dropping', { aggregate, payload })
          continue
        }
        pool.upsert(aggregate, id, payload as Record<string, unknown>)
        let ids = seen.get(aggregate)
        if (!ids) seen.set(aggregate, (ids = new Set()))
        ids.add(String(id))
        if (SCHEMA_VERSIONS[aggregate] != null) {
          persistBatch.push({
            aggregate,
            id: String(id),
            schema_version: SCHEMA_VERSIONS[aggregate]!,
            data: payload as Record<string, unknown>,
          })
          if (persistBatch.length >= 200) await flushPersistBatch()
        }
      } else if ('__end__' in line) {
        // Bootstrap finished successfully — persist any tail and
        // advance the cursor.
        await flushPersistBatch()
        if (bootstrapMeta) {
          await reconcileWithSnapshot(bootstrapMeta.groups_granted ?? [], seen)
          if (!keepCursor) {
            // The end line advances the cursor to now, which tears any cached
            // group not currently subscribed; prune before persisting.
            await pruneTornWatermarks()
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
            // with a delta instead of re-streaming the snapshot.
            const granted = bootstrapMeta.groups_granted ?? []
            const completed = groups.filter((g) => granted.includes(g))
            if (completed.length > 0) {
              await idb.setGroupWatermarks(state.handle, completed, bootstrapMeta.last_xid8)
            }
            await idb.setModelVersionsTag(state.handle, MODEL_VERSIONS_TAG)
          }
        }
      } else if ('__error__' in line) {
        logger.error('bootstrap streamed error envelope', { line })
      }
    }
  }
  await flushPersistBatch()
  // Replays what the snapshot's horizon passed, for every subscribed group.
  // Rows come back to their latest state: events carry whole rows, in order.
  if (keepCursor && bootstrapMeta && pool.currentEpoch() === epoch) {
    await catchUpViaDelta()
  }
}

/** Tear-down: release the IDB handle, reset the pool, drop subs. */
export async function tearDown(): Promise<void> {
  stopDeltaPollFallback()
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
  }, POLL_INTERVAL_MS)
}

function stopDeltaPollFallback(): void {
  if (state.pollTimer) {
    clearInterval(state.pollTimer)
    state.pollTimer = null
  }
}
