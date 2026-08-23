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
import { noteServerEcho } from './optimisticCreates'
import { setReferenceFetcher } from '@nosdesk/core/sync/composables'
import { notifySyncActions } from '@nosdesk/core/sync/observers'
import { applyWorkspaceCapabilities } from '@/composables/useWorkspaceCapabilities'
import { purgeAllCollabDocs } from '@/utils/collabLocalCache'
import { apiBaseUrl, transport } from '@nosdesk/core/transport'
import { workspaceHeaders, workspaceReady, workspaceReadyRef } from '@/services/activeWorkspace'
import type {
  BootstrapLine,
  BootstrapMeta,
  DeltaResponse,
  SyncAction,
  SyncAggregate,
} from '@nosdesk/core/sync/types'

interface LifecycleState {
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
}

const state: LifecycleState = {
  handle: null,
  schemaHash: '',
  pollTimer: null,
  memoryOnly: false,
  resyncing: false,
  openArgs: null,
  caughtUp: false,
}

/** Memory-only counts as hydrated (it still bootstraps and serves; only
 *  warm-start persistence is lost). Gating subscribe/runBootstrap/pullDelta on
 *  `state.handle` alone stranded memory-only groups empty until a force-quit. */
function isHydrated(): boolean {
  return state.handle !== null || state.memoryOnly
}

/** A workspace-scoped fetch (bootstrap/delta) may only fire once the runtime is
 *  hydrated AND a workspace is selected. `workspaceReady()` is the same gate the
 *  Colada queries use (true in host mode, or once the path-mode slug is set);
 *  without it the sync engine sends header-less requests that fail 400
 *  NoWorkspaceSelected, the 10s delta poll spamming them all session. */
function canFetchWorkspace(): boolean {
  return isHydrated() && workspaceReady()
}

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
 * Split groups into warm (cached snapshot a delta can bring current) and cold
 * (needs the full bootstrap stream). A group is warm only when ALL of:
 *
 *  - persistence is live and the rehydrated pool actually holds rows;
 *  - the cached cursor is a real xid8 (a 0 cursor means no commit-safe
 *    baseline to delta from);
 *  - the cached rows were written under the current client aggregate
 *    versions (a SCHEMA_VERSIONS bump filters old rows out on rehydrate,
 *    and only a bootstrap re-fetches them — a delta never re-streams the
 *    snapshot);
 *  - the group holds a bootstrap watermark. Watermarks are written when a
 *    group's snapshot stream completes and pruned when a session advances
 *    the cursor while the group is unsubscribed (see pruneTornWatermarks),
 *    so presence means "these cached rows have tracked the cursor
 *    continuously since their snapshot".
 */
async function partitionWarmGroups(
  groups: string[],
): Promise<{ warm: string[]; cold: string[] }> {
  if (!state.handle || pool.getLastXid8() <= 0 || pool.size() === 0) {
    return { warm: [], cold: groups }
  }
  const versionsTag = await idb.getModelVersionsTag(state.handle)
  if (versionsTag !== MODEL_VERSIONS_TAG) {
    return { warm: [], cold: groups }
  }
  const watermarks = (await idb.getGroupWatermarks(state.handle)) ?? {}
  const warm: string[] = []
  const cold: string[] = []
  for (const g of groups) {
    ;(watermarks[g] != null ? warm : cold).push(g)
  }
  return { warm, cold }
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
  const { schemaHash, instanceId } = await fetchServerIdentity()
  await hydrate(userUuid, schemaHash, instanceId, workspaceSlug)
  const { attachSseBridge } = await import('@/sync/sseBridge')
  attachSseBridge()
}

const POLL_INTERVAL_MS = 10_000

/**
 * Schema versions per aggregate, mirroring `sync::registry` on the
 * server. Rows in IndexedDB whose recorded schema_version doesn't
 * match the entry here get filtered on rehydrate and re-fetched on
 * the next bootstrap.
 *
 * When the server's manifest schema_version bumps, bump the matching
 * entry here in the same commit.
 */
