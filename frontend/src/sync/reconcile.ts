/**
 * Reconciling the pool against the server's picture: the workspace snapshot
 * prune, and following whether the viewer sees the workspace's assets.
 */
import { logger } from '@nosdesk/core/utils/logger'
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import { state } from './runtimeState'
import { runBootstrap } from './lifecycle'
import type { SyncAggregate } from '@nosdesk/core/sync/types'

/**
 * Aggregates a workspace-wide bootstrap streams in full (see the server's
 * `handlers/sync/bootstrap.rs`: every ticket, project, page, collection,
 * workflow state, user and asset the caller can see). A snapshot of these
 * is authoritative, so a local row the snapshot did not carry is stale: a
 * delete missed while the stream was down, or a row that never belonged
 * to this workspace. Comments and attachments are streamed per ticket
 * group only, so they are never pruned here.
 */
const SNAPSHOT_COMPLETE_AGGREGATES: SyncAggregate[] = [
  'ticket',
  'project',
  'documentation_collection',
  'documentation_page',
  'workflow_state',
  'user',
  'asset',
]

/**
 * After a bootstrap that carried the workspace group, drop local rows of
 * the fully-streamed aggregates that the snapshot did not include. The
 * stream otherwise only upserts, so this is the one place a stale row is
 * reconciled against the server's whole picture.
 */
export async function reconcileWithSnapshot(
  granted: string[],
  seen: Map<SyncAggregate, Set<string>>,
): Promise<void> {
  if (!granted.some((g) => g.startsWith('workspace:'))) return
  let pruned = 0
  for (const aggregate of SNAPSHOT_COMPLETE_AGGREGATES) {
    const ids = seen.get(aggregate) ?? new Set<string>()
    const stale: string[] = []
    for (const [id] of pool.entries(aggregate)) {
      if (!ids.has(id)) stale.push(id)
    }
    for (const id of stale) {
      pool.remove(aggregate, id)
      if (state.handle) await idb.deleteModel(state.handle, aggregate, id)
      pruned += 1
    }
  }
  if (pruned > 0) logger.info('sync bootstrap: pruned rows absent from the snapshot', { pruned })
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
    let dropped = 0
    for (const aggregate of ASSET_AGGREGATES) {
      const ids = Array.from(pool.entries(aggregate), ([id]) => id)
      for (const id of ids) {
        pool.remove(aggregate, id)
        if (state.handle) await idb.deleteModel(state.handle, aggregate, id)
        dropped += 1
      }
    }
    if (key) writeAssetsHidden(key, true)
    if (dropped > 0) logger.info('sync: assets hidden from this viewer; dropped them', { dropped })
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
