//! Proves the notification bell/inbox is scoped to the ACTIVE workspace.
//!
//! The inbox service methods filter by `user_uuid` and now run under
//! `run_in_workspace(active_ws)`, so RLS on `notifications`
//! (`workspace_id = app.workspace_id`) confines every read / count / mutation
//! to the active workspace. This is the regression guard for the pre-Model-C
//! behavior where the bell spanned ALL of a user's workspaces: a user active in
//! workspace A would see (and could mutate) workspace B's notifications, and
//! clicking one deep-linked into the wrong workspace.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::models::NewNotification;
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};

mod common;

fn any_notification_type_id(conn: &mut backend::db::DbConnection) -> i32 {
    use backend::schema::notification_types;
    notification_types::table
        .select(notification_types::id)
        .order(notification_types::id)
        .first(conn)
        .expect("a seeded notification_type exists")
}

/// Insert a notification pinned to `workspace_id`. `NewNotification` carries no
/// workspace_id: the column defaults to `app.workspace_id`, which the pin sets,
/// exactly as `persist_notification` does in production.
fn insert_notification(
    conn: &mut backend::db::DbConnection,
    workspace_id: i32,
    user: Uuid,
    type_id: i32,
    title: &str,
) {
    use backend::schema::notifications;
    let actor = ActorContext::system("test:notif_insert").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        diesel::insert_into(notifications::table)
            .values(NewNotification {
                uuid: Uuid::now_v7(),
                user_uuid: user,
                notification_type_id: type_id,
                entity_type: "ticket".to_string(),
                entity_id: 1,
                title: title.to_string(),
                body: None,
                metadata: None,
                channels_delivered: serde_json::json!([]),
            })
            .execute(c)
    })
    .expect("insert notification pinned to workspace");
}

/// Titles visible to a `user_uuid`-filtered read pinned to `workspace_id` — the
/// exact shape of every inbox method's query under `run_in_workspace`.
fn titles_pinned(conn: &mut backend::db::DbConnection, ws_id: i32, user: Uuid) -> Vec<String> {
    use backend::schema::notifications::dsl::*;
    let actor = ActorContext::system("test:notif_read").with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        notifications
            .filter(user_uuid.eq(user))
            .order(id.asc())
            .select(title)
            .load::<String>(c)
    })
    .expect("read notifications pinned to workspace")
}

#[test]
fn inbox_reads_are_scoped_to_the_active_workspace() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;
    let type_id = any_notification_type_id(&mut conn);

    insert_notification(&mut conn, ws.a.workspace_id, user, type_id, "in A");
    insert_notification(&mut conn, ws.b.workspace_id, user, type_id, "in B");

    // The fix: a workspace-pinned, user-filtered read sees only that workspace.
    assert_eq!(
        titles_pinned(&mut conn, ws.a.workspace_id, user),
        vec!["in A".to_string()],
        "workspace A's bell must show only A's notification"
    );
    assert_eq!(
        titles_pinned(&mut conn, ws.b.workspace_id, user),
        vec!["in B".to_string()],
        "workspace B's bell must show only B's notification"
    );

    // Sanity + pre-fix contrast: both rows exist for this user; an unpinned
    // BYPASS read (what `background_run` did before this fix) sees BOTH — the
    // cross-workspace bleed the pin removes.
    let actor = ActorContext::system("test:notif_bypass");
    let both = with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        use backend::schema::notifications::dsl::*;
        notifications
            .filter(user_uuid.eq(user))
            .order(id.asc())
            .select(title)
            .load::<String>(c)
    })
    .expect("bypass read");
    assert_eq!(
        both,
        vec!["in A".to_string(), "in B".to_string()],
        "sanity: both notifications exist; only the workspace pin scopes the bell"
    );
}
