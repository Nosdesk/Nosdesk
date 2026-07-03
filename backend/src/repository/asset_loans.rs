//! Device loan ledger: issue, return, edit, and read loans.
//!
//! A loan is the source of truth for an asset's custody (who holds it,
//! until when, against which ticket). Issuing and returning a loan also
//! keep `assets.status` in step (`on_loan` <-> the pre-loan status) and
//! write an `asset_lifecycle_events` row, so the status timeline stays
//! unified with repair / retire. Every write emits a sync action.

use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::QueryResult;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetLifecycleEvent, AssetLoan, AssetLoanChange, NewAssetLifecycleEvent, NewAssetLoan,
    SyncAggregate, SyncOp,
};
use crate::repository::assets::emit_asset_event;
use crate::schema::{asset_lifecycle_events, asset_loans, assets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Statuses an asset can be loaned out from. A device already in repair,
/// retired, lost, disposed, or out on another loan is not loanable.
pub const LOANABLE_FROM: &[&str] = &["in_service", "in_stock"];

/// Inputs to issue a loan. `status_before` is read from the live asset by
/// [`issue`], not trusted from the caller.
pub struct IssueLoan {
    pub asset_id: i32,
    pub borrower_user_uuid: Uuid,
    /// Start date. `None` starts the loan now; a past date backdates it.
    pub loaned_at: Option<NaiveDate>,
    pub due_back: Option<NaiveDate>,
    pub ticket_id: Option<i32>,
    pub notes: Option<String>,
    pub actor_uuid: Option<Uuid>,
}

/// Domain errors the loan flow surfaces, so handlers map them to the right
/// HTTP status without sniffing diesel internals.
#[derive(Debug)]
pub enum LoanError {
    AssetNotFound,
    /// Asset is in a status it can't be loaned from (carries the status).
    NotLoanable(String),
    AlreadyOnLoan,
    /// Borrower or ticket reference doesn't resolve (FK violation).
    InvalidReference,
    LoanNotFound,
    AlreadyReturned,
    Db(DieselError),
}

impl From<DieselError> for LoanError {
    fn from(e: DieselError) -> Self {
        LoanError::Db(e)
    }
}

fn loan_sync_payload(row: &AssetLoan) -> serde_json::Value {
    json!({
        "id": row.id,
        "asset_id": row.asset_id,
        "borrower_user_uuid": row.borrower_user_uuid,
        "loaned_at": row.loaned_at,
        "due_back": row.due_back,
        "returned_at": row.returned_at,
        "ticket_id": row.ticket_id,
        "notes": row.notes,
        "actor_uuid": row.actor_uuid,
        "returned_by_uuid": row.returned_by_uuid,
    })
}

/// Groups a loan event routes to: workspace, the asset, the borrower's
/// private group (so their own loans resolve, and the portal can surface
/// them later), and the linked ticket when set.
fn loan_groups(loan: &AssetLoan) -> Vec<String> {
    let mut g = groups::workspace();
    g.push(format!("asset:{}", loan.asset_id));
    g.push(format!("user:{}", loan.borrower_user_uuid));
    if let Some(ticket_id) = loan.ticket_id {
        g.push(format!("ticket:{ticket_id}"));
    }
    g
}

/// The loan-event `SyncEmit`. The `emit::record` call stays inline at each
/// write site (the sync-emit lint wants it locally visible), so this just
/// assembles the payload + groups the three writers share.
fn loan_emit(loan: &AssetLoan, op: SyncOp, event_type: &'static str) -> SyncEmit<'static> {
    SyncEmit {
        aggregate: SyncAggregate::AssetLoan,
        aggregate_id: loan.id.to_string(),
        op,
        event_type,
        data: loan_sync_payload(loan),
        groups: loan_groups(loan),
        causation_id: None,
    }
}

/// Write the lifecycle log row for a loan status move and emit it,
/// referencing the loan so the timeline can deep-link to it.
fn log_lifecycle(
    conn: &mut DbConnection,
    asset_id: i32,
    from_status: &str,
    to_status: &str,
    ticket_id: Option<i32>,
    loan_id: i32,
    actor_uuid: Option<Uuid>,
) -> QueryResult<()> {
    let event: AssetLifecycleEvent = diesel::insert_into(asset_lifecycle_events::table)
        .values(&NewAssetLifecycleEvent {
            asset_id,
            from_status: Some(from_status.to_string()),
            to_status: to_status.to_string(),
            reason: None,
            ticket_id,
            metadata: json!({ "loan_id": loan_id }),
            actor_uuid,
        })
        .get_result(conn)?;
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::AssetLifecycleEvent,
            aggregate_id: event.id.to_string(),
            op: SyncOp::Insert,
            event_type: "asset_lifecycle_event.created",
            data: json!({
                "id": event.id,
                "asset_id": event.asset_id,
                "from_status": event.from_status,
                "to_status": event.to_status,
                "reason": event.reason,
                "ticket_id": event.ticket_id,
                "metadata": event.metadata,
                "actor_uuid": event.actor_uuid,
                "occurred_at": event.occurred_at,
            }),
            groups: groups::workspace(),
            causation_id: None,
        },
    )?;
    Ok(())
}

