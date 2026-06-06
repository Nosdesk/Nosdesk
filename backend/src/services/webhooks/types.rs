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
    AssetLinked,
    AssetUnlinked,

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

    // SLA events
    TicketSlaBreached,
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
            Self::AssetCreated => "asset.created",
            Self::AssetUpdated => "asset.updated",
            Self::AssetDeleted => "asset.deleted",
            Self::AssetLinked => "asset.linked",
            Self::AssetUnlinked => "asset.unlinked",
            Self::ProjectAssigned => "project.assigned",
            Self::ProjectUnassigned => "project.unassigned",
            Self::TicketLinked => "ticket.linked",
            Self::TicketUnlinked => "ticket.unlinked",
            Self::DocumentationCreated => "documentation.created",
            Self::DocumentationUpdated => "documentation.updated",
            Self::UserCreated => "user.created",
            Self::UserUpdated => "user.updated",
            Self::UserDeleted => "user.deleted",
            Self::TicketSlaBreached => "ticket.sla_breached",
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
            "asset.created" => Some(Self::AssetCreated),
            "asset.updated" => Some(Self::AssetUpdated),
            "asset.deleted" => Some(Self::AssetDeleted),
            "asset.linked" => Some(Self::AssetLinked),
            "asset.unlinked" => Some(Self::AssetUnlinked),
            "project.assigned" => Some(Self::ProjectAssigned),
            "project.unassigned" => Some(Self::ProjectUnassigned),
            "ticket.linked" => Some(Self::TicketLinked),
            "ticket.unlinked" => Some(Self::TicketUnlinked),
            "documentation.created" => Some(Self::DocumentationCreated),
            "documentation.updated" => Some(Self::DocumentationUpdated),
            "user.created" => Some(Self::UserCreated),
            "user.updated" => Some(Self::UserUpdated),
            "user.deleted" => Some(Self::UserDeleted),
            "ticket.sla_breached" => Some(Self::TicketSlaBreached),
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
            "ticket.sla_breached",
        ]
    }

    /// Webhook events dispatched straight from SSE — only the ones that
    /// don't yet have a `sync_actions` source. Everything else flows
    /// through the webhook cursor dispatcher (see `from_sync_action`).
    ///
    /// These gap events (asset/ticket link/unlink + SLA breach) are
    /// broadcast on the single instance that committed the mutation, so
    /// SSE delivery stays single-fire. They move to `from_sync_action`
    /// once their transactional sync emits land.
    pub fn from_sse_event(event: &SseEvent) -> Option<Self> {
        match event {
            SseEvent::AssetLinked { .. } => Some(Self::AssetLinked),
            SseEvent::AssetUnlinked { .. } => Some(Self::AssetUnlinked),
            SseEvent::TicketLinked { .. } => Some(Self::TicketLinked),
            SseEvent::TicketUnlinked { .. } => Some(Self::TicketUnlinked),
            SseEvent::SlaBreached { .. } => Some(Self::TicketSlaBreached),
            _ => None,
        }
    }

    /// Map a `sync_actions.event_type` string to the webhook event it
    /// drives. Covers every webhook event that has a sync_actions
    /// source; the gap events stay on `from_sse_event` for now. The
    /// several specific ticket-change event_types all collapse to
    /// `ticket.updated` (one webhook per change).
    pub fn from_sync_action(event_type: &str) -> Option<Self> {
        Some(match event_type {
            "ticket.created" => Self::TicketCreated,
            "ticket.updated"
            | "ticket.workflow_state_changed"
            | "ticket.assignee_changed"
            | "ticket.priority_changed"
            | "ticket.title_changed"
            | "ticket.category_changed"
            | "ticket.verification_changed" => Self::TicketUpdated,
            "ticket.deleted" => Self::TicketDeleted,
            "comment.created" => Self::CommentAdded,
            "comment.deleted" => Self::CommentDeleted,
            "attachment.created" => Self::AttachmentAdded,
            "attachment.deleted" => Self::AttachmentDeleted,
            "asset.created" => Self::AssetCreated,
            "asset.updated" => Self::AssetUpdated,
            "asset.deleted" => Self::AssetDeleted,
            "project_ticket.added" => Self::ProjectAssigned,
            "project_ticket.removed" => Self::ProjectUnassigned,
            "documentation_page.created" => Self::DocumentationCreated,
            "documentation_page.metadata_changed" | "documentation_page.verified" => {
                Self::DocumentationUpdated
            }
            "user.created" => Self::UserCreated,
            "user.updated" => Self::UserUpdated,
            "user.deleted" => Self::UserDeleted,
            _ => return None,
        })
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
            WebhookEventType::AssetLinked,
            WebhookEventType::AssetUnlinked,
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
            WebhookEventType::TicketSlaBreached,
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
        assert_eq!(WebhookEventType::all().len(), 22);
    }

    #[test]
    fn from_sse_heartbeat_is_none() {
        let heartbeat = SseEvent::Heartbeat {
            timestamp: chrono::Utc::now(),
        };
        assert!(WebhookEventType::from_sse_event(&heartbeat).is_none());
    }

    #[test]
    fn from_sse_event_only_covers_gap_events() {
        // Covered events flow through from_sync_action now; from_sse_event
        // must return None for them so they don't double-fire.
        let created = SseEvent::TicketCreated {
            ticket_id: 1,
            ticket: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
        };
        assert!(WebhookEventType::from_sse_event(&created).is_none());

        // Gap events (no sync_actions source yet) stay on the SSE path.
        let linked = SseEvent::AssetLinked {
            ticket_id: 1,
            device_id: 2,
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(
            WebhookEventType::from_sse_event(&linked),
            Some(WebhookEventType::AssetLinked)
        );
    }

    #[test]
    fn from_sync_action_maps_covered_events() {
        use WebhookEventType::*;
        let cases = [
            ("ticket.created", Some(TicketCreated)),
            ("ticket.workflow_state_changed", Some(TicketUpdated)),
            ("ticket.assignee_changed", Some(TicketUpdated)),
            ("ticket.deleted", Some(TicketDeleted)),
            ("comment.created", Some(CommentAdded)),
            ("attachment.created", Some(AttachmentAdded)),
            ("asset.created", Some(AssetCreated)),
            ("project_ticket.added", Some(ProjectAssigned)),
            ("project_ticket.removed", Some(ProjectUnassigned)),
            ("documentation_page.created", Some(DocumentationCreated)),
            (
                "documentation_page.metadata_changed",
                Some(DocumentationUpdated),
            ),
            ("documentation_page.verified", Some(DocumentationUpdated)),
            ("user.created", Some(UserCreated)),
            // Not webhook events / no source.
            ("documentation_page.visibility_changed", None),
            ("workflow_state.created", None),
            ("cycle.created", None),
            // Gap events are NOT in from_sync_action (no sync emit yet).
            ("asset.linked", None),
            ("ticket.sla_breached", None),
        ];
        for (event_type, expected) in cases {
            assert_eq!(
                WebhookEventType::from_sync_action(event_type),
                expected,
                "from_sync_action({event_type})"
            );
        }
    }
}
