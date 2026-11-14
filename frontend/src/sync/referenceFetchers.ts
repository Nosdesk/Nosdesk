/**
 * Lazy REST fetchers that fill pool rows the sync stream did not deliver.
 * Installed as the pool's reference fetcher at hydrate.
 */
import { logger } from '@nosdesk/core/utils/logger'
import * as pool from '@nosdesk/core/sync/pool'
import type { SyncAggregate } from '@nosdesk/core/sync/types'

export async function referenceFetcher(aggregate: SyncAggregate, ids: string[]): Promise<void> {
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
  if (aggregate === 'ticket') {
    await fetchMissingTickets(ids)
    return
  }
  logger.debug('useReference fetch (stub)', { aggregate, ids })
}

/** Aggregates the reference fetcher can fill from REST. */
export const FETCHABLE: ReadonlySet<SyncAggregate> = new Set(['user', 'asset', 'cycle', 'ticket'])

/**
 * Lazy fetcher for the `ticket` aggregate. The workspace bootstrap ships
 * every ticket, so this fires only when a live update arrives for a row
 * this pool never received (its create frame was missed across a
 * reconnect, or the ticket became visible to a restricted viewer).
 * Merging the update into nothing would mint a half row with no
 * timestamps; the REST row is complete.
 */
const ticketFetchesInFlight = new Set<number>()

async function fetchMissingTickets(ids: string[]): Promise<void> {
  // A burst of updates for one missed row asks once.
  const wanted = ids
    .map(Number)
    .filter((id) => Number.isFinite(id) && !ticketFetchesInFlight.has(id))
  if (wanted.length === 0) return
  wanted.forEach((id) => ticketFetchesInFlight.add(id))
  try {
    const [{ default: ticketService }, { apiTicketToSync }] = await Promise.all([
      import('@nosdesk/core/services/ticketService'),
      import('@/sync/stores/tickets'),
    ])
    for (const id of wanted) {
      try {
        const ticket = await ticketService.getTicketById(id)
        pool.upsert('ticket', ticket.id, apiTicketToSync(ticket))
      } catch (err) {
        logger.warn('Lazy ticket fetch failed', { id, error: err })
      }
    }
  } finally {
    wanted.forEach((id) => ticketFetchesInFlight.delete(id))
  }
}

/**
 * Lazy reference fetcher for the `user` aggregate. Bootstrap loads
 * the full workspace user set up-front, so this only fires when an
 * unknown uuid is referenced (e.g. a comment whose author was
 * created mid-session before that user.created SSE arrived).
 *
 * Lives in the sync engine (rather than the directory composable) so
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
 * payload produced by `repository::assets::asset_sync_payload`.
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
    status: asset.status,
    primary_user_uuid: asset.primary_user_uuid ?? null,
    managed_by_user_uuid: asset.managed_by_user_uuid ?? null,
    attributes: asset.attributes ?? {},
    quantity: asset.quantity ?? null,
    unit: asset.unit ?? null,
    external_sync_source: asset.external_sync_source ?? null,
    low_stock_threshold: asset.low_stock_threshold ?? null,
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