pub fn list_for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Vec<AssetLoan>> {
    asset_loans::table
        .filter(asset_loans::asset_id.eq(asset_id))
        .order(asset_loans::loaned_at.desc())
        .load(conn)
}

/// Loans issued against a ticket (the loaner-from-ticket flow links them).
/// Newest first.
pub fn list_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<AssetLoan>> {
    asset_loans::table
        .filter(asset_loans::ticket_id.eq(ticket_id))
        .order(asset_loans::loaned_at.desc())
        .load(conn)
}

pub fn active_for_asset(conn: &mut DbConnection, asset_id: i32) -> QueryResult<Option<AssetLoan>> {
    asset_loans::table
        .filter(asset_loans::asset_id.eq(asset_id))
        .filter(asset_loans::returned_at.is_null())
        .first(conn)
        .optional()
}

// ---- Due-back reminders (scheduler) --------------------------------

/// A loan that needs a due-back reminder. The reminder job dispatches a
/// notification to the borrower and stamps the matching `*_notified_at`.
#[derive(Debug, Clone)]
pub struct ReminderCandidate {
    pub loan_id: i32,
    pub asset_id: i32,
    pub asset_name: String,
    pub borrower_user_uuid: Uuid,
    pub due_back: NaiveDate,
    pub ticket_id: Option<i32>,
    pub workspace_id: i32,
}

type CandidateRow = (i32, i32, String, Uuid, Option<NaiveDate>, Option<i32>, i32);

fn to_candidate(row: CandidateRow) -> ReminderCandidate {
    let (loan_id, asset_id, asset_name, borrower_user_uuid, due_back, ticket_id, workspace_id) =
        row;
    ReminderCandidate {
        loan_id,
        asset_id,
        asset_name,
        borrower_user_uuid,
        // Both queries filter due_back to a real date, so this is always Some.
        due_back: due_back.unwrap_or_default(),
        ticket_id,
        workspace_id,
    }
}

/// Active loans now past their due date and not yet flagged overdue.
/// Capped at `limit` (most-overdue first); the reminder job picks up any
/// remainder on its next tick.
pub fn overdue_reminder_candidates(
    conn: &mut DbConnection,
    today: NaiveDate,
    limit: i64,
) -> QueryResult<Vec<ReminderCandidate>> {
    asset_loans::table
        .inner_join(assets::table)
        .filter(asset_loans::returned_at.is_null())
        .filter(asset_loans::due_back.lt(today))
        .filter(asset_loans::overdue_notified_at.is_null())
        .select((
            asset_loans::id,
            asset_loans::asset_id,
            assets::name,
            asset_loans::borrower_user_uuid,
            asset_loans::due_back,
            asset_loans::ticket_id,
            asset_loans::workspace_id,
        ))
        .order(asset_loans::due_back.asc())
        .limit(limit)
        .load::<CandidateRow>(conn)
        .map(|rows| rows.into_iter().map(to_candidate).collect())
}

