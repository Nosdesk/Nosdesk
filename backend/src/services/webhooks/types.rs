//! Webhook Types
//!
//! Event type mapping between SSE events and webhook event strings.

use serde::Serialize;
use uuid::Uuid;

use crate::handlers::sse::SseEvent;

/// Webhook event types that map to SSE events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WebhookEventType {
    // Ticket events
    TicketCreated,
    TicketUpdated,
    TicketDeleted,

    // Comment events
    CommentAdded,
    CommentDeleted,

    // Attachment events
    AttachmentAdded,
    AttachmentDeleted,

    // Asset events
    AssetCreated,
    AssetUpdated,
    AssetDeleted,
    DeviceLinked,
    DeviceUnlinked,

    // Project events
    ProjectAssigned,
    ProjectUnassigned,

    // Ticket relationship events
    TicketLinked,
    TicketUnlinked,

    // Documentation events
    DocumentationCreated,
    DocumentationUpdated,

    // User events
    UserCreated,
    UserUpdated,
    UserDeleted,
}

impl WebhookEventType {
    /// Convert to string representation used in webhook payloads
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TicketCreated => "ticket.created",
            Self::TicketUpdated => "ticket.updated",
            Self::TicketDeleted => "ticket.deleted",
            Self::CommentAdded => "comment.added",
            Self::CommentDeleted => "comment.deleted",
            Self::AttachmentAdded => "attachment.added",
            Self::AttachmentDeleted => "attachment.deleted",
            Self::AssetCreated => "device.created",
            Self::AssetUpdated => "device.updated",
            Self::AssetDeleted => "device.deleted",
            Self::DeviceLinked => "device.linked",
            Self::DeviceUnlinked => "device.unlinked",
            Self::ProjectAssigned => "project.assigned",
            Self::ProjectUnassigned => "project.unassigned",
            Self::TicketLinked => "ticket.linked",
            Self::TicketUnlinked => "ticket.unlinked",
            Self::DocumentationCreated => "documentation.created",
            Self::DocumentationUpdated => "documentation.updated",
            Self::UserCreated => "user.created",
            Self::UserUpdated => "user.updated",
            Self::UserDeleted => "user.deleted",
        }
    }

    /// Parse from string
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ticket.created" => Some(Self::TicketCreated),
            "ticket.updated" => Some(Self::TicketUpdated),
            "ticket.deleted" => Some(Self::TicketDeleted),
            "comment.added" => Some(Self::CommentAdded),
            "comment.deleted" => Some(Self::CommentDeleted),
            "attachment.added" => Some(Self::AttachmentAdded),
            "attachment.deleted" => Some(Self::AttachmentDeleted),
            "device.created" => Some(Self::AssetCreated),
            "device.updated" => Some(Self::AssetUpdated),
            "device.deleted" => Some(Self::AssetDeleted),
            "device.linked" => Some(Self::DeviceLinked),
            "device.unlinked" => Some(Self::DeviceUnlinked),
            "project.assigned" => Some(Self::ProjectAssigned),
            "project.unassigned" => Some(Self::ProjectUnassigned),
            "ticket.linked" => Some(Self::TicketLinked),
            "ticket.unlinked" => Some(Self::TicketUnlinked),
            "documentation.created" => Some(Self::DocumentationCreated),
            "documentation.updated" => Some(Self::DocumentationUpdated),
            "user.created" => Some(Self::UserCreated),
            "user.updated" => Some(Self::UserUpdated),
            "user.deleted" => Some(Self::UserDeleted),
            _ => None,
        }
    }

    /// Get all available event types (for API endpoint)
    pub fn all() -> Vec<&'static str> {
        vec![
            "ticket.created",
            "ticket.updated",
            "ticket.deleted",
            "comment.added",
            "comment.deleted",
            "attachment.added",
            "attachment.deleted",
            "device.created",
            "device.updated",
            "device.deleted",
            "device.linked",
            "device.unlinked",
            "project.assigned",
            "project.unassigned",
            "ticket.linked",
            "ticket.unlinked",
            "documentation.created",
            "documentation.updated",
            "user.created",
            "user.updated",
            "user.deleted",
        ]
    }

    /// Map from SSE SseEvent to WebhookEventType
    pub fn from_sse_event(event: &SseEvent) -> Option<Self> {
        match event {
            SseEvent::TicketCreated { .. } => Some(Self::TicketCreated),
            SseEvent::TicketUpdated { .. } => Some(Self::TicketUpdated),
            SseEvent::TicketDeleted { .. } => Some(Self::TicketDeleted),
            SseEvent::CommentAdded { .. } => Some(Self::CommentAdded),
            SseEvent::CommentDeleted { .. } => Some(Self::CommentDeleted),
            SseEvent::AttachmentAdded { .. } => Some(Self::AttachmentAdded),
            SseEvent::AttachmentDeleted { .. } => Some(Self::AttachmentDeleted),
            SseEvent::AssetCreated { .. } => Some(Self::AssetCreated),
            SseEvent::AssetUpdated { .. } => Some(Self::AssetUpdated),
            SseEvent::AssetDeleted { .. } => Some(Self::AssetDeleted),
            SseEvent::DeviceLinked { .. } => Some(Self::DeviceLinked),
            SseEvent::DeviceUnlinked { .. } => Some(Self::DeviceUnlinked),
            SseEvent::ProjectAssigned { .. } => Some(Self::ProjectAssigned),
            SseEvent::ProjectUnassigned { .. } => Some(Self::ProjectUnassigned),
            SseEvent::TicketLinked { .. } => Some(Self::TicketLinked),
            SseEvent::TicketUnlinked { .. } => Some(Self::TicketUnlinked),
            SseEvent::DocumentationCreated { .. } => Some(Self::DocumentationCreated),
            SseEvent::DocumentationUpdated { .. } => Some(Self::DocumentationUpdated),
            SseEvent::UserCreated { .. } => Some(Self::UserCreated),
            SseEvent::UserUpdated { .. } => Some(Self::UserUpdated),
            SseEvent::UserDeleted { .. } => Some(Self::UserDeleted),
            // Soft-delete is the customer-facing "user is gone"
            // signal subscribers should react to. Purge fires
            // ~30 days later as a no-op for webhook consumers
            // (the user is already gone from their perspective).
            // Restore is rare and has no webhook contract yet.
            SseEvent::UserSoftDeleted { .. } => Some(Self::UserDeleted),
            SseEvent::UserPurged { .. } => None,
            SseEvent::UserRestored { .. } => None,
            // Internal events not exposed to webhooks
            SseEvent::CollectionUpdated { .. } => None,
            SseEvent::Heartbeat { .. } => None,
            SseEvent::ViewersChanged { .. } => None,
            SseEvent::TicketFieldPreviewed { .. } => None,
            SseEvent::NotificationReceived { .. } => None,
            SseEvent::KnowledgeGapDetected { .. } => None,
            SseEvent::KnowledgeGapResolved { .. } => None,
            // Sync engine outbox carries a batch of sync_actions; the
            // webhook recipient surface doesn't fan these out (consumers
            // hit /api/sync/delta directly), so no resource_type to
            // report.
            SseEvent::SyncActions { .. } => None,
        }
    }
}

