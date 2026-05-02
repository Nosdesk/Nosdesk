//! Sync action recording.
//!
//! Every tier-1 repository write must call [`record`] once in the
//! same transaction as the SQL write. The helper centralises the
//! schema-version lookup and actor projection so call sites stay
//! terse:
//!
//! ```ignore
//! conn.transaction(|conn| {
//!     let updated = diesel::update(...).get_result(conn)?;
//!     let groups = sync::groups::for_ticket(conn, &updated)?;
//!     sync::emit::record(conn, sync::emit::SyncEmit {
//!         aggregate: SyncAggregate::Ticket,
//!         aggregate_id: updated.id.to_string(),
//!         op: SyncOp::Update,
//!         event_type: "ticket.workflow_state_changed",
//!         data: json!({ "workflow_state_id": new_state_id }),
//!         groups,
//!         actor,
//!         causation_id: None,
//!     })?;
//!     Ok(updated)
//! })?
//! ```

use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{SyncAggregate, SyncOp};
use crate::schema::sync_actions;
use crate::sync::actor::ActorContext;
use crate::sync::registry;

/// Inputs for a single sync_actions row. The `actor` field provides
/// `actor_uuid`, `actor_kind`, `actor_ref`, `correlation_id`, and
/// `client_tx_id` so call sites pass one struct instead of five
/// scattered arguments.
pub struct SyncEmit<'a> {
    pub aggregate: SyncAggregate,
    pub aggregate_id: String,
    pub op: SyncOp,
    pub event_type: &'a str,
    pub data: Value,
    pub groups: Vec<String>,
    pub actor: &'a ActorContext,
    /// Optional pointer to the event that caused this one. Use for
    /// fan-out chains (assignment rule → ticket update, plugin event
    /// → backend write).
    pub causation_id: Option<Uuid>,
}

/// Insert one row into `sync_actions`. Returns the assigned
/// `sync_id`, which the caller can stash if it needs to write a
/// causation pointer in a follow-up event in the same request.
pub fn record(conn: &mut DbConnection, e: SyncEmit<'_>) -> QueryResult<i64> {
    let schema_version = registry::schema_version_for(e.aggregate);
    let actor_kind = e.actor.kind.as_str();

    diesel::insert_into(sync_actions::table)
        .values((
            sync_actions::aggregate.eq(e.aggregate),
            sync_actions::aggregate_id.eq(&e.aggregate_id),
            sync_actions::op.eq(e.op),
            sync_actions::event_type.eq(e.event_type),
            sync_actions::schema_version.eq(schema_version),
            sync_actions::data.eq(&e.data),
            sync_actions::groups.eq(&e.groups),
            sync_actions::actor_uuid.eq(e.actor.uuid),
            sync_actions::actor_kind.eq(actor_kind),
            sync_actions::actor_ref.eq(e.actor.reference.as_deref()),
            sync_actions::correlation_id.eq(e.actor.correlation_id),
            sync_actions::causation_id.eq(e.causation_id),
            sync_actions::client_tx_id.eq(e.actor.client_tx_id.as_deref()),
        ))
        .returning(sync_actions::sync_id)
        .get_result(conn)
}
