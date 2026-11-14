/**
 * Warm launch: which subscribed groups a delta can bring current from the
 * cached snapshot, and the torn-cache invariant that keeps that answer true.
 */
import { logger } from '@nosdesk/core/utils/logger'
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import { state } from './runtimeState'
import { MODEL_VERSIONS_TAG } from './schemaVersions'

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
export async function partitionWarmGroups(
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
 * Drop bootstrap watermarks for groups that are NOT subscribed at the moment
 * this session first advances the cursor. From that advance on, the cursor
 * moves past events those groups never apply, so their cached rows stop
 * tracking it: a later launch must snapshot them again, not delta from a
 * cursor they don't match. Runs once per session, and must complete BEFORE
 * the first advanced cursor is persisted — a crash that persisted the moved
 * cursor but kept the stale watermark would revive the torn cache as warm.
 */
let watermarksPruned = false

export async function pruneTornWatermarks(): Promise<void> {
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

/** Re-arm the once-per-session prune; called on tear-down. */
export function resetWatermarkPrune(): void {
  watermarksPruned = false
}
