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
    Asset, AssetDisposal, AssetLifecycleEvent, AssetStatus, NewAssetDisposal,
    NewAssetLifecycleEvent, SyncAggregate, SyncOp, SANITIZATION_METHODS,
};
use crate::repository::asset_loans as asset_loans_repo;
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
    /// Disposal record captured in the same transaction. Required for a
    /// transition to `disposed` and refused for every other target.
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

/// Why a transition was refused, so the handler maps each case to a clear
/// client error without sniffing diesel internals.
#[derive(Debug)]
pub enum TransitionError {
    AssetNotFound,
    /// The asset is already in the requested status.
    NoOp,
    /// `disposed` is terminal: nothing moves an asset out of it.
    FromDisposed,
    /// The asset has an active loan; only returning the loan moves it off
    /// `on_loan`.
    OnLoanReturnViaLoan,
    /// `on_loan` as a manual target only restores an asset whose loan is
    /// still active; otherwise it comes from issuing a loan.
    OnLoanNeedsActiveLoan,
    /// A transition to `disposed` must carry the disposal record.
    DisposalRequired,
    /// A disposal record was sent with a target other than `disposed`.
    DisposalNotAllowed,
    /// `sanitization_method` is not one of [`SANITIZATION_METHODS`].
    InvalidSanitizationMethod(String),
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

/// The disposal record must accompany `disposed` and nothing else, and carry
/// a known sanitization method. Checked before any row is read.
fn check_disposal(
    to_status: &str,
    disposal: Option<&DisposalInput>,
) -> Result<(), TransitionError> {
    let to_disposed = to_status == AssetStatus::Disposed.as_str();
    match disposal {
        None if to_disposed => Err(TransitionError::DisposalRequired),
        None => Ok(()),
        Some(_) if !to_disposed => Err(TransitionError::DisposalNotAllowed),
        Some(d) if !SANITIZATION_METHODS.contains(&d.sanitization_method.as_str()) => Err(
            TransitionError::InvalidSanitizationMethod(d.sanitization_method.clone()),
        ),
        Some(_) => Ok(()),
    }
}

/// Move an asset to `to_status` and log the transition. Returns the
/// updated asset and the new event.
///
/// Enforces the status rules: a move to the current status is a no-op;
/// `disposed` is terminal and needs its disposal record; `on_loan` belongs to
/// the loan ledger, so an asset with an active loan leaves it only through
/// [`asset_loans::return_loan`](crate::repository::asset_loans::return_loan),
/// and enters it here only to restore an asset whose loan is still active.
pub fn transition(
    conn: &mut DbConnection,
    input: TransitionInput,
) -> Result<(Asset, AssetLifecycleEvent), TransitionError> {
    check_disposal(&input.to_status, input.disposal.as_ref())?;

    // emit::record fires for both writes inside this transaction.
    conn.transaction::<(Asset, AssetLifecycleEvent), TransitionError, _>(|conn| {
        // Lock the asset so a concurrent loan issue/return can't change its
        // status between the checks below and the update.
        let current: Asset = assets::table
            .find(input.asset_id)
            .for_no_key_update()
            .first(conn)?;
        let from_status = current.status.clone();

        if from_status == input.to_status {
            return Err(TransitionError::NoOp);
        }
        if from_status == AssetStatus::Disposed.as_str() {
            return Err(TransitionError::FromDisposed);
        }
        let has_active_loan = asset_loans_repo::active_for_asset(conn, input.asset_id)?.is_some();
        if from_status == AssetStatus::OnLoan.as_str() && has_active_loan {
            return Err(TransitionError::OnLoanReturnViaLoan);
        }
        if input.to_status == AssetStatus::OnLoan.as_str() && !has_active_loan {
            return Err(TransitionError::OnLoanNeedsActiveLoan);
        }

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

    fn asset(conn: &mut DbConnection, name: &str) -> Asset {
        create_device(
            conn,
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
            },
        )
        .unwrap()
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
        assert!(matches!(err, TransitionError::NoOp));
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
        assert!(matches!(err, TransitionError::DisposalRequired));
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
        assert!(matches!(err, TransitionError::DisposalNotAllowed));
        assert!(disposal_for_asset(&mut conn, a.id).unwrap().is_none());
    }

    #[test]
    fn disposal_is_recorded_and_terminal() {
        let mut conn = setup_test_connection();
        let a = asset(&mut conn, "Lifecycle-6");
        for method in SANITIZATION_METHODS {
            assert!(check_disposal("disposed", disposal(method).as_ref()).is_ok());
        }
        let (_, event) = transition(&mut conn, dispose(a.id, "destroy")).unwrap();
        let record = disposal_for_asset(&mut conn, a.id).unwrap().unwrap();
        assert_eq!(record.sanitization_method, "destroy");
        assert_eq!(record.lifecycle_event_id, Some(event.id));

        for target in ["in_service", "in_stock", "retired", "lost"] {
            let err = transition(&mut conn, to(a.id, target)).unwrap_err();
            assert!(matches!(err, TransitionError::FromDisposed), "{target}");
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
            assert!(
                matches!(err, TransitionError::OnLoanReturnViaLoan),
                "{target}"
            );
        }
        let err = transition(&mut conn, dispose(a.id, "clear")).unwrap_err();
        assert!(matches!(err, TransitionError::OnLoanReturnViaLoan));
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
        assert!(matches!(err, TransitionError::OnLoanNeedsActiveLoan));
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
}
