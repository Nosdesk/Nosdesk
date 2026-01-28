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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_type_code_roundtrip() {
        let variants = [
            NotificationTypeCode::TicketAssigned,
            NotificationTypeCode::TicketStatusChanged,
            NotificationTypeCode::CommentAdded,
            NotificationTypeCode::Mentioned,
            NotificationTypeCode::TicketCreatedRequester,
        ];
        for variant in &variants {
            let s = variant.as_str();
            let parsed = NotificationTypeCode::from_str(s).unwrap();
            assert_eq!(*variant, parsed);
        }
    }

    #[test]
    fn notification_type_code_unknown_returns_none() {
        assert!(NotificationTypeCode::from_str("unknown").is_none());
        assert!(NotificationTypeCode::from_str("").is_none());
    }

    #[test]
    fn notification_type_titles() {
        let variants = [
            NotificationTypeCode::TicketAssigned,
            NotificationTypeCode::TicketStatusChanged,
            NotificationTypeCode::CommentAdded,
            NotificationTypeCode::Mentioned,
            NotificationTypeCode::TicketCreatedRequester,
        ];
        for variant in &variants {
            assert!(!variant.title().is_empty(), "{:?} has empty title", variant);
        }
    }

    #[test]
    fn notification_channel_roundtrip() {
        let channels = [
            NotificationChannel::InApp,
            NotificationChannel::Email,
            NotificationChannel::Push,
        ];
        for ch in &channels {
            let s = ch.as_str();
            let parsed = NotificationChannel::from_str(s).unwrap();
            assert_eq!(*ch, parsed);
        }
    }

    #[test]
    fn notification_entity_ticket_methods() {
        let entity = NotificationEntity::Ticket { id: 42, title: "Test".to_string() };
        assert_eq!(entity.entity_type(), "ticket");
        assert_eq!(entity.entity_id(), 42);
        assert_eq!(entity.ticket_id(), 42);
    }

    #[test]
    fn notification_entity_comment_methods() {
        let entity = NotificationEntity::Comment {
            id: 10,
            ticket_id: 42,
            ticket_title: "Test".to_string(),
        };
        assert_eq!(entity.entity_type(), "comment");
        assert_eq!(entity.entity_id(), 10);
        assert_eq!(entity.ticket_id(), 42);
    }

    #[test]
    fn notification_payload_builder() {
        let actor = NotificationActor {
            uuid: Uuid::new_v4(),
            name: "Actor".to_string(),
            avatar_thumb: None,
        };
        let entity = NotificationEntity::Ticket { id: 1, title: "T".to_string() };
        let payload = NotificationPayload::new(
            NotificationTypeCode::TicketAssigned,
            Uuid::new_v4(),
            actor,
            entity,
        );
        // Defaults
        assert!(payload.body.is_none());
        assert_eq!(payload.metadata, serde_json::json!({}));
        assert_eq!(payload.title, "Assigned to Ticket");

        // Builder methods
        let payload = payload
            .with_body("body text")
            .with_title("Custom Title")
            .with_metadata(serde_json::json!({"key": "val"}));
        assert_eq!(payload.body.as_deref(), Some("body text"));
        assert_eq!(payload.title, "Custom Title");
        assert_eq!(payload.metadata, serde_json::json!({"key": "val"}));
    }

    #[test]
    fn deliverable_to_event_conversion() {
        let actor = NotificationActor {
            uuid: Uuid::new_v4(),
            name: "Actor".to_string(),
            avatar_thumb: None,
        };
        let entity = NotificationEntity::Comment {
            id: 5,
            ticket_id: 10,
            ticket_title: "Ticket".to_string(),
        };
        let notif_uuid = Uuid::new_v4();
        let deliverable = DeliverableNotification {
            id: Some(1),
            uuid: notif_uuid,
            payload: NotificationPayload::new(
                NotificationTypeCode::CommentAdded,
                Uuid::new_v4(),
                actor,
                entity,
            ).with_body("hello"),
            channels: vec![NotificationChannel::InApp],
        };

        let event = NotificationEvent::from(&deliverable);
        assert_eq!(event.id, notif_uuid);
        assert_eq!(event.notification_type, "comment_added");
        assert_eq!(event.entity_type, "comment");
        assert_eq!(event.entity_id, 5);
        assert_eq!(event.ticket_id, 10);
        assert_eq!(event.body.as_deref(), Some("hello"));
    }
}