const SCHEMA_VERSIONS: Partial<Record<SyncAggregate, number>> = {
  ticket: 1,
  project: 1,
  project_ticket: 1,
  ticket_asset: 1,
  linked_ticket: 1,
  workflow_state: 1,
  comment: 1,
  attachment: 1,
  assignment: 1,
  group_membership: 1,
  plugin: 1,
  user: 1,
  asset: 1,
  asset_media: 2,
  asset_lifecycle_event: 1,
  cycle: 1,
  documentation_page: 1,
  documentation_collection: 1,
}

/**
 * Tag identifying the client aggregate versions the cache was written under.
 * Compared in partitionWarmGroups: rows persisted under a different map may
 * have been filtered out on rehydrate, and only a full bootstrap re-fetches
 * them, so any mismatch disqualifies the warm path wholesale.
 */
const MODEL_VERSIONS_TAG = JSON.stringify(SCHEMA_VERSIONS)

/**
 * Drop bootstrap watermarks for groups that are NOT subscribed at the moment
 * this session first advances the cursor. From that advance on, the cursor
 * moves past events those groups never apply, so their cached rows stop
 * tracking it: a later launch must snapshot them again, not delta from a
 * cursor they don't match. Runs once per session, and must complete BEFORE
 * the first advanced cursor is persisted — a crash that persisted the moved
 * cursor but kept the stale watermark would revive the torn cache as warm.
 */
let watermarksPruned = false

async function pruneTornWatermarks(): Promise<void> {
  if (watermarksPruned) return
  watermarksPruned = true
  if (!state.handle) return
  const marks = (await idb.getGroupWatermarks(state.handle)) ?? {}
  const subscribed = pool.getSubscribedGroups()
  const kept: Record<string, number> = {}
  for (const [g, xid8] of Object.entries(marks)) {
    if (subscribed.has(g)) kept[g] = xid8
  }
  if (Object.keys(kept).length !== Object.keys(marks).length) {
    logger.info('sync: pruning watermarks for unsubscribed groups', {
      dropped: Object.keys(marks).filter((g) => !subscribed.has(g)),
    })
    await idb.replaceGroupWatermarks(state.handle, kept)
  }
}

/**
 * Junction aggregates whose pool key is a composite of their fields
 * rather than a standalone `id`/`uuid`. Their sync rows ARE full rows
 * (the payload carries every field); they just key on the composite.
 * `project_ticket` is the canonical example — a project<->ticket
 * association with no surrogate key.
 */
const COMPOSITE_KEY: Partial<
  Record<SyncAggregate, (d: Record<string, unknown>) => string | null>
> = {
  project_ticket: (d) =>
    d.project_id != null && d.ticket_id != null ? `${d.project_id}:${d.ticket_id}` : null,
  ticket_asset: (d) =>
    d.ticket_id != null && d.asset_id != null ? `${d.ticket_id}:${d.asset_id}` : null,
  linked_ticket: (d) =>
    d.ticket_id != null && d.linked_ticket_id != null
      ? `${d.ticket_id}:${d.linked_ticket_id}`
      : null,
  cycle_ticket: (d) =>
    d.cycle_id != null && d.ticket_id != null ? `${d.cycle_id}:${d.ticket_id}` : null,
  // One assignment per ticket — keyed by ticket_id, not a surrogate id.
  assignment: (d) => (d.ticket_id != null ? String(d.ticket_id) : null),
}

/**
 * The pool key for a sync row. Junction aggregates derive it from
 * their fields (see COMPOSITE_KEY). Every other aggregate keys on its
 * own primary key carried in the payload — and for those, a payload
 * lacking both `id` and `uuid` is a *side event* (e.g. documentation
 * `visibility_changed`, knowledge_gap signals, the synthetic `data`
 * audit events): it references a row by aggregate_id without carrying
 * the row, so it returns null and the caller skips the pool write
 * rather than minting a partial/phantom row. Used by both the
 * bootstrap snapshot loader and the live delta/SSE applier so the two
 * paths key rows identically.
 */
