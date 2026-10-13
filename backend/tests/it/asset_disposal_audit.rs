//! A disposal record is audited and append-only for the app role.
//!
//! Recording one writes an audit row; the app role can't update or delete it;
//! deleting its asset, purging its actor and hard-deleting its workspace still
//! work, because foreign-key actions run as the table owner.

use chrono::Utc;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};
use uuid::Uuid;

use backend::models::NewAsset;
use backend::repository::asset_lifecycle::{self, DisposalInput, TransitionInput};
use backend::repository::{assets as asset_repo, workspaces};
use backend::schema::asset_disposals;
use backend::sync::actor::ActorContext;
use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, TestPooledConn};

const REF: &str = "test:asset_disposal_audit";

#[derive(QueryableByName)]
struct Op {
    #[diesel(sql_type = Text)]
    op: String,
}

/// Dispose of a new asset in `workspace_id`, recorded as `actor`. Gives back
/// the asset id and the disposal id.
fn disposed_asset(pool: &TestPool, workspace_id: i32, actor: Option<Uuid>) -> (i32, i32) {
    run_in_workspace(pool, REF, workspace_id, |c| {
        let asset = asset_repo::create_device(
            c,
            NewAsset {
                name: "Old laptop".to_string(),
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
        )?;
        asset_lifecycle::transition(
            c,
            TransitionInput {
                asset_id: asset.id,
                to_status: "disposed".to_string(),
                reason: None,
                ticket_id: None,
                metadata: serde_json::json!({}),
                actor_uuid: actor,
                disposal: Some(DisposalInput {
                    sanitization_method: "destroy".to_string(),
                    data_bearing: true,
                    certificate_file_id: None,
                    itad_vendor: Some("Shredders Ltd".to_string()),
                    notes: None,
                }),
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        let disposal = asset_lifecycle::disposal_for_asset(c, asset.id)?.expect("disposal record");
        Ok((asset.id, disposal.id))
    })
    .expect("dispose of an asset")
}

/// The audit ops recorded for disposal `id`, oldest first.
fn audit_ops(conn: &mut TestPooledConn, id: i32) -> Vec<String> {
    with_actor_bypass_context(conn, &ActorContext::system(REF), |c| {
        diesel::sql_query(
            "SELECT op::text AS op FROM audit_log \
             WHERE table_name = 'asset_disposals' AND pk_text = $1::text ORDER BY id",
        )
        .bind::<Integer, _>(id)
        .load::<Op>(c)
    })
    .expect("read audit rows")
    .into_iter()
    .map(|r| r.op)
    .collect()
}

fn disposal_exists(conn: &mut TestPooledConn, id: i32) -> bool {
    with_actor_bypass_context(conn, &ActorContext::system(REF), |c| {
        asset_disposals::table
            .find(id)
            .select(asset_disposals::id)
            .first::<i32>(c)
            .optional()
    })
    .expect("read disposal")
    .is_some()
}

#[test]
fn disposals_are_audited_and_append_only() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;

    let (asset_id, disposal_id) = disposed_asset(&pool, ws, Some(seeded.a.admin_uuid));
    let mut conn = pool.get().expect("conn");
    assert_eq!(
        audit_ops(&mut conn, disposal_id),
        ["I"],
        "recording is audited"
    );

    for sql in [
        "UPDATE asset_disposals SET notes = 'edited'",
        "DELETE FROM asset_disposals",
    ] {
        let err = run_in_workspace(&pool, REF, ws, |c| diesel::sql_query(sql).execute(c))
            .expect_err("the app role can't change a disposal");
        assert!(
            err.to_string().contains("permission denied"),
            "{sql}: {err}"
        );
    }

    run_in_workspace(&pool, REF, ws, |c| {
        asset_repo::delete_device(c, asset_id, None).map(|_| ())
    })
    .expect("delete the asset");
    assert!(
        !disposal_exists(&mut conn, disposal_id),
        "deleting the asset still removes its disposal"
    );
    assert_eq!(audit_ops(&mut conn, disposal_id), ["I", "D"]);
}

#[test]
fn purging_the_actor_keeps_the_disposal() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let actor = seeded.a.member_uuid;

    let (_, disposal_id) = disposed_asset(&pool, ws, Some(actor));
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, ws)?;
        backend::repository::users::purge_user(&actor, c, None)
    })
    .expect("purge the actor");

    let kept_actor = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        asset_disposals::table
            .find(disposal_id)
            .select(asset_disposals::actor_uuid)
            .first::<Option<Uuid>>(c)
    })
    .expect("the disposal survives the purge");
    assert_eq!(kept_actor, None, "the purged actor is cleared");
    assert_eq!(audit_ops(&mut conn, disposal_id), ["I", "U"]);
}

#[test]
fn hard_deleting_the_workspace_removes_its_disposals() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.b.workspace_id;

    let (_, disposal_id) = disposed_asset(&pool, ws, None);
    let mut conn = pool.get().expect("conn");
    let deleted = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        workspaces::archive_workspace(c, ws)?;
        workspaces::hard_delete_workspace(c, ws, Utc::now() + chrono::Duration::hours(1))
    })
    .expect("hard-delete the workspace");
    assert_eq!(deleted, 1);
    assert!(!disposal_exists(&mut conn, disposal_id));
}
