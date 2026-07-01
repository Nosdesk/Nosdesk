//! Asset lifecycle transitions and their append-only event log.
//!
//! A transition does two writes in one transaction: it updates the
//! current `assets.status` and inserts an `asset_lifecycle_events`
//! row capturing the move (from/to, reason, linked ticket, and any
//! state-specific `metadata`). Both the asset and the new event emit
//! sync actions, so list views and the per-asset timeline stay live.

use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetDisposal, AssetLifecycleEvent, NewAssetDisposal, NewAssetLifecycleEvent,
    SyncAggregate, SyncOp,
};
use crate::repository::assets::emit_asset_event;
use crate::schema::{asset_disposals, asset_lifecycle_events, assets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

fn event_sync_payload(row: &AssetLifecycleEvent) -> serde_json::Value {
    json!({
        "id": row.id,
        "asset_id": row.asset_id,
        "from_status": row.from_status,
        "to_status": row.to_status,
        "reason": row.reason,
        "ticket_id": row.ticket_id,
        "metadata": row.metadata,
        "actor_uuid": row.actor_uuid,
        "occurred_at": row.occurred_at,
    })
}

pub fn list_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
) -> QueryResult<Vec<AssetLifecycleEvent>> {
    asset_lifecycle_events::table
        .filter(asset_lifecycle_events::asset_id.eq(asset_id))
        .order(asset_lifecycle_events::occurred_at.desc())
        .load(conn)
}

/// Inputs for a single lifecycle transition. `to_status` is assumed
/// validated by the caller (handler) against `AssetStatus`.
pub struct TransitionInput {
    pub asset_id: i32,
    pub to_status: String,
    pub reason: Option<String>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<uuid::Uuid>,
    /// Disposal record to capture in the same transaction. Only meaningful for
    /// a transition to `disposed`; `None` for every other transition.
    pub disposal: Option<DisposalInput>,
}

/// The disposal fields captured alongside a transition to `disposed`. Mirrors
/// the settable columns of `asset_disposals`; the asset, event link, and actor
/// are filled in from the transition itself.
pub struct DisposalInput {
    pub sanitization_method: String,
    pub data_bearing: bool,
    pub certificate_file_id: Option<i32>,
    pub itad_vendor: Option<String>,
    pub notes: Option<String>,
}

/// Move an asset to `to_status` and log the transition. Returns the
/// updated asset and the new event. A no-op transition (to the
/// current status) is rejected by the caller, not here; this records
/// whatever it is given.
pub fn transition(
    conn: &mut DbConnection,
    input: TransitionInput,
) -> QueryResult<(Asset, AssetLifecycleEvent)> {
    // emit::record fires for both writes inside this transaction.
    conn.transaction::<(Asset, AssetLifecycleEvent), Error, _>(|conn| {
        let current: Asset = assets::table.find(input.asset_id).first(conn)?;
        let from_status = current.status.clone();

        let updated: Asset = diesel::update(assets::table.find(input.asset_id))
            .set(assets::status.eq(&input.to_status))
            .get_result(conn)?;
        emit_asset_event(conn, &updated, SyncOp::Update, "asset.updated")?;

        let event: AssetLifecycleEvent = diesel::insert_into(asset_lifecycle_events::table)
            .values(&NewAssetLifecycleEvent {
                asset_id: input.asset_id,
                from_status: Some(from_status),
                to_status: input.to_status,
                reason: input.reason,
                ticket_id: input.ticket_id,
                metadata: input.metadata,
                actor_uuid: input.actor_uuid,
            })
            .get_result(conn)?;

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::AssetLifecycleEvent,
                aggregate_id: event.id.to_string(),
                op: SyncOp::Insert,
                event_type: "asset_lifecycle_event.created",
                data: event_sync_payload(&event),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;

        // Capture the disposal record in the same transaction, linked to the
        // event just written. Detail lives only here (not duplicated into the
        // event metadata); it is read on demand for the asset detail + exports.
        if let Some(d) = input.disposal {
            diesel::insert_into(asset_disposals::table)
                .values(&NewAssetDisposal {
                    asset_id: input.asset_id,
                    lifecycle_event_id: Some(event.id),
                    sanitization_method: d.sanitization_method,
                    data_bearing: d.data_bearing,
                    certificate_file_id: d.certificate_file_id,
                    itad_vendor: d.itad_vendor,
                    notes: d.notes,
                    actor_uuid: input.actor_uuid,
                })
                .execute(conn)?;
        }

        Ok((updated, event))
    })
}

/// The disposal record for an asset, if it has been disposed. Disposal is
/// terminal, so there is at most one; ordered newest-first for safety.
pub fn disposal_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
) -> QueryResult<Option<AssetDisposal>> {
    asset_disposals::table
        .filter(asset_disposals::asset_id.eq(asset_id))
        .order(asset_disposals::occurred_at.desc())
        .first(conn)
        .optional()
}

/// Every lifecycle event for the given assets, grouped by asset and newest-first
/// within each, for the history export. Actor + ticket stay as ids (the ticket
/// id is the correlation handle); RLS scopes rows to the workspace as usual. No
/// LIMIT: the export never silently truncates.
pub fn history_for_export(
    conn: &mut DbConnection,
    asset_ids: &[i32],
) -> QueryResult<Vec<AssetLifecycleEvent>> {
    asset_lifecycle_events::table
        .filter(asset_lifecycle_events::asset_id.eq_any(asset_ids))
        .order((
            asset_lifecycle_events::asset_id.asc(),
            asset_lifecycle_events::occurred_at.desc(),
        ))
        .load(conn)
}
