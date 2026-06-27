/**
 * Minimal IndexedDB wrapper for the sync engine.
 *
 * Uses the native IDB API rather than pulling in the `idb` package
 * (~3KB gzipped) — the surface we need is small (open / put / getAll
 * / delete) and the verbosity stays in this one file. If the
 * runtime grows to need transactions across multiple stores or
 * cursor-driven streaming, pulling `idb` becomes worth it.
 *
 * Database name is scoped per-user-per-schema-hash so:
 * - Shared machines don't leak rows across user accounts.
 * - A schema-hash bump on the server forces a fresh database
 *   (the lifecycle layer detects the mismatch and wipes via
 *   `wipe(currentDb)`).
 */
import { logger } from '@nosdesk/core/utils/logger'
import type { PushTransaction, SyncAggregate } from './types'

interface ModelRow {
  aggregate: SyncAggregate
  id: string
  /** Schema version stamped onto each row. Out-of-date rows get
   * dropped on rehydrate; see `loadModels`. */
  schema_version: number
  /** Serialised row payload as it came off the wire. */
  data: Record<string, unknown>
}

interface MetaRow {
  key: string
  value: unknown
}

const STORE_MODELS = 'models'
const STORE_TRANSACTIONS = 'transactions'
const STORE_META = 'meta'

const META_KEY_LAST_SYNC_ID = 'last_sync_id'
const META_KEY_LAST_XID8 = 'last_xid8'
const META_KEY_SCHEMA_HASH = 'schema_hash'
const META_KEY_SUBSCRIBED_GROUPS = 'subscribed_groups'
const META_KEY_INSTANCE_ID = 'instance_id'

export interface IdbHandle {
  db: IDBDatabase
  /** Database name; useful for `wipe()`. */
  name: string
}

/**
 * Open the per-user / per-workspace / per-schema database. Creates the object
 * stores on first run.
 *
 * `workspaceSlug` enters the name only in single-origin path mode, where one
 * origin serves several workspaces and the cache must not be shared between
 * them. Host mode passes no slug and keeps the original name (subdomain installs
 * are already isolated per-origin by the browser). The slug is a safe cache key
 * because retired slugs are never reused, so it never points at two workspaces.
 */
export function open(
  userUuid: string,
  schemaHash: string,
  workspaceSlug?: string | null,
): Promise<IdbHandle> {
  const name = ['nosdesk-sync', userUuid, workspaceSlug || null, schemaHash]
    .filter(Boolean)
    .join('-')
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(name, 1)
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains(STORE_MODELS)) {
        const store = db.createObjectStore(STORE_MODELS, { keyPath: ['aggregate', 'id'] })
        store.createIndex('aggregate', 'aggregate')
      }
      if (!db.objectStoreNames.contains(STORE_TRANSACTIONS)) {
        const store = db.createObjectStore(STORE_TRANSACTIONS, { keyPath: 'tx_id' })
        store.createIndex('createdAt', 'createdAt')
      }
      if (!db.objectStoreNames.contains(STORE_META)) {
        db.createObjectStore(STORE_META, { keyPath: 'key' })
      }
    }
    req.onsuccess = () => resolve({ db: req.result, name })
    req.onerror = () => reject(req.error)
  })
}

/** Delete a previously-opened database wholesale. Used when the
 * server's schema_hash diverges from the client's persisted hash. */
export function wipe(name: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.deleteDatabase(name)
    req.onsuccess = () => resolve()
    req.onerror = () => reject(req.error)
    req.onblocked = () => {
      // Another tab still has the DB open. Resolve anyway — the
      // wipe will complete as soon as that tab releases the
      // handle. Logging here helps diagnose stuck states in dev.
      logger.warn('IDB wipe blocked; another tab still holds an open handle', { name })
      resolve()
    }
  })
}

// -------------------- models -----------------------

export function putModels(
  handle: IdbHandle,
  rows: ModelRow[],
): Promise<void> {
  if (rows.length === 0) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const tx = handle.db.transaction(STORE_MODELS, 'readwrite')
    const store = tx.objectStore(STORE_MODELS)
    for (const row of rows) {
      store.put(row)
    }
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
    tx.onabort = () => reject(tx.error ?? new DOMException('Transaction aborted', 'AbortError'))
  })
}

/** Read every persisted row of an aggregate. Filters out rows
 * whose `schema_version` doesn't match the supplied current value
 * — those become refetches at next bootstrap rather than
 * potentially-incompatible cached state. */
