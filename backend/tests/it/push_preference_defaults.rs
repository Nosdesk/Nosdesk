//! Push's default: derived from device presence, never written down.
//!
//! Push is absent from every notification type's `default_channels`, so it
//! resolves in `resolve_channel` instead: `off` while the user has no device
//! (nothing to send to), and `instant` on the types the policy names once one
//! registers.
//!
//! This used to be done by seeding rows into `notification_preferences` when a
//! device registered. Those rows were byte-identical to ones the user wrote by
//! toggling the switch, which defeated the resolver's tri-state (absent means
//! inherit, present means explicit): the UI could not tell a default from a
//! choice, and a workspace admin's default was silently overridden by rows the
//! user never authored. The property pinned below is that registering a device
//! writes nothing at all.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::services::notifications::preferences::PreferenceService;
use backend::services::notifications::types::{
    NotificationChannel, NotificationFrequency, NotificationTypeCode,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_bypass_context;

/// Count of stored push preference rows for a user.
fn stored_push_rows(conn: &mut backend::db::DbConnection, user: Uuid) -> i64 {
    use backend::schema::notification_preferences as np;
    np::table
        .filter(np::user_uuid.eq(user))
        .filter(np::channel.eq("push"))
        .count()
        .get_result(conn)
        .expect("count push prefs")
}

fn register_device(conn: &mut backend::db::DbConnection, user: Uuid, workspace: i32, token: &str) {
    let actor = ActorContext::system("test:register_device").with_workspace(workspace);
    with_actor_bypass_context::<_, diesel::result::Error>(conn, &actor, |c| {
        backend::repository::push_devices::register(c, user, workspace, "ios", token, None)?;
        Ok(())
    })
    .expect("register device");
}

/// Whether push resolves to deliver-now for a type.
async fn push_delivers(
    service: &PreferenceService,
    user: &Uuid,
    workspace: i32,
    code: &str,
) -> bool {
    let type_code = NotificationTypeCode::from_str(code).expect("known notification type");
    service
        .get_enabled_channels(user, workspace, &type_code)
        .await
        .expect("resolve channels")
        .contains(&NotificationChannel::Push)
}

#[actix_web::rt::test]
async fn push_is_off_until_a_device_exists_then_on_for_the_named_types() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;
    let workspace = ws.a.workspace_id;
    let service = PreferenceService::new(db.pool());

    assert!(
        !push_delivers(&service, &user, workspace, "ticket_assigned").await,
        "with no device there is nothing to send to, so push must not resolve on"
    );

    register_device(&mut conn, user, workspace, "test-token-1");
    service.invalidate_for_device_change().await;

    assert!(
        push_delivers(&service, &user, workspace, "ticket_assigned").await,
        "an assignment makes a ticket yours; it pushes once a device exists"
    );
    assert!(
        push_delivers(&service, &user, workspace, "mentioned").await,
        "a mention asks for you specifically; it pushes once a device exists"
    );
}

/// Comment activity interrupts in-app but must not push: on a busy ticket it is
/// the noisiest thing a helpdesk produces. Pins that the policy is its own list
/// rather than the type's `interrupts` flag, which also covers comments.
#[actix_web::rt::test]
async fn a_registered_device_does_not_turn_on_the_noisy_types() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;
    let workspace = ws.a.workspace_id;

    register_device(&mut conn, user, workspace, "test-token-2");
    let service = PreferenceService::new(db.pool());

    assert!(
        !push_delivers(&service, &user, workspace, "comment_added").await,
        "a phone buzzing for every reply is how users learn to disable push"
    );
}

/// The reason the default moved out of stored rows. Registering a device must
/// leave `notification_preferences` untouched, so absent still means "default"
/// and the settings UI can distinguish it from a choice the user made.
#[actix_web::rt::test]
async fn registering_a_device_stores_no_preferences() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;

    assert_eq!(stored_push_rows(&mut conn, user), 0);
    register_device(&mut conn, user, ws.a.workspace_id, "test-token-3");
    assert_eq!(
        stored_push_rows(&mut conn, user),
        0,
        "the default is derived, never written; a stored row must mean the user chose it"
    );
}

/// A user who turns push off keeps it off when another device registers.
/// `register` runs on every launch, so this is the regression that a
/// write-on-register default would reintroduce.
#[actix_web::rt::test]
async fn an_explicit_opt_out_survives_a_new_device() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;
    let workspace = ws.a.workspace_id;
    let service = PreferenceService::new(db.pool());

    register_device(&mut conn, user, workspace, "test-token-4");
    service.invalidate_for_device_change().await;
    assert!(push_delivers(&service, &user, workspace, "ticket_assigned").await);

    service
        .set_preference(
            &user,
            &NotificationTypeCode::from_str("ticket_assigned").expect("known type"),
            NotificationChannel::Push,
            NotificationFrequency::Off,
        )
        .await
        .expect("opt out");

    register_device(&mut conn, user, workspace, "test-token-5");
    service.invalidate_for_device_change().await;

    assert!(
        !push_delivers(&service, &user, workspace, "ticket_assigned").await,
        "an explicit off must outrank the default, however many devices register"
    );
}
