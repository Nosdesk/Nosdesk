use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== WEBHOOK MODELS =====

/// Webhook configuration (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct Webhook {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<Option<String>>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: i32,
    pub disabled_reason: Option<String>,
    pub workspace_id: i32,
}

/// New webhook for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct NewWebhook {
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<Option<String>>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_by: Option<Uuid>,
}

/// Webhook update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct WebhookUpdate {
    pub name: Option<String>,
    pub url: Option<String>,
    pub secret: Option<String>,
    pub events: Option<Vec<Option<String>>>,
    pub enabled: Option<bool>,
    pub headers: Option<serde_json::Value>,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: Option<i32>,
    pub disabled_reason: Option<Option<String>>,
}

/// Webhook delivery record
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct WebhookDelivery {
    pub id: i32,
    pub uuid: Uuid,
    pub webhook_id: i32,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub request_headers: Option<serde_json::Value>,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub attempt_number: i32,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub next_retry_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
}

/// New webhook delivery for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct NewWebhookDelivery {
    pub webhook_id: i32,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub request_headers: Option<serde_json::Value>,
    pub attempt_number: i32,
    /// When the retry worker may (re)deliver this row. The immediate-send path
    /// (worker first attempt) leaves this `None` and delivers off the in-memory
    /// queue. The transactional-outbox drain sets a short grace window so that
    /// if the process dies before the queued delivery runs, the retry worker
    /// still picks the row up: the durable record, not the queue, is the source
    /// of truth. See `services::webhooks::service::dispatch_outbox_batch`.
    pub next_retry_at: Option<NaiveDateTime>,
}

/// Webhook delivery update
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct WebhookDeliveryUpdate {
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub next_retry_at: Option<Option<NaiveDateTime>>,
    pub attempt_number: Option<i32>,
}

// ===== WEBHOOK API TYPES =====

/// Request to create a webhook
#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
}

/// Request to update a webhook
#[derive(Debug, Deserialize)]
pub struct UpdateWebhookRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
    pub regenerate_secret: Option<bool>,
}

/// Webhook response (hides full secret)
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret_preview: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: i32,
    pub disabled_reason: Option<String>,
}

impl Webhook {
    /// Returns a preview of the secret (first 12 chars + "...")
    pub fn secret_preview(&self) -> String {
        format!("{}...", self.secret.chars().take(12).collect::<String>())
    }
}

impl From<Webhook> for WebhookResponse {
    fn from(w: Webhook) -> Self {
        // Compute secret_preview before any moves
        let secret_preview = w.secret_preview();
        WebhookResponse {
            uuid: w.uuid,
            name: w.name,
            url: w.url,
            secret_preview,
            events: w.events.into_iter().flatten().collect(),
            enabled: w.enabled,
            headers: w.headers,
            created_at: w.created_at,
            updated_at: w.updated_at,
            last_triggered_at: w.last_triggered_at,
            failure_count: w.failure_count,
            disabled_reason: w.disabled_reason,
        }
    }
}

/// Webhook created response (shows full secret once)
#[derive(Debug, Serialize)]
pub struct WebhookCreatedResponse {
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<String>,
}

/// Delivery history entry
#[derive(Debug, Serialize)]
pub struct WebhookDeliveryResponse {
    pub uuid: Uuid,
    pub event_type: String,
    pub response_status: Option<i32>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub attempt_number: i32,
}
