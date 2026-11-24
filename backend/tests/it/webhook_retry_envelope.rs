//! A retried webhook delivery sends the envelope its first attempt sent,
//! unchanged: the same id, event, timestamp and data, so the body and its
//! signature match byte for byte and a consumer can drop the duplicate.
//!
//! The first attempt goes to a loopback address, which the SSRF guard
//! refuses, so it fails without sending anything and schedules a retry.

use chrono::Utc;
use diesel::prelude::*;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use backend::models::{SyncAggregate, SyncOp};
use backend::repository::webhooks as webhook_repo;
use backend::schema::webhook_deliveries;
use backend::services::webhooks::delivery::{request_body, WebhookDeliveryWorker};
use backend::services::webhooks::signature::sign_payload;
use backend::services::webhooks::WebhookService;
use backend::sync::emit::{self, SyncEmit};
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};
use backend::sync::ActorContext;

use crate::common;

const REF: &str = "test:webhook_retry_envelope";
const SECRET: &str = "whsec_retry";

#[actix_web::test]
async fn a_retry_sends_the_first_attempts_envelope_unchanged() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;

    let webhook = run_in_workspace(&pool, REF, ws, |c| {
        webhook_repo::create_webhook(
            c,
            "retry-probe".into(),
            "http://127.0.0.1:9/hook".into(),
            SECRET.into(),
            vec!["ticket.created".into()],
            None,
            None,
        )
    })
    .expect("create webhook");
    let data = json!({
        "id": 41,
        "title": "Printer jams on tray 2",
        "tags": ["hardware", { "z": 1.5, "a": null }],
    });
    run_in_workspace(&pool, REF, ws, |c| {
        emit::record(
            c,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: "41".into(),
                op: SyncOp::Insert,
                event_type: "ticket.created",
                data: data.clone(),
                groups: vec!["workspace".into()],
                causation_id: None,
            },
        )
    })
    .expect("emit a ticket event");

    // The outbox drain makes the first attempt's task and its delivery row.
    let (tasks, _) = with_actor_bypass_context(
        &mut pool.get().expect("conn"),
        &ActorContext::system("test:webhook_retry_envelope_drain"),
        WebhookService::drain_batch_txn,
    )
    .expect("drain the outbox");
    let first = tasks
        .into_iter()
        .find(|t| t.webhook_id == webhook.id)
        .expect("a task for the webhook");
    let delivery_id = first.delivery_id.expect("the drain records the delivery");
    let first_body = request_body(&first.payload).expect("first body");
    let first_id = first.payload.id;
    let first_timestamp = first.payload.timestamp;

    // The first attempt fails and schedules a retry.
    let (tx, rx) = mpsc::channel(1);
    let worker = actix_web::rt::spawn(WebhookDeliveryWorker::new(db.runtime_pool(2), rx).run());
    tx.send(first).await.expect("queue the first attempt");
    drop(tx);
    worker.await.expect("worker ran to completion");

    // Make the retry due now.
    run_in_workspace(&pool, REF, ws, |c| {
        diesel::update(webhook_deliveries::table.find(delivery_id))
            .set(
                webhook_deliveries::next_retry_at
                    .eq(Some(Utc::now().naive_utc() - chrono::Duration::minutes(1))),
            )
            .execute(c)
    })
    .expect("make the retry due");

    let retries = with_actor_bypass_context(
        &mut pool.get().expect("conn"),
        &ActorContext::system("test:webhook_retry_envelope_retry"),
        WebhookService::retry_tasks,
    )
    .expect("read due retries");
    let retry = retries
        .into_iter()
        .find(|t| t.delivery_id == Some(delivery_id))
        .expect("the failed delivery is due a retry");

    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.payload.id, first_id, "the retry keeps the event's id");
    assert_eq!(retry.payload.event_type, "ticket.created");
    assert_eq!(retry.payload.timestamp, first_timestamp);
    let retry_body = request_body(&retry.payload).expect("retry body");
    assert_eq!(retry_body, first_body, "the retry sends the same bytes");
    assert_eq!(
        sign_payload(&retry_body, &retry.webhook_secret),
        sign_payload(&first_body, SECRET),
        "and the same signature"
    );

    // The data is the event's, not the first envelope wrapped again.
    let sent: Value = serde_json::from_str(&retry_body).expect("a JSON body");
    assert_eq!(sent["id"], json!(first_id.to_string()));
    assert_eq!(sent["data"], data);
}
