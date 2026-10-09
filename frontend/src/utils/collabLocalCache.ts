/**
 * Shared helpers for the browser-side collaborative-document cache
 * (`y-indexeddb`). Each collab doc is persisted in its own IndexedDB
 * database named after its docId (`ws-{workspaceUuid}_{kind}-{id}`).
 *
 * This module owns the touch-map key, the cross-tab "open" lock and the
 * bulk purge so every caller agrees on one source of truth:
 *   - `stores/collabSession.ts`: holds a doc's lock while it is open, and
 *     the LRU prune of individual docs.
 *   - `sync/lifecycle.ts`: the epoch fence, which wipes everything when the
 *     server's database instance id changes.
 *   - `stores/workspaceReset.ts`: sign-out and workspace switch.
 *
 * Deleting a database another tab has open closes that tab's connection
 * to it. Each tab holds a shared Web Lock per open doc, so a cleanup that
 * should leave open docs alone can tell which ones another tab has.
 */
import { clearDocument as clearIdbDocument } from 'y-indexeddb'
import { logger } from '@nosdesk/core/utils/logger'

/**
 * localStorage key holding `{ docId: lastTouchedMs }` for every collab
 * doc with a local store. Persistent across reloads so the LRU prune
 * and the epoch wipe both know which databases exist.
 */
export const COLLAB_IDB_TOUCH_KEY = 'nosdesk:collab-idb-touched'

/** Every collab docId (and thus its y-indexeddb database name) starts
 *  with this workspace-namespace prefix. */
const COLLAB_DB_PREFIX = 'ws-'

/** Web Lock name for a doc's local store. */
const LOCK_PREFIX = 'nosdesk:collab-idb:'

function readTouchMap(): Record<string, number> {
  if (typeof localStorage === 'undefined') return {}
  try {
    const raw = localStorage.getItem(COLLAB_IDB_TOUCH_KEY)
    return raw ? (JSON.parse(raw) as Record<string, number>) : {}
  } catch {
    return {}
  }
}

/** Drop docs from the touch map, re-read so a touch from another tab in
 *  the meantime survives. */
export function forgetTouchedDocs(docIds: Iterable<string>): void {
  if (typeof localStorage === 'undefined') return
  const map = readTouchMap()
  for (const docId of docIds) delete map[docId]
  try {
    if (Object.keys(map).length === 0) localStorage.removeItem(COLLAB_IDB_TOUCH_KEY)
    else localStorage.setItem(COLLAB_IDB_TOUCH_KEY, JSON.stringify(map))
  } catch {
    // Quota / sandboxed origin; nothing else to do.
  }
}

/** The Web Locks API, where the browser has it (Safari and WKWebView from
 *  15.4, Chromium from 69, Firefox from 96; secure contexts only). */
function lockManager(): LockManager | null {
  if (typeof navigator === 'undefined') return null
  return navigator.locks ?? null
}

/** Whether this tab can tell which docs other tabs have open. Without it, a
 *  cleanup that should spare open docs has to skip everything. */
export function canSeeOpenDocs(): boolean {
  return lockManager() !== null
}

/**
 * Hold a shared lock on a doc's local store for as long as this tab has it
 * open. Returns the release, which resolves once the lock is let go. A no-op
 * where the browser has no Web Locks.
 */
export function holdCollabDocLock(docId: string): () => Promise<void> {
  const locks = lockManager()
  if (!locks) return async () => {}
  let letGo: (() => void) | null = null
  let released = false
  // Releasing before the lock is granted (it may be queued behind another
  // tab's delete) withdraws the request rather than waiting for it.
  const pending = new AbortController()
  const held = locks
    .request(LOCK_PREFIX + docId, { mode: 'shared', signal: pending.signal }, () => {
      // Granted after the release: let go at once.
      if (released) return
      return new Promise<void>((resolve) => (letGo = resolve))
    })
    .catch((err: unknown) => {
      if (!pending.signal.aborted) logger.warn('Collab cache: lock request failed', { docId, err })
    })
  return () => {
    released = true
    if (letGo) letGo()
    else pending.abort()
    return held.then(() => {})
  }
}

/**
 * Delete a doc's local store unless another tab has the doc open. Resolves
 * to whether it was deleted; false where the browser has no Web Locks.
 */
export async function clearCollabDocIfClosed(docId: string): Promise<boolean> {
  const locks = lockManager()
  if (!locks) return false
  return locks.request(
    LOCK_PREFIX + docId,
    { mode: 'exclusive', ifAvailable: true },
    async (lock) => {
      if (!lock) return false
      await clearIdbDocument(docId)
      return true
    },
  )
}

export interface PurgeOptions {
  /**
   * Also delete docs other tabs have open. Sign-out and the epoch fence set
   * it: after either, no tab's copy should stay on disk. A tab whose copy is
   * deleted under it carries on without one (`collabSession` turns its
   * persistence off) and its edits still reach the server. A workspace switch
   * leaves it unset: other tabs may be working in either workspace.
   */
  includeOpen: boolean
}

/**
 * Delete every local collaborative-document store and forget the deleted
 * ones in the touch map. Best-effort and idempotent. Without `includeOpen`,
 * docs another tab has open are kept, and so is everything where the browser
 * cannot say which those are.
 */
export async function purgeAllCollabDocs({ includeOpen }: PurgeOptions): Promise<void> {
  if (!includeOpen && !canSeeOpenDocs()) return
  const names = new Set<string>(Object.keys(readTouchMap()))

  // Catch orphans the touch map missed (e.g. localStorage cleared
  // independently). `indexedDB.databases()` is unsupported on some
  // browsers (notably Firefox); the touch map is the fallback there.
  try {
    if (typeof indexedDB !== 'undefined' && typeof indexedDB.databases === 'function') {
      const dbs = await indexedDB.databases()
      for (const db of dbs) {
        if (db.name && db.name.startsWith(COLLAB_DB_PREFIX)) names.add(db.name)
      }
    }
  } catch (err) {
    logger.warn('purgeAllCollabDocs: indexedDB.databases() enumeration failed', { err })
  }

  const cleared: string[] = []
  await Promise.all(
    [...names].map(async (docId) => {
      try {
        if (includeOpen) await clearIdbDocument(docId)
        else if (!(await clearCollabDocIfClosed(docId))) return
        cleared.push(docId)
      } catch (err) {
        logger.warn('purgeAllCollabDocs: clearDocument failed', { docId, err })
      }
    }),
  )
  forgetTouchedDocs(cleared)
}