/// Webhook payload envelope sent to external endpoints
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub id: Uuid,
    pub event_type: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_roundtrip() {
        let variants = [
            WebhookEventType::TicketCreated,
            WebhookEventType::TicketUpdated,
            WebhookEventType::TicketDeleted,
            WebhookEventType::CommentAdded,
            WebhookEventType::CommentDeleted,
            WebhookEventType::AttachmentAdded,
            WebhookEventType::AttachmentDeleted,
            WebhookEventType::AssetCreated,
            WebhookEventType::DeviceLinked,
            WebhookEventType::DeviceUnlinked,
            WebhookEventType::AssetUpdated,
            WebhookEventType::ProjectAssigned,
            WebhookEventType::ProjectUnassigned,
            WebhookEventType::TicketLinked,
            WebhookEventType::TicketUnlinked,
            WebhookEventType::DocumentationCreated,
            WebhookEventType::DocumentationUpdated,
            WebhookEventType::UserCreated,
            WebhookEventType::UserUpdated,
            WebhookEventType::UserDeleted,
        ];
        for variant in &variants {
            let s = variant.as_str();
            let parsed = WebhookEventType::from_str(s).unwrap_or_else(|| {
                panic!(
                    "Failed to roundtrip variant {:?} through as_str/from_str",
                    variant
                );
            });
            assert_eq!(*variant, parsed);
        }
    }

    #[test]
    fn from_str_unknown_returns_none() {
        assert!(WebhookEventType::from_str("nonexistent.event").is_none());
        assert!(WebhookEventType::from_str("").is_none());
        assert!(WebhookEventType::from_str("ticket").is_none());
    }

    #[test]
    fn all_returns_correct_count() {
        assert_eq!(WebhookEventType::all().len(), 21);
    }

    #[test]
    fn from_sse_heartbeat_is_none() {
        let heartbeat = SseEvent::Heartbeat {
            timestamp: chrono::Utc::now(),
        };
        assert!(WebhookEventType::from_sse_event(&heartbeat).is_none());
    }
}
