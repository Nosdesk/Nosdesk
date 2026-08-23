/**
 * Wire types for the sync engine. These mirror the Rust-side
 * `handlers::sync::*` shapes; treat them as the API contract
 * between the two sides.
 *
 * Aggregate names match the `sync_aggregate` Postgres ENUM and the
 * `SyncAggregate` Rust enum (`backend/sync-models/<name>.json`).
 * Adding a new aggregate is a coordinated change on both sides plus
 * a manifest update.
 */

export type SyncAggregate =
  | 'ticket'
  | 'project'
  | 'project_ticket'
  | 'ticket_asset'
  | 'linked_ticket'
  | 'cycle_ticket'
  | 'workflow_state'
  | 'comment'
  | 'attachment'
  | 'assignment'
  | 'group_membership'
  | 'plugin'
  | 'user'
  | 'asset'
  | 'asset_media'
  | 'asset_lifecycle_event'
  | 'asset_usage'
  | 'asset_audit'
  | 'asset_loan'
  | 'cycle'
  | 'notification'
  | 'documentation_page'
  | 'documentation_collection'

export type SyncOp = 'I' | 'U' | 'D' | 'A'

/**
 * One row of `sync_actions` as returned by /api/sync/delta and
 * pushed via the SSE SyncActions frame. The shape matches
 * `handlers::sync::delta::ActionRow` on the server.
 */
export interface SyncAction {
  sync_id: number
  aggregate: SyncAggregate
  aggregate_id: string
  op: SyncOp
  event_type: string
  schema_version: number
  data: Record<string, unknown>
  /**
   * Postgres returns array elements as nullable so the type in TS
   * carries the nullability; in practice every group is a non-null
   * string. Filter `?? []` rather than mapping to bare strings to
   * keep the surface honest about the substrate.
   */
  groups: Array<string | null>
  actor_uuid: string | null
  actor_kind: string
  actor_ref: string | null
  correlation_id: string | null
  causation_id: string | null
  occurred_at: string
}

export interface DeltaResponse {
  actions: SyncAction[]
  /** Commit-safe cursor for the next request, paired with
   * `last_sync_id` as `(last_xid8, last_sync_id)`. See the backend
   * `crate::sync::feed` module. */
  last_xid8: number
  last_sync_id: number
  has_more: boolean
  /** The cursor sent predates the oldest action the server still retains, so
   * a delta cannot reconstruct current state: the missed deletes have been
   * pruned, and a bootstrap only upserts so it cannot remove them either.
   * The client must wipe its cache and re-bootstrap. Optional so a client
   * running against an older backend simply never sees it. */
  resync_required?: boolean
  /** Current workspace capability flags, same shape as the bootstrap
   * `__meta__` flags. Carried on every delta so a warm launch that catches
   * up without re-streaming the snapshot still converges on current flags.
   * Absent (older backend, or a failed probe server-side) means "keep the
   * flags you have". */
  capabilities?: {
    sla_enabled?: boolean
  }
}

export interface BootstrapMeta {
  /** Server's compiled schema hash (NOSDESK_SCHEMA_HASH). */
  server_schema: string
  /** Commit-safe cursor floor for subsequent /api/sync/delta calls,
   * paired with `last_sync_id`. Seeded at `horizon - 1` so the
   * snapshot and the first delta partition with no gap. */
  last_xid8: number
  /** Cursor for subsequent /api/sync/delta calls. */
  last_sync_id: number
  /** The intersection of requested groups and the caller's permitted set. */
  groups_granted: string[]
  /** Workspace capability flags. Read once per bootstrap. The
   * frontend uses these to gate optional UI surfaces — feature
   * chrome is hidden entirely when the workspace hasn't opted in. */
  sla_enabled?: boolean
}

/**
 * Per-row payload streamed during /api/sync/bootstrap. Each line in
 * the NDJSON response is one of these envelope shapes — the
 * `__meta__` header, a `__model__`-tagged row, the closing
 * `__end__`, or an `__error__` if streaming failed mid-snapshot.
 */
export type BootstrapLine =
  | { __meta__: BootstrapMeta }
  | { __model__: SyncAggregate; [field: string]: unknown }
  | { __end__: { last_xid8: number; last_sync_id: number } }
  | { __error__: string; detail?: string }

export interface PushTransaction {
  tx_id: string
  aggregate: SyncAggregate
  model_id: string
  op: SyncOp
  patch: Record<string, unknown>
  base_sync_id?: number | null
}

export interface PushResponse {
  applied: string[]
  rejected: Array<{ tx_id: string; reason: string; detail: string }>
  last_sync_id: number
}
