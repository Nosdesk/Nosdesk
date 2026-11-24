//! Asset lifecycle transitions and their append-only event log.
//!
//! A transition does two writes in one transaction: it updates the
//! current `assets.status` and inserts an `asset_lifecycle_events`
//! row capturing the move (from/to, reason, linked ticket, and any
//! state-specific `metadata`). Both the asset and the new event emit
//! sync actions, so list views and the per-asset timeline stay live.
//!
//! A transition may also write an asset record, when the caller asks: one
//! into a status a record type opens with opens a record of that type, and
//! one out of it completes the asset's open record. Callers that don't ask
//! (rules, imports, loans) write none. A record never moves a status.
//!
//! This module is the only writer of `assets.status` after create, and
//! every write here passes [`status_rules`] first, under the asset's row
//! lock: a transition ([`transition`]), the loan ledger's issue and return
//! ([`move_for_loan`]), and a create in a status of its own ([`create`]).

use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetDisposal, AssetLifecycleEvent, AssetRecord, AssetRecordType, AssetStatus,
    NewAssetDisposal, NewAssetLifecycleEvent, RecordSource, SyncAggregate, SyncOp,
    SANITIZATION_METHODS,
};
use crate::repository::asset_loans as asset_loans_repo;
use crate::repository::asset_record_types::{self as record_types_repo, RecordTypeError};
use crate::repository::asset_records::{
    self as records_repo, Cost, NewRecord, RecordChanges, RecordError, WorkDate,
};
use crate::repository::assets as assets_repo;
use crate::schema::{asset_disposals, asset_lifecycle_events, assets};
use crate::services::assets::record_catalogue;
use crate::services::assets::status_rules::{self, MoveFacts, MoveRefused, Mover, RefusalCode};
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

/// Reason on the event an import writes for each asset it creates.
pub const IMPORTED_REASON: &str = "Imported";

/// The first lifecycle event of a new asset, from no status to the one it
/// was created in.
pub struct FirstEvent {
    pub reason: Option<String>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<uuid::Uuid>,
}

impl FirstEvent {
    /// The event an import writes for each asset it creates, naming the job.
    pub fn imported(actor_uuid: Option<uuid::Uuid>, import_job_id: uuid::Uuid) -> Self {
        Self {
            reason: Some(IMPORTED_REASON.to_string()),
            metadata: json!({ "import_job_id": import_job_id }),
            actor_uuid,
        }
    }
}

/// Create an asset in `status`, which must be one an asset may start in,
/// linked to the catalog model `model_id` when given. With `first`, the
/// asset's first lifecycle event is written too, from no status. The
/// insert sends one `asset.created` carrying the status.
pub fn create(
    conn: &mut DbConnection,
    new_asset: crate::models::NewAsset,
    model_id: Option<i32>,
    status: AssetStatus,
    first: Option<FirstEvent>,
) -> Result<(Asset, Option<AssetLifecycleEvent>), MoveError> {
    status_rules::check_initial(status)?;
    conn.transaction::<_, MoveError, _>(|conn| {
        let asset = assets_repo::create_device_in(conn, new_asset, model_id, status)?;
        let event = match first {
            Some(first) => Some(insert_event(
                conn,
                NewAssetLifecycleEvent {
                    asset_id: asset.id,
                    from_status: None,
                    to_status: asset.status.clone(),
                    reason: first.reason,
                    ticket_id: None,
                    metadata: first.metadata,
                    actor_uuid: first.actor_uuid,
                },
            )?),
            None => None,
        };
        Ok((asset, event))
    })
}

/// Insert a lifecycle event and send `asset_lifecycle_event.created`.
fn insert_event(
    conn: &mut DbConnection,
    event: NewAssetLifecycleEvent,
) -> QueryResult<AssetLifecycleEvent> {
    let event: AssetLifecycleEvent = diesel::insert_into(asset_lifecycle_events::table)
        .values(&event)
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
    Ok(event)
}

