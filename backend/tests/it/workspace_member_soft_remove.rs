//! Soft-removed memberships: the row survives, the access does not.
//!
//! `remove_membership` stamps `removed_at` instead of deleting, so a former
//! colleague's name still resolves on the tickets and comments they left
//! behind. Everything else must behave as if the row were gone, and the
//! re-invite path has to work, since a conflicting row now always exists.

#![allow(clippy::expect_used)]

use diesel::prelude::*;

use backend::db::DbConnection;
use backend::repository::workspaces::{
    add_membership, count_staff_members, membership, remove_membership, AddMembershipOutcome,
    RemoveMembershipOutcome, SeatWriteAuthority,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn try_as_actor<T>(
    conn: &mut DbConnection,
    workspace_id: i32,
    user: uuid::Uuid,
    f: impl FnOnce(&mut DbConnection) -> QueryResult<T>,
) -> QueryResult<T> {
    let actor = ActorContext::user(user, None).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f)
}

fn as_actor<T>(
    conn: &mut DbConnection,
    workspace_id: i32,
    user: uuid::Uuid,
    f: impl FnOnce(&mut DbConnection) -> QueryResult<T>,
) -> T {
    try_as_actor(conn, workspace_id, user, f).expect("actor txn")
}

/// Whether adding this member succeeded. The seat-limit trigger raises, which
/// aborts the transaction, so the error has to propagate out of the actor
/// wrapper for the rollback to happen; swallowing it inside leaves the
/// connection poisoned for every later statement.
fn try_add_staff(conn: &mut DbConnection, workspace_id: i32, user: uuid::Uuid) -> bool {
    try_as_actor(conn, workspace_id, user, |c| {
        add_membership(
            c,
            workspace_id,
            user,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )
    })
    .is_ok()
}

/// The raw row, ignoring `removed_at`, which is what identity resolution does.
fn row_exists(conn: &mut DbConnection, workspace_id: i32, user: uuid::Uuid) -> bool {
    use backend::schema::workspace_members as wm;
    diesel::select(diesel::dsl::exists(
        wm::table
            .filter(wm::workspace_id.eq(workspace_id))
            .filter(wm::user_uuid.eq(user)),
    ))
    .get_result(conn)
    .expect("row exists")
}

fn set_seat_limit(conn: &mut DbConnection, workspace_id: i32, limit: i32) {
    use backend::schema::workspaces;
    diesel::update(workspaces::table.filter(workspaces::id.eq(workspace_id)))
        .set(workspaces::seat_limit.eq(limit))
        .execute(conn)
        .expect("set seat limit");
}

#[test]
fn removal_keeps_the_row_but_ends_the_membership() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ws = crate::common::mint_workspace(&mut conn, "acme-soft-remove", "Acme Soft Remove");
    let leaver = crate::common::insert_user(&mut conn, "The Leaver");

    as_actor(&mut conn, ws, leaver.uuid, |c| {
        add_membership(
            c,
            ws,
            leaver.uuid,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        Ok(())
    });
    assert!(
        as_actor(&mut conn, ws, leaver.uuid, |c| membership(
            c,
            ws,
            leaver.uuid
        ))
        .is_some(),
        "precondition: an active membership"
    );

    let outcome = as_actor(&mut conn, ws, leaver.uuid, |c| {
        remove_membership(c, ws, leaver.uuid, SeatWriteAuthority::ControlPlane)
    });
    assert!(matches!(outcome, RemoveMembershipOutcome::Removed));

    assert!(
        as_actor(&mut conn, ws, leaver.uuid, |c| membership(
            c,
            ws,
            leaver.uuid
        ))
        .is_none(),
        "a removed member must not resolve as a member"
    );
    assert!(
        row_exists(&mut conn, ws, leaver.uuid),
        "but the row must survive, or their name vanishes from everything they touched"
    );
}

