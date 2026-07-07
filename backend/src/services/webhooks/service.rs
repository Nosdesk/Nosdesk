//! Webhook Service
//!
//! Central service that listens to SSE events and delivers them to registered webhooks.

use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::db::Pool;
use crate::repository::webhooks as webhook_repo;

use super::delivery::{DeliveryTask, WebhookDeliveryWorker};
use super::types::{WebhookEventType, WebhookPayload};

/// Delivery queue capacity
const DELIVERY_QUEUE_SIZE: usize = 1000;

/// Webhook service that orchestrates event listening and delivery
pub struct WebhookService {
    pool: Pool,
    delivery_tx: mpsc::Sender<DeliveryTask>,
}

impl WebhookService {
    /// Create a new WebhookService and start background workers
    pub fn new(pool: Pool) -> Self {
        // Create bounded channel for delivery tasks
        let (delivery_tx, delivery_rx) = mpsc::channel::<DeliveryTask>(DELIVERY_QUEUE_SIZE);

        // Start delivery worker
        let worker = WebhookDeliveryWorker::new(pool.clone(), delivery_rx);
        tokio::spawn(async move {
            worker.run().await;
        });

        // Start the webhook outbox dispatcher — the sole event source.
        // It drains the transactional `webhook_outbox` (populated by a
        // trigger atomically with each sync_action) with a FOR UPDATE
        // SKIP LOCKED claim, so each event delivers exactly once across
        // instances with no skipped events. Every webhook event has a
        // sync_actions source now, so there is no SSE listener.
        let outbox_pool = pool.clone();
        let outbox_tx = delivery_tx.clone();
        tokio::spawn(async move {
            Self::outbox_dispatcher(outbox_pool, outbox_tx).await;
        });

        // Start retry worker (checks for failed deliveries needing retry)
        let retry_pool = pool.clone();
        let retry_tx = delivery_tx.clone();
        tokio::spawn(async move {
            Self::retry_worker(retry_pool, retry_tx).await;
        });

        tracing::info!("Webhook service started");

        Self { pool, delivery_tx }
    }

    /// Single-consumer dispatcher that drains the `webhook_outbox` and
    /// turns each enqueued sync_action into webhook deliveries exactly
    /// once across instances. Polls on a short interval; the FOR UPDATE
    /// SKIP LOCKED claim means concurrent instances never process the
    /// same rows.
    async fn outbox_dispatcher(pool: Pool, delivery_tx: mpsc::Sender<DeliveryTask>) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        tracing::info!("Webhook outbox dispatcher started");