/// The asset, locked `FOR NO KEY UPDATE` until the transaction ends, so a
/// concurrent transition or loan can't change its status between a check
/// and the write.
pub fn lock(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Asset> {
    assets::table.find(asset_id).for_no_key_update().first(conn)
}

/// [`status_rules::check_move`] from a stored status. A value outside the
/// nine (there is no CHECK) moves as an in-service asset does, and a move
/// to `in_service` itself is checked from `in_stock`, whose row is the
/// same, so the value can always be put right.
pub fn check_from_stored(
    stored: &str,
    to: AssetStatus,
    facts: MoveFacts,
) -> Result<(), MoveRefused> {
    let from = match AssetStatus::from_str_opt(stored) {
        Some(from) => from,
        None if to == AssetStatus::InService => AssetStatus::InStock,
        None => AssetStatus::InService,
    };
    status_rules::check_move(from, to, facts)
}

/// Set an asset's `status` and send `asset.updated`. Only the moves in this
/// module call it, each after [`status_rules`] passed it.
fn set_status(conn: &mut DbConnection, asset_id: i32, status: AssetStatus) -> QueryResult<Asset> {
    // emit::record fires inside emit_asset_event.
    let device: Asset = diesel::update(assets::table.find(asset_id))
        .set(assets::status.eq(status.as_str()))
        .get_result(conn)?;
    assets_repo::emit_asset_event(conn, &device, SyncOp::Update, "asset.updated")?;
    Ok(device)
}

/// A loan ledger move.
pub enum LoanMove {
    /// A loan was issued: into `on_loan`.
    Issue,
    /// A loan was returned: back to the status it recorded.
    Return { to: AssetStatus },
}

/// Why a create in a status, or the loan ledger's move, didn't happen.
#[derive(Debug)]
pub enum MoveError {
    Refused(MoveRefused),
    Db(Error),
}

impl From<Error> for MoveError {
    fn from(e: Error) -> Self {
        MoveError::Db(e)
    }
}

impl From<MoveRefused> for MoveError {
    fn from(e: MoveRefused) -> Self {
        MoveError::Refused(e)
    }
}

/// Whether the loan ledger may make `mv` on `asset`: an issue only from a
/// loanable status, a return only from `on_loan`. The status it moves to. A
/// stored status outside the nine is neither, so the ledger never records
/// one as the status a return goes back to.
pub fn check_for_loan(asset: &Asset, mv: &LoanMove) -> Result<AssetStatus, MoveRefused> {
    let (mover, to) = match mv {
        LoanMove::Issue => (Mover::LoanIssue, AssetStatus::OnLoan),
        LoanMove::Return { to } => (Mover::LoanReturn, *to),
    };
    let Some(from) = AssetStatus::from_str_opt(&asset.status) else {
        let code = match mover {
            Mover::LoanIssue => RefusalCode::NotLoanable,
            _ => RefusalCode::NotOnLoan,
        };
        return Err(MoveRefused {
            code,
            from: None,
            to,
        });
    };
    status_rules::check_move(
        from,
        to,
        MoveFacts {
            mover,
            active_loan: true,
            disposal: false,
        },
    )?;
    Ok(to)
}

/// Move `asset`, locked by the caller with [`lock`], for the loan ledger,
/// and log the move with the loan in its `metadata`. Checked as the ledger:
/// an issue only from a loanable status, a return only from `on_loan`.
#[allow(clippy::too_many_arguments)]
pub fn move_for_loan(
    conn: &mut DbConnection,
    asset: &Asset,
    mv: LoanMove,
    loan_id: i32,
    ticket_id: Option<i32>,
    actor_uuid: Option<uuid::Uuid>,
    reason: Option<&str>,
) -> Result<Asset, MoveError> {
    let to = check_for_loan(asset, &mv)?;
    let to_status = to.as_str().to_string();
    let updated = set_status(conn, asset.id, to)?;
    insert_event(
        conn,
        NewAssetLifecycleEvent {
            asset_id: asset.id,
            from_status: Some(asset.status.clone()),
            to_status,
            reason: reason.map(str::to_string),
            ticket_id,
            metadata: json!({ "loan_id": loan_id }),
            actor_uuid,
        },
    )?;
    Ok(updated)
}

/// Inputs for a single lifecycle transition. `to_status` is one of the
/// `AssetStatus` keys; any other is refused as unknown.
pub struct TransitionInput {
    pub asset_id: i32,
    pub to_status: String,
    pub reason: Option<String>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<uuid::Uuid>,
    /// Disposal record captured in the same transaction. Required for a
    /// transition to `disposed` and refused for every other target.
    pub disposal: Option<DisposalInput>,
    /// Open a record of the type `to_status` opens with. `None` writes none.
    pub record: Option<RecordInput>,
    /// Complete the asset's open record of the type the status it leaves
    /// opens with. `None` leaves it open.
    pub complete_record: Option<CompleteRecordInput>,
}

/// The record a transition opens. The type is the one the target status
/// opens with; the work starts at the transition, linked to its ticket.
#[derive(Debug, Clone, Default)]
pub struct RecordInput {
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub vendor: Option<String>,
    pub performed_by_uuid: Option<uuid::Uuid>,
    pub reference: Option<String>,
    /// Values for the type's own fields.
    pub attributes: serde_json::Value,
}

/// The completion a transition out of a status writes on the record it
/// opened.
#[derive(Debug, Clone, Default)]
pub struct CompleteRecordInput {
    /// Which open record; needed only when more than one is open.
    pub record_id: Option<i32>,
    pub outcome: Option<String>,
    pub cost: Option<Cost>,
    /// When the work finished; `None` is the moment of the transition.
    pub completed: Option<WorkDate>,
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

/// Why a transition was refused, so the handler maps each case to a clear
/// client error without sniffing diesel internals.
#[derive(Debug)]
pub enum TransitionError {
    AssetNotFound,
    /// The status rules refuse the move ([`status_rules`]). A move to the
    /// status the asset has is refused as `same_status`.
    Refused(MoveRefused),
    /// `to_status` isn't one of the `AssetStatus` keys.
    UnknownStatus(String),
    /// `sanitization_method` is not one of [`SANITIZATION_METHODS`].
    InvalidSanitizationMethod(String),
    /// A record was asked to be opened, but no live record type opens with
    /// `status`.
    NoRecordType {
        status: String,
    },
    /// A completion was asked for, but the asset has no open record a move
    /// out of its status completes.
    NoOpenRecord,
    /// More than one record is open; the completion must name one of them.
    RecordChoiceRequired {
        open: Vec<i32>,
    },
    /// The named record isn't an open record of this kind on the asset.
    RecordNotOpen(i32),
    /// The record didn't pass the record rules.
    Record(RecordError),
    Db(Error),
}

impl From<Error> for TransitionError {
    fn from(e: Error) -> Self {
        match e {
            Error::NotFound => TransitionError::AssetNotFound,
            e => TransitionError::Db(e),
        }
    }
}

impl From<MoveRefused> for TransitionError {
    fn from(e: MoveRefused) -> Self {
        TransitionError::Refused(e)
    }
}

impl From<RecordError> for TransitionError {
    fn from(e: RecordError) -> Self {
        match e {
            RecordError::Db(e) => TransitionError::Db(e),
            e => TransitionError::Record(e),
        }
    }
}

/// A disposal record carries a known sanitization method. Checked before any
/// row is read; whether the move may carry a record at all is the status
/// rules' call.
fn check_sanitization(disposal: Option<&DisposalInput>) -> Result<(), TransitionError> {
    match disposal {
        Some(d) if !SANITIZATION_METHODS.contains(&d.sanitization_method.as_str()) => Err(
            TransitionError::InvalidSanitizationMethod(d.sanitization_method.clone()),
        ),
        _ => Ok(()),
    }
}

/// Move an asset to `to_status` and log the transition. Returns the
/// updated asset and the new event.
///
/// Checked by [`status_rules::check_move`] under the asset's row lock: a
/// move to the current status is refused as `same_status`; `disposed` is
/// terminal and needs its disposal record; `on_loan` belongs to the loan
/// ledger, so an asset with an active loan leaves it only through
/// [`asset_loans::return_loan`](crate::repository::asset_loans::return_loan),
/// and enters it here only to restore an asset whose loan is still active;
/// nothing moves back into `on_order`.
///
/// With `complete_record`, the open record the status it leaves opened is
/// completed; with `record`, a record of the type `to_status` opens with is
/// opened, starting at the transition, and the event's `metadata` names it
/// (`record_id`) beside the repair keys older readers know (`vendor`,
/// `rma_number`, `offsite`, `expected_return`).
pub fn transition(
    conn: &mut DbConnection,
    input: TransitionInput,
) -> Result<(Asset, AssetLifecycleEvent), TransitionError> {
    let to = AssetStatus::from_str_opt(&input.to_status)
        .ok_or_else(|| TransitionError::UnknownStatus(input.to_status.clone()))?;
    check_sanitization(input.disposal.as_ref())?;

    // emit::record fires for both writes inside this transaction.
    conn.transaction::<(Asset, AssetLifecycleEvent), TransitionError, _>(|conn| {
        // Lock the asset so a concurrent loan issue/return can't change its
        // status between the checks below and the update.
        let current = lock(conn, input.asset_id)?;
        let from_status = current.status.clone();
        let has_active_loan = asset_loans_repo::active_for_asset(conn, input.asset_id)?.is_some();
        check_from_stored(
            &from_status,
            to,
            MoveFacts::by_hand(has_active_loan, input.disposal.is_some()),
        )?;

        // The records the move writes, resolved before anything is.
        let completing = match &input.complete_record {
            Some(c) => Some(record_to_complete(conn, input.asset_id, &from_status, c)?),
            None => None,
        };
        let opening = match &input.record {
            Some(_) => Some(type_to_open(conn, &input.to_status, input.actor_uuid)?),
            None => None,
        };

        let updated = set_status(conn, input.asset_id, to)?;

        // `record_id` is the server's to write: it names the record this
        // move opened, never one a caller claims.
        let mut metadata = input.metadata;
        if let Some(map) = metadata.as_object_mut() {
            map.remove("record_id");
            if let Some(record) = &input.record {
                map.extend(legacy_repair_keys(record));
            }
        }

        let mut event: AssetLifecycleEvent = diesel::insert_into(asset_lifecycle_events::table)
            .values(&NewAssetLifecycleEvent {
                asset_id: input.asset_id,
                from_status: Some(from_status),
                to_status: input.to_status,
                reason: input.reason,
                ticket_id: input.ticket_id,
                metadata,
                actor_uuid: input.actor_uuid,
            })
            .get_result(conn)?;

        if let (Some(record), Some(c)) = (completing, input.complete_record) {
            let changes = RecordChanges {
                completed: Some(Some(c.completed.unwrap_or(WorkDate::At(event.occurred_at)))),
                outcome: c.outcome.map(Some),
                cost: c.cost.map(Some),
                ..Default::default()
            };
            records_repo::update(conn, record.id, record.version, changes, input.actor_uuid)?;
        }

        if let (Some(rtype), Some(r)) = (opening, input.record) {
            let written = records_repo::create(
                conn,
                NewRecord {
                    asset_id: input.asset_id,
                    record_type_id: rtype.id,
                    summary: r.summary,
                    notes: r.notes,
                    occurred: WorkDate::At(event.occurred_at),
                    completed: None,
                    outcome: None,
                    vendor: r.vendor,
                    performed_by_uuid: r.performed_by_uuid,
                    reference: r.reference,
                    cost: None,
                    ticket_id: event.ticket_id,
                    replacement_asset_id: None,
                    snapshot: None,
                    attributes: r.attributes,
                    source: RecordSource::Transition,
                    source_ref: None,
                    import_job_id: None,
                    rule_id: None,
                    lifecycle_event_id: Some(event.id),
                    recorded_by: input.actor_uuid,
                },
            )?;
            let mut metadata = event.metadata.clone();
            if let Some(map) = metadata.as_object_mut() {
                map.insert("record_id".to_string(), json!(written.record.id));
            }
            event = diesel::update(asset_lifecycle_events::table.find(event.id))
                .set(asset_lifecycle_events::metadata.eq(metadata))
                .get_result(conn)?;
        }

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

/// The live type a move into `status` opens a record of. When the workspace
/// has never added the catalogue type that opens with it, it is added now;
/// one it added and then archived, or pointed elsewhere, stays that way.
fn type_to_open(
    conn: &mut DbConnection,
    status: &str,
    actor: Option<uuid::Uuid>,
) -> Result<AssetRecordType, TransitionError> {
    let missing = || TransitionError::NoRecordType {
        status: status.to_string(),
    };
    if let Some(rtype) = record_types_repo::opened_by_status(conn, status)? {
        return Ok(rtype);
    }
    let Some(entry) = record_catalogue::CATALOGUE
        .iter()
        .find(|e| e.opens_with_status.map(AssetStatus::as_str) == Some(status))
    else {
        return Err(missing());
    };
    if record_types_repo::get_by_slug(conn, entry.slug)?.is_some() {
        return Err(missing());
    }
    let added =
        record_types_repo::add_from_catalogue(conn, entry.slug, actor).map_err(|e| match e {
            RecordTypeError::Db(e) => TransitionError::Db(e),
            _ => missing(),
        })?;
    if added.archived_at.is_none() && added.opens_with_status.as_deref() == Some(status) {
        Ok(added)
    } else {
        Err(missing())
    }
}

/// The open record a move out of `status` completes: the one named, else
/// the only one open of those the move may complete (of the type `status`
/// opens with, or opened by a move into it).
fn record_to_complete(
    conn: &mut DbConnection,
    asset_id: i32,
    status: &str,
    input: &CompleteRecordInput,
) -> Result<AssetRecord, TransitionError> {
    let mut open = records_repo::open_for_leaving(conn, asset_id, status)?;
    match input.record_id {
        Some(id) => open
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(TransitionError::RecordNotOpen(id)),
        None if open.len() == 1 => Ok(open.remove(0)),
        None if open.is_empty() => Err(TransitionError::NoOpenRecord),
        None => Err(TransitionError::RecordChoiceRequired {
            open: open.iter().map(|r| r.id).collect(),
        }),
    }
}

/// The repair keys an `in_repair` event's metadata has always carried, from
/// the record it opens, so readers of the event that predate records (the
/// history export, older clients) still find them.
fn legacy_repair_keys(record: &RecordInput) -> serde_json::Map<String, serde_json::Value> {
    let text = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| json!(t))
    };
    let mut keys = serde_json::Map::new();
    for (key, value) in [
        ("vendor", text(&record.vendor)),
        ("rma_number", text(&record.reference)),
        ("offsite", record.attributes.get("offsite").cloned()),
        (
            "expected_return",
            record.attributes.get("expected_return").cloned(),
        ),
    ] {
        if let Some(value) = value.filter(|v| !v.is_null()) {
            keys.insert(key.to_string(), value);
        }
    }
    keys
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

/// Distinct ITAD vendors previously entered on disposals, for the disposal-form
/// suggestions datalist so the same vendor isn't retyped. Workspace-scoped by
/// RLS; trimmed, non-empty, case-insensitively sorted.
// sync-audit-only: read-only distinct SELECT, no row write
pub fn list_itad_vendors(conn: &mut DbConnection) -> QueryResult<Vec<String>> {
    let raw: Vec<String> = asset_disposals::table
        .filter(asset_disposals::itad_vendor.is_not_null())
        .select(asset_disposals::itad_vendor.assume_not_null())
        .distinct()
        .load(conn)?;
    let mut vendors: Vec<String> = raw
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();
    vendors.sort_by_key(|v| v.to_lowercase());
    vendors.dedup();
    Ok(vendors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAsset;
    use crate::repository::asset_loans::{self, IssueLoan};
    use crate::repository::assets::{create_device, get_device_by_id};
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn refusal(err: TransitionError) -> RefusalCode {
        match err {
            TransitionError::Refused(r) => r.code,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn asset(conn: &mut DbConnection, name: &str) -> Asset {
        create_device(conn, new_asset(name)).unwrap()
    }

    fn new_asset(name: &str) -> NewAsset {
        NewAsset {
            name: name.to_string(),
            serial_number: None,
            manufacturer: None,
            model: None,
            location: None,
            notes: None,
            primary_user_uuid: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: json!({}),
            quantity: None,
            unit: None,
            external_sync_source: None,
            low_stock_threshold: None,
            created_by: None,
        }
    }

    fn to(asset_id: i32, status: &str) -> TransitionInput {
        TransitionInput {
            asset_id,
            to_status: status.to_string(),
            reason: None,
            ticket_id: None,
            metadata: json!({}),
            actor_uuid: None,
            disposal: None,
            record: None,
            complete_record: None,
        }
    }

    fn disposal(method: &str) -> Option<DisposalInput> {
        Some(DisposalInput {
            sanitization_method: method.to_string(),
            data_bearing: true,
            certificate_file_id: None,
            itad_vendor: None,
            notes: None,
        })
    }

    fn dispose(asset_id: i32, method: &str) -> TransitionInput {
        TransitionInput {
            disposal: disposal(method),
            ..to(asset_id, "disposed")
        }
    }

    fn status(conn: &mut DbConnection, asset_id: i32) -> String {
        get_device_by_id(conn, asset_id).unwrap().status
    }

    #[test]
    fn plain_transition_moves_status_and_logs_event() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-1");
        let (updated, event) = transition(&mut conn, to(a.id, "in_repair")).unwrap();
        assert_eq!(updated.status, "in_repair");
        assert_eq!(event.from_status.as_deref(), Some("in_service"));
        assert_eq!(event.to_status, "in_repair");
    }

    #[test]
    fn same_status_is_a_no_op() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-2");
        let err = transition(&mut conn, to(a.id, "in_service")).unwrap_err();
        assert_eq!(refusal(err), RefusalCode::SameStatus);
    }

    #[test]
    fn missing_asset_is_not_found() {
        let mut conn = setup_test_connection();
        let err = transition(&mut conn, to(i32::MAX, "in_repair")).unwrap_err();
        assert!(matches!(err, TransitionError::AssetNotFound));
    }

    #[test]
    fn disposing_requires_the_disposal_record() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-3");
        let err = transition(&mut conn, to(a.id, "disposed")).unwrap_err();
        assert_eq!(refusal(err), RefusalCode::NeedsDisposal);
        assert_eq!(status(&mut conn, a.id), "in_service");
    }

    #[test]
    fn unknown_sanitization_method_is_refused() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-4");
        let err = transition(&mut conn, dispose(a.id, "shred")).unwrap_err();
        assert!(matches!(err, TransitionError::InvalidSanitizationMethod(m) if m == "shred"));
        assert_eq!(status(&mut conn, a.id), "in_service");
    }

    #[test]
    fn disposal_record_with_another_target_is_refused() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-5");
        let input = TransitionInput {
            disposal: disposal("clear"),
            ..to(a.id, "retired")
        };
        let err = transition(&mut conn, input).unwrap_err();
        assert_eq!(refusal(err), RefusalCode::DisposalNotAllowed);
        assert!(disposal_for_asset(&mut conn, a.id).unwrap().is_none());
    }

    #[test]
    fn disposal_is_recorded_and_terminal() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-6");
        for method in SANITIZATION_METHODS {
            assert!(check_sanitization(disposal(method).as_ref()).is_ok());
        }
        let (_, event) = transition(&mut conn, dispose(a.id, "destroy")).unwrap();
        let record = disposal_for_asset(&mut conn, a.id).unwrap().unwrap();
        assert_eq!(record.sanitization_method, "destroy");
        assert_eq!(record.lifecycle_event_id, Some(event.id));

        for target in ["in_service", "in_stock", "retired", "lost"] {
            let err = transition(&mut conn, to(a.id, target)).unwrap_err();
            assert_eq!(refusal(err), RefusalCode::Terminal, "{target}");
        }
        assert_eq!(status(&mut conn, a.id), "disposed");
    }

    #[test]
    fn active_loan_leaves_on_loan_only_by_return() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Lifecycle-7");
        let loan = asset_loans::issue(
            &mut conn,
            IssueLoan {
                asset_id: a.id,
                borrower_user_uuid: borrower.uuid,
                loaned_at: None,
                due_back: None,
                ticket_id: None,
                notes: None,
                actor_uuid: None,
            },
        )
        .unwrap();

        for target in ["in_service", "lost"] {
            let err = transition(&mut conn, to(a.id, target)).unwrap_err();
            assert_eq!(refusal(err), RefusalCode::LoanOwned, "{target}");
        }
        let err = transition(&mut conn, dispose(a.id, "clear")).unwrap_err();
        assert_eq!(refusal(err), RefusalCode::LoanOwned);
        assert_eq!(status(&mut conn, a.id), "on_loan");

        asset_loans::return_loan(&mut conn, a.id, loan.id, chrono::Utc::now(), None, None).unwrap();
        assert_eq!(status(&mut conn, a.id), "in_service");
        transition(&mut conn, to(a.id, "in_repair")).unwrap();
    }

    #[test]
    fn on_loan_target_needs_an_active_loan() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-8");
        let err = transition(&mut conn, to(a.id, "on_loan")).unwrap_err();
        assert_eq!(refusal(err), RefusalCode::NeedsActiveLoan);
    }

    #[test]
    fn stranded_on_loan_without_a_loan_can_be_moved() {
        // An asset left at `on_loan` with no open ledger row (e.g. its loan
        // row was deleted) must stay recoverable through a manual transition.
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-9");
        diesel::update(assets::table.find(a.id))
            .set(assets::status.eq("on_loan"))
            .execute(&mut conn)
            .unwrap();
        let (updated, _) = transition(&mut conn, to(a.id, "in_stock")).unwrap();
        assert_eq!(updated.status, "in_stock");
    }

    #[test]
    fn a_new_asset_starts_in_its_status_with_one_event() {
        let mut conn = setup_test_connection();
        let mut new = new_asset("Lifecycle-10");
        new.notes = Some("ordered".to_string());
        let (a, event) = create(
            &mut conn,
            new,
            None,
            AssetStatus::OnOrder,
            Some(FirstEvent {
                reason: Some("Ordered".to_string()),
                metadata: json!({}),
                actor_uuid: None,
            }),
        )
        .unwrap();
        assert_eq!(a.status, "on_order");
        let event = event.unwrap();
        assert_eq!(event.from_status, None);
        assert_eq!(event.to_status, "on_order");
        assert_eq!(list_for_asset(&mut conn, a.id).unwrap().len(), 1);

        for status in [AssetStatus::OnLoan, AssetStatus::Disposed] {
            let err = create(&mut conn, new_asset("Lifecycle-11"), None, status, None).unwrap_err();
            assert!(matches!(err, MoveError::Refused(_)), "{status:?}");
        }
    }

    #[test]
    fn nothing_moves_back_into_on_order() {
        use crate::services::assets::status_rules::{Cell, INTO_ON_ORDER};
        let mut conn = setup_test_connection();
        let (a, _) = create(
            &mut conn,
            new_asset("Lifecycle-12"),
            None,
            AssetStatus::OnOrder,
            None,
        )
        .unwrap();
        transition(&mut conn, to(a.id, "in_stock")).unwrap();
        let back = transition(&mut conn, to(a.id, "on_order"));
        if INTO_ON_ORDER == Cell::Allowed {
            back.unwrap();
        } else {
            assert_eq!(refusal(back.unwrap_err()), RefusalCode::StartOnly);
            assert_eq!(status(&mut conn, a.id), "in_stock");
        }
    }

    #[test]
    fn a_status_outside_the_nine_can_be_put_right() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-13");
        diesel::update(assets::table.find(a.id))
            .set(assets::status.eq("broken"))
            .execute(&mut conn)
            .unwrap();
        let (updated, event) = transition(&mut conn, to(a.id, "in_service")).unwrap();
        assert_eq!(updated.status, "in_service");
        assert_eq!(event.from_status.as_deref(), Some("broken"));
    }
}
