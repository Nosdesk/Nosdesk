//! Sync action recording.
//!
//! Every tier-1 repository write must call [`record`] once in the
//! same transaction as the SQL write. The helper centralises the
//! schema-version lookup and pulls actor + correlation context from
//! the session-local GUCs that [`crate::sync::session::set_actor`]
//! sets at the start of each transaction. Call sites stay terse:
//!
//! ```ignore
//! conn.transaction(|conn| {
//!     sync::session::set_actor(conn, &actor)?;
//!     let updated = diesel::update(...).get_result(conn)?;
//!     let groups = sync::groups::for_ticket(conn, &updated)?;
//!     sync::emit::record(conn, sync::emit::SyncEmit {
//!         aggregate: SyncAggregate::Ticket,
//!         aggregate_id: updated.id.to_string(),
//!         op: SyncOp::Update,
//!         event_type: "ticket.workflow_state_changed",
//!         data: json!({ "workflow_state_id": new_state_id }),
//!         groups,
//!         causation_id: None,
//!     })?;
//!     Ok(updated)
//! })?
//! ```
//!
//! When the GUCs are unset (background tasks that forgot to call
//! `set_actor`, tests that exercise repositories directly), the row
//! still writes with `actor_kind = 'system'` and `actor_uuid = NULL`,
//! so the substrate never blocks a write — but consumers can spot the
//! anonymous events.

use diesel::prelude::*;
use diesel::sql_types::{Array, BigInt, Int2, Jsonb, Text};
use serde_json::Value;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{SyncAggregate, SyncOp};
use crate::sync::registry;

/// Inputs for a single sync_actions row.
pub struct SyncEmit<'a> {
    pub aggregate: SyncAggregate,
    pub aggregate_id: String,
    pub op: SyncOp,
    pub event_type: &'a str,
    pub data: Value,
    pub groups: Vec<String>,
    /// Optional pointer to the event that caused this one. Use for
    /// fan-out chains (assignment rule → ticket update, plugin event
    /// → backend write).
    pub causation_id: Option<Uuid>,
}

#[derive(diesel::QueryableByName)]
struct InsertedRow {
    #[diesel(sql_type = BigInt)]
    sync_id: i64,
}

/// Insert one row into `sync_actions`. Returns the assigned
/// `sync_id`, which the caller can stash if it needs to write a
/// causation pointer in a follow-up event in the same request.
///
/// Actor and correlation columns are resolved from session-local
/// GUCs at INSERT time via `current_setting`. Calling `set_actor`
/// inside the same transaction is the canonical way to populate
/// them; without it the row gets `actor_kind = 'system'` and NULL
/// elsewhere.
///
/// The [`groups::WORKSPACE_GROUP`](crate::sync::groups::WORKSPACE_GROUP)
/// placeholder is resolved to `workspace:<pinned id>` here, from the
/// same `app.workspace_id` GUC the row's `workspace_id` column
/// defaults to — the stored group and the row's tenancy cannot drift.
/// An unpinned transaction leaves the placeholder unresolved, but the
/// insert fails on the column's NOT NULL default before that row
/// could ever be stored.
pub fn record(conn: &mut DbConnection, e: SyncEmit<'_>) -> QueryResult<i64> {
    let schema_version = registry::schema_version_for(e.aggregate);
    let aggregate_str = e.aggregate.as_str();
    let op_str = e.op.as_str();
    let causation_text = e.causation_id.map(|u| u.to_string()).unwrap_or_default();

    let row: InsertedRow = diesel::sql_query(
        "INSERT INTO sync_actions ( \
             aggregate, aggregate_id, op, event_type, schema_version, data, groups, \
             actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id \
         ) VALUES ( \
             $1::sync_aggregate, $2, $3::sync_op, $4, $5, $6, \
             array_replace($7, 'workspace', 'workspace:' || NULLIF(current_setting('app.workspace_id', true), '')), \
             NULLIF(current_setting('app.actor_uuid', true), '')::UUID, \
             COALESCE(NULLIF(current_setting('app.actor_kind', true), ''), 'system'), \
             NULLIF(current_setting('app.actor_ref', true), ''), \
             NULLIF(current_setting('app.correlation_id', true), '')::UUID, \
             NULLIF($8, '')::UUID, \
             NULLIF(current_setting('app.client_tx_id', true), '') \
         ) \
         RETURNING sync_id",
    )
    .bind::<Text, _>(aggregate_str)
    .bind::<Text, _>(&e.aggregate_id)
    .bind::<Text, _>(op_str)
    .bind::<Text, _>(e.event_type)
    .bind::<Int2, _>(schema_version)
    .bind::<Jsonb, _>(&e.data)
    .bind::<Array<Text>, _>(&e.groups)
    .bind::<Text, _>(causation_text)
    .get_result(conn)?;
    Ok(row.sync_id)
}
