/**
 * Shared helpers for the browser-side collaborative-document cache
 * (`y-indexeddb`). Each collab doc is persisted in its own IndexedDB
 * database, `collab:v2:{docId}` (`collabDbName`), where docId is
 * `ws-{workspaceUuid}_{kind}-{id}`.
 *
 * Clients before editor schema 2 named the database after the bare docId.
 * Such a database only holds what an older client received, so opening the
 * doc merges it into the new one and deletes it once no tab holds it
 * (`readLegacyCollabDoc`, `clearLegacyCollabDocIfClosed`). A stale tab may
 * still write to it; the next open merges again, which is idempotent.
 *
 * This module owns the touch-map key, the cross-tab "open" lock and the
 * bulk purge so every caller agrees on one source of truth:
 *   - `stores/collabSession.ts`: holds a doc's lock while it is open, the
 *     LRU prune of individual docs and the one-time merge.
 *   - `sync/lifecycle.ts`: the epoch fence, which wipes everything when the
 *     server's database instance id changes.
 *   - `stores/workspaceReset.ts`: sign-out and workspace switch.
 *
 * Deleting a database another tab has open closes that tab's connection
 * to it. Each tab holds a shared Web Lock per open database, named after the
 * database, so a cleanup that should leave open docs alone can tell which
 * ones another tab has. An older tab holds the same lock name for the bare
 * docId database it has open.
 */
import * as Y from 'yjs'
import { IndexeddbPersistence, clearDocument as clearIdbDocument } from 'y-indexeddb'
import { logger } from '@nosdesk/core/utils/logger'
import { isTauriRuntime } from '@/platform'

/**
 * localStorage key holding `{ databaseName: lastTouchedMs }` for every collab
 * database with a local store. Persistent across reloads so the LRU prune
 * and the epoch wipe both know which databases exist. Older clients wrote
 * bare docIds here; those entries name legacy databases.
 */
export const COLLAB_IDB_TOUCH_KEY = 'nosdesk:collab-idb-touched'

/** Prefix of a current collab database name. */
const COLLAB_DB_PREFIX = 'collab:v2:'

/** Every collab docId, and so every legacy database name, starts with this
 *  workspace-namespace prefix. */
const LEGACY_DB_PREFIX = 'ws-'

/** Web Lock name prefix; the database name follows. */
const LOCK_PREFIX = 'nosdesk:collab-idb:'

/** The IndexedDB database holding `docId`'s local copy. */
export function collabDbName(docId: string): string {
  return COLLAB_DB_PREFIX + docId
}

/** Whether `name` is a current collab database (not a legacy one). */
export function isCollabDbName(name: string): boolean {
  return name.startsWith(COLLAB_DB_PREFIX)
}

function isLegacyDbName(name: string): boolean {
  return name.startsWith(LEGACY_DB_PREFIX)
}

function readTouchMap(): Record<string, number> {
  if (typeof localStorage === 'undefined') return {}
  try {
    const raw = localStorage.getItem(COLLAB_IDB_TOUCH_KEY)
    return raw ? (JSON.parse(raw) as Record<string, number>) : {}
  } catch {
    return {}
  }
}

/** Database names in the touch map, with when each was last touched. */
export function touchedCollabDbs(): Map<string, number> {
  return new Map(Object.entries(readTouchMap()))
}

/** Record that `docId`'s database was just used. */
export function touchCollabDoc(docId: string): void {
  if (typeof localStorage === 'undefined') return
  const map = readTouchMap()
  map[collabDbName(docId)] = Date.now()
  try {
    localStorage.setItem(COLLAB_IDB_TOUCH_KEY, JSON.stringify(map))
  } catch {
    // Quota / sandboxed origins; tracking degrades to per-tab.
  }
}

/** Drop databases from the touch map, re-read so a touch from another tab
 *  in the meantime survives. */
