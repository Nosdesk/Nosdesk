//! Notification type definitions
//!
//! Core types for the notification system including notification types,
//! delivery channels, and payload structures.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Notification type codes - matches database notification_types.code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationTypeCode {
    TicketAssigned,
    TicketStatusChanged,
    CommentAdded,
    Mentioned,
    TicketCreatedRequester,
}

impl NotificationTypeCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TicketAssigned => "ticket_assigned",
            Self::TicketStatusChanged => "ticket_status_changed",
            Self::CommentAdded => "comment_added",
            Self::Mentioned => "mentioned",
            Self::TicketCreatedRequester => "ticket_created_requester",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ticket_assigned" => Some(Self::TicketAssigned),
            "ticket_status_changed" => Some(Self::TicketStatusChanged),
            "comment_added" => Some(Self::CommentAdded),
            "mentioned" => Some(Self::Mentioned),
            "ticket_created_requester" => Some(Self::TicketCreatedRequester),
            _ => None,
        }
    }

    /// Get human-readable title for this notification type
    pub fn title(&self) -> &'static str {
        match self {
            Self::TicketAssigned => "Assigned to Ticket",
            Self::TicketStatusChanged => "Ticket Status Changed",
            Self::CommentAdded => "New Comment",
            Self::Mentioned => "Mentioned",
            Self::TicketCreatedRequester => "Ticket Created",
        }
    }
}

/// Delivery channel types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    InApp,
    Email,
    Push,
}

impl NotificationChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InApp => "in_app",
            Self::Email => "email",
            Self::Push => "push",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "in_app" => Some(Self::InApp),
            "email" => Some(Self::Email),
            "push" => Some(Self::Push),
            _ => None,
        }
    }
}

/// Entity types that can be notification sources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationEntity {
    Ticket { id: i32, title: String },
    Comment { id: i32, ticket_id: i32, ticket_title: String },
}

impl NotificationEntity {
    pub fn entity_type(&self) -> &'static str {
        match self {
            Self::Ticket { .. } => "ticket",
            Self::Comment { .. } => "comment",
        }
    }

    pub fn entity_id(&self) -> i32 {
        match self {
            Self::Ticket { id, .. } => *id,
            Self::Comment { id, .. } => *id,
        }
    }

    /// Get the ticket ID (for navigation)
    pub fn ticket_id(&self) -> i32 {
        match self {
            Self::Ticket { id, .. } => *id,
            Self::Comment { ticket_id, .. } => *ticket_id,
        }
    }
}

/// Actor who triggered the notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationActor {
    pub uuid: Uuid,
    pub name: String,
    pub avatar_thumb: Option<String>,
}

/// Core notification data structure - input to NotificationService
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPayload {
    pub notification_type: NotificationTypeCode,
    pub recipient_uuid: Uuid,
    pub actor: NotificationActor,
    pub entity: NotificationEntity,
    pub title: String,
    pub body: Option<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

impl NotificationPayload {
    pub fn new(
        notification_type: NotificationTypeCode,
        recipient_uuid: Uuid,
        actor: NotificationActor,
        entity: NotificationEntity,
    ) -> Self {
        let title = notification_type.title().to_string();
        Self {
            notification_type,
            recipient_uuid,
            actor,
            entity,
            title,
            body: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        }
    }

    #[allow(dead_code)]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Notification ready for delivery (after preference checks)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverableNotification {
    pub id: Option<i32>,
    pub uuid: Uuid,
    pub payload: NotificationPayload,
    pub channels: Vec<NotificationChannel>,
}

/// SSE notification event sent to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub id: Uuid,
    pub notification_type: String,
    pub title: String,
    pub body: Option<String>,
    pub entity_type: String,
    pub entity_id: i32,
    pub ticket_id: i32,
    pub actor: NotificationActor,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

impl From<&DeliverableNotification> for NotificationEvent {
    fn from(notification: &DeliverableNotification) -> Self {
        Self {
            id: notification.uuid,
            notification_type: notification.payload.notification_type.as_str().to_string(),
            title: notification.payload.title.clone(),
            body: notification.payload.body.clone(),
            entity_type: notification.payload.entity.entity_type().to_string(),
            entity_id: notification.payload.entity.entity_id(),
            ticket_id: notification.payload.entity.ticket_id(),
            actor: notification.payload.actor.clone(),
            metadata: notification.payload.metadata.clone(),
            timestamp: notification.payload.created_at,
        }
    }
}
