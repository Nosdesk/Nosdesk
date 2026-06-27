/**
 * Shared helpers for the browser-side collaborative-document cache
 * (`y-indexeddb`). Each collab doc is persisted in its own IndexedDB
 * database named after its docId (`ws-{workspaceUuid}_{kind}-{id}`).
 *
 * This module owns the touch-map key and the bulk purge so the two
 * callers agree on one source of truth:
 *   - `stores/collabSession.ts` — LRU prune of individual docs.
 *   - `sync/lifecycle.ts` — the epoch fence: wipe everything when the
 *     server's database instance id changes (cached docs belong to a
 *     different data generation). See
 *     docs/plans/collab-stale-cache-fence.md.
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

function readTouchedDocIds(): string[] {
  if (typeof localStorage === 'undefined') return []
  try {
    const raw = localStorage.getItem(COLLAB_IDB_TOUCH_KEY)
    if (!raw) return []
    return Object.keys(JSON.parse(raw) as Record<string, number>)
  } catch {
    return []
  }
}

/**
 * Delete every local collaborative-document store and clear the touch
 * map. Best-effort and idempotent. Called by the epoch fence on a
 * database-instance change and on logout.
 */
export async function purgeAllCollabDocs(): Promise<void> {
  const names = new Set<string>(readTouchedDocIds())

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

  await Promise.all(
    [...names].map((docId) =>
      clearIdbDocument(docId).catch((err) =>
        logger.warn('purgeAllCollabDocs: clearDocument failed', { docId, err }),
      ),
    ),
  )

  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.removeItem(COLLAB_IDB_TOUCH_KEY)
    } catch {
      // Quota / sandboxed origin; nothing else to do.
    }
  }
}
