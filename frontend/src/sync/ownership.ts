/**
 * Which rows the owner model governs, and the rules that keep a row's owners
 * in step with its groups' watermarks.
 */
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import type { SyncAggregate } from '@nosdesk/core/sync/types'

/**
 * Aggregates a snapshot carries: a group's bootstrap streams every row of
 * these it owns, so a row its snapshot did not carry no longer belongs to it.
 * Ownership governs only these. `cycle` has no snapshot and keeps its REST
 * rows as they are.
 */
export const SNAPSHOT_BACKED: ReadonlySet<SyncAggregate> = new Set<SyncAggregate>([
  'ticket',
  'project',
  'project_ticket',
  'documentation_page',
  'documentation_collection',
  'workflow_state',
  'user',
  'asset',
  'comment',
  'attachment',
  'ticket_asset',
  'linked_ticket',
])

function onlyLocal(owners: ReadonlySet<string>): boolean {
  for (const o of owners) if (o !== pool.LOCAL_OWNER) return false
  return true
}

/**
 * Take `groups` off the pool as `stripGroups` does, and delete the cached
 * copies of the rows no other cached owner keeps, when the copy is owned by
 * `groups` alone (see idb.deleteModelsOwnedOnlyBy). Without this, a group
 * that gets a watermark back before the next hydrate would revive a cached
 * row its new snapshot no longer carries: the strip walks only the pool. The
 * delete is issued before this returns. Returns the rows removed from the
 * pool.
 */
export function stripGroupsAndCache(
  handle: idb.IdbHandle | null,
  groups: ReadonlySet<string>,
): { removed: Array<[SyncAggregate, string]>; cached: Promise<void> } {
  const uncached: Array<[SyncAggregate, string]> = []
  const removed = stripGroups(groups, uncached)
  const cached =
    handle && uncached.length > 0
      ? idb.deleteModelsOwnedOnlyBy(handle, uncached, groups)
      : Promise.resolve()
  return { removed, cached }
}

/** Whether `group` is a workspace group, whose snapshot strips `legacy`. */
export function isWorkspaceGroup(group: string): boolean {
  return group.startsWith('workspace:')
}

/**
 * The owners a cached row of a snapshot-backed aggregate keeps at hydrate:
 * those holding a watermark, and `legacy` while a workspace group holds one.
 * A group without a watermark has no snapshot the cursor tracks, so it owns
 * nothing. An empty result drops the row. `local` was never cached.
 */
export function ownersOnHydrate(
  owners: readonly string[],
  watermarks: Readonly<Record<string, unknown>>,
): string[] {
  const workspaceWarm = Object.keys(watermarks).some(
    (g) => isWorkspaceGroup(g) && watermarks[g] != null,
  )
  return owners.filter((o) =>
    o === pool.LEGACY_OWNER ? workspaceWarm : o !== pool.LOCAL_OWNER && watermarks[o] != null,
  )
}

/**
 * Take `groups` off every pooled row of a snapshot-backed aggregate and
 * remove the rows left with no owners. Returns the removed rows' keys for the
 * caller to drop from the cache, or leave to the next hydrate. Rows that lost
 * a group and kept only owners the cache never records (`local`) are
 * appended to `uncached`, when given, with the removed ones.
 */
export function stripGroups(
  groups: ReadonlySet<string>,
  uncached?: Array<[SyncAggregate, string]>,
): Array<[SyncAggregate, string]> {
  const removed: Array<[SyncAggregate, string]> = []
  if (groups.size === 0) return removed
  for (const aggregate of SNAPSHOT_BACKED) {
    const ids = Array.from(pool.entries(aggregate), ([id]) => id)
    for (const id of ids) {
      const owners = pool.ownersOf(aggregate, id)
      if (![...groups].some((g) => owners.has(g))) continue
      let left = owners.size
      for (const g of groups) left = pool.stripOwner(aggregate, id, g)
      if (left === 0) {
        pool.remove(aggregate, id)
        removed.push([aggregate, id])
        uncached?.push([aggregate, id])
      } else if (uncached && onlyLocal(pool.ownersOf(aggregate, id))) {
        uncached.push([aggregate, id])
      }
    }
  }
  return removed
}