function rowKey(aggregate: SyncAggregate, data: Record<string, unknown>): string | number | null {
  const composite = COMPOSITE_KEY[aggregate]
  if (composite) return composite(data)
  return ((data.id ?? data.uuid) as string | number | undefined) ?? null
}

/** Warm the engine for an authenticated user. Idempotent —
 * subsequent calls are no-ops while the handle is live (or while
 * memory-only mode is active). */
export async function hydrate(
  userUuid: string,
  schemaHash: string,
  instanceId = '',
  workspaceSlug: string | null = null,
): Promise<void> {
  if (state.handle || state.memoryOnly) return
  pool.setSchemaHash(schemaHash)
  state.schemaHash = schemaHash
  state.openArgs = { userUuid, workspaceSlug }
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

  // Two reasons to throw away the local cache and re-bootstrap clean:
  //   - schema mismatch: cached rows may not fit the new schema.
  //   - instance mismatch (epoch fence): the cached data belongs to a
  //     different database generation. We only trust an instance id the
  //     server actually reported (non-empty), and an absent persisted
  //     id counts as a mismatch so the first load after this ships
  //     clears caches written before instance ids existed.
  const schemaChanged = !!persistedHash && persistedHash !== schemaHash
  const instanceChanged = instanceId !== '' && persistedInstance !== instanceId

  if (schemaChanged || instanceChanged) {
    logger.info('Sync cache stale; wiping local cache and rebootstrapping', {
      schemaChanged,
      instanceChanged,
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
    // notes onto recycled ids. Purge them too.
    if (instanceChanged) {
      await purgeAllCollabDocs()
    }
  }

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

/** Fetch the server's compiled schema hash and database instance id.
 * The schema hash names the IndexedDB; the instance id is the epoch
 * fence (a change means a different database generation). `instanceId`
 * falls back to `''` so a fetch/DB hiccup never triggers a wipe. */
/**
 * `fetch` with credentials that survives an expired (15 min) access
 * token. The sync runtime uses raw fetch (streaming bootstrap, no
 * axios), so it doesn't inherit the axios client's refresh-on-401. On a
 * 401 we run the shared, deduplicated refresh once and retry; that
 * single refresh is coordinated with the axios client so the
 * token-rotating endpoint is never hit twice concurrently. On refresh
 * failure the original 401 response is returned and the caller backs
 * off (the axios client owns redirect-to-login for a dead session).
 */
async function syncFetch(path: string): Promise<Response> {
  // The sync engine uses raw fetch (streaming JSONL), so the axios interceptor
  // doesn't apply here; resolve base URL, auth headers, the selection header
  // (empty in host mode), and credential mode from the transport seam directly.
  const url = `${apiBaseUrl()}${path}`
  const credentials: RequestCredentials = transport().auth.useCredentials ? 'include' : 'omit'
  const headers = { ...workspaceHeaders(), ...transport().auth.authHeaders() }
  const res = await fetch(url, { credentials, headers })
  if (res.status !== 401) return res
  const refreshed = await transport().auth.refresh()
  if (!refreshed) return res
  return fetch(url, { credentials, headers })
}

export async function fetchServerIdentity(): Promise<{
  schemaHash: string
  instanceId: string
}> {
  try {
    const res = await syncFetch('/sync/schema')
    if (!res.ok) return { schemaHash: 'unknown', instanceId: '' }
    const body = (await res.json()) as { server_schema?: string; instance_id?: string }
    return {
      schemaHash: body.server_schema ?? 'unknown',
      instanceId: body.instance_id ?? '',
    }
  } catch (e) {
    logger.warn('Failed to fetch /sync/schema; falling back to "unknown"', { error: e })
    return { schemaHash: 'unknown', instanceId: '' }
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
 * Wipe the local cache and re-bootstrap, in response to the server reporting
 * `resync_required` — our cursor has aged past `SYNC_ACTIONS_RETENTION_DAYS`,
 * so the deletes we missed have been pruned and no delta can deliver them.
 *
 * Bootstrap alone cannot repair this: the stream only upserts, so a row deleted
 * while we were away is never removed, it just stops being mentioned. Only
 * dropping the cache first clears those phantoms.
 *
 * Collaborative-document caches are deliberately NOT purged. Those are keyed by
 * the database instance epoch, which has not changed here; this is our cursor
 * being old, not the server being a different database. Purging them would cost
 * every open document a re-download for no correctness gain.
 */
async function resyncFromRetentionGap(): Promise<void> {
  if (state.resyncing) return
  state.resyncing = true
  try {
    // Capture before pool.reset() drops them: these are the groups to bring
    // back, and re-subscribing is what makes the bootstrap fetch them.
    const groups = Array.from(pool.getSubscribedGroups())
    logger.warn('sync: cursor aged out of server retention; wiping and re-bootstrapping', {
      groups,
    })

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
    logger.error('sync: retention resync failed; will retry on a later poll', { error: e })
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
  try {
    const res = await syncFetch(url)
    if (!res.ok) {
      logger.warn('sync delta failed', { status: res.status })
      return false
    }
    const body = (await res.json()) as DeltaResponse
    if (body.resync_required) {
      // Return without applying: this page is a partial view of a span we
      // cannot complete, and the re-bootstrap replaces it wholesale.
      await resyncFromRetentionGap()
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
    state.caughtUp = true
    pool.setCursor(body.last_xid8, body.last_sync_id)
    if (state.handle) {
      await idb.setLastSyncId(state.handle, body.last_sync_id)
      await idb.setLastXid8(state.handle, body.last_xid8)
    }
    // Same observer fan-out as the SSE path, so imperative consumers
    // (useSyncActions) stay live even when SSE is wedged and this 10s
    // poll is the delivery path.
    notifySyncActions(body.actions)
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
async function runBootstrap(groups: string[]): Promise<void> {
  // Needs a hydrated runtime AND a selected workspace (not an IDB handle). In
  // memory-only mode the persistence writes below are individually skipped, but
  // the fetch + pool.upsert still run so views populate.
  if (!canFetchWorkspace()) return
  const url = `/sync/bootstrap?groups=${encodeURIComponent(groups.join(','))}&schema=${encodeURIComponent(state.schemaHash)}`
  let res: Response
  try {
    res = await syncFetch(url)
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
  const persistBatch: idb.ModelRow[] = []
  const flushPersistBatch = async () => {
    if (state.handle && persistBatch.length > 0) {
      await idb.putModels(state.handle, persistBatch.splice(0, persistBatch.length))
    }
  }

  while (true) {
    const { done, value } = await reader.read()
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
      } else if ('__model__' in line) {
        const { __model__: aggregate, ...payload } = line
        const id = rowKey(aggregate, payload as Record<string, unknown>)
        if (id == null) {
          logger.warn('bootstrap row missing key; dropping', { aggregate, payload })
          continue
        }
        pool.upsert(aggregate, id, payload as Record<string, unknown>)
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
          // The end line advances the cursor to now, which tears any cached
          // group not currently subscribed; prune before persisting.
          await pruneTornWatermarks()
          state.caughtUp = true
          // Advance the in-memory cursor always; persist it only when IDB is
          // available (memory-only keeps the cursor in the pool for delta polls).
          pool.setCursor(bootstrapMeta.last_xid8, bootstrapMeta.last_sync_id)
          if (state.handle) {
            await idb.setLastSyncId(state.handle, bootstrapMeta.last_sync_id)
            await idb.setLastXid8(state.handle, bootstrapMeta.last_xid8)
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
}

function applyActions(actions: SyncAction[]): void {
  for (const action of actions) {
    if (action.op === 'D') {
      // Deletes may carry only the key; fall back to aggregate_id for
      // own-pk aggregates whose delete payload is otherwise empty.
      const id = rowKey(action.aggregate, action.data) ?? action.aggregate_id
      if (id == null) continue
      pool.remove(action.aggregate, id)
      if (state.handle) {
        void idb.deleteModel(state.handle, action.aggregate, String(id))
      }
      continue
    }
    // `rowKey` returns null for side events (an own-pk aggregate whose
    // payload carries no primary key — e.g. documentation
    // visibility_changed). Skipping those avoids writing a partial row
    // over the cached one; observers still receive them via
    // notifySyncActions for consumers that care.
    const id = rowKey(action.aggregate, action.data)
    if (id == null) continue
    // A comment's server echo carrying a correlation_id is a pending optimistic
    // create: suppress it from the view (it arrives without its attachments)
    // until the REST reply swaps the temp for the complete row. Structural, by
    // id, no dedup heuristic.
    if (action.aggregate === 'comment' && action.correlation_id) {
      noteServerEcho(action.correlation_id, Number(id))
    }
    pool.upsert(action.aggregate, id, action.data)
    if (state.handle && SCHEMA_VERSIONS[action.aggregate] != null) {
      void idb.putModels(state.handle, [
        {
          aggregate: action.aggregate,
          id: String(id),
          schema_version: SCHEMA_VERSIONS[action.aggregate]!,
          data: action.data,
        },
      ])
    }
  }
}

async function referenceFetcher(aggregate: SyncAggregate, ids: string[]): Promise<void> {
  // Reference fetch lives outside the bootstrap protocol — it
  // queries the existing per-aggregate REST endpoints (the ones
  // the legacy UI also reads from). Keeping the bootstrap minimal
  // and the lazy-fetch pluggable lets aggregates ship sync support
  // before they ship a typed bootstrap loader.
  //
  // Per-aggregate dispatch. For aggregates without a dedicated
  // fetcher we log and bail; the bootstrap covered the snapshot
  // and SSE keeps it current, so a missed lazy fetch only matters
  // for entities created strictly between bootstrap and the first
  // SSE frame — vanishingly rare.
  if (aggregate === 'user') {
    await fetchMissingUsers(ids)
    return
  }
  if (aggregate === 'asset') {
    await fetchMissingAssets(ids)
    return
  }
  if (aggregate === 'cycle') {
    await fetchMissingCycles(ids)
    return
  }
  logger.debug('useReference fetch (stub)', { aggregate, ids })
}

/**
 * Lazy reference fetcher for the `user` aggregate. Bootstrap loads
 * the full workspace user set up-front, so this only fires when an
 * unknown uuid is referenced (e.g. a comment whose author was
 * created mid-session before that user.created SSE arrived).
 *
 * Lives in lifecycle.ts (rather than the directory composable) so
 * it's installed once at hydrate-time, before any UserCell mounts.
 */
async function fetchMissingUsers(uuids: string[]): Promise<void> {
  if (uuids.length === 0) return
  // Lazy import: keeps the userService bundle out of the sync
  // engine's hot path until the lazy fetcher actually fires.
  const { default: userService } = await import('@/services/userService')
  try {
    const users = await userService.getUsersBatch(uuids)
    for (const user of users) {
      pool.upsert('user', user.uuid, {
        uuid: user.uuid,
        name: user.name,
        email: user.email,
        platform_role: user.platform_role,
        workspace_role: user.workspace_role ?? null,
        pronouns: user.pronouns ?? null,
        avatar_url: user.avatar_url ?? null,
        avatar_thumb: user.avatar_thumb ?? null,
      })
    }
  } catch (err) {
    logger.warn('Lazy user fetch failed', { ids: uuids, error: err })
  }
}

/**
 * Lazy reference fetcher for the `asset` aggregate. Bootstrap
 * ships every workspace asset, so this only fires for entries
 * created in the gap between bootstrap and the first SSE frame.
 * Falls back to per-id `getAssetById` because the REST surface
 * doesn't expose a batch endpoint today; the rarity of the
 * missing-lookup path makes the round-trip cost acceptable.
 *
 * Cache row shape must stay in lockstep with
 * `backend/sync-models/asset.json` and the SSE / bootstrap
 * payload produced by `repository::devices::asset_sync_payload`.
 * The full REST `Asset` DTO carries more fields (warranty,
 * Microsoft Graph IDs, etc.) that the sync pool deliberately
 * drops; `toAssetCacheRow` is the single projection point so
 * the lazy path can't drift from the live event stream.
 */
function toAssetCacheRow(asset: import('@nosdesk/core/types/asset').Asset): Record<string, unknown> {
  return {
    id: asset.id,
    name: asset.name,
    kind: asset.kind ?? 'generic',
    serial_number: asset.serial_number ?? null,
    manufacturer: asset.manufacturer ?? null,
    model: asset.model ?? null,
    asset_tag: asset.asset_tag ?? null,
    location: asset.location ?? null,
    primary_user_uuid: asset.primary_user_uuid ?? null,
    attributes: asset.attributes ?? {},
    quantity: asset.quantity ?? null,
    unit: asset.unit ?? null,
    external_sync_source: asset.external_sync_source ?? null,
  }
}

async function fetchMissingAssets(ids: string[]): Promise<void> {
  if (ids.length === 0) return
  const { getAssetById } = await import('@/services/assetService')
  for (const idStr of ids) {
    const id = Number(idStr)
    if (!Number.isFinite(id)) continue
    try {
      const asset = await getAssetById(id)
      pool.upsert('asset', asset.id, toAssetCacheRow(asset))
    } catch (err) {
      logger.warn('Lazy asset fetch failed', { id, error: err })
    }
  }
}

/**
 * Lazy reference fetcher for the `cycle` aggregate. Bootstrap does
 * not ship cycle rows (only `cycle_id` denormalised onto tickets),
 * so the pool-native ticket detail view resolves the cycle chip
 * through this. The workspace cycle set is small and bounded, so
 * one `listWorkspace` call covers every missing id at once; we ask
 * for all states explicitly because the default list elides
 * completed/planned cycles, and a ticket can sit in any of them.
 *
 * The full REST row goes in: the pool is now the single read home
 * for cycles (useProjectCycles / useCycleStats), so dates, the
 * frozen completion_snapshot, and archived_at must all survive
 * here — a thin row would starve those consumers, and shallow-merge
 * means a later thin write can't erase a previously seeded field.
 */
function toCycleCacheRow(
  c: import('@nosdesk/core/services/cyclesService').Cycle,
): Record<string, unknown> {
  return { ...c }
}

async function fetchMissingCycles(ids: string[]): Promise<void> {
  if (ids.length === 0) return
  const { cyclesService } = await import('@nosdesk/core/services/cyclesService')
  try {
    const cycles = await cyclesService.listWorkspace(['planned', 'active', 'completed'])
    for (const c of cycles) {
      pool.upsert('cycle', c.id, toCycleCacheRow(c))
    }
  } catch (err) {
    logger.warn('Lazy cycle fetch failed', { ids, error: err })
  }
}

/** SSE handler: applies actions, advances the cursor, persists. */
export function applySseFrame(actions: SyncAction[], lastXid8: number, lastSyncId: number): void {
  applyActions(actions)
  // Until the session's first catch-up completes, a frame's rows apply but
  // the cursor holds at its cached value: jumping it to the frame's "now"
  // would make the in-flight catch-up delta skip the whole offline span
  // (see LifecycleState.caughtUp). The catch-up re-delivers these events.
  if (!state.caughtUp) {
    notifySyncActions(actions)
    return
  }
  pool.setCursor(lastXid8, lastSyncId)
  if (state.handle) {
    const handle = state.handle
    // Prune-then-persist, sequenced: the advanced cursor must never be
    // persisted ahead of the torn-watermark prune (see pruneTornWatermarks).
    void (async () => {
      await pruneTornWatermarks()
      await idb.setLastSyncId(handle, lastSyncId)
      await idb.setLastXid8(handle, lastXid8)
    })()
  }
  // Notify imperative observers (useSyncActions) after the pool is
  // updated. This is the live SSE path, so observers fire on the
  // cross-machine sync stream; initial hydrate deliberately does not.
  notifySyncActions(actions)
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
  state.caughtUp = false
  watermarksPruned = false
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