export function forgetTouchedDbs(names: Iterable<string>): void {
  if (typeof localStorage === 'undefined') return
  const map = readTouchMap()
  for (const name of names) delete map[name]
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

/** The app shell runs a single webview, so this context's own open docs
 *  are every open doc there is. */
function isOnlyContext(): boolean {
  return isTauriRuntime()
}

/** Whether this context can tell which docs others have open: through Web
 *  Locks, or because there are no others. Without it, a cleanup that should
 *  spare open docs has to skip everything. */
export function canSeeOpenDocs(): boolean {
  return lockManager() !== null || isOnlyContext()
}

/**
 * Hold a shared lock on a doc's local store for as long as this tab has it
 * open. Returns the release, which resolves once the lock is let go. A no-op
 * where the browser has no Web Locks.
 */
export function holdCollabDocLock(docId: string): () => Promise<void> {
  const dbName = collabDbName(docId)
  const locks = lockManager()
  if (!locks) return async () => {}
  let letGo: (() => void) | null = null
  let released = false
  // Releasing before the lock is granted (it may be queued behind another
  // tab's delete) withdraws the request rather than waiting for it.
  const pending = new AbortController()
  const held = locks
    .request(LOCK_PREFIX + dbName, { mode: 'shared', signal: pending.signal }, () => {
      // Granted after the release: let go at once.
      if (released) return
      return new Promise<void>((resolve) => (letGo = resolve))
    })
    .catch((err: unknown) => {
      if (!pending.signal.aborted) logger.warn('Collab cache: lock request failed', { dbName, err })
    })
  return () => {
    released = true
    if (letGo) letGo()
    else pending.abort()
    return held.then(() => {})
  }
}

/**
 * Delete a database unless another tab has it open. Resolves to whether it
 * was deleted. Without Web Locks: in the app shell it is deleted (the caller
 * skips this context's own open docs, and there is no other context); in a
 * browser nothing is.
 */
async function clearDbIfClosed(dbName: string): Promise<boolean> {
  const locks = lockManager()
  if (!locks) {
    if (!isOnlyContext()) return false
    await clearIdbDocument(dbName)
    return true
  }
  return locks.request(
    LOCK_PREFIX + dbName,
    { mode: 'exclusive', ifAvailable: true },
    async (lock) => {
      if (!lock) return false
      await clearIdbDocument(dbName)
      return true
    },
  )
}

/** Delete `docId`'s local store unless another tab has the doc open.
 *  Resolves to whether it was deleted. */
export function clearCollabDocIfClosed(docId: string): Promise<boolean> {
  return clearDbIfClosed(collabDbName(docId))
}

/** Delete `docId`'s legacy database unless an older tab still has it open,
 *  and forget it in the touch map once deleted. */
export async function clearLegacyCollabDocIfClosed(docId: string): Promise<boolean> {
  if (!(await clearDbIfClosed(docId))) return false
  forgetTouchedDbs([docId])
  return true
}

/** Delete both of `docId`'s databases, open or not, and forget them. For a
 *  doc the server deleted. */
export async function clearCollabDocEverywhere(docId: string): Promise<void> {
  await Promise.all([clearIdbDocument(collabDbName(docId)), clearIdbDocument(docId)])
  forgetTouchedDbs([collabDbName(docId), docId])
}

/**
 * Whether `docId` has a legacy database. Asked of `indexedDB.databases()`
 * where the browser has it, else of the touch map. Never by opening it:
 * opening a database that doesn't exist creates it.
 */
async function legacyCollabDocExists(docId: string): Promise<boolean> {
  if (typeof indexedDB !== 'undefined' && typeof indexedDB.databases === 'function') {
    try {
      return (await indexedDB.databases()).some((db) => db.name === docId)
    } catch (err) {
      logger.warn('Collab cache: indexedDB.databases() failed', { err })
    }
  }
  return docId in readTouchMap()
}

/**
 * The content of `docId`'s legacy database as one update, or null when it
 * has none. Read through a scratch doc, so nothing written to the live doc
 * lands in the database about to be deleted. GC stays off, as on the live
 * doc, so deleted content reaches the revision history intact.
 */
export async function readLegacyCollabDoc(docId: string): Promise<Uint8Array | null> {
  if (!(await legacyCollabDocExists(docId))) return null
  const scratch = new Y.Doc({ gc: false })
  const legacy = new IndexeddbPersistence(docId, scratch)
  try {
    await legacy.whenSynced
    return Y.encodeStateAsUpdate(scratch)
  } finally {
    await legacy.destroy().catch(() => {})
    scratch.destroy()
  }
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
 * cannot say which those are. Legacy databases go only with `includeOpen`:
 * one not yet merged may hold edits an older client never got to the server,
 * and a workspace switch shouldn't lose them.
 */
export async function purgeAllCollabDocs({ includeOpen }: PurgeOptions): Promise<void> {
  if (!includeOpen && !canSeeOpenDocs()) return
  const names = new Set<string>(Object.keys(readTouchMap()))

  // Catch orphans the touch map missed (e.g. localStorage cleared
  // independently). `indexedDB.databases()` is unsupported on some
  // browsers (notably older Firefox); the touch map is the fallback there.
  try {
    if (typeof indexedDB !== 'undefined' && typeof indexedDB.databases === 'function') {
      const dbs = await indexedDB.databases()
      for (const db of dbs) {
        if (db.name && (isCollabDbName(db.name) || isLegacyDbName(db.name))) names.add(db.name)
      }
    }
  } catch (err) {
    logger.warn('purgeAllCollabDocs: indexedDB.databases() enumeration failed', { err })
  }

  const cleared: string[] = []
  await Promise.all(
    [...names].map(async (dbName) => {
      try {
        if (includeOpen) await clearIdbDocument(dbName)
        else if (!isCollabDbName(dbName) || !(await clearDbIfClosed(dbName))) return
        cleared.push(dbName)
      } catch (err) {
        logger.warn('purgeAllCollabDocs: clearDocument failed', { dbName, err })
      }
    }),
  )
  forgetTouchedDbs(cleared)
}
