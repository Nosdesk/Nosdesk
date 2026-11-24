/**
 * Which rows the owner model governs, and the rules that keep a row's owners
 * in step with its groups' watermarks.
 */
import * as pool from '@nosdesk/core/sync/pool'
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
 * caller to drop from the cache, or leave to the next hydrate.
 */
export function stripGroups(groups: ReadonlySet<string>): Array<[SyncAggregate, string]> {
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
      }
    }
  }
  return removed
}
