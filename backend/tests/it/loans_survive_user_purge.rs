//! Purging a user keeps their loans.
//!
//! The ledger rows outlive the account with a NULL borrower. A loan still out
//! is returned first through the normal return path, so the asset leaves
//! `on_loan` with a lifecycle event in its own workspace, even when the purge
//! runs pinned to another of the user's workspaces.

use diesel::prelude::*;
use uuid::Uuid;

use backend::models::{AssetLifecycleEvent, AssetLoan, NewAsset};
use backend::repository::{asset_loans, assets as asset_repo};
use backend::schema::{asset_lifecycle_events, asset_loans as loans_t, assets};
use backend::sync::actor::ActorContext;
use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool};

const REF: &str = "test:loans_survive_user_purge";

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
        attributes: serde_json::json!({}),
        quantity: None,
        unit: None,
        external_sync_source: None,
        low_stock_threshold: None,
    }
}

/// Issue a loan of a new asset to `borrower` in `workspace_id`; returning it
/// first when `returned` is set. Gives back the asset id.
fn loan_in(pool: &TestPool, workspace_id: i32, borrower: Uuid, returned: bool) -> i32 {
    run_in_workspace(pool, REF, workspace_id, |c| {
        let asset = asset_repo::create_device(c, new_asset("Loaner"))?;
        let loan = asset_loans::issue(
            c,
            asset_loans::IssueLoan {
                asset_id: asset.id,
                borrower_user_uuid: borrower,
                loaned_at: None,
                due_back: None,
                ticket_id: None,
                notes: None,
                actor_uuid: None,
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        if returned {
            asset_loans::return_loan(c, asset.id, loan.id, chrono::Utc::now(), None, None)
                .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        }
        Ok(asset.id)
    })
    .expect("issue loan")
}

#[test]
fn a_purge_returns_active_loans_and_keeps_the_ledger() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let borrower = seeded.a.member_uuid;

    let past_in_a = loan_in(&pool, a, borrower, true);
    let active_in_a = loan_in(&pool, a, borrower, false);
    let active_in_b = loan_in(&pool, b, borrower, false);

    // As the purge worker runs it: elevated, pinned to the home workspace.
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, a)?;
        backend::repository::users::purge_user(&borrower, c, None)
    })
    .expect("purge the borrower");

    let (loans, statuses, events) =
        with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
            let ids = [past_in_a, active_in_a, active_in_b];
            let loans: Vec<AssetLoan> = loans_t::table
                .filter(loans_t::asset_id.eq_any(ids))
                .order(loans_t::asset_id.asc())
                .load(c)?;
            let statuses: Vec<(i32, String)> = assets::table
                .filter(assets::id.eq_any(ids))
                .select((assets::id, assets::status))
                .load(c)?;
            let events: Vec<AssetLifecycleEvent> = asset_lifecycle_events::table
                .filter(asset_lifecycle_events::asset_id.eq_any([active_in_a, active_in_b]))
                .filter(asset_lifecycle_events::reason.eq("Borrower account deleted"))
                .load(c)?;
            Ok::<_, diesel::result::Error>((loans, statuses, events))
        })
        .expect("read back");

    assert_eq!(loans.len(), 3, "every loan survives the purge");
    assert!(
        loans.iter().all(|l| l.borrower_user_uuid.is_none()),
        "no loan names the purged borrower"
    );
    assert!(
        loans.iter().all(|l| l.returned_at.is_some()),
        "the active loans were returned"
    );
    assert!(
        statuses.iter().all(|(_, s)| s == "in_service"),
        "each asset is back at its pre-loan status: {statuses:?}"
    );

    assert_eq!(events.len(), 2, "one return event per active loan");
    for event in &events {
        assert_eq!(event.from_status.as_deref(), Some("on_loan"));
        assert_eq!(event.to_status, "in_service");
        let expected = if event.asset_id == active_in_b { b } else { a };
        assert_eq!(
            event.workspace_id, expected,
            "the return is recorded in the loan's own workspace"
        );
    }
}
