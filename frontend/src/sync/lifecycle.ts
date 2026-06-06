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
import { logger } from '@/utils/logger'
import * as pool from './pool'
import * as idb from './idb'
import * as queue from './queue'
import { setReferenceFetcher } from './composables'
import { notifySyncActions } from './observers'
import { applyWorkspaceCapabilities } from '@/composables/useWorkspaceCapabilities'
import type {
  BootstrapLine,
  BootstrapMeta,
  DeltaResponse,
  SyncAction,
  SyncAggregate,
} from './types'

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
}

const state: LifecycleState = {
  handle: null,
  schemaHash: '',
  pollTimer: null,
  memoryOnly: false,
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
  documentation_page: 1,
  documentation_collection: 1,
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
export async function hydrate(userUuid: string, schemaHash: string): Promise<void> {
  if (state.handle || state.memoryOnly) return
  pool.setSchemaHash(schemaHash)
  state.schemaHash = schemaHash
  setReferenceFetcher(referenceFetcher)

  // Try to open IDB; degrade gracefully if it's unavailable
  // (private browsing, quota, blocked, deleted-while-open). The
  // runtime stays usable — bootstrap still happens, the pool
  // still holds rows, optimistic writes still apply — just
  // without warm-start persistence.
  try {
    state.handle = await idb.open(userUuid, schemaHash)
  } catch (e) {
    logger.warn('IndexedDB unavailable; degrading sync engine to memory-only', { error: e })
    state.memoryOnly = true
    queue.setIdbHandle(null)
    startDeltaPollFallback()
    return
  }

  queue.setIdbHandle(state.handle)

  const persistedHash = await idb.getSchemaHash(state.handle)
  if (persistedHash && persistedHash !== schemaHash) {
    // Schema mismatch — wipe and reopen at the new hash. The
    // bootstrap below replays the snapshot; nothing user-visible
    // is lost beyond pending-but-unflushed optimistic writes,
    // which we can't replay safely against a different schema.
    logger.info('Sync schema hash changed; wiping local cache and rebootstrapping', {
      previous: persistedHash,
      current: schemaHash,
    })
    const oldName = state.handle.name
    state.handle.db.close()
    await idb.wipe(oldName)
    state.handle = await idb.open(userUuid, schemaHash)
    queue.setIdbHandle(state.handle)
  }

  // Rehydrate the in-memory pool from cached rows. Rows whose
  // schema_version is stale are filtered out by `loadModels`.
  const rows = await idb.loadModels(state.handle, SCHEMA_VERSIONS)
  for (const r of rows) {
    pool.upsert(r.aggregate, r.id, r.data)
  }

  const lastCachedSyncId = (await idb.getLastSyncId(state.handle)) ?? 0
  pool.setLastSyncId(lastCachedSyncId)

  // Persist hash for the next warm start; if this is a fresh
  // database the put is a no-op replacement.
  await idb.setSchemaHash(state.handle, schemaHash)

  // Replay any persisted optimistic transactions (a previous tab
  // crash between persist and flush would have left them in the
  // store).
  void queue.flush()

  // Start the periodic delta-poll fallback. The SSE bridge will
  // also push updates; the poll is belt-and-braces for cases
  // where SSE is wedged behind a corporate proxy or the EventSource
  // is mid-reconnect. Idempotent — only starts a single timer.
  startDeltaPollFallback()
}

/** Fetch the server's compiled schema hash. Cheap (no DB
 * round-trip on the server). Cache the result on the lifecycle
 * state so warm-start doesn't re-fetch. */
export async function fetchServerSchemaHash(): Promise<string> {
  try {
    const res = await fetch('/api/sync/schema', { credentials: 'include' })
    if (!res.ok) return 'unknown'
    const body = (await res.json()) as { server_schema?: string }
    return body.server_schema ?? 'unknown'
  } catch (e) {
    logger.warn('Failed to fetch /api/sync/schema; falling back to "unknown"', { error: e })
    return 'unknown'
  }
}

/**
 * Subscribe to a sync group. If the group isn't already in the
 * pool, fetches a per-group bootstrap. Idempotent — calling twice
 * with the same group runs no extra work.
 */
export async function subscribe(group: string): Promise<void> {
  if (pool.getSubscribedGroups().has(group)) return
  pool.subscribe(group)
  if (!state.handle) {
    logger.warn('subscribe() called before hydrate(); deferring bootstrap', { group })
    return
  }
  await runBootstrap([group])
}

/**
 * Pull a delta from the server for the currently-subscribed groups.
 * Called on warm start (after rehydrate), on SSE reconnect, and on
 * a periodic-poll fallback when SSE is wedged.
 */
export async function pullDelta(): Promise<void> {
  if (!state.handle) return
  const groups = Array.from(pool.getSubscribedGroups())
  if (groups.length === 0) return
  const from = pool.getLastSyncId()
  const url =
    `/api/sync/delta?from=${from}&groups=${encodeURIComponent(groups.join(','))}`
  try {
    const res = await fetch(url, { credentials: 'include' })
    if (!res.ok) {
      logger.warn('sync delta failed', { status: res.status })
      return
    }
    const body = (await res.json()) as DeltaResponse
    applyActions(body.actions)
    pool.setLastSyncId(body.last_sync_id)
    if (state.handle) await idb.setLastSyncId(state.handle, body.last_sync_id)
    // Same observer fan-out as the SSE path, so imperative consumers
    // (useSyncActions) stay live even when SSE is wedged and this 10s
    // poll is the delivery path.
    notifySyncActions(body.actions)
  } catch (e) {
    logger.warn('sync delta network error', { error: e })
  }
}

/**
 * Fetch and stream a bootstrap for the given groups, applying each
 * row to the pool as it arrives. Used both on cold start (with the
 * full subscription list) and for incremental group expansion.
 */
async function runBootstrap(groups: string[]): Promise<void> {
  if (!state.handle) return
  const url = `/api/sync/bootstrap?groups=${encodeURIComponent(groups.join(','))}&schema=${encodeURIComponent(state.schemaHash)}`
  let res: Response
  try {
    res = await fetch(url, { credentials: 'include' })
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
        if (bootstrapMeta && state.handle) {
          pool.setLastSyncId(bootstrapMeta.last_sync_id)
          await idb.setLastSyncId(state.handle, bootstrapMeta.last_sync_id)
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
function toAssetCacheRow(asset: import('@/types/asset').Asset): Record<string, unknown> {
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

/** SSE handler: applies actions, advances the cursor, persists. */
export function applySseFrame(actions: SyncAction[], lastSyncId: number): void {
  applyActions(actions)
  pool.setLastSyncId(lastSyncId)
  if (state.handle) {
    void idb.setLastSyncId(state.handle, lastSyncId)
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