        loop {
            interval.tick().await;
            // Drain while batches keep coming full so a backlog catches
            // up without waiting a tick per batch.
            loop {
                match Self::dispatch_outbox_batch(&pool, &delivery_tx).await {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(e) => {
                        tracing::error!(error = %e, "Webhook outbox dispatch failed");
                        break;
                    }
                }
            }
        }
    }

    /// Claim a batch of outbox rows, queue the matching webhook
    /// deliveries, and delete the claimed rows — all in one transaction
    /// so a row is removed only once its deliveries are committed-queued.
    /// Non-webhook events are deleted too (a no-op delivery-wise) so the
    /// outbox drains. Returns whether the batch was full (more remain).
    async fn dispatch_outbox_batch(
        pool: &Pool,
        delivery_tx: &mpsc::Sender<DeliveryTask>,
    ) -> Result<bool, String> {
        use crate::schema::{sync_actions, webhook_outbox};
        use diesel::prelude::*;

        const OUTBOX_BATCH: i64 = 500;

        let (tasks, more): (Vec<DeliveryTask>, bool) = crate::sync::session::background_run(
            pool,
            "background:webhook_outbox_dispatch",
            |conn| {
                conn.transaction::<_, diesel::result::Error, _>(|conn| {
                    // Claim the oldest outbox rows, skipping any another
                    // instance is already processing.
                    let claimed: Vec<i64> = webhook_outbox::table
                        .select(webhook_outbox::sync_id)
                        .order(webhook_outbox::sync_id.asc())
                        .limit(OUTBOX_BATCH)
                        .for_update()
                        .skip_locked()
                        .load(conn)?;
                    if claimed.is_empty() {
                        return Ok((Vec::new(), false));
                    }
                    let full = claimed.len() as i64 == OUTBOX_BATCH;

                    // Resolve the source rows for event_type + data.
                    let rows: Vec<(i64, String, serde_json::Value)> = sync_actions::table
                        .filter(sync_actions::sync_id.eq_any(&claimed))
                        .select((
                            sync_actions::sync_id,
                            sync_actions::event_type,
                            sync_actions::data,
                        ))
                        .load(conn)?;

                    // Cache the subscriber lookup per event type so a
                    // batch of N same-type rows is one query, not N.
                    let mut subscriber_cache: std::collections::HashMap<
                        &'static str,
                        Vec<crate::models::Webhook>,
                    > = std::collections::HashMap::new();
                    let mut tasks: Vec<DeliveryTask> = Vec::new();

                    for (_sync_id, event_type, data) in &rows {
                        let Some(webhook_type) = WebhookEventType::from_sync_action(event_type)
                        else {
                            continue;
                        };
                        let event_type_str = webhook_type.as_str();
                        if !subscriber_cache.contains_key(event_type_str) {
                            let subs = webhook_repo::get_webhooks_for_event(conn, event_type_str)?;
                            subscriber_cache.insert(event_type_str, subs);
                        }
                        let subscribers = &subscriber_cache[event_type_str];
                        if subscribers.is_empty() {
                            continue;
                        }
                        let payload = WebhookPayload {
                            id: Uuid::now_v7(),
                            event_type: event_type_str.to_string(),
                            timestamp: Utc::now(),
                            data: data.clone(),
                        };
                        for webhook in subscribers {
                            tasks.push(DeliveryTask {
                                webhook_id: webhook.id,
                                webhook_url: webhook.url.clone(),
                                webhook_secret: webhook.secret.clone(),
                                webhook_headers: webhook.headers.clone(),
                                payload: payload.clone(),
                                attempt: 1,
                                delivery_id: None,
                            });
                        }
                    }

                    // Delete every claimed row (webhook or not) so the
                    // outbox drains. They're locked by this transaction,
                    // so no other instance can re-claim them.
                    diesel::delete(
                        webhook_outbox::table.filter(webhook_outbox::sync_id.eq_any(&claimed)),
                    )
                    .execute(conn)?;

                    Ok((tasks, full))
                })
            },
        )
        .map_err(|e| format!("DB error: {e}"))?;

        // Enqueue outside the claim transaction so the row locks release
        // promptly; HTTP delivery + retry durability is handled by the
        // delivery worker exactly as for the SSE gap path.
        for task in tasks {
            if let Err(e) = delivery_tx.send(task).await {
                tracing::error!(error = %e, "Failed to queue webhook delivery from outbox");
            }
        }

        Ok(more)
    }

    /// Background worker that retries failed deliveries
    async fn retry_worker(pool: Pool, delivery_tx: mpsc::Sender<DeliveryTask>) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

        tracing::info!("Webhook retry worker started");

        loop {
            interval.tick().await;

            if let Err(e) = Self::process_retries(&pool, &delivery_tx).await {
                tracing::error!(error = %e, "Failed to process webhook retries");
            }
        }
    }

    /// Process pending retries
    async fn process_retries(
        pool: &Pool,
        delivery_tx: &mpsc::Sender<DeliveryTask>,
    ) -> Result<(), String> {
        // webhook_deliveries + webhooks are RLS-enabled; the retry
        // worker runs cross-workspace. Fetch pending + their
        // webhook rows in one bypass txn.
        let tasks: Vec<DeliveryTask> = crate::sync::session::background_run(
            pool,
            "background:webhook_process_retries",
            |conn| {
                let pending = webhook_repo::get_pending_retries(conn)?;
                let mut out = Vec::with_capacity(pending.len());
                for delivery in pending {
                    match webhook_repo::get_webhook_by_id(conn, delivery.webhook_id) {
                        Ok(webhook) if webhook.enabled => {
                            out.push(DeliveryTask {
                                webhook_id: webhook.id,
                                webhook_url: webhook.url,
                                webhook_secret: webhook.secret,
                                webhook_headers: webhook.headers,
                                payload: WebhookPayload {
                                    id: delivery.uuid,
                                    event_type: delivery.event_type,
                                    timestamp: Utc::now(),
                                    data: delivery.payload,
                                },
                                attempt: delivery.attempt_number + 1,
                                // Reuse this delivery row rather than inserting a
                                // new one per retry (retry-storm fix).
                                delivery_id: Some(delivery.id),
                            });
                        }
                        Ok(_) => {
                            // Webhook is now disabled; skip.
                        }
                        Err(e) => {
                            tracing::warn!(
                                delivery_id = delivery.id,
                                webhook_id = delivery.webhook_id,
                                error = %e,
                                "Webhook not found for retry"
                            );
                        }
                    }
                }
                Ok::<_, diesel::result::Error>(out)
            },
        )
        .map_err(|e| format!("DB error: {e}"))?;

        if tasks.is_empty() {
            return Ok(());
        }

        tracing::debug!(count = tasks.len(), "Processing pending webhook retries");

        for task in tasks {
            let delivery_id_for_log = task.webhook_id;
            if let Err(e) = delivery_tx.send(task).await {
                tracing::error!(
                    webhook_id = delivery_id_for_log,
                    error = %e,
                    "Failed to queue retry"
                );
            }
        }

        Ok(())
    }

    /// Send a test event to a webhook
    pub async fn send_test_event(&self, webhook_id: i32) -> Result<(), String> {
        let webhook = crate::sync::session::background_run(
            &self.pool,
            "background:webhook_test_event_lookup",
            |conn| webhook_repo::get_webhook_by_id(conn, webhook_id),
        )
        .map_err(|e| format!("DB error: {e}"))?;

        let payload = WebhookPayload {
            id: Uuid::now_v7(),
            event_type: "webhook.test".to_string(),
            timestamp: Utc::now(),
            data: serde_json::json!({
                "message": "This is a test webhook delivery",
                "webhook_id": webhook.uuid,
                "webhook_name": webhook.name,
            }),
        };

        let task = DeliveryTask {
            webhook_id: webhook.id,
            webhook_url: webhook.url,
            webhook_secret: webhook.secret,
            webhook_headers: webhook.headers,
            payload,
            attempt: 1,
            delivery_id: None,
        };

        self.delivery_tx
            .send(task)
            .await
            .map_err(|e| format!("Failed to queue test delivery: {e}"))
    }
}
