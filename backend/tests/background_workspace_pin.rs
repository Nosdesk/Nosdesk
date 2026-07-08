//! Background writes to tenant tables must pin the workspace.
//!
//! `notifications`, `outbound_emails`, etc. default `workspace_id` from the
//! `app.workspace_id` GUC. A plain `background_run` uses a system actor
//! with no workspace, so the GUC is empty, the default resolves to NULL,
//! and the insert fails the NOT NULL constraint — the exact failure seen
//! when posting a public comment (outbound reply + notification both
//! dropped). `run_in_workspace` pins the workspace (RLS-enforced) so the write
//! lands. This reproduces the bug and proves the fix on the notifications
//! table.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::Integer;

use backend::models::{NewNotification, Notification};
use backend::schema::notifications;
use backend::sync::session::{background_run, run_in_workspace};

mod common;

#[derive(QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}

fn a_notification_type_id(conn: &mut PgConnection) -> i32 {
    diesel::sql_query("SELECT id FROM notification_types ORDER BY id LIMIT 1")
        .get_result::<IdRow>(conn)
        .expect("a seeded notification_type")
        .id
}

fn new_notification(user_uuid: uuid::Uuid, type_id: i32) -> NewNotification {
    NewNotification {
        uuid: uuid::Uuid::now_v7(),
        user_uuid,
        notification_type_id: type_id,
        entity_type: "comment".to_string(),
        entity_id: 1,
        title: "New Comment".to_string(),
        body: Some("hello".to_string()),
        metadata: None,
        channels_delivered: serde_json::json!([]),
    }
}

#[test]
fn unpinned_background_write_fails_pinned_one_succeeds() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (ws_id, user_uuid, type_id) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "bgpin", "BG Pin");
        let user = common::insert_user(&mut conn, "Recipient");
        let tid = a_notification_type_id(&mut conn);
        (ws, user.uuid, tid)
    };

    // OLD path: no workspace pin -> workspace_id default resolves NULL ->
    // the NOT NULL constraint rejects the insert. The test pool's
    // customizer seeds a session-level app.workspace_id='1' (unlike
    // production's ResetAppGucs, which clears it), so clear it inside the
    // txn to reproduce the production "no ambient workspace" state.
    let unpinned = background_run(&pool, "test:bg_unpinned", |conn| {
        diesel::sql_query("SELECT set_config('app.workspace_id', '', true) AS c").execute(conn)?;
        diesel::insert_into(notifications::table)
            .values(new_notification(user_uuid, type_id))
            .execute(conn)
    });
    assert!(
        unpinned.is_err(),
        "an unpinned background insert must fail the workspace_id NOT NULL constraint"
    );

    // NEW path: pin the workspace (RLS-enforced runtime role) -> the default
    // resolves to it and the RLS WITH CHECK passes -> success.
    let row = run_in_workspace(&pool, "test:bg_pinned", ws_id, |conn| {
        diesel::insert_into(notifications::table)
            .values(new_notification(user_uuid, type_id))
            .get_result::<Notification>(conn)
    })
    .expect("a pinned background insert must succeed");

    assert_eq!(
        row.workspace_id, ws_id,
        "the row lands in the pinned workspace, not NULL"
    );
}
