//! Webhook Delivery Worker
//!
//! Handles HTTP delivery of webhook payloads with retry logic.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use tokio::sync::mpsc;

use crate::db::Pool;
use crate::models::{NewWebhookDelivery, WebhookDeliveryUpdate, WebhookUpdate};
use crate::repository::webhooks as webhook_repo;

use super::signature::sign_payload;
use super::types::WebhookPayload;

/// Maximum number of delivery attempts
const MAX_RETRIES: i32 = 5;

/// Initial retry delay in seconds
const INITIAL_RETRY_DELAY_SECS: u64 = 1;

/// Maximum retry delay in seconds (1 hour)
const MAX_RETRY_DELAY_SECS: u64 = 3600;

/// HTTP request timeout in seconds
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Number of consecutive failures before auto-disabling webhook
const AUTO_DISABLE_THRESHOLD: i32 = 10;

/// Delivery task sent to the worker
pub struct DeliveryTask {
    pub webhook_id: i32,
    pub webhook_url: String,
    pub webhook_secret: String,
    pub webhook_headers: Option<serde_json::Value>,
    pub payload: WebhookPayload,
    pub attempt: i32,
}

/// Worker that processes webhook delivery tasks
pub struct WebhookDeliveryWorker {
    pool: Pool,
    receiver: mpsc::Receiver<DeliveryTask>,
    client: Client,
}