export function loadModels(
  handle: IdbHandle,
  schemaVersionByAggregate: Partial<Record<SyncAggregate, number>>,
): Promise<ModelRow[]> {
  return new Promise((resolve, reject) => {
    const tx = handle.db.transaction(STORE_MODELS, 'readonly')
    const store = tx.objectStore(STORE_MODELS)
    const req = store.getAll()
    req.onsuccess = () => {
      const all = req.result as ModelRow[]
      resolve(
        all.filter(
          (r) =>
            schemaVersionByAggregate[r.aggregate] != null &&
            schemaVersionByAggregate[r.aggregate] === r.schema_version,
        ),
      )
    }
    req.onerror = () => reject(req.error)
  })
}

export function deleteModel(
  handle: IdbHandle,
  aggregate: SyncAggregate,
  id: string,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = handle.db.transaction(STORE_MODELS, 'readwrite')
    tx.objectStore(STORE_MODELS).delete([aggregate, id])
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
}

// -------------------- transactions -----------------

export function putTransaction(handle: IdbHandle, tx: PushTransaction & { createdAt: number; inverse?: Record<string, unknown> }): Promise<void> {
  return new Promise((resolve, reject) => {
    const store = handle.db
      .transaction(STORE_TRANSACTIONS, 'readwrite')
      .objectStore(STORE_TRANSACTIONS)
    const req = store.put(tx)
    req.onsuccess = () => resolve()
    req.onerror = () => reject(req.error)
  })
}

export function deleteTransaction(handle: IdbHandle, txId: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const store = handle.db
      .transaction(STORE_TRANSACTIONS, 'readwrite')
      .objectStore(STORE_TRANSACTIONS)
    const req = store.delete(txId)
    req.onsuccess = () => resolve()
    req.onerror = () => reject(req.error)
  })
}

export function loadTransactions(
  handle: IdbHandle,
): Promise<Array<PushTransaction & { createdAt: number; inverse?: Record<string, unknown> }>> {
  return new Promise((resolve, reject) => {
    const tx = handle.db.transaction(STORE_TRANSACTIONS, 'readonly')
    const req = tx.objectStore(STORE_TRANSACTIONS).getAll()
    req.onsuccess = () =>
      resolve(req.result as Array<PushTransaction & { createdAt: number; inverse?: Record<string, unknown> }>)
    req.onerror = () => reject(req.error)
  })
}

// -------------------- meta -------------------------

function putMeta(handle: IdbHandle, key: string, value: unknown): Promise<void> {
  return new Promise((resolve, reject) => {
    const store = handle.db
      .transaction(STORE_META, 'readwrite')
      .objectStore(STORE_META)
    const req = store.put({ key, value } satisfies MetaRow)
    req.onsuccess = () => resolve()
    req.onerror = () => reject(req.error)
  })
}

function getMeta<T>(handle: IdbHandle, key: string): Promise<T | undefined> {
  return new Promise((resolve, reject) => {
    const tx = handle.db.transaction(STORE_META, 'readonly')
    const req = tx.objectStore(STORE_META).get(key)
    req.onsuccess = () => resolve((req.result as MetaRow | undefined)?.value as T | undefined)
    req.onerror = () => reject(req.error)
  })
}

export function setLastSyncId(handle: IdbHandle, id: number): Promise<void> {
  return putMeta(handle, META_KEY_LAST_SYNC_ID, id)
}

export function getLastSyncId(handle: IdbHandle): Promise<number | undefined> {
  return getMeta<number>(handle, META_KEY_LAST_SYNC_ID)
}

export function setLastXid8(handle: IdbHandle, xid8: number): Promise<void> {
  return putMeta(handle, META_KEY_LAST_XID8, xid8)
}

export function getLastXid8(handle: IdbHandle): Promise<number | undefined> {
  return getMeta<number>(handle, META_KEY_LAST_XID8)
}

export function setSchemaHash(handle: IdbHandle, hash: string): Promise<void> {
  return putMeta(handle, META_KEY_SCHEMA_HASH, hash)
}

export function getSchemaHash(handle: IdbHandle): Promise<string | undefined> {
  return getMeta<string>(handle, META_KEY_SCHEMA_HASH)
}

export function setInstanceId(handle: IdbHandle, id: string): Promise<void> {
  return putMeta(handle, META_KEY_INSTANCE_ID, id)
}

export function getInstanceId(handle: IdbHandle): Promise<string | undefined> {
  return getMeta<string>(handle, META_KEY_INSTANCE_ID)
}

export function setSubscribedGroups(handle: IdbHandle, groups: string[]): Promise<void> {
  return putMeta(handle, META_KEY_SUBSCRIBED_GROUPS, groups)
}

export function getSubscribedGroups(handle: IdbHandle): Promise<string[] | undefined> {
  return getMeta<string[]>(handle, META_KEY_SUBSCRIBED_GROUPS)
}

export type { ModelRow }
