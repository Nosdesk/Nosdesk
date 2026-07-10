//! Webhook outbox drain is at-least-once.
//!
//! The drain used to delete the claimed `webhook_outbox` rows and only queue
//! the deliveries in memory; the durable `webhook_deliveries` row was written
//! later by the delivery worker. A crash between the commit and that write
//! dropped the fan-out. The drain now records a durable delivery row per
//! subscriber in the SAME transaction as the outbox delete, with a
//! `next_retry_at` grace so a lost in-memory send is recovered by the retry
//! worker. This drives the transactional core directly and asserts the durable
//! record exists (retry-eligible) exactly when the outbox row is gone.

#![allow(clippy::expect_used)]

mod common;

use diesel::prelude::*;

use backend::models::{SyncAggregate, SyncOp};
use backend::schema::{webhook_deliveries, webhook_outbox};
use backend::services::webhooks::WebhookService;
use backend::sync::session::with_actor_bypass_context;
use backend::sync::ActorContext;

#[test]
fn outbox_drain_records_durable_delivery_atomically() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool();
    let mut conn = pool.get().expect("conn");

    // A subscriber in the bootstrap workspace (id=1, pinned by the pool
    // customizer) for the ticket.created event.
    let webhook = backend::repository::webhooks::create_webhook(
        &mut conn,
        "durability-probe".into(),
        "https://example.invalid/hook".into(),
        "secret".into(),
        vec!["ticket.created".into()],
        None,
        None,
    )
    .expect("create webhook");

    // Emit a matching sync action. The AFTER INSERT trigger
    // (tr_sync_actions_webhook_outbox) enqueues its webhook_outbox row.
    let sync_id = backend::sync::emit::record(
        &mut conn,
        backend::sync::emit::SyncEmit {
            aggregate: SyncAggregate::Ticket,
            aggregate_id: "1".into(),
            op: SyncOp::Insert,
            event_type: "ticket.created",
            data: serde_json::json!({ "id": 1 }),
            groups: vec!["workspace".into()],
            causation_id: None,
        },
    )
    .expect("emit sync action");

    // Precondition: outbox row present, no delivery recorded yet.
    let outbox_before: i64 = webhook_outbox::table
        .filter(webhook_outbox::sync_id.eq(sync_id))
        .count()
        .get_result(&mut conn)
        .expect("count outbox before");
    assert_eq!(
        outbox_before, 1,
        "the emit should have enqueued the outbox row"
    );
    let deliveries_before: i64 = webhook_deliveries::table
        .filter(webhook_deliveries::webhook_id.eq(webhook.id))
        .count()
        .get_result(&mut conn)
        .expect("count deliveries before");
    assert_eq!(deliveries_before, 0);

    // Drain the batch through the real transactional core (bypass context, as
    // background_run supplies in production). No delivery worker runs, so this
    // isolates the durability boundary from the in-memory send.
    let (tasks, _more) = with_actor_bypass_context(
        &mut conn,
        &ActorContext::system("test:webhook_outbox_drain"),
        WebhookService::drain_batch_txn,
    )
    .expect("drain batch");

    // One task, carrying the id of a row that was persisted inside the drain
    // transaction (not left for the worker to insert).
    assert_eq!(tasks.len(), 1, "one subscriber => one delivery task");
    assert!(
        tasks[0].delivery_id.is_some(),
        "the delivery row is created in-transaction, so the task carries its id"
    );

    // Postcondition: the outbox row is gone AND a durable, retry-eligible
    // delivery row exists. If the process died right now, the retry worker
    // would still deliver it (delivered_at NULL, next_retry_at set).
    let outbox_after: i64 = webhook_outbox::table
        .filter(webhook_outbox::sync_id.eq(sync_id))
        .count()
        .get_result(&mut conn)
        .expect("count outbox after");
    assert_eq!(outbox_after, 0, "the claimed outbox row is deleted");

    let recorded: Vec<(Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>)> =
        webhook_deliveries::table
            .filter(webhook_deliveries::webhook_id.eq(webhook.id))
            .select((
                webhook_deliveries::delivered_at,
                webhook_deliveries::next_retry_at,
            ))
            .load(&mut conn)
            .expect("load deliveries after");
    assert_eq!(recorded.len(), 1, "the fan-out is durably recorded");
    assert!(
        recorded[0].0.is_none(),
        "not yet delivered (the worker never ran)"
    );
    assert!(
        recorded[0].1.is_some(),
        "next_retry_at is set so the retry worker recovers a lost in-memory send"
    );
}
