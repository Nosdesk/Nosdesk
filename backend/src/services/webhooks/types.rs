//! Webhook Types
//!
//! Event type mapping between SSE events and webhook event strings.

use serde::Serialize;
use uuid::Uuid;

use crate::handlers::sse::TicketEvent;

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

    // Device events
    DeviceCreated,
    DeviceLinked,
    DeviceUnlinked,
    DeviceUpdated,

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
            Self::DeviceCreated => "device.created",
            Self::DeviceLinked => "device.linked",
            Self::DeviceUnlinked => "device.unlinked",
            Self::DeviceUpdated => "device.updated",
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
            "device.created" => Some(Self::DeviceCreated),
            "device.linked" => Some(Self::DeviceLinked),
            "device.unlinked" => Some(Self::DeviceUnlinked),
            "device.updated" => Some(Self::DeviceUpdated),
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
            "device.linked",
            "device.unlinked",
            "device.updated",
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

    /// Map from SSE TicketEvent to WebhookEventType
    pub fn from_sse_event(event: &TicketEvent) -> Option<Self> {
        match event {
            TicketEvent::TicketCreated { .. } => Some(Self::TicketCreated),
            TicketEvent::TicketUpdated { .. } => Some(Self::TicketUpdated),
            TicketEvent::TicketDeleted { .. } => Some(Self::TicketDeleted),
            TicketEvent::CommentAdded { .. } => Some(Self::CommentAdded),
            TicketEvent::CommentDeleted { .. } => Some(Self::CommentDeleted),
            TicketEvent::AttachmentAdded { .. } => Some(Self::AttachmentAdded),
            TicketEvent::AttachmentDeleted { .. } => Some(Self::AttachmentDeleted),
            TicketEvent::DeviceCreated { .. } => Some(Self::DeviceCreated),
            TicketEvent::DeviceLinked { .. } => Some(Self::DeviceLinked),
            TicketEvent::DeviceUnlinked { .. } => Some(Self::DeviceUnlinked),
            TicketEvent::DeviceUpdated { .. } => Some(Self::DeviceUpdated),
            TicketEvent::ProjectAssigned { .. } => Some(Self::ProjectAssigned),
            TicketEvent::ProjectUnassigned { .. } => Some(Self::ProjectUnassigned),
            TicketEvent::TicketLinked { .. } => Some(Self::TicketLinked),
            TicketEvent::TicketUnlinked { .. } => Some(Self::TicketUnlinked),
            TicketEvent::DocumentationCreated { .. } => Some(Self::DocumentationCreated),
            TicketEvent::DocumentationUpdated { .. } => Some(Self::DocumentationUpdated),
            TicketEvent::UserCreated { .. } => Some(Self::UserCreated),
            TicketEvent::UserUpdated { .. } => Some(Self::UserUpdated),
            TicketEvent::UserDeleted { .. } => Some(Self::UserDeleted),
            // Internal events not exposed to webhooks
            TicketEvent::Heartbeat { .. } => None,
            TicketEvent::ViewerCountChanged { .. } => None,
            TicketEvent::NotificationReceived { .. } => None,
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
