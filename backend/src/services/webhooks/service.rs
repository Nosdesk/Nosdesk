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

        Self { delivery_tx }
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

    /// Claim a batch of outbox rows, record a durable delivery row per
    /// subscriber, delete the claimed rows, then queue the deliveries for
    /// immediate send. Returns whether the batch was full (more remain).
    ///
    /// Durability: the `webhook_deliveries` rows are written in the SAME
    /// transaction as the outbox delete (see [`Self::drain_batch_txn`]), so the
    /// fan-out is at-least-once. If the process dies after the transaction
    /// commits but before the queued in-memory delivery runs, the retry worker
    /// recovers the pending rows; the durable record, not the queue, is the
    /// source of truth. The enqueue happens after the transaction so the row
    /// locks release promptly.
    async fn dispatch_outbox_batch(
        pool: &Pool,
        delivery_tx: &mpsc::Sender<DeliveryTask>,
    ) -> Result<bool, String> {
        let (tasks, more): (Vec<DeliveryTask>, bool) = crate::sync::session::background_run(
            pool,
            "background:webhook_outbox_dispatch",
            Self::drain_batch_txn,
        )
        .map_err(|e| format!("DB error: {e}"))?;

        for task in tasks {
            if let Err(e) = delivery_tx.send(task).await {
                tracing::error!(error = %e, "Failed to queue webhook delivery from outbox");
            }
        }

        Ok(more)
    }

    /// Transactional core of the outbox drain, split out so it can be driven
    /// directly in tests. Assumes it runs inside a bypass context
    /// ([`crate::sync::session::with_actor_bypass_context`] supplies the outer
    /// transaction and the `nosdesk_admin` role), which `background_run` above
    /// provides.
    ///
    /// Claims the oldest outbox rows, fans each out to its subscribers, records
    /// a durable `webhook_deliveries` row per subscriber, then deletes the
    /// claimed outbox rows — atomically. The delivery rows carry a short
    /// `next_retry_at` grace so a row whose in-memory send never runs (a crash)
    /// is picked up by the retry worker instead of being lost.
    pub fn drain_batch_txn(
        conn: &mut crate::db::DbConnection,
    ) -> diesel::result::QueryResult<(Vec<DeliveryTask>, bool)> {
        use crate::schema::{sync_actions, webhook_outbox};
        use diesel::prelude::*;

        const OUTBOX_BATCH: i64 = 500;
        // Grace before a recorded-but-unsent row becomes retry-eligible.
        // Comfortably longer than a healthy instance's queue latency, so the
        // retry worker only ever touches rows the in-memory send genuinely
        // missed (a crash), not ones merely in flight.
        const RECOVERY_GRACE_SECS: i64 = 300;

        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            // Claim the oldest outbox rows, skipping any another instance is
            // already processing.
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

            // Resolve the source rows for workspace_id + event_type + data.
            // workspace_id is load-bearing: this drain runs BYPASSRLS, so
            // subscribers must be scoped to the event's own workspace by hand
            // (RLS gives no cover here).
            let rows: Vec<(i64, i32, String, serde_json::Value)> = sync_actions::table
                .filter(sync_actions::sync_id.eq_any(&claimed))
                .select((
                    sync_actions::sync_id,
                    sync_actions::workspace_id,
                    sync_actions::event_type,
                    sync_actions::data,
                ))
                .load(conn)?;

            // Cache the subscriber lookup per (workspace, event type) so a batch
            // of N same-type rows in a workspace is one query, not N. Build
            // (workspace_id, task) pairs; the workspace pins each durable insert.
            let mut subscriber_cache: std::collections::HashMap<
                (i32, &'static str),
                Vec<crate::models::Webhook>,
            > = std::collections::HashMap::new();
            let mut pending: Vec<(i32, DeliveryTask)> = Vec::new();

            for (_sync_id, workspace_id, event_type, data) in &rows {
                let Some(webhook_type) = WebhookEventType::from_sync_action(event_type) else {
                    continue;
                };
                let event_type_str = webhook_type.as_str();
                let cache_key = (*workspace_id, event_type_str);
                let subscribers = match subscriber_cache.entry(cache_key) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        let subs = webhook_repo::get_webhooks_for_event(
                            conn,
                            *workspace_id,
                            event_type_str,
                        )?;
                        e.insert(subs)
                    }
                };
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
                    pending.push((
                        *workspace_id,
                        DeliveryTask {
                            webhook_id: webhook.id,
                            webhook_url: webhook.url.clone(),
                            webhook_secret: webhook.secret.clone(),
                            webhook_headers: webhook.headers.clone(),
                            payload: payload.clone(),
                            attempt: 1,
                            delivery_id: None,
                        },
                    ));
                }
            }

            // Record a durable delivery row per task, grouped by workspace so
            // the audit trigger + the workspace_id column default resolve to
            // each row's own workspace. Sorted so we re-pin once per workspace,
            // not once per row. Pinning sets only app.workspace_id and leaves
            // the bypass role intact (unlike a full actor-context switch).
            pending.sort_by_key(|(ws, _)| *ws);
            let recover_at =
                Utc::now().naive_utc() + chrono::Duration::seconds(RECOVERY_GRACE_SECS);
            let mut tasks: Vec<DeliveryTask> = Vec::with_capacity(pending.len());
            let mut pinned: Option<i32> = None;
            for (workspace_id, mut task) in pending {
                if pinned != Some(workspace_id) {
                    crate::sync::session::pin_workspace(conn, workspace_id)?;
                    pinned = Some(workspace_id);
                }
                let delivery = webhook_repo::create_delivery(
                    conn,
                    crate::models::NewWebhookDelivery {
                        webhook_id: task.webhook_id,
                        event_type: task.payload.event_type.clone(),
                        payload: serde_json::to_value(&task.payload).unwrap_or_default(),
                        request_headers: Some(serde_json::json!({
                            "X-Nosdesk-Signature": "sha256=***",
                            "X-Nosdesk-Event": &task.payload.event_type,
                            "X-Nosdesk-Delivery": task.payload.id.to_string(),
                        })),
                        attempt_number: task.attempt,
                        next_retry_at: Some(recover_at),
                    },
                )?;
                task.delivery_id = Some(delivery.id);
                tasks.push(task);
            }

            // Delete every claimed row (webhook or not) so the outbox drains.
            // They remain locked by this transaction, so no other instance can
            // re-claim them.
            diesel::delete(webhook_outbox::table.filter(webhook_outbox::sync_id.eq_any(&claimed)))
                .execute(conn)?;

            Ok((tasks, full))
        })
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

    /// Send a test event to a webhook. Takes the caller's already-resolved
    /// `Webhook` (looked up on the request's RLS-scoped connection) rather than
    /// re-reading it by id on a BYPASSRLS connection — `get_webhook_by_id` has
    /// no workspace filter, so a bypass re-read could return another tenant's
    /// webhook. Passing the validated row keeps the whole path tenant-safe.
    pub async fn send_test_event(&self, webhook: &crate::models::Webhook) -> Result<(), String> {
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
            webhook_url: webhook.url.clone(),
            webhook_secret: webhook.secret.clone(),
            webhook_headers: webhook.headers.clone(),
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
