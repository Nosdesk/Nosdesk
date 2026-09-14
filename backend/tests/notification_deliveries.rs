//! Per-channel delivery rows: written once per notification, retried with
//! backoff, failed once the attempts are spent, and claimed exclusively.
#![allow(clippy::expect_used)]

use diesel::prelude::*;

use backend::models::{NewNotification, Notification};
use backend::schema::{notification_deliveries as nd, notifications};
use backend::services::notifications::deliveries::{
    insert_for, mark_attempt_failed, mark_delivered, pending_channels, MAX_ATTEMPTS,
    STATUS_DELIVERED, STATUS_FAILED, STATUS_PENDING,
};
use backend::services::notifications::types::{
    NotificationActor, NotificationChannel, NotificationEntity, NotificationPayload,
    NotificationTypeCode,
};
use backend::sync::session::run_in_workspace;
use backend::sync::ActorKind;

mod common;

fn a_notification_type_id(conn: &mut PgConnection) -> i32 {
    notifications::table
        .select(notifications::notification_type_id)
        .first(conn)
        .ok()
        .unwrap_or_else(|| {
            backend::schema::notification_types::table
                .select(backend::schema::notification_types::id)
                .order(backend::schema::notification_types::id.asc())
                .first(conn)
                .expect("a seeded notification type")
        })
}

fn seed(pool: &backend::db::Pool, ws_id: i32, user: uuid::Uuid) -> (i32, NotificationPayload) {
    let payload = NotificationPayload::new(
        NotificationTypeCode::TicketAssigned,
        user,
        NotificationActor {
            uuid: uuid::Uuid::nil(),
            name: "System".into(),
            avatar_thumb: None,
            kind: ActorKind::System,
        },
        NotificationEntity::Ticket {
            id: 1,
            title: "T".into(),
        },
        ws_id,
    );
    let type_id = run_in_workspace(pool, "test:type", ws_id, |conn| {
        Ok(a_notification_type_id(conn))
    })
    .expect("type id");
    let p = payload.clone();
    let id = run_in_workspace(pool, "test:seed", ws_id, move |conn| {
        let n: Notification = diesel::insert_into(notifications::table)
            .values(NewNotification {
                uuid: uuid::Uuid::now_v7(),
                user_uuid: user,
                notification_type_id: type_id,
                entity_type: "ticket".into(),
                entity_id: 1,
                title: "Assigned".into(),
                body: None,
                metadata: None,
                channels_delivered: serde_json::json!([]),
                interrupts: true,
                source_sync_id: None,
            })
            .get_result(conn)?;
        insert_for(
            conn,
            n.id,
            ws_id,
            &p,
            &[
                NotificationChannel::InApp,
                NotificationChannel::Email,
                NotificationChannel::Push,
            ],
            &[NotificationChannel::InApp],
        )?;
        Ok(n.id)
    })
    .expect("seed notification + deliveries");
    (id, payload)
}

#[test]
fn rows_are_written_once_and_in_app_starts_delivered() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool();
    let (ws, user) = {
        let mut conn = pool.get().expect("conn");
        (
            common::mint_workspace(&mut conn, "deliv", "Deliveries"),
            common::insert_user(&mut conn, "deliv-recipient").uuid,
        )
    };
    let (id, payload) = seed(&pool, ws, user);

    let statuses: Vec<(String, String)> = run_in_workspace(&pool, "test:read", ws, |conn| {
        nd::table
            .filter(nd::notification_id.eq(id))
            .order(nd::channel.asc())
            .select((nd::channel, nd::status))
            .load(conn)
    })
    .expect("rows");
    assert_eq!(
        statuses,
        vec![
            ("email".to_string(), STATUS_PENDING.to_string()),
            ("in_app".to_string(), STATUS_DELIVERED.to_string()),
            ("push".to_string(), STATUS_PENDING.to_string()),
        ]
    );

    // A redelivery inserts nothing and sees the two still owed.
    run_in_workspace(&pool, "test:again", ws, |conn| {
        insert_for(
            conn,
            id,
            ws,
            &payload,
            &[
                NotificationChannel::InApp,
                NotificationChannel::Email,
                NotificationChannel::Push,
            ],
            &[NotificationChannel::InApp],
        )?;
        let pending = pending_channels(conn, id)?;
        assert_eq!(
            pending,
            vec![NotificationChannel::Email, NotificationChannel::Push]
        );
        Ok(())
    })
    .expect("idempotent");
}

#[test]
fn a_failed_attempt_reschedules_then_fails_when_spent() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool();
    let (ws, user) = {
        let mut conn = pool.get().expect("conn");
        (
            common::mint_workspace(&mut conn, "deliv2", "Deliveries 2"),
            common::insert_user(&mut conn, "deliv-recipient-2").uuid,
        )
    };
    let (id, _) = seed(&pool, ws, user);

    let read = |pool: &backend::db::Pool| -> (i16, String, Option<String>, bool) {
        run_in_workspace(pool, "test:read", ws, |conn| {
            nd::table
                .filter(nd::notification_id.eq(id))
                .filter(nd::channel.eq("push"))
                .select((
                    nd::attempts,
                    nd::status,
                    nd::last_error,
                    nd::next_attempt_at.is_not_null(),
                ))
                .first(conn)
        })
        .expect("row")
    };

    run_in_workspace(&pool, "test:fail1", ws, |conn| {
        mark_attempt_failed(conn, id, NotificationChannel::Push, "dpa_required")
    })
    .expect("fail");
    let (attempts, status, err, scheduled) = read(&pool);
    assert_eq!(
        (attempts, status.as_str(), err.as_deref(), scheduled),
        (1, STATUS_PENDING, Some("dpa_required"), true)
    );

    for _ in 1..MAX_ATTEMPTS {
        run_in_workspace(&pool, "test:failn", ws, |conn| {
            mark_attempt_failed(conn, id, NotificationChannel::Push, "unreachable")
        })
        .expect("fail");
    }
    let (attempts, status, _, scheduled) = read(&pool);
    assert_eq!(
        (attempts, status.as_str(), scheduled),
        (MAX_ATTEMPTS, STATUS_FAILED, false)
    );

    // Email, untouched, still delivers and updates the read model too.
    run_in_workspace(&pool, "test:ok", ws, |conn| {
        mark_delivered(conn, id, NotificationChannel::Email)
    })
    .expect("delivered");
    let pending = run_in_workspace(&pool, "test:pending", ws, |conn| pending_channels(conn, id))
        .expect("pending");
    assert!(
        pending.is_empty(),
        "nothing left pending: email delivered, push failed"
    );
}
