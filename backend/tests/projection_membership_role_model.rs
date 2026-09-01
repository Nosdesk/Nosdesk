//! Production-faithful test for the provisioning membership write.
//!
//! The other integration tests run the HTTP handlers with the test
//! connection's BASE role left as the superuser (`nosdesk`), so they never
//! exercise the production role model: in hosted production the runtime
//! connects as `nosdesk_app` (NOBYPASSRLS) and the membership grant relies
//! on `SET LOCAL ROLE nosdesk_admin` working via the
//! `GRANT nosdesk_admin TO nosdesk_app WITH SET TRUE` grant. That gap is
//! why a membership write could "succeed" in tests yet write nothing in
//! production.
//!
//! This test pins the connection's session role to `nosdesk_app` first, so
//! `ensure_membership` runs through the real `with_actor_bypass_context`
//! elevation path with a real commit, then reads the row back as the
//! superuser to observe the true committed state. It asserts the ROW, not
//! a return value.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::BigInt;

use backend::repository::workspaces;
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};

mod common;

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn membership_count(conn: &mut PgConnection, workspace_id: i32, user_uuid: uuid::Uuid) -> i64 {
    diesel::sql_query(
        "SELECT count(*) AS n FROM workspace_members \
         WHERE workspace_id = $1 AND user_uuid = $2",
    )
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .bind::<diesel::sql_types::Uuid, _>(user_uuid)
    .get_result::<CountRow>(conn)
    .expect("count")
    .n
}

#[test]
fn provisioning_membership_persists_under_nosdesk_app_role() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    // Arrange: a workspace + user (created as the superuser base role).
    let mut conn = pool.get().expect("conn");
    let ws_id = common::mint_workspace(&mut conn, "rolecheck", "Role Check");
    let user = common::insert_user(&mut conn, "Owner Person");

    // Pin the SESSION role to nosdesk_app, mirroring the production runtime
    // connection. From here, the membership write must elevate to
    // nosdesk_admin via the role grant exactly as it does in production.
    diesel::sql_query("SET ROLE nosdesk_app")
        .execute(&mut conn)
        .expect("set role nosdesk_app");

    // Act: the exact production write path — bypass context + the
    // self-verifying upsert, committed for real (the pool does not wrap
    // tests in a rolled-back transaction).
    let actor = ActorContext::system("test:provision_membership").with_workspace(ws_id);
    let persisted =
        with_actor_bypass_context::<String, diesel::result::Error>(&mut conn, &actor, |c| {
            workspaces::ensure_membership(c, ws_id, user.uuid, "owner")
        })
        .expect("ensure_membership must succeed");
    assert_eq!(persisted, "owner", "RETURNING must echo the persisted role");

    // Assert the ROW, as the superuser (RLS bypassed), on a separate
    // checkout — the true committed state, not a return value or a log.
    diesel::sql_query("RESET ROLE")
        .execute(&mut conn)
        .expect("reset role");
    drop(conn);

    let mut verify = pool.get().expect("verify conn");
    assert_eq!(
        membership_count(&mut verify, ws_id, user.uuid),
        1,
        "the membership row must exist after a committed provisioning write"
    );
}

/// The membership 403 gate reads `workspace_members` under RLS. On a raw
/// connection without `app.workspace_id` set, the isolation policy hides
/// the row, so a real member reads as "not a member" — the production bug.
/// The fix scopes the read through `with_actor_context` (workspace pinned),
/// which sets `app.workspace_id` so the row is visible. This test pins the
/// session role to `nosdesk_app` (the production runtime role) so RLS is
/// actually enforced — the other integration tests run as the superuser
/// and would see the row regardless, which is why they missed this.
#[test]
fn membership_gate_needs_workspace_scope_to_see_the_row() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let mut conn = pool.get().expect("conn");
    let ws_id = common::mint_workspace(&mut conn, "gatecheck", "Gate Check");
    let user = common::insert_user(&mut conn, "Member Person");
    // Seed the membership (as superuser; the write path is covered above).
    workspaces::add_membership(
        &mut conn,
        ws_id,
        user.uuid,
        "admin",
        workspaces::SeatWriteAuthority::ControlPlane,
    )
    .expect("seed membership");

    // Mirror the production runtime: nosdesk_app (RLS-enforced) with no
    // tenant scope established (the gate's raw connection state).
    diesel::sql_query("SET ROLE nosdesk_app")
        .execute(&mut conn)
        .expect("set role nosdesk_app");
    diesel::sql_query("SELECT set_config('app.workspace_id', '', false) AS c")
        .execute(&mut conn)
        .expect("clear workspace guc");

    // Unscoped read (the OLD gate): RLS hides the row -> false "not a member".
    let unscoped = workspaces::membership(&mut conn, ws_id, user.uuid).expect("unscoped read");
    assert!(
        unscoped.is_none(),
        "without app.workspace_id, RLS hides the membership row (the bug)"
    );

    // Scoped read (the FIX): pin the workspace via with_actor_context so
    // app.workspace_id is set; the row is now visible.
    let scoped = with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user_at_workspace(user.uuid, ws_id),
        |c| workspaces::membership(c, ws_id, user.uuid),
    )
    .expect("scoped read");
    assert!(
        scoped.is_some(),
        "scoping the read to the resolved workspace makes the membership visible"
    );
}
