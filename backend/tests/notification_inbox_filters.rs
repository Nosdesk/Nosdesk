//! The inbox list is server-truth per tab: `unread_only` and
//! `notification_type` narrow the query, and keyset paging survives rows
//! leaving the set between pages (a row marked read on the Unread tab must
//! neither hide the next unseen row nor repeat one).

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use diesel::prelude::*;
use tokio::sync::RwLock;
use uuid::Uuid;

use backend::models::NewNotification;
use backend::services::notifications::{InboxFilter, NotificationService};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn type_id(conn: &mut backend::db::DbConnection, type_code: &str) -> i32 {
    use backend::schema::notification_types;
    notification_types::table
        .filter(notification_types::code.eq(type_code))
        .select(notification_types::id)
        .first(conn)
        .expect("seeded notification type")
}

/// Insert one notification. Rows get distinct `created_at` values via the
/// `at` offset (seconds before now) so ordering is deterministic; the last
/// two share a timestamp to exercise the id tiebreak.
fn insert(
    conn: &mut backend::db::DbConnection,
    workspace_id: i32,
    user: Uuid,
    type_id: i32,
    title: &str,
    read: bool,
    at: chrono::NaiveDateTime,
) -> i32 {
    use backend::schema::notifications;
    let actor = ActorContext::system("test:notif_insert").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let id: i32 = diesel::insert_into(notifications::table)
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
                interrupts: true,
                source_sync_id: None,
            })
            .returning(notifications::id)
            .get_result(c)?;
        diesel::update(notifications::table.find(id))
            .set((
                notifications::created_at.eq(at),
                notifications::is_read.eq(read),
            ))
            .execute(c)?;
        Ok(id)
    })
    .expect("insert notification")
}

fn service(pool: backend::db::Pool) -> NotificationService {
    NotificationService::new(pool, Arc::new(RwLock::new(HashMap::new())))
}

#[test]
fn tabs_are_filtered_server_side_and_page_by_cursor() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;
    let ws_id = ws.a.workspace_id;
    let mention = type_id(&mut conn, "mentioned");
    let comment = type_id(&mut conn, "comment_added");

    let base = chrono::Utc::now().naive_utc() - chrono::Duration::hours(1);
    let at = |secs: i64| base + chrono::Duration::seconds(secs);
    // Newest first once listed: m5, c4, m3(read), c2, m1(read), c0 where
    // c0 and m1 share a timestamp.
    let ids: Vec<i32> = vec![
        insert(&mut conn, ws_id, user, comment, "c0", false, at(0)),
        insert(&mut conn, ws_id, user, mention, "m1", true, at(0)),
        insert(&mut conn, ws_id, user, comment, "c2", false, at(2)),
        insert(&mut conn, ws_id, user, mention, "m3", true, at(3)),
        insert(&mut conn, ws_id, user, comment, "c4", false, at(4)),
        insert(&mut conn, ws_id, user, mention, "m5", false, at(5)),
    ];
    // A row in the other workspace must never leak into A's tabs.
    insert(
        &mut conn,
        ws.b.workspace_id,
        user,
        mention,
        "other-ws",
        false,
        at(9),
    );
    drop(conn);

    let svc = service(pool.clone());
    let rt = actix_web::rt::System::new();
    let titles = |filter: InboxFilter, limit: i64| -> Vec<String> {
        rt.block_on(svc.list(&user, ws_id, filter, limit, 0))
            .expect("list")
            .into_iter()
            .map(|n| n.title)
            .collect()
    };

    assert_eq!(
        titles(InboxFilter::default(), 50),
        ["m5", "c4", "m3", "c2", "m1", "c0"],
        "all: newest first, id breaks the shared timestamp"
    );
    assert_eq!(
        titles(
            InboxFilter {
                unread_only: true,
                ..Default::default()
            },
            50
        ),
        ["m5", "c4", "c2", "c0"]
    );
    assert_eq!(
        titles(
            InboxFilter {
                notification_type: Some("mentioned".into()),
                ..Default::default()
            },
            50
        ),
        ["m5", "m3", "m1"]
    );
    assert_eq!(
        titles(
            InboxFilter {
                unread_only: true,
                notification_type: Some("mentioned".into()),
                ..Default::default()
            },
            50
        ),
        ["m5"]
    );

    // Unread tab, page size 2: first page is m5, c4. Mark c4 read before
    // asking for the next page (what the Unread tab does optimistically).
    // With the cursor the next page is c2, c0; an offset of 2 would have
    // skipped c2.
    let unread = || InboxFilter {
        unread_only: true,
        ..Default::default()
    };
    let first = rt
        .block_on(svc.list(&user, ws_id, unread(), 2, 0))
        .expect("page 1");
    assert_eq!(
        first.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
        ["m5", "c4"]
    );
    let last = first.last().expect("two rows");
    rt.block_on(svc.mark_read(&user, ws_id, &[ids[4]]))
        .expect("mark c4 read");
    let second = rt
        .block_on(svc.list(
            &user,
            ws_id,
            InboxFilter {
                before: Some((last.created_at, last.id)),
                ..unread()
            },
            2,
            0,
        ))
        .expect("page 2");
    assert_eq!(
        second.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
        ["c2", "c0"],
        "cursor paging must not skip c2 after c4 left the unread set"
    );

    // Type-scoped mark-all-read clears only mentions.
    let cleared = rt
        .block_on(svc.mark_all_read(&user, ws_id, Some("mentioned")))
        .expect("mark mentions read");
    assert_eq!(cleared, 1, "only m5 was an unread mention");
    assert_eq!(titles(unread(), 50), ["c2", "c0"]);
}
