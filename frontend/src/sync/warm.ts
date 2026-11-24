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
 * (needs the full bootstrap stream), and say which cold groups are only stale
 * (their cached rows still track the cursor). A group is warm only when ALL
 * of:
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
 *    continuously since their snapshot";
 *  - that snapshot is fresh (see isFreshWatermark). A delta never removes a
 *    row whose removal it missed, so a snapshot past the max age is taken
 *    again, and its strip drops whatever lingered.
 */
export async function partitionWarmGroups(
  groups: string[],
): Promise<{ warm: string[]; cold: string[]; stale: string[] }> {
  if (!state.handle || pool.getLastXid8() <= 0 || pool.size() === 0) {
    return { warm: [], cold: groups, stale: [] }
  }
  const versionsTag = await idb.getModelVersionsTag(state.handle)
  if (versionsTag !== MODEL_VERSIONS_TAG) {
    return { warm: [], cold: groups, stale: [] }
  }
  // Read in turn with the watermark writes: a drop already queued (a group
  // let go and taken again) must count, or the group would delta from rows
  // its unsubscribe stripped.
  const handle = state.handle
  const watermarks = (await serialized(() => idb.getGroupWatermarks(handle))) ?? {}
  const maxAge = snapshotMaxAgeSecs()
  const now = Date.now()
  const warm: string[] = []
  const cold: string[] = []
  // Cold for being too old, not for lacking a snapshot: the cached rows a
  // delta keeps current until the new snapshot strips them.
  const stale: string[] = []
  for (const g of groups) {
    if (watermarks[g] != null) sessionMarks.set(g, watermarks[g])
    else sessionMarks.delete(g)
    if (isFreshWatermark(watermarks[g], maxAge, now)) {
      warm.push(g)
    } else {
      cold.push(g)
      if (watermarks[g] != null) stale.push(g)
    }
  }
  return { warm, cold, stale }
}

/** The max age when the server never sent one (a 1.1 server): a day. */
export const DEFAULT_SNAPSHOT_MAX_AGE_SECS = 86_400

/** How old a snapshot may grow, in seconds; 0 means never too old. */
export function snapshotMaxAgeSecs(): number {
  return state.snapshotMaxAgeSecs ?? DEFAULT_SNAPSHOT_MAX_AGE_SECS
}

/**
 * Whether a watermark stands for a snapshot a delta may still bring current.
 * Its age is measured on this device's clock, which also stamped it, so skew
 * against the server's clock plays no part. A watermark from before ages were
 * kept (a bare xid8) is stale whatever the max age: it marks a snapshot that
 * never stripped `legacy` owners. A stamp in the future (the clock was set
 * back) is stale too.
 */
export function isFreshWatermark(
  mark: idb.StoredWatermark | undefined,
  maxAgeSecs: number,
  now: number,
): boolean {
  if (mark == null || typeof mark === 'number') return false
  if (maxAgeSecs <= 0) return true
  const age = now - mark.at
  return age >= 0 && age < maxAgeSecs * 1000
}

/**
 * Take the max age a bootstrap or delta carried (absent from a 1.1 server,
 * which leaves the current value), and keep it for the next launch.
 */
export function noteSnapshotMaxAge(secs: number | undefined): void {
  if (typeof secs !== 'number' || secs === state.snapshotMaxAgeSecs) return
  state.snapshotMaxAgeSecs = secs
  if (state.handle) {
    void idb.setSnapshotMaxAge(state.handle, secs).catch((error: unknown) => {
      logger.warn('sync: storing the snapshot max age failed', { error })
    })
  }
}

/**
 * Each group's watermark as this session knows it: read at its warm split,
 * then as this session records and drops them. Not the cached map, which
 * other tabs write too: another tab's fresh snapshot stripped its own pool,
 * not this one, so this session's age is what says this pool is stale.
 */
const sessionMarks = new Map<string, idb.StoredWatermark>()

/** Forget this session's watermarks (tear-down, or a wiped cache). */
export function resetSessionMarks(): void {
  sessionMarks.clear()
}

/** Of `groups`, those this session holds a watermark for that is no longer
 * fresh. A group without one is cold already, or its first snapshot is still
 * streaming. */
export function staleWatermarkedGroups(groups: readonly string[]): string[] {
  const maxAge = snapshotMaxAgeSecs()
  const now = Date.now()
  return groups.filter((g) => {
    const mark = sessionMarks.get(g)
    return mark != null && !isFreshWatermark(mark, maxAge, now)
  })
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

/** Record that each group's snapshot completed at `xid8`, now (see
 * idb.setGroupWatermarks), in turn with every other watermark write. */
export function recordGroupWatermarks(
  handle: idb.IdbHandle,
  groups: readonly string[],
  xid8: number,
): Promise<void> {
  if (groups.length === 0) return Promise.resolve()
  const mark: idb.GroupWatermark = { xid8, at: Date.now() }
  for (const g of groups) sessionMarks.set(g, mark)
  return serialized(() => idb.setGroupWatermarks(handle, [...groups], mark))
}

/** Drop the watermarks of `groups`, in turn with every other watermark write.
 * Their cached rows then count as cold on the next launch. */
export function dropGroupWatermarks(handle: idb.IdbHandle, groups: Iterable<string>): Promise<void> {
  const dropped = new Set(groups)
  if (dropped.size === 0) return Promise.resolve()
  for (const g of dropped) sessionMarks.delete(g)
  return serialized(async () => {
    const marks = (await idb.getGroupWatermarks(handle)) ?? {}
    const kept: Record<string, idb.StoredWatermark> = {}
    for (const [g, mark] of Object.entries(marks)) {
      if (!dropped.has(g)) kept[g] = mark
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
