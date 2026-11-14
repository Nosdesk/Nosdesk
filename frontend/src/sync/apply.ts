/**
 * Applying live sync actions (delta pages and SSE frames) to the pool and
 * the IndexedDB cache, and the row keys both the live path and the bootstrap
 * loader use.
 */
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import { noteServerEcho } from './optimisticCreates'
import { notifySyncActions } from '@nosdesk/core/sync/observers'
import { state } from './runtimeState'
import { SCHEMA_VERSIONS } from './schemaVersions'
import { pruneTornWatermarks } from './warm'
import { FETCHABLE, referenceFetcher } from './referenceFetchers'
import type { SyncAction, SyncAggregate } from '@nosdesk/core/sync/types'

/**
 * Junction aggregates whose pool key is a composite of their fields
 * rather than a standalone `id`/`uuid`. Their sync rows ARE full rows
 * (the payload carries every field); they just key on the composite.
 * `project_ticket` is the canonical example — a project<->ticket
 * association with no surrogate key.
 */
const COMPOSITE_KEY: Partial<
  Record<SyncAggregate, (d: Record<string, unknown>) => string | null>
> = {
  project_ticket: (d) =>
    d.project_id != null && d.ticket_id != null ? `${d.project_id}:${d.ticket_id}` : null,
  ticket_asset: (d) =>
    d.ticket_id != null && d.asset_id != null ? `${d.ticket_id}:${d.asset_id}` : null,
  linked_ticket: (d) =>
    d.ticket_id != null && d.linked_ticket_id != null
      ? `${d.ticket_id}:${d.linked_ticket_id}`
      : null,
  cycle_ticket: (d) =>
    d.cycle_id != null && d.ticket_id != null ? `${d.cycle_id}:${d.ticket_id}` : null,
  // One assignment per ticket — keyed by ticket_id, not a surrogate id.
  assignment: (d) => (d.ticket_id != null ? String(d.ticket_id) : null),
}

/**
 * The pool key for a sync row. Junction aggregates derive it from
 * their fields (see COMPOSITE_KEY). Every other aggregate keys on its
 * own primary key carried in the payload — and for those, a payload
 * lacking both `id` and `uuid` is a *side event* (e.g. knowledge_gap
 * signals, the synthetic `data` audit events): it references a row by aggregate_id without carrying
 * the row, so it returns null and the caller skips the pool write
 * rather than minting a partial/phantom row. Used by both the
 * bootstrap snapshot loader and the live delta/SSE applier so the two
 * paths key rows identically.
 */
export function rowKey(aggregate: SyncAggregate, data: Record<string, unknown>): string | number | null {
  const composite = COMPOSITE_KEY[aggregate]
  if (composite) return composite(data)
  return ((data.id ?? data.uuid) as string | number | undefined) ?? null
}

export function applyActions(actions: SyncAction[]): void {
  for (const action of actions) {
    if (action.op === 'D') {
      // Deletes may carry only the key; fall back to aggregate_id for
      // own-pk aggregates whose delete payload is otherwise empty.
      const id = rowKey(action.aggregate, action.data) ?? action.aggregate_id
      if (id == null) continue
      // Observers run after the row is gone; keep it on the action for them.
      const prior = pool.get<Record<string, unknown>>(action.aggregate, id)
      if (prior) action.prior = { ...prior }
      pool.remove(action.aggregate, id)
      if (state.handle) {
        void idb.deleteModel(state.handle, action.aggregate, String(id))
      }
      continue
    }
    // `rowKey` returns null for side events (an own-pk aggregate whose
    // payload carries no primary key, e.g. a knowledge_gap signal).
    // Skipping those avoids writing a partial row over the cached one;
    // observers still receive them via notifySyncActions for consumers
    // that care.
    const id = rowKey(action.aggregate, action.data)
    if (id == null) continue
    // An update for a row this pool never received: its create was
    // missed (a frame lost across a reconnect) or it only just became
    // visible. Merging the partial payload into nothing would mint a
    // half row (no timestamps, no immutable fields); fetch the whole row
    // instead where a fetcher exists.
    if (action.op === 'U' && FETCHABLE.has(action.aggregate) && !pool.has(action.aggregate, id)) {
      void referenceFetcher(action.aggregate, [String(id)])
      continue
    }
    // A comment's server echo carrying a correlation_id is a pending optimistic
    // create: suppress it from the view (it arrives without its attachments)
    // until the REST reply swaps the temp for the complete row. Structural, by
    // id, no dedup heuristic.
    if (action.aggregate === 'comment' && action.correlation_id) {
      noteServerEcho(action.correlation_id, Number(id))
    }
    const merged = pool.upsert(action.aggregate, id, action.data)
    if (state.handle && SCHEMA_VERSIONS[action.aggregate] != null) {
      // Persist the merged row, not the event payload: an update event
      // carries only the fields it changed, and `put` replaces, so
      // caching the payload would degrade the row to that subset and the
      // next warm launch would rehydrate it without its other fields.
      void idb.putModels(state.handle, [
        {
          aggregate: action.aggregate,
          id: String(id),
          schema_version: SCHEMA_VERSIONS[action.aggregate]!,
          data: { ...merged },
        },
      ])
    }
  }
}

/** SSE handler: applies actions, advances the cursor, persists. */
export function applySseFrame(actions: SyncAction[], lastXid8: number, lastSyncId: number): void {
  applyActions(actions)
  // Until the session's first catch-up completes, a frame's rows apply but
  // the cursor holds at its cached value: jumping it to the frame's "now"
  // would make the in-flight catch-up delta skip the whole offline span
  // (see LifecycleState.caughtUp). The catch-up re-delivers these events.
  if (!state.caughtUp) {
    notifySyncActions(actions)
    return
  }
  pool.setCursor(lastXid8, lastSyncId)
  if (state.handle) {
    const handle = state.handle
    // Prune-then-persist, sequenced: the advanced cursor must never be
    // persisted ahead of the torn-watermark prune (see pruneTornWatermarks).
    void (async () => {
      await pruneTornWatermarks()
      await idb.setLastSyncId(handle, lastSyncId)
      await idb.setLastXid8(handle, lastXid8)
    })()
  }
  // Notify imperative observers (useSyncActions) after the pool is
  // updated. This is the live SSE path, so observers fire on the
  // cross-machine sync stream; initial hydrate deliberately does not.
  notifySyncActions(actions)
}
