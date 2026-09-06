//! Seeding push preferences when a user's first device registers.
//!
//! Push is off by default for every notification type, which is correct for a
//! browser: nothing to send to, no OS permission. The mobile app, though, asks
//! for notification permission at sign-in and registers a device — and until
//! this seeding existed, granting that permission changed nothing observable.
//!
//! The property that matters is not "it turns push on". It is that it can never
//! turn push on for someone who has turned it off, because a notification
//! setting that reverts itself is worse than one that was never seeded.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_bypass_context;

mod common;

/// The (type code, enabled) push preferences a user currently holds.
fn push_prefs(conn: &mut backend::db::DbConnection, user: Uuid) -> Vec<(String, bool)> {
    use backend::schema::{notification_preferences as np, notification_types as nt};
    np::table
        .inner_join(nt::table.on(nt::id.eq(np::notification_type_id)))
        .filter(np::user_uuid.eq(user))
        .filter(np::channel.eq("push"))
        .select((nt::code, np::enabled))
        .order(nt::code)
        .load(conn)
        .expect("load push prefs")
}

fn seed(conn: &mut backend::db::DbConnection, user: Uuid, workspace: i32) {
    let actor = ActorContext::system("test:seed_push").with_workspace(workspace);
    with_actor_bypass_context::<_, diesel::result::Error>(conn, &actor, |c| {
        backend::repository::push_devices::seed_push_defaults(c, user, workspace)?;
        Ok(())
    })
    .expect("seed");
}

#[test]
fn a_first_device_gets_assignments_and_mentions() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;

    assert!(
        push_prefs(&mut conn, user).is_empty(),
        "push is off by default for every type; nothing is seeded until a device exists"
    );

    seed(&mut conn, user, ws.a.workspace_id);

    assert_eq!(
        push_prefs(&mut conn, user),
        vec![
            ("mentioned".to_string(), true),
            ("ticket_assigned".to_string(), true),
        ],
        "both are addressed to one person by name; comment activity is deliberately absent"
    );
}

/// The guard that matters. A user who turns a push preference off and later
/// signs in on another device must stay off: `register` is called on every
/// launch, so a seed that overwrote would silently re-enable it forever.
#[test]
fn seeding_never_overwrites_a_users_choice() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;

    seed(&mut conn, user, ws.a.workspace_id);

    // The user opens settings and turns mentions off.
    {
        use backend::schema::{notification_preferences as np, notification_types as nt};
        let actor = ActorContext::system("test:user_opt_out").with_workspace(ws.a.workspace_id);
        with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            let mentioned_id: i32 = nt::table
                .filter(nt::code.eq("mentioned"))
                .select(nt::id)
                .first(c)?;
            diesel::update(
                np::table
                    .filter(np::user_uuid.eq(user))
                    .filter(np::notification_type_id.eq(mentioned_id))
                    .filter(np::channel.eq("push")),
            )
            .set((np::enabled.eq(false), np::frequency.eq("off")))
            .execute(c)?;
            Ok(())
        })
        .expect("opt out");
    }

    // A second device registers, so the seed runs again.
    seed(&mut conn, user, ws.a.workspace_id);

    assert_eq!(
        push_prefs(&mut conn, user),
        vec![
            ("mentioned".to_string(), false),
            ("ticket_assigned".to_string(), true),
        ],
        "the opt-out must survive; ON CONFLICT DO NOTHING is what guarantees it"
    );
}

/// Idempotent, because the client may call register on every launch.
#[test]
fn seeding_twice_creates_no_duplicates() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;

    seed(&mut conn, user, ws.a.workspace_id);
    let after_first = push_prefs(&mut conn, user);
    seed(&mut conn, user, ws.a.workspace_id);

    assert_eq!(push_prefs(&mut conn, user), after_first);
}
