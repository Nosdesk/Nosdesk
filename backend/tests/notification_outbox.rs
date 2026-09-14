//! The notification outbox: what the trigger enqueues, how a claim behaves,
//! and that a derived notification cannot be persisted twice.
//!
//! The dispatcher loop itself is not exercised here (it needs a running
//! service with channels); its transactional pieces are, through the same
//! functions it calls.
#![allow(clippy::expect_used)]

use diesel::prelude::*;

use backend::db::DbConnection;
use backend::models::{NewNotification, SyncAggregate, SyncOp};
use backend::schema::{notification_outbox, notifications};
use backend::services::notifications::outbox::{ack, claim_batch, fail, MAX_ATTEMPTS};
use backend::sync::emit::{record, SyncEmit};

mod common;

fn emit(conn: &mut DbConnection, event_type: &'static str, data: serde_json::Value) -> i64 {
    record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::Ticket,
            aggregate_id: "1".into(),
            op: SyncOp::Update,
            event_type,
            data,
            groups: vec!["workspace".into()],
            causation_id: None,
        },
    )
    .expect("emit sync action")
}

fn enqueued(conn: &mut DbConnection, sync_id: i64) -> bool {
    notification_outbox::table
        .filter(notification_outbox::sync_id.eq(sync_id))
        .count()
        .get_result::<i64>(conn)
        .expect("count outbox")
        == 1
}

#[test]
fn trigger_enqueues_only_rows_the_deriver_acts_on() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool();
    let mut conn = pool.get().expect("conn");

    let a = "0192aaaa-0000-7000-8000-00000000000a";

    // A full row without the previous_* key: every ticket emitter carries
    // assignee_uuid, so its presence alone must not enqueue.
    let tag_edit = emit(
        &mut conn,
        "ticket.tag_ids",
        serde_json::json!({"id": 1, "assignee_uuid": a}),
    );
    assert!(
        !enqueued(&mut conn, tag_edit),
        "a tag edit is not an assignment"
    );

    let assigned = emit(
        &mut conn,
        "ticket.assignee_changed",
        serde_json::json!({"id": 1, "assignee_uuid": a, "previous_assignee_uuid": null}),
    );
    assert!(enqueued(&mut conn, assigned), "an assignment is enqueued");

    let unchanged = emit(
        &mut conn,
        "ticket.updated",
        serde_json::json!({"id": 1, "assignee_uuid": a, "previous_assignee_uuid": a,
                           "workflow_state_id": 2, "previous_workflow_state_id": 2}),
    );
    assert!(
        !enqueued(&mut conn, unchanged),
        "nothing changed, nothing enqueued"
    );

    let masked = emit(
        &mut conn,
        "ticket.workflow_state_changed",
        serde_json::json!({"id": 1, "assignee_uuid": a, "previous_assignee_uuid": null,
                           "workflow_state_id": 3, "previous_workflow_state_id": 2}),
    );
    assert!(
        enqueued(&mut conn, masked),
        "a combined update is enqueued regardless of its event_type label"
    );

    let comment = emit(
        &mut conn,
        "comment.created",
        serde_json::json!({"id": 5, "ticket_id": 1}),
    );
    assert!(enqueued(&mut conn, comment));
}

#[test]
fn claim_is_exclusive_and_ack_and_fail_do_what_they_say() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool();
    let mut conn = pool.get().expect("conn");

    let sync_id = emit(
        &mut conn,
        "comment.created",
        serde_json::json!({"id": 5, "ticket_id": 1, "content": "hi"}),
    );

    let first = claim_batch(&mut conn).expect("claim");
    let ours = first
        .iter()
        .find(|c| c.sync_id == sync_id)
        .expect("our row is claimable");
    assert_eq!(ours.attempts, 1);
    assert_eq!(ours.event_type.as_deref(), Some("comment.created"));
    assert!(ours.occurred_at.is_some(), "the source row is joined in");

    // A second claim skips it: it is claimed and not yet stale.
    let second = claim_batch(&mut conn).expect("claim again");
    assert!(
        !second.iter().any(|c| c.sync_id == sync_id),
        "a claimed row is not handed out twice"
    );

    // Failing reschedules with backoff and releases the claim.
    fail(&mut conn, sync_id, 1, "notify").expect("fail");
    type Stamp = Option<chrono::DateTime<chrono::Utc>>;
    let (attempts, claimed_at, last_error, dead_at): (i16, Stamp, Option<String>, Stamp) =
        notification_outbox::table
            .filter(notification_outbox::sync_id.eq(sync_id))
            .select((
                notification_outbox::attempts,
                notification_outbox::claimed_at,
                notification_outbox::last_error,
                notification_outbox::dead_at,
            ))
            .first(&mut conn)
            .expect("row");
    assert_eq!(attempts, 1);
    assert!(claimed_at.is_none(), "the claim is released");
    assert_eq!(last_error.as_deref(), Some("notify"));
    assert!(dead_at.is_none());

    // The last failure dead-letters rather than deletes.
    fail(&mut conn, sync_id, MAX_ATTEMPTS, "notify").expect("final fail");
    let dead: Stamp = notification_outbox::table
        .filter(notification_outbox::sync_id.eq(sync_id))
        .select(notification_outbox::dead_at)
        .first(&mut conn)
        .expect("row");
    assert!(dead.is_some(), "kept for inspection");
    assert!(
        !claim_batch(&mut conn)
            .expect("claim")
            .iter()
            .any(|c| c.sync_id == sync_id),
        "a dead row is never claimed"
    );

    ack(&mut conn, &[sync_id]).expect("ack");
    assert!(!enqueued(&mut conn, sync_id), "ack deletes");
}

#[test]
fn a_derived_notification_persists_once_per_event_recipient_and_type() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool();
    let mut conn = pool.get().expect("conn");
    let user = common::insert_user(&mut conn, "outbox-recipient");
    let type_id: i32 = backend::schema::notification_types::table
        .select(backend::schema::notification_types::id)
        .order(backend::schema::notification_types::id.asc())
        .first(&mut conn)
        .expect("a seeded notification type");

    let row = || NewNotification {
        uuid: uuid::Uuid::now_v7(),
        user_uuid: user.uuid,
        notification_type_id: type_id,
        entity_type: "ticket".into(),
        entity_id: 1,
        title: "Assigned".into(),
        body: None,
        metadata: None,
        channels_delivered: serde_json::json!([]),
        interrupts: true,
        source_sync_id: Some(4242),
    };
    let inserted = |conn: &mut DbConnection| {
        diesel::insert_into(notifications::table)
            .values(&row())
            .on_conflict_do_nothing()
            .execute(conn)
            .expect("insert")
    };
    assert_eq!(inserted(&mut conn), 1, "first delivery inserts");
    assert_eq!(inserted(&mut conn), 0, "the retry inserts nothing");

    // A handler-raised notification has no source and is never deduplicated.
    let mut plain = row();
    plain.source_sync_id = None;
    for _ in 0..2 {
        plain.uuid = uuid::Uuid::now_v7();
        assert_eq!(
            diesel::insert_into(notifications::table)
                .values(&plain)
                .on_conflict_do_nothing()
                .execute(&mut conn)
                .expect("insert"),
            1
        );
    }
}
