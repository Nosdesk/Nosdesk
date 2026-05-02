/**
 * Object pool: in-memory home for every aggregate row the sync
 * engine has loaded. Module-level singleton — not a Pinia store.
 *
 * Why a module-level singleton, not a Pinia store:
 * - Pinia stores are immortal (Pinia Colada explicitly warns about
 *   this); a long-lived per-row store would never get GC'd.
 * - DevTools choke on a Pinia state graph with thousands of rows.
 * - The pool is read by `useEntity`/`useReference` composables that
 *   wrap `pool.get` in `computed`; Pinia's machinery isn't in the
 *   path.
 *
 * Why `reactive(new Map())`:
 * - Vue 3 fixed Map reactivity — set/delete trigger effects, so
 *   iterators in components react to row insertions and deletions
 *   without an external subscriber registry.
 *
 * Why `shallowReactive(row)` for individual rows:
 * - Root-level field writes auto-trigger; nested mutation does not.
 *   Acceptable because rows are flat by schema design (FKs as ids,
 *   no nested objects).
 * - `shallowRef` + `Object.assign` (a tempting alternative) does
 *   NOT trigger reactivity on inner mutation — would require an
 *   error-prone explicit `triggerRef` after every assignment.
 */
import { reactive, shallowReactive, type ShallowReactive } from 'vue'
import type { SyncAggregate } from './types'

type Key = `${SyncAggregate}:${string}`

const rows = reactive(new Map<Key, ShallowReactive<Record<string, unknown>>>())

/**
 * Process-wide cursor. Every successful delta / bootstrap / SSE
 * frame advances this; the lifecycle layer reads it back when
 * issuing the next delta request.
 */
let lastSyncId = 0

/**
 * Compiled-into-the-binary schema hash (mirrors the server's
 * NOSDESK_SCHEMA_HASH). Set once at lifecycle bootstrap; consumers
 * compare it to the persisted IndexedDB hash to decide whether to
 * wipe-and-rebootstrap on warm start.
 */
let schemaHash = ''

/**
 * Groups the runtime is currently subscribed to. The router's
 * route-change hook adds new groups on entry; a group-eviction
 * task drops them after a 5-minute grace.
 */
const subscribedGroups = reactive(new Set<string>())

function key(aggregate: SyncAggregate, id: string | number): Key {
  return `${aggregate}:${id}` as Key
}

export function upsert<T extends object>(
  aggregate: SyncAggregate,
  id: string | number,
  data: T,
): ShallowReactive<T> {
  const k = key(aggregate, id)
  const existing = rows.get(k)
  if (existing) {
    // shallowReactive root-level writes auto-trigger; this updates
    // the same ShallowReactive instance every consumer of get() is
    // already holding.
    Object.assign(existing, data)
    return existing as ShallowReactive<T>
  }
  const created = shallowReactive({ ...data }) as ShallowReactive<T>
  rows.set(k, created as ShallowReactive<Record<string, unknown>>)
  return created
}

export function patch<T extends object>(
  aggregate: SyncAggregate,
  id: string | number,
  partial: Partial<T>,
): ShallowReactive<T> | undefined {
  const existing = rows.get(key(aggregate, id))
  if (!existing) return undefined
  Object.assign(existing, partial)
  return existing as ShallowReactive<T>
}

export function remove(aggregate: SyncAggregate, id: string | number): boolean {
  return rows.delete(key(aggregate, id))
}

export function get<T extends object>(
  aggregate: SyncAggregate,
  id: string | number,
): ShallowReactive<T> | undefined {
  return rows.get(key(aggregate, id)) as ShallowReactive<T> | undefined
}

export function has(aggregate: SyncAggregate, id: string | number): boolean {
  return rows.has(key(aggregate, id))
}

/**
 * Iterate every row of an aggregate. Touches the reactive Map so
 * the caller's `computed` re-evaluates when a row of `aggregate`
 * is inserted or deleted. Yields the live ShallowReactive
 * instances; mutations propagate to all subscribers.
 */
export function* iterate<T extends object>(
  aggregate: SyncAggregate,
): IterableIterator<ShallowReactive<T>> {
  const prefix = `${aggregate}:`
  for (const [k, row] of rows) {
    if (k.startsWith(prefix)) yield row as ShallowReactive<T>
  }
}

export function size(): number {
  return rows.size
}

export function getLastSyncId(): number {
  return lastSyncId
}

export function setLastSyncId(id: number): void {
  if (id > lastSyncId) lastSyncId = id
}

export function getSchemaHash(): string {
  return schemaHash
}

export function setSchemaHash(hash: string): void {
  schemaHash = hash
}

export function getSubscribedGroups(): ReadonlySet<string> {
  return subscribedGroups
}

export function subscribe(group: string): void {
  subscribedGroups.add(group)
}

export function unsubscribe(group: string): boolean {
  return subscribedGroups.delete(group)
}

/**
 * Wipe every row, reset the cursor and subscriptions. Called by
 * the lifecycle layer on schema-hash mismatch or sign-out.
 */
export function reset(): void {
  rows.clear()
  subscribedGroups.clear()
  lastSyncId = 0
  schemaHash = ''
}