impl WebhookDeliveryWorker {
    /// Create a new delivery worker
    pub fn new(pool: Pool, receiver: mpsc::Receiver<DeliveryTask>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .user_agent("Nosdesk-Webhook/1.0")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            pool,
            receiver,
            client,
        }
    }

    /// Run the delivery worker (processes tasks from the channel)
    pub async fn run(mut self) {
        while let Some(task) = self.receiver.recv().await {
            if let Err(e) = self.deliver(task).await {
                tracing::error!(error = %e, "Webhook delivery failed");
            }
        }
        tracing::info!("Webhook delivery worker shutting down");
    }

    /// Deliver a single webhook
    async fn deliver(&self, task: DeliveryTask) -> Result<(), String> {
        let payload_json = serde_json::to_string(&task.payload)
            .map_err(|e| format!("Failed to serialize payload: {e}"))?;

        // Generate signature
        let signature = sign_payload(&payload_json, &task.webhook_secret);

        // Build request
        let mut request = self
            .client
            .post(&task.webhook_url)
            .header("Content-Type", "application/json")
            .header("X-Nosdesk-Signature", &signature)
            .header("X-Nosdesk-Event", &task.payload.event_type)
            .header("X-Nosdesk-Delivery", task.payload.id.to_string());

        // Add custom headers from webhook config
        if let Some(headers) = &task.webhook_headers {
            if let Some(obj) = headers.as_object() {
                for (key, value) in obj {
                    if let Some(v) = value.as_str() {
                        request = request.header(key, v);
                    }
                }
            }
        }

        // Record start time
        let start = std::time::Instant::now();

        // Create delivery record
        let mut conn = self.pool.get().map_err(|e| format!("DB error: {e}"))?;
        let delivery = webhook_repo::create_delivery(
            &mut conn,
            NewWebhookDelivery {
                webhook_id: task.webhook_id,
                event_type: task.payload.event_type.clone(),
                payload: serde_json::to_value(&task.payload).unwrap_or_default(),
                request_headers: Some(serde_json::json!({
                    "X-Nosdesk-Signature": "sha256=***",
                    "X-Nosdesk-Event": &task.payload.event_type,
                    "X-Nosdesk-Delivery": task.payload.id.to_string(),
                })),
                attempt_number: task.attempt,
            },
        )?;

        // Send request
        let result = request.body(payload_json).send().await;
        let duration_ms = start.elapsed().as_millis() as i32;

        match result {
            Ok(response) => {
                let status = response.status().as_u16() as i32;
                let response_body = response.text().await.ok();

                if (200..300).contains(&status) {
                    // Success
                    self.handle_success(&mut conn, &task, delivery.id, status, response_body, duration_ms)?;
                } else {
                    // HTTP error - schedule retry
                    self.handle_failure(
                        &mut conn,
                        &task,
                        delivery.id,
                        status,
                        response_body,
                        duration_ms,
                        None,
                    )?;
                }
            }
            Err(e) => {
                // Network error - schedule retry
                self.handle_failure(
                    &mut conn,
                    &task,
                    delivery.id,
                    0,
                    None,
                    duration_ms,
                    Some(e.to_string()),
                )?;
            }
        }

        Ok(())
    }

    /// Handle successful delivery
    fn handle_success(
        &self,
        conn: &mut crate::db::DbConnection,
        task: &DeliveryTask,
        delivery_id: i32,
        status: i32,
        response_body: Option<String>,
        duration_ms: i32,
    ) -> Result<(), String> {
        // Update delivery record
        webhook_repo::update_delivery(
            conn,
            delivery_id,
            WebhookDeliveryUpdate {
                response_status: Some(status),
                response_body,
                duration_ms: Some(duration_ms),
                delivered_at: Some(Utc::now().naive_utc()),
                next_retry_at: Some(None),
                ..Default::default()
            },
        )?;

        // Reset failure count on success
        webhook_repo::update_webhook(
            conn,
            task.webhook_id,
            WebhookUpdate {
                last_triggered_at: Some(Utc::now().naive_utc()),
                failure_count: Some(0),
                disabled_reason: Some(None),
                ..Default::default()
            },
        )?;

        tracing::debug!(
            webhook_id = task.webhook_id,
            status = status,
            duration_ms = duration_ms,
            "Webhook delivered successfully"
        );

        Ok(())
    }

    /// Handle failed delivery (schedule retry if applicable)
    fn handle_failure(
        &self,
        conn: &mut crate::db::DbConnection,
        task: &DeliveryTask,
        delivery_id: i32,
        status: i32,
        response_body: Option<String>,
        duration_ms: i32,
        error_message: Option<String>,
    ) -> Result<(), String> {
        // Calculate next retry time (exponential backoff with jitter)
        let next_retry = if task.attempt < MAX_RETRIES {
            let base_delay = INITIAL_RETRY_DELAY_SECS * 2u64.pow(task.attempt as u32 - 1);
            let jitter = rand::random::<u64>() % (base_delay / 2 + 1);
            let delay = std::cmp::min(base_delay + jitter, MAX_RETRY_DELAY_SECS);
            Some(Utc::now().naive_utc() + chrono::Duration::seconds(delay as i64))
        } else {
            None
        };

        // Update delivery record
        webhook_repo::update_delivery(
            conn,
            delivery_id,
            WebhookDeliveryUpdate {
                response_status: Some(status),
                response_body,
                duration_ms: Some(duration_ms),
                error_message: error_message.clone(),
                next_retry_at: Some(next_retry),
                ..Default::default()
            },
        )?;

        // Increment failure count
        let webhook = webhook_repo::get_webhook_by_id(conn, task.webhook_id)?;
        let new_failure_count = webhook.failure_count + 1;

        let mut update = WebhookUpdate {
            failure_count: Some(new_failure_count),
            ..Default::default()
        };

        // Auto-disable if too many consecutive failures
        if new_failure_count >= AUTO_DISABLE_THRESHOLD {
            update.enabled = Some(false);
            update.disabled_reason = Some(Some(format!(
                "Auto-disabled after {new_failure_count} consecutive failures"
            )));
            tracing::warn!(
                webhook_id = task.webhook_id,
                failures = new_failure_count,
                "Webhook auto-disabled due to consecutive failures"
            );
        }

        webhook_repo::update_webhook(conn, task.webhook_id, update)?;

        tracing::warn!(
            webhook_id = task.webhook_id,
            attempt = task.attempt,
            status = status,
            error = ?error_message,
            next_retry = ?next_retry,
            "Webhook delivery failed"
        );

        Ok(())
    }
}
