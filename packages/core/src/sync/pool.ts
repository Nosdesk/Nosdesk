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
 * Ticket number to id. A number names a ticket within its workspace and the
 * pool holds one workspace, so routes and `#N` references resolve through
 * here. Kept in step with the `ticket` rows by upsert, patch, remove and
 * reset.
 */
const ticketIdsByNumber = reactive(new Map<number, number>())

/** Re-point the index after a ticket row was written, `before` being its prior number. */
function indexTicket(id: string | number, before: unknown, row: Record<string, unknown>): void {
  const ticketId = Number(id)
  if (typeof before === 'number' && before !== row.number && ticketIdsByNumber.get(before) === ticketId) {
    ticketIdsByNumber.delete(before)
  }
  if (typeof row.number === 'number') ticketIdsByNumber.set(row.number, ticketId)
}

/**
 * Process-wide change-feed cursor: the composite `(xid8, sync_id)`
 * pair, advanced lexicographically. `sync_id` alone is not commit-safe
 * (a sequence is assigned at INSERT, so its order can diverge from
 * commit order); the server pairs it with the transaction id `xid8`
 * and only serves rows below the commit horizon. See the backend
 * `crate::sync::feed` module. Every successful delta / bootstrap / SSE
 * frame advances this via `setCursor`; the lifecycle layer reads it
 * back when issuing the next delta request.
 *
 * `lastXid8 === 0` means "not yet seeded by an xid8-bearing source"
 * (a fresh client, or a warm start from a cache written before the
 * commit-safe cursor shipped). The lifecycle layer omits `from_xid8`
 * in that state so the server serves the legacy horizon-gated
 * catch-up; the bootstrap that follows every subscribe reseeds a real
 * xid8.
 */
let lastSyncId = 0
let lastXid8 = 0

/**
 * Compiled-into-the-binary schema hash (mirrors the server's
 * NOSDESK_SCHEMA_HASH). Set once at lifecycle bootstrap; consumers
 * compare it to the persisted IndexedDB hash to decide whether to
 * wipe-and-rebootstrap on warm start.
 */
let schemaHash = ''

/**
 * Groups the runtime is currently subscribed to, sent with every delta
 * request. The workspace group stays for the session; a view adds a ticket
 * or project group on entry and drops it a grace period after the last view
 * holding it leaves (`useGroupSubscription` in the app).
 */
const subscribedGroups = reactive(new Set<string>())

/**
 * When each group was last unsubscribed, as a tick of `subscriptionClock`. A
 * bootstrap reads the clock when it starts; a group unsubscribed after that
 * (re-subscribed since or not) is no longer that bootstrap's to stamp,
 * strip or watermark.
 */
const unsubscribedAt = new Map<string, number>()
let subscriptionClock = 0

/**
 * Owner of a row a REST read or an optimistic write created, with no stream
 * behind it. Session scoped: a snapshot never strips it and it is never
 * cached. The first stream owner to reach the row replaces it.
 */
export const LOCAL_OWNER = 'local'

/**
 * Owner of a cached row written by a build that did not record owners. The
 * first workspace snapshot strips it.
 */
export const LEGACY_OWNER = 'legacy'

/**
 * Row owners: the groups whose snapshot or live events put each row in the
 * pool, plus `local` and `legacy`. Kept beside the rows, not on them, so rows
 * stay flat and an owner change triggers no effects. Sets are never mutated
 * once stored (a change stores a new one), so a bootstrap can share one set
 * across every row it creates.
 */
const owners = new Map<Key, ReadonlySet<string>>()

/**
 * When each row was last written by a live action (delta or SSE), as a tick
 * of `touchClock`. A snapshot reconcile reads the clock when its stream
 * starts and leaves alone rows touched after that.
 */
const touchedAt = new Map<Key, number>()
let touchClock = 0

const NO_OWNERS: ReadonlySet<string> = new Set()

function key(aggregate: SyncAggregate, id: string | number): Key {
  return `${aggregate}:${id}` as Key
}

/** `current` with `added` merged in; `current` itself when nothing is new. */
function mergeOwners(
  current: ReadonlySet<string> | undefined,
  added: ReadonlySet<string>,
): ReadonlySet<string> {
  if (!current || current.size === 0) return added
  if (added.size === 0) return current
  let merged: Set<string> | null = null
  for (const owner of added) {
    if (current.has(owner)) continue
    merged ??= new Set(current)
    merged.add(owner)
  }
  if (!merged) return current
  // A stream now carries the row, so it is no longer only local.
  if (!added.has(LOCAL_OWNER)) merged.delete(LOCAL_OWNER)
  return merged
}

function asOwnerSet(list: Iterable<string>): ReadonlySet<string> {
  return list instanceof Set ? list : new Set(list)
}

/**
 * Write a row. With `rowOwners`, they join the row's owners (pass the same
 * set for many rows and it is shared, not copied). Without, a new row is
 * owned `local` and an existing row keeps the owners it has. An empty
 * `rowOwners` is not the same as none: it creates a row with no owners.
 */
export function upsert<T extends object>(
  aggregate: SyncAggregate,
  id: string | number,
  data: T,
  rowOwners?: Iterable<string>,
): ShallowReactive<T> {
  const k = key(aggregate, id)
  const existing = rows.get(k)
  if (existing) {
    const before = existing.number
    // shallowReactive root-level writes auto-trigger; this updates
    // the same ShallowReactive instance every consumer of get() is
    // already holding.
    Object.assign(existing, data)
    if (aggregate === 'ticket') indexTicket(id, before, existing)
    if (rowOwners) owners.set(k, mergeOwners(owners.get(k), asOwnerSet(rowOwners)))
    return existing as ShallowReactive<T>
  }
  const created = shallowReactive({ ...data }) as ShallowReactive<T>
  rows.set(k, created as ShallowReactive<Record<string, unknown>>)
  if (aggregate === 'ticket') indexTicket(id, undefined, created as Record<string, unknown>)
  owners.set(k, rowOwners ? asOwnerSet(rowOwners) : new Set([LOCAL_OWNER]))
  return created
}