#[test]
fn a_removed_staff_member_frees_their_seat() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ws = crate::common::mint_workspace(&mut conn, "acme-seat-freed", "Acme Seat Freed");
    let first = crate::common::insert_user(&mut conn, "First Agent");
    let second = crate::common::insert_user(&mut conn, "Second Agent");

    as_actor(&mut conn, ws, first.uuid, |c| {
        add_membership(c, ws, first.uuid, "agent", SeatWriteAuthority::ControlPlane)?;
        Ok(())
    });
    set_seat_limit(&mut conn, ws, 1);

    // The seat is taken, so a second agent is refused.
    assert!(
        !try_add_staff(&mut conn, ws, second.uuid),
        "precondition: the seat cap is enforced"
    );

    as_actor(&mut conn, ws, first.uuid, |c| {
        remove_membership(c, ws, first.uuid, SeatWriteAuthority::ControlPlane)
    });
    assert_eq!(
        as_actor(&mut conn, ws, first.uuid, |c| count_staff_members(c, ws)),
        0,
        "a removed staff member must stop consuming a licensed seat"
    );

    assert!(
        try_add_staff(&mut conn, ws, second.uuid),
        "and the freed seat must be usable"
    );
}

#[test]
fn re_adding_a_removed_member_restores_them() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ws = crate::common::mint_workspace(&mut conn, "acme-rejoin", "Acme Rejoin");
    let returner = crate::common::insert_user(&mut conn, "The Returner");

    as_actor(&mut conn, ws, returner.uuid, |c| {
        add_membership(
            c,
            ws,
            returner.uuid,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        Ok(())
    });
    as_actor(&mut conn, ws, returner.uuid, |c| {
        remove_membership(c, ws, returner.uuid, SeatWriteAuthority::ControlPlane)
    });

    // The row still exists, so the insert conflicts. Under the old
    // `ON CONFLICT DO NOTHING` this silently left them removed.
    let outcome = as_actor(&mut conn, ws, returner.uuid, |c| {
        add_membership(
            c,
            ws,
            returner.uuid,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )
    });
    assert!(matches!(outcome, AddMembershipOutcome::Added(n) if n == 1));
    assert!(
        as_actor(&mut conn, ws, returner.uuid, |c| membership(
            c,
            ws,
            returner.uuid
        ))
        .is_some(),
        "re-adding a removed member must make them a member again"
    );
}

/// Re-invites are hires. Removing a staff member frees their seat, so coming
/// back has to compete for one like anyone else, or a workspace could cycle
/// people through removal and re-adding to exceed its licence.
///
/// Worth knowing which mechanism enforces this. Re-activation runs through
/// `add_membership`'s `ON CONFLICT DO UPDATE`, and a conflicting upsert fires
/// BEFORE INSERT and then BEFORE UPDATE, so the seat check refuses on the
/// INSERT pass. The `OLD.removed_at IS NULL` condition added to the trigger's
/// UPDATE short-circuit is defence-in-depth for a future direct
/// `UPDATE ... SET removed_at = NULL`, and this test passes with or without
/// it; it is the behaviour that is pinned here, not that clause.
#[test]
fn re_adding_a_removed_staff_member_still_respects_the_seat_cap() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");

    let ws = crate::common::mint_workspace(&mut conn, "acme-rejoin-cap", "Acme Rejoin Cap");
    let leaver = crate::common::insert_user(&mut conn, "Departed Agent");
    let replacement = crate::common::insert_user(&mut conn, "Replacement Agent");

    as_actor(&mut conn, ws, leaver.uuid, |c| {
        add_membership(
            c,
            ws,
            leaver.uuid,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        Ok(())
    });
    set_seat_limit(&mut conn, ws, 1);

    // They leave, and the one seat is given to somebody else.
    as_actor(&mut conn, ws, leaver.uuid, |c| {
        remove_membership(c, ws, leaver.uuid, SeatWriteAuthority::ControlPlane)
    });
    assert!(
        try_add_staff(&mut conn, ws, replacement.uuid),
        "the freed seat goes to the replacement"
    );

    // Now the leaver comes back. The seat is gone, so this must be refused.
    assert!(
        !try_add_staff(&mut conn, ws, leaver.uuid),
        "re-activating a removed staff member consumes a seat and must be capped \
         like any other hire"
    );
}