/// Active loans due back within the horizon and not yet flagged due-soon.
/// Capped at `limit` (soonest-due first); the remainder waits for the next tick.
pub fn due_soon_reminder_candidates(
    conn: &mut DbConnection,
    today: NaiveDate,
    horizon: NaiveDate,
    limit: i64,
) -> QueryResult<Vec<ReminderCandidate>> {
    asset_loans::table
        .inner_join(assets::table)
        .filter(asset_loans::returned_at.is_null())
        .filter(asset_loans::due_back.ge(today))
        .filter(asset_loans::due_back.le(horizon))
        .filter(asset_loans::due_soon_notified_at.is_null())
        .select((
            asset_loans::id,
            asset_loans::asset_id,
            assets::name,
            asset_loans::borrower_user_uuid,
            asset_loans::due_back,
            asset_loans::ticket_id,
            asset_loans::workspace_id,
        ))
        .order(asset_loans::due_back.asc())
        .limit(limit)
        .load::<CandidateRow>(conn)
        .map(|rows| rows.into_iter().map(to_candidate).collect())
}

// sync-audit-only: reminder bookkeeping stamp, not a user-facing change
pub fn mark_overdue_notified(conn: &mut DbConnection, loan_id: i32) -> QueryResult<usize> {
    diesel::update(asset_loans::table.find(loan_id))
        .set(asset_loans::overdue_notified_at.eq(Utc::now()))
        .execute(conn)
}

// sync-audit-only: reminder bookkeeping stamp, not a user-facing change
pub fn mark_due_soon_notified(conn: &mut DbConnection, loan_id: i32) -> QueryResult<usize> {
    diesel::update(asset_loans::table.find(loan_id))
        .set(asset_loans::due_soon_notified_at.eq(Utc::now()))
        .execute(conn)
}

// sync-audit-only: reminder bookkeeping stamp, not a user-facing change
/// Batched `mark_overdue_notified`: stamp many loans in one UPDATE. The
/// reminder sweep groups the loans it successfully notified by workspace and
/// stamps each group once, instead of one connection checkout per loan.
pub fn mark_overdue_notified_batch(
    conn: &mut DbConnection,
    loan_ids: &[i32],
) -> QueryResult<usize> {
    diesel::update(asset_loans::table.filter(asset_loans::id.eq_any(loan_ids)))
        .set(asset_loans::overdue_notified_at.eq(Utc::now()))
        .execute(conn)
}

// sync-audit-only: reminder bookkeeping stamp, not a user-facing change
/// Batched `mark_due_soon_notified`: stamp many loans in one UPDATE.
pub fn mark_due_soon_notified_batch(
    conn: &mut DbConnection,
    loan_ids: &[i32],
) -> QueryResult<usize> {
    diesel::update(asset_loans::table.filter(asset_loans::id.eq_any(loan_ids)))
        .set(asset_loans::due_soon_notified_at.eq(Utc::now()))
        .execute(conn)
}

