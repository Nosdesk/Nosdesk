/**
 * Warm launch: which subscribed groups a delta can bring current from the
 * cached snapshot, and the torn-cache invariant that keeps that answer true.
 */
import { logger } from '@nosdesk/core/utils/logger'
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import { state } from './runtimeState'
import { MODEL_VERSIONS_TAG } from './schemaVersions'
import { stripGroupsAndCache } from './ownership'

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
  // Read in turn with the watermark writes: a drop already queued (a group
  // let go and taken again) must count, or the group would delta from rows
  // its unsubscribe stripped.
  const handle = state.handle
  const watermarks = (await serialized(() => idb.getGroupWatermarks(handle))) ?? {}
  const warm: string[] = []
  const cold: string[] = []
  for (const g of groups) {
    ;(watermarks[g] != null ? warm : cold).push(g)
  }
  return { warm, cold }
}

/**
 * Every read-modify-write of the watermark map runs on this chain, one after
 * another. Two interleaved ones would each write back the map they read, so a
 * set landing after a drop could bring the dropped group back.
 */
let watermarkChain: Promise<unknown> = Promise.resolve()

function serialized<T>(write: () => Promise<T>): Promise<T> {
  const run = watermarkChain.then(write, write)
  watermarkChain = run.catch(() => {})
  return run
}

/** Record that each group's snapshot completed at `xid8` (see
 * idb.setGroupWatermarks), in turn with every other watermark write. */
export function recordGroupWatermarks(
  handle: idb.IdbHandle,
  groups: readonly string[],
  xid8: number,
): Promise<void> {
  if (groups.length === 0) return Promise.resolve()
  return serialized(() => idb.setGroupWatermarks(handle, [...groups], xid8))
}

/** Drop the watermarks of `groups`, in turn with every other watermark write.
 * Their cached rows then count as cold on the next launch. */
export function dropGroupWatermarks(handle: idb.IdbHandle, groups: Iterable<string>): Promise<void> {
  const dropped = new Set(groups)
  if (dropped.size === 0) return Promise.resolve()
  return serialized(async () => {
    const marks = (await idb.getGroupWatermarks(handle)) ?? {}
    const kept: Record<string, number> = {}
    for (const [g, xid8] of Object.entries(marks)) {
      if (!dropped.has(g)) kept[g] = xid8
    }
    if (Object.keys(kept).length !== Object.keys(marks).length) {
      await idb.replaceGroupWatermarks(handle, kept)
    }
  })
}

/**
 * Drop bootstrap watermarks for groups that are NOT subscribed at the moment
 * this session first advances the cursor. From that advance on, the cursor
 * moves past events those groups never apply, so their cached rows stop
 * tracking it: a later launch must snapshot them again, not delta from a
 * cursor they don't match. Runs once per session, and must complete BEFORE
 * the first advanced cursor is persisted — a crash that persisted the moved
 * cursor but kept the stale watermark would revive the torn cache as warm.
 *
 * A group that loses its watermark loses its rows at the same moment: it is
 * stripped from every pooled row's owners, and a row it owned alone goes,
 * with its cached copy when the cache says the torn groups alone own it.
 * Other cached copies are left for the next hydrate, which drops rows no
 * watermarked group owns (another tab may still be using them). A bootstrap
 * runs this before its strip, so a row owned by a torn group and by a group
 * whose snapshot no longer carries it goes in the same pass.
 */
let watermarksPruned = false

export async function pruneTornWatermarks(): Promise<void> {
  if (watermarksPruned) return
  watermarksPruned = true
  if (!state.handle) return
  const marks = (await idb.getGroupWatermarks(state.handle)) ?? {}
  const subscribed = pool.getSubscribedGroups()
  const dropped = Object.keys(marks).filter((g) => !subscribed.has(g))
  if (dropped.length > 0) {
    logger.info('sync: pruning watermarks for unsubscribed groups', { dropped })
    await dropGroupWatermarks(state.handle, dropped)
    const { removed, cached } = stripGroupsAndCache(state.handle, new Set(dropped))
    if (removed.length > 0) {
      logger.info('sync: dropped rows owned only by pruned groups', { removed: removed.length })
    }
    try {
      await cached
    } catch (error) {
      logger.warn('sync: dropping cached rows of pruned groups failed', { error })
    }
  }
}

/** Re-arm the once-per-session prune; called on tear-down. */
export function resetWatermarkPrune(): void {
  watermarksPruned = false
}
