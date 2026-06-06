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
  | 'workflow_state'
  | 'comment'
  | 'attachment'
  | 'assignment'
  | 'group_membership'
  | 'plugin'
  | 'user'
  | 'asset'
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
  last_sync_id: number
  has_more: boolean
}

export interface BootstrapMeta {
  /** Server's compiled schema hash (NOSDESK_SCHEMA_HASH). */
  server_schema: string
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
  | { __end__: { last_sync_id: number } }
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
