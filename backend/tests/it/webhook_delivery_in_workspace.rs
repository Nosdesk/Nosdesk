//! The webhook delivery worker records each attempt where the app runs as
//! `nosdesk_app`: it reads and writes the webhook's rows pinned to the
//! webhook's workspace, so row security shows them. Driven through a pool
//! shaped like production's, where a pooled connection starts with no
//! workspace.
//!
//! The task's URL is a loopback address, which the SSRF guard refuses, so the
//! attempt takes the failure path without sending anything: the delivery row
//! records the refusal and schedules a retry, and the webhook's failure count
//! goes up.

use chrono::Utc;
use diesel::prelude::*;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use backend::schema::{webhook_deliveries, webhooks};
use backend::services::webhooks::delivery::{DeliveryTask, WebhookDeliveryWorker};
use backend::services::webhooks::types::WebhookPayload;
use backend::sync::session::run_in_workspace;

use crate::common;

const REF: &str = "test:webhook_delivery_in_workspace";

#[actix_web::test]
async fn a_delivery_attempt_is_recorded_in_the_webhooks_workspace() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let ws = &seeded.b;

    let (tx, rx) = mpsc::channel(1);
    let worker = actix_web::rt::spawn(WebhookDeliveryWorker::new(db.runtime_pool(2), rx).run());
    tx.send(DeliveryTask {
        webhook_id: ws.webhook_id,
        workspace_id: ws.workspace_id,
        webhook_url: "http://127.0.0.1:9/hook".to_string(),
        webhook_secret: "whsec_test".to_string(),
        webhook_headers: None,
        payload: WebhookPayload {
            id: Uuid::now_v7(),
            event_type: common::FIXTURE_WEBHOOK_EVENT.to_string(),
            timestamp: Utc::now(),
            data: json!({ "id": 1 }),
        },
        attempt: 1,
        delivery_id: None,
    })
    .await
    .expect("queue delivery");
    drop(tx);
    worker.await.expect("worker ran to completion");

    let pool = db.pool_with_size(1);
    let (attempt, error, retry_scheduled): (i32, Option<String>, bool) =
        run_in_workspace(&pool, REF, ws.workspace_id, |c| {
            webhook_deliveries::table
                .filter(webhook_deliveries::webhook_id.eq(ws.webhook_id))
                .select((
                    webhook_deliveries::attempt_number,
                    webhook_deliveries::error_message,
                    webhook_deliveries::next_retry_at.is_not_null(),
                ))
                .first(c)
        })
        .expect("the attempt has a delivery row");
    assert_eq!(attempt, 1);
    assert!(
        error.as_deref().is_some_and(|e| e.contains("SSRF guard")),
        "{error:?}"
    );
    assert!(retry_scheduled);

    let failures: i32 = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        webhooks::table
            .find(ws.webhook_id)
            .select(webhooks::failure_count)
            .first(c)
    })
    .expect("reload webhook");
    assert_eq!(failures, 1);
}