/** Add owners to a pooled row. No-op for a row the pool doesn't hold. */
export function addOwners(aggregate: SyncAggregate, id: string | number, added: Iterable<string>): void {
  const k = key(aggregate, id)
  if (!rows.has(k)) return
  owners.set(k, mergeOwners(owners.get(k), asOwnerSet(added)))
}

/**
 * Take `owner` off a pooled row and return how many owners it has left (the
 * caller removes a row left with none). A row the pool doesn't hold has 0.
 */
export function stripOwner(aggregate: SyncAggregate, id: string | number, owner: string): number {
  const k = key(aggregate, id)
  const current = owners.get(k)
  if (!current) return 0
  if (!current.has(owner)) return current.size
  const next = new Set(current)
  next.delete(owner)
  owners.set(k, next)
  return next.size
}

/** The row's owners; empty for a row the pool doesn't hold. */
export function ownersOf(aggregate: SyncAggregate, id: string | number): ReadonlySet<string> {
  return owners.get(key(aggregate, id)) ?? NO_OWNERS
}

/** Record a live action's write to a row (see `touchedAt`). */
export function touch(aggregate: SyncAggregate, id: string | number): void {
  touchedAt.set(key(aggregate, id), ++touchClock)
}

/** The tick of the row's last live write; 0 when none this session. */
export function lastTouch(aggregate: SyncAggregate, id: string | number): number {
  return touchedAt.get(key(aggregate, id)) ?? 0
}

/** The current tick: a row with `lastTouch` above it was touched afterwards. */
export function currentTouch(): number {
  return touchClock
}

export function patch<T extends object>(
  aggregate: SyncAggregate,
  id: string | number,
  partial: Partial<T>,
): ShallowReactive<T> | undefined {
  const existing = rows.get(key(aggregate, id))
  if (!existing) return undefined
  const before = existing.number
  Object.assign(existing, partial)
  if (aggregate === 'ticket') indexTicket(id, before, existing)
  return existing as ShallowReactive<T>
}

export function remove(aggregate: SyncAggregate, id: string | number): boolean {
  const k = key(aggregate, id)
  if (aggregate === 'ticket') {
    const number = rows.get(k)?.number
    if (typeof number === 'number' && ticketIdsByNumber.get(number) === Number(id)) {
      ticketIdsByNumber.delete(number)
    }
  }
  owners.delete(k)
  touchedAt.delete(k)
  return rows.delete(k)
}

/**
 * The id of the pooled ticket with this number, or `undefined` when the pool
 * doesn't hold it. Reactive: a `computed` reading it re-runs when the ticket
 * arrives.
 */
export function ticketIdForNumber(number: number): number | undefined {
  return ticketIdsByNumber.get(number)
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

/**
 * Every `(id, row)` of an aggregate. For reconciliation against a
 * snapshot: the caller decides which ids the snapshot did not carry.
 */
export function* entries<T extends object>(
  aggregate: SyncAggregate,
): IterableIterator<[string, ShallowReactive<T>]> {
  const prefix = `${aggregate}:`
  for (const [k, row] of rows) {
    if (k.startsWith(prefix)) yield [k.slice(prefix.length), row as ShallowReactive<T>]
  }
}

export function size(): number {
  return rows.size
}

export function getLastSyncId(): number {
  return lastSyncId
}

export function getLastXid8(): number {
  return lastXid8
}

/**
 * Advance the composite cursor if `(xid8, syncId)` is lexicographically
 * greater than what we hold. Monotonic: out-of-order frames (an SSE
 * push racing a delta response) can't rewind the cursor.
 */
export function setCursor(xid8: number, syncId: number): void {
  if (xid8 > lastXid8 || (xid8 === lastXid8 && syncId > lastSyncId)) {
    lastXid8 = xid8
    lastSyncId = syncId
  }
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
  if (!subscribedGroups.delete(group)) return false
  unsubscribedAt.set(group, ++subscriptionClock)
  return true
}

/** The current subscription tick, for `unsubscribedSince`. */
export function currentSubscriptionTick(): number {
  return subscriptionClock
}

/** Whether `group` was unsubscribed after `tick`. */
export function unsubscribedSince(group: string, tick: number): boolean {
  return (unsubscribedAt.get(group) ?? 0) > tick
}

/**
 * Wipe every row, reset the cursor and subscriptions. Called by
 * the lifecycle layer on schema-hash mismatch or sign-out.
 */
// Bumped by `reset` (workspace teardown). A fetch that started before a reset
// captures the epoch and drops its result if it changed: its rows belong to
// the previous workspace.
let epoch = 0

/** The pool's current epoch, for async writers to compare after a fetch. */
export function currentEpoch(): number {
  return epoch
}

export function reset(): void {
  epoch++
  rows.clear()
  ticketIdsByNumber.clear()
  owners.clear()
  touchedAt.clear()
  touchClock = 0
  subscribedGroups.clear()
  unsubscribedAt.clear()
  subscriptionClock = 0
  lastSyncId = 0
  lastXid8 = 0
  schemaHash = ''
}
