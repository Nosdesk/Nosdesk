/**
 * Reconciling the pool against the server's picture: the snapshot strip by
 * owner, and following whether the viewer sees the workspace's assets.
 */
import { logger } from '@nosdesk/core/utils/logger'
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import { state } from './runtimeState'
import { runBootstrap } from './lifecycle'
import { cachedOwners } from './apply'
import { SCHEMA_VERSIONS } from './schemaVersions'
import { isWorkspaceGroup, SNAPSHOT_BACKED } from './ownership'
import type { SyncAggregate } from '@nosdesk/core/sync/types'

/**
 * At a snapshot's end, take each granted group off every row of a
 * snapshot-backed aggregate the stream did not carry, and `legacy` too when
 * the workspace group was granted; a row left with no owners is gone from
 * the server's picture (deleted while the stream was down, moved out of the
 * group, or never this workspace's) and is removed from the pool and the
 * cache. A row a live action wrote after `startedAt` (the touch clock when
 * the bootstrap began) is left alone: it may postdate the snapshot, which
 * then could not have carried it. `local` rows keep `local`, so they stay.
 *
 * `granted` must be the groups the server granted, not the request: a
 * denied group's snapshot carried nothing and must strip nothing.
 */
export async function reconcileByOwner(
  granted: readonly string[],
  seen: ReadonlyMap<SyncAggregate, ReadonlySet<string>>,
  startedAt: number,
): Promise<void> {
  if (granted.length === 0) return
  const stripped = new Set(granted)
  if (granted.some(isWorkspaceGroup)) stripped.add(pool.LEGACY_OWNER)
  const removed: Array<[SyncAggregate, string]> = []
  const changed: idb.ModelRow[] = []
  for (const aggregate of SNAPSHOT_BACKED) {
    const carried = seen.get(aggregate)
    // Collected first: the loop below deletes from the map entries() walks.
    const ids: string[] = []
    for (const [id] of pool.entries(aggregate)) {
      if (!carried?.has(id) && pool.lastTouch(aggregate, id) <= startedAt) ids.push(id)
    }
    for (const id of ids) {
      const before = pool.ownersOf(aggregate, id)
      let left = before.size
      for (const g of stripped) left = pool.stripOwner(aggregate, id, g)
      if (left === 0) {
        pool.remove(aggregate, id)
        removed.push([aggregate, id])
      } else if (left !== before.size && SCHEMA_VERSIONS[aggregate] != null) {
        // Still owned, by fewer groups: cache that, or the next launch would
        // count the stripped group as an owner again.
        changed.push({
          aggregate,
          id,
          schema_version: SCHEMA_VERSIONS[aggregate]!,
          data: { ...pool.get<Record<string, unknown>>(aggregate, id) },
          owners: cachedOwners(aggregate, id),
        })
      }
    }
  }
  if (state.handle) {
    // Both issued before either is awaited, so no other write lands between.
    await Promise.all([
      idb.deleteModels(state.handle, removed),
      changed.length > 0 ? idb.putModels(state.handle, changed) : undefined,
    ])
  }
  if (removed.length > 0) {
    logger.info('sync bootstrap: removed rows no snapshot owns', { removed: removed.length })
  }
}

/** Records about assets the pool holds for the viewer. */
const ASSET_AGGREGATES: SyncAggregate[] = ['asset', 'ticket_asset']

/** Set while followAssetVisibility re-reads the snapshot, so a poll that
 * lands meanwhile doesn't start a second one. */
let assetResync = false

/** Where this browser remembers that it dropped the workspace's assets. */
function assetsHiddenKey(): string | null {
  if (!state.openArgs) return null
  return `nosdesk:sync:assets-hidden:${state.openArgs.userUuid}:${state.openArgs.workspaceSlug ?? ''}`
}

function readAssetsHidden(key: string): boolean {
  try {
    return localStorage.getItem(key) === '1'
  } catch {
    return false
  }
}

function writeAssetsHidden(key: string, hidden: boolean): void {
  try {
    if (hidden) localStorage.setItem(key, '1')
    else localStorage.removeItem(key)
  } catch {
    // Storage blocked: a later bootstrap still brings the assets back.
  }
}

/**
 * Keep the pool in step with whether the viewer sees the workspace's assets
 * (`assets_visible`; a workspace can show requesters none). Hidden: drop
 * every asset and ticket link the pool holds, and remember having done so.
 * Shown again after that: re-read the workspace snapshot, which brings them
 * back (a delta never re-sends rows it sent before). The memory outlives a
 * reload because a warm launch catches up with a delta. `source`: where the
 * flag came from; a workspace snapshot carries the assets itself, and one for
 * other groups leaves bringing them back to the next delta.
 */
export async function followAssetVisibility(
  visible: boolean | undefined,
  source: 'delta' | 'workspace-snapshot' | 'other-snapshot',
): Promise<void> {
  if (visible === undefined) return
  const key = assetsHiddenKey()
  if (!visible) {
    const dropped: Array<[SyncAggregate, string]> = []
    for (const aggregate of ASSET_AGGREGATES) {
      const ids = Array.from(pool.entries(aggregate), ([id]) => id)
      for (const id of ids) {
        pool.remove(aggregate, id)
        dropped.push([aggregate, id])
      }
    }
    if (state.handle && dropped.length > 0) await idb.deleteModels(state.handle, dropped)
    if (key) writeAssetsHidden(key, true)
    if (dropped.length > 0) {
      logger.info('sync: assets hidden from this viewer; dropped them', { dropped: dropped.length })
    }
    return
  }
  if (!key || !readAssetsHidden(key)) return
  if (source === 'workspace-snapshot') {
    writeAssetsHidden(key, false)
    return
  }
  if (source === 'other-snapshot') return
  if (assetResync || state.resyncing) return
  const workspaceGroups = Array.from(pool.getSubscribedGroups()).filter((g) =>
    g.startsWith('workspace:'),
  )
  if (workspaceGroups.length === 0) return
  assetResync = true
  try {
    logger.info('sync: assets shown to this viewer again; re-reading the snapshot')
    await runBootstrap(workspaceGroups)
  } finally {
    assetResync = false
  }
}
