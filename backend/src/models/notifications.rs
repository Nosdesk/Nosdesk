use super::users::User;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Notification Models
// ============================================================================

/// Notification type definition
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::notification_types)]
pub struct NotificationType {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub default_channels: serde_json::Value,
    pub created_at: NaiveDateTime,
    /// Whether this kind interrupts by default: drives the default in-app
    /// frequency (`true` → `instant`/toast, `false` → `quiet`/bell-only). A user
    /// can still override per channel. Must stay LAST to match `schema.rs`.
    pub interrupts: bool,
}

/// Persistent notification record
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notifications)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct Notification {
    pub id: i32,
    pub uuid: Uuid,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub channels_delivered: serde_json::Value,
    pub is_read: bool,
    pub read_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
    /// Engagement-state axes (see the notification_engagement_state
    /// migration): unseen vs unread, reversible archive, snooze.
    pub seen_at: Option<NaiveDateTime>,
    pub archived_at: Option<NaiveDateTime>,
    pub snoozed_until: Option<NaiveDateTime>,
    /// Whether this notification interrupted (toast / desktop) vs landed
    /// quietly in the bell. Reflects the recipient's resolved delivery at
    /// send time; the send path counts recent `interrupts = true` rows to
    /// cap interrupt bursts.
    pub interrupts: bool,
    /// The `sync_actions` row this was derived from, or null for a
    /// notification a handler raised directly. With `(user_uuid,
    /// notification_type_id)` it is unique, which is what makes a
    /// redelivery of the same event insert nothing.
    pub source_sync_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notifications)]
pub struct NewNotification {
    pub uuid: Uuid,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub channels_delivered: serde_json::Value,
    pub interrupts: bool,
    pub source_sync_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notification_rate_limits)]
pub struct NewNotificationRateLimit {
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
}

/// API response for notification preferences (grouped by type).
///
/// `channels` (channel → enabled bool) is kept for backward compatibility with
/// the current toggle UI (`enabled = frequency != off`). `frequencies` (channel
/// → `instant` | `digest` | `off`) is the new per-cell frequency the upgraded
/// select UI reads. Additive so backend + frontend can deploy independently.
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationPreferenceResponse {
    pub notification_type: String,
    pub notification_name: String,
    pub description: Option<String>,
    pub category: String,
    pub channels: std::collections::HashMap<String, bool>,
    pub frequencies: std::collections::HashMap<String, String>,
    /// Channels the workspace admin has `locked` for this type — the user
    /// cannot override these (the UI disables the cell). Additive.
    pub locked: std::collections::HashMap<String, bool>,
}

/// API response for a workspace admin's notification DEFAULTS (grouped by type).
/// `frequencies` is the workspace default per channel (falling back to the
/// system default); `locked` marks cells users cannot override.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceNotificationDefaultResponse {
    pub notification_type: String,
    pub notification_name: String,
    pub description: Option<String>,
    pub category: String,
    pub frequencies: std::collections::HashMap<String, String>,
    pub locked: std::collections::HashMap<String, bool>,
}

/// API response for a notification
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationResponse {
    pub id: i32,
    pub uuid: Uuid,
    pub notification_type: String,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub is_read: bool,
    /// Engagement state surfaced to the client: `seen_at` drives the
    /// badge (unseen count), `archived_at` hides from the active inbox
    /// without deleting, `snoozed_until` defers re-surfacing.
    pub seen_at: Option<NaiveDateTime>,
    pub archived_at: Option<NaiveDateTime>,
    pub snoozed_until: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}
