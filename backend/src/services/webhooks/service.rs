//! Webhook Service
//!
//! Central service that listens to SSE events and delivers them to registered webhooks.

use std::sync::Arc;

use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::sse::{SseState, TicketEvent};
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
    pub fn new(pool: Pool, sse_state: Arc<SseState>) -> Self {
        // Create bounded channel for delivery tasks
        let (delivery_tx, delivery_rx) = mpsc::channel::<DeliveryTask>(DELIVERY_QUEUE_SIZE);

        // Start delivery worker
        let worker = WebhookDeliveryWorker::new(pool.clone(), delivery_rx);
        tokio::spawn(async move {
            worker.run().await;
        });

        // Start event listener
        let listener_pool = pool.clone();
        let listener_tx = delivery_tx.clone();
        let receiver = sse_state.sender.subscribe();

        tokio::spawn(async move {
            Self::event_listener(listener_pool, receiver, listener_tx).await;
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

    /// Background task that listens to SSE events
    async fn event_listener(
        pool: Pool,
        mut receiver: tokio::sync::broadcast::Receiver<TicketEvent>,
        delivery_tx: mpsc::Sender<DeliveryTask>,
    ) {
        tracing::info!("Webhook event listener started");

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    // Map SSE event to webhook event type
                    if let Some(event_type) = WebhookEventType::from_sse_event(&event) {
                        if let Err(e) =
                            Self::process_event(&pool, &delivery_tx, event_type, &event).await
                        {
                            tracing::error!(error = %e, "Failed to process webhook event");
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(count, "Webhook listener lagged behind SSE events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("SSE channel closed, webhook listener stopping");
                    break;
                }
            }
        }
    }

    /// Process a single event and queue deliveries
    async fn process_event(
        pool: &Pool,
        delivery_tx: &mpsc::Sender<DeliveryTask>,
        event_type: WebhookEventType,
        event: &TicketEvent,
    ) -> Result<(), String> {
        let event_type_str = event_type.as_str();

        // Get enabled webhooks subscribed to this event
        let mut conn = pool.get().map_err(|e| format!("DB error: {e}"))?;
        let webhooks = webhook_repo::get_webhooks_for_event(&mut conn, event_type_str)?;

        if webhooks.is_empty() {
            return Ok(());
        }

        // Create payload envelope
        let payload = WebhookPayload {
            id: Uuid::now_v7(),
            event_type: event_type_str.to_string(),
            timestamp: Utc::now(),
            data: serde_json::to_value(event).unwrap_or_default(),
        };

        tracing::debug!(
            event_type = event_type_str,
            webhook_count = webhooks.len(),
            "Processing event for webhooks"
        );

        // Queue delivery for each webhook
        for webhook in webhooks {
            let task = DeliveryTask {
                webhook_id: webhook.id,
                webhook_url: webhook.url,
                webhook_secret: webhook.secret,
                webhook_headers: webhook.headers,
                payload: payload.clone(),
                attempt: 1,
            };

            if let Err(e) = delivery_tx.send(task).await {
                tracing::error!(
                    webhook_id = webhook.id,
                    error = %e,
                    "Failed to queue webhook delivery"
                );
            }
        }

        Ok(())
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
        let mut conn = pool.get().map_err(|e| format!("DB error: {e}"))?;

        // Get deliveries ready for retry
        let pending = webhook_repo::get_pending_retries(&mut conn)?;

        if pending.is_empty() {
            return Ok(());
        }

        tracing::debug!(count = pending.len(), "Processing pending webhook retries");

        for delivery in pending {
            // Get the webhook
            match webhook_repo::get_webhook_by_id(&mut conn, delivery.webhook_id) {
                Ok(webhook) => {
                    // Skip if webhook is now disabled
                    if !webhook.enabled {
                        continue;
                    }

                    let task = DeliveryTask {
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
                    };

                    if let Err(e) = delivery_tx.send(task).await {
                        tracing::error!(
                            delivery_id = delivery.id,
                            error = %e,
                            "Failed to queue retry"
                        );
                    }
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

        Ok(())
    }

    /// Send a test event to a webhook
    pub async fn send_test_event(&self, webhook_id: i32) -> Result<(), String> {
        let mut conn = self.pool.get().map_err(|e| format!("DB error: {e}"))?;
        let webhook = webhook_repo::get_webhook_by_id(&mut conn, webhook_id)?;

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
        };

        self.delivery_tx
            .send(task)
            .await
            .map_err(|e| format!("Failed to queue test delivery: {e}"))
    }
}