/// Issue a loan: an asset enters a borrower's custody. Sets the asset
/// `on_loan`, logs the transition, and emits the loan + asset + lifecycle
/// sync events, atomically.
pub fn issue(conn: &mut DbConnection, input: IssueLoan) -> Result<AssetLoan, LoanError> {
    conn.transaction::<AssetLoan, LoanError, _>(|conn| {
        let asset: Asset = assets::table
            .find(input.asset_id)
            .first(conn)
            .optional()?
            .ok_or(LoanError::AssetNotFound)?;

        // An already-loaned asset gets the clear "already on loan" error
        // rather than the generic not-loanable one; the unique index below
        // is the race-safe backstop for concurrent issues.
        if asset.status == "on_loan" {
            return Err(LoanError::AlreadyOnLoan);
        }
        if !LOANABLE_FROM.contains(&asset.status.as_str()) {
            return Err(LoanError::NotLoanable(asset.status));
        }

        // The partial unique index `(workspace_id, asset_id) WHERE
        // returned_at IS NULL` is the race-safe guard; map its violation to
        // a clean AlreadyOnLoan rather than a 500.
        // Resolve the start timestamp. Today (or omitted) keeps the
        // precise issue time; a backdated date anchors at noon UTC to
        // avoid a timezone day-shift. The handler caps the date at today,
        // so future starts don't reach here.
        let now = chrono::Utc::now();
        let loaned_at = match input.loaned_at {
            Some(d) if d < now.date_naive() => {
                let ndt = d.and_hms_opt(12, 0, 0).unwrap_or_default();
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc)
            }
            _ => now,
        };

        let loan: AssetLoan = match diesel::insert_into(asset_loans::table)
            .values(&NewAssetLoan {
                asset_id: input.asset_id,
                borrower_user_uuid: input.borrower_user_uuid,
                loaned_at,
                due_back: input.due_back,
                ticket_id: input.ticket_id,
                status_before: asset.status.clone(),
                notes: input.notes,
                actor_uuid: input.actor_uuid,
            })
            .get_result(conn)
        {
            Ok(l) => l,
            Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                return Err(LoanError::AlreadyOnLoan)
            }
            Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
                return Err(LoanError::InvalidReference)
            }
            Err(e) => return Err(LoanError::Db(e)),
        };

        let updated: Asset = diesel::update(assets::table.find(input.asset_id))
            .set(assets::status.eq("on_loan"))
            .get_result(conn)?;
        emit_asset_event(conn, &updated, SyncOp::Update, "asset.updated")?;

        log_lifecycle(
            conn,
            loan.asset_id,
            &loan.status_before,
            "on_loan",
            loan.ticket_id,
            loan.id,
            loan.actor_uuid,
        )?;
        emit::record(conn, loan_emit(&loan, SyncOp::Insert, "asset_loan.issued"))?;
        Ok(loan)
    })
}

/// Return a loan: stamps `returned_at`, reverts the asset to its pre-loan
/// status, logs the transition, and emits the updates.
pub fn return_loan(
    conn: &mut DbConnection,
    asset_id: i32,
    loan_id: i32,
    returned_at: DateTime<Utc>,
    returned_by: Option<Uuid>,
    notes: Option<String>,
) -> Result<AssetLoan, LoanError> {
    conn.transaction::<AssetLoan, LoanError, _>(|conn| {
        let loan: AssetLoan = asset_loans::table
            .find(loan_id)
            .filter(asset_loans::asset_id.eq(asset_id))
            .first(conn)
            .optional()?
            .ok_or(LoanError::LoanNotFound)?;
        if loan.returned_at.is_some() {
            return Err(LoanError::AlreadyReturned);
        }

        let updated_loan: AssetLoan = diesel::update(asset_loans::table.find(loan_id))
            .set(&AssetLoanChange {
                returned_at: Some(Some(returned_at)),
                returned_by_uuid: Some(returned_by),
                notes: notes.map(Some),
                ..Default::default()
            })
            .get_result(conn)?;

        let updated_asset: Asset = diesel::update(assets::table.find(asset_id))
            .set(assets::status.eq(&loan.status_before))
            .get_result(conn)?;
        emit_asset_event(conn, &updated_asset, SyncOp::Update, "asset.updated")?;

        log_lifecycle(
            conn,
            asset_id,
            "on_loan",
            &loan.status_before,
            loan.ticket_id,
            loan.id,
            returned_by,
        )?;
        emit::record(
            conn,
            loan_emit(&updated_loan, SyncOp::Update, "asset_loan.updated"),
        )?;
        Ok(updated_loan)
    })
}

/// Edit an active loan's due date and/or notes. A no-op (both `None`)
/// returns the loan unchanged.
pub fn edit(
    conn: &mut DbConnection,
    asset_id: i32,
    loan_id: i32,
    due_back: Option<Option<NaiveDate>>,
    notes: Option<Option<String>>,
) -> Result<AssetLoan, LoanError> {
    conn.transaction::<AssetLoan, LoanError, _>(|conn| {
        let loan: AssetLoan = asset_loans::table
            .find(loan_id)
            .filter(asset_loans::asset_id.eq(asset_id))
            .first(conn)
            .optional()?
            .ok_or(LoanError::LoanNotFound)?;
        if loan.returned_at.is_some() {
            return Err(LoanError::AlreadyReturned);
        }
        if due_back.is_none() && notes.is_none() {
            return Ok(loan);
        }
        let updated: AssetLoan = diesel::update(asset_loans::table.find(loan_id))
            .set(&AssetLoanChange {
                due_back,
                notes,
                ..Default::default()
            })
            .get_result(conn)?;
        emit::record(
            conn,
            loan_emit(&updated, SyncOp::Update, "asset_loan.updated"),
        )?;
        Ok(updated)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAsset;
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
                attributes: serde_json::json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
            },
        )
        .unwrap()
    }

    fn issue_input(asset_id: i32, borrower: Uuid) -> IssueLoan {
        IssueLoan {
            asset_id,
            borrower_user_uuid: borrower,
            loaned_at: None,
            due_back: None,
            ticket_id: None,
            notes: None,
            actor_uuid: None,
        }
    }

    #[test]
    fn issue_puts_asset_on_loan() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-1");
        let loan = issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap();
        assert!(loan.returned_at.is_none());
        assert_eq!(loan.status_before, "in_service");
        assert_eq!(get_device_by_id(&mut conn, a.id).unwrap().status, "on_loan");
        assert!(active_for_asset(&mut conn, a.id).unwrap().is_some());
    }

    #[test]
    fn second_active_loan_is_rejected() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-2");
        issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap();
        let err = issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap_err();
        assert!(matches!(err, LoanError::AlreadyOnLoan));
    }

    #[test]
    fn non_loanable_status_is_rejected() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-3");
        diesel::update(assets::table.find(a.id))
            .set(assets::status.eq("in_repair"))
            .execute(&mut conn)
            .unwrap();
        let err = issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap_err();
        assert!(matches!(err, LoanError::NotLoanable(s) if s == "in_repair"));
    }

    #[test]
    fn return_reverts_status_and_frees_the_asset() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-4");
        let loan = issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap();
        let returned = return_loan(
            &mut conn,
            a.id,
            loan.id,
            Utc::now(),
            Some(borrower.uuid),
            None,
        )
        .unwrap();
        assert!(returned.returned_at.is_some());
        assert_eq!(
            get_device_by_id(&mut conn, a.id).unwrap().status,
            "in_service"
        );
        assert!(active_for_asset(&mut conn, a.id).unwrap().is_none());
        // The asset is loanable again now that it's back.
        issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap();
    }

    #[test]
    fn returning_twice_is_rejected() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-5");
        let loan = issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap();
        return_loan(&mut conn, a.id, loan.id, Utc::now(), None, None).unwrap();
        let err = return_loan(&mut conn, a.id, loan.id, Utc::now(), None, None).unwrap_err();
        assert!(matches!(err, LoanError::AlreadyReturned));
    }

    #[test]
    fn list_for_ticket_returns_linked_loans() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let ticket =
            TestFixtures::create_ticket(&mut conn, "loaner please", Some(borrower.uuid), None);
        let a = asset(&mut conn, "Loaner-T");
        issue(
            &mut conn,
            IssueLoan {
                asset_id: a.id,
                borrower_user_uuid: borrower.uuid,
                loaned_at: None,
                due_back: None,
                ticket_id: Some(ticket.id),
                notes: None,
                actor_uuid: None,
            },
        )
        .unwrap();
        let rows = list_for_ticket(&mut conn, ticket.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ticket_id, Some(ticket.id));
    }

    #[test]
    fn overdue_candidate_appears_until_stamped() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-OD");
        let today = Utc::now().date_naive();
        let past = today - chrono::Duration::days(3);
        let loan = issue(
            &mut conn,
            IssueLoan {
                asset_id: a.id,
                borrower_user_uuid: borrower.uuid,
                loaned_at: None,
                due_back: Some(past),
                ticket_id: None,
                notes: None,
                actor_uuid: None,
            },
        )
        .unwrap();

        let before = overdue_reminder_candidates(&mut conn, today, 1000).unwrap();
        assert!(
            before.iter().any(|c| c.loan_id == loan.id),
            "overdue loan is a candidate"
        );

        mark_overdue_notified(&mut conn, loan.id).unwrap();
        let after = overdue_reminder_candidates(&mut conn, today, 1000).unwrap();
        assert!(
            !after.iter().any(|c| c.loan_id == loan.id),
            "stamped loan stops re-appearing"
        );
    }

    #[test]
    fn due_soon_candidate_respects_horizon() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let soon = asset(&mut conn, "Loaner-Soon");
        let far = asset(&mut conn, "Loaner-Far");
        let today = Utc::now().date_naive();
        let horizon = today + chrono::Duration::days(2);
        let soon_loan = issue(
            &mut conn,
            IssueLoan {
                asset_id: soon.id,
                borrower_user_uuid: borrower.uuid,
                loaned_at: None,
                due_back: Some(today + chrono::Duration::days(1)),
                ticket_id: None,
                notes: None,
                actor_uuid: None,
            },
        )
        .unwrap();
        issue(
            &mut conn,
            IssueLoan {
                asset_id: far.id,
                borrower_user_uuid: borrower.uuid,
                loaned_at: None,
                due_back: Some(today + chrono::Duration::days(10)),
                ticket_id: None,
                notes: None,
                actor_uuid: None,
            },
        )
        .unwrap();

        let candidates = due_soon_reminder_candidates(&mut conn, today, horizon, 1000).unwrap();
        assert!(
            candidates.iter().any(|c| c.loan_id == soon_loan.id),
            "loan due tomorrow is due-soon"
        );
        assert!(
            !candidates.iter().any(|c| c.asset_id == far.id),
            "loan due in 10 days is outside the horizon"
        );
    }

    #[test]
    fn overdue_candidates_respect_scan_cap() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let today = Utc::now().date_naive();
        let past = today - chrono::Duration::days(5);
        for i in 0..3 {
            let a = asset(&mut conn, &format!("Cap-{i}"));
            issue(
                &mut conn,
                IssueLoan {
                    asset_id: a.id,
                    borrower_user_uuid: borrower.uuid,
                    loaned_at: None,
                    due_back: Some(past),
                    ticket_id: None,
                    notes: None,
                    actor_uuid: None,
                },
            )
            .unwrap();
        }
        // Three overdue loans exist; a cap of 2 returns at most 2.
        assert_eq!(
            overdue_reminder_candidates(&mut conn, today, 2)
                .unwrap()
                .len(),
            2,
            "scan cap bounds the batch"
        );
    }

    #[test]
    fn batch_stamp_marks_all_and_stops_reappearing() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let today = Utc::now().date_naive();
        let past = today - chrono::Duration::days(3);
        let issue_overdue = |conn: &mut DbConnection, name: &str| {
            let a = asset(conn, name);
            issue(
                conn,
                IssueLoan {
                    asset_id: a.id,
                    borrower_user_uuid: borrower.uuid,
                    loaned_at: None,
                    due_back: Some(past),
                    ticket_id: None,
                    notes: None,
                    actor_uuid: None,
                },
            )
            .unwrap()
        };
        let l1 = issue_overdue(&mut conn, "Batch-1");
        let l2 = issue_overdue(&mut conn, "Batch-2");

        let stamped = mark_overdue_notified_batch(&mut conn, &[l1.id, l2.id]).unwrap();
        assert_eq!(stamped, 2, "both loans stamped in one UPDATE");

        let after = overdue_reminder_candidates(&mut conn, today, 1000).unwrap();
        assert!(
            !after
                .iter()
                .any(|c| c.loan_id == l1.id || c.loan_id == l2.id),
            "batch-stamped loans stop re-appearing"
        );
    }

    #[test]
    fn edit_updates_due_date() {
        let mut conn = setup_test_connection();
        let borrower = TestFixtures::create_user(&mut conn, "borrower", "user");
        let a = asset(&mut conn, "Loaner-6");
        let loan = issue(&mut conn, issue_input(a.id, borrower.uuid)).unwrap();
        let due = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let edited = edit(&mut conn, a.id, loan.id, Some(Some(due)), None).unwrap();
        assert_eq!(edited.due_back, Some(due));
    }
}
