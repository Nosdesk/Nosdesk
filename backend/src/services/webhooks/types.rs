//! Webhook Types
//!
//! Event type mapping between SSE events and webhook event strings.

use serde::Serialize;
use uuid::Uuid;

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

    /// Every event type, in the order the API lists them. `all()` and
    /// `from_str` both derive from this, so an event name can't be offered
    /// without also being accepted and delivered.
    pub const ALL: [Self; 22] = [
        Self::TicketCreated,
        Self::TicketUpdated,
        Self::TicketDeleted,
        Self::CommentAdded,
        Self::CommentDeleted,
        Self::AttachmentAdded,
        Self::AttachmentDeleted,
        Self::AssetCreated,
        Self::AssetUpdated,
        Self::AssetDeleted,
        Self::AssetLinked,
        Self::AssetUnlinked,
        Self::ProjectAssigned,
        Self::ProjectUnassigned,
        Self::TicketLinked,
        Self::TicketUnlinked,
        Self::DocumentationCreated,
        Self::DocumentationUpdated,
        Self::UserCreated,
        Self::UserUpdated,
        Self::UserDeleted,
        Self::TicketSlaBreached,
    ];

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s)
    }

    /// Get all available event types (for API endpoint)
    pub fn all() -> Vec<&'static str> {
        Self::ALL.iter().map(Self::as_str).collect()
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
            | "ticket.verification_changed"
            | "ticket.resolution_notes_changed"
            // A merge source's row, in its merged state, with the merge
            // fields. The destination's `ticket.merged` carries counts, not
            // a ticket, and raises nothing.
            | "ticket.merged_into" => Self::TicketUpdated,
            "ticket.deleted" => Self::TicketDeleted,
            "comment.created" => Self::CommentAdded,
            "comment.deleted" => Self::CommentDeleted,
            // Added means on a ticket: created on a comment, or a draft
            // upload attached to one. A draft alone has no workspace
            // audience (its uploader's, or none for a guest's), so its
            // events raise nothing. A file a merge moves with its reply
            // (`attachment.moved`) isn't added either.
            "attachment.created" | "attachment.attached" => Self::AttachmentAdded,
            "attachment.deleted" => Self::AttachmentDeleted,
            "asset.created" => Self::AssetCreated,
            "asset.updated" => Self::AssetUpdated,
            "asset.deleted" => Self::AssetDeleted,
            // Ticket<->asset and ticket<->ticket links became their own
            // sync aggregates (Phase 2); keep the external webhook event
            // names stable by mapping the new event types here.
            "ticket_asset.added" => Self::AssetLinked,
            "ticket_asset.removed" => Self::AssetUnlinked,
            "linked_ticket.added" => Self::TicketLinked,
            "linked_ticket.removed" => Self::TicketUnlinked,
            "ticket.sla_breached" => Self::TicketSlaBreached,
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
    fn every_listed_event_round_trips() {
        for variant in WebhookEventType::ALL {
            assert_eq!(
                WebhookEventType::from_str(variant.as_str()),
                Some(variant),
                "{variant:?} through as_str/from_str"
            );
        }
        for name in WebhookEventType::all() {
            let parsed = WebhookEventType::from_str(name)
                .unwrap_or_else(|| panic!("all() offers {name}, which doesn't parse"));
            assert_eq!(parsed.as_str(), name);
        }
    }

    /// `ALL` lists every variant once. The match has no wildcard, so a new
    /// variant doesn't compile here until it's considered.
    #[test]
    fn all_lists_every_variant_once() {
        use WebhookEventType::*;
        fn listed(e: WebhookEventType) -> bool {
            match e {
                TicketCreated | TicketUpdated | TicketDeleted | CommentAdded | CommentDeleted
                | AttachmentAdded | AttachmentDeleted | AssetCreated | AssetUpdated
                | AssetDeleted | AssetLinked | AssetUnlinked | ProjectAssigned
                | ProjectUnassigned | TicketLinked | TicketUnlinked | DocumentationCreated
                | DocumentationUpdated | UserCreated | UserUpdated | UserDeleted
                | TicketSlaBreached => WebhookEventType::ALL.contains(&e),
            }
        }
        assert!(WebhookEventType::ALL.into_iter().all(listed));
        let unique: std::collections::HashSet<_> = WebhookEventType::ALL.into_iter().collect();
        assert_eq!(
            unique.len(),
            WebhookEventType::ALL.len(),
            "a variant listed twice"
        );
    }

    #[test]
    fn from_str_unknown_returns_none() {
        assert!(WebhookEventType::from_str("nonexistent.event").is_none());
        assert!(WebhookEventType::from_str("").is_none());
        assert!(WebhookEventType::from_str("ticket").is_none());
    }

    #[test]
    fn asset_events_use_the_asset_prefix() {
        let all = WebhookEventType::all();
        assert!(all.iter().all(|e| !e.starts_with("device.")));
        for e in [
            "asset.created",
            "asset.updated",
            "asset.deleted",
            "asset.linked",
            "asset.unlinked",
        ] {
            assert!(all.contains(&e), "{e}");
        }
    }

    #[test]
    fn from_sync_action_maps_covered_events() {
        use WebhookEventType::*;
        let cases = [
            ("ticket.created", Some(TicketCreated)),
            ("ticket.workflow_state_changed", Some(TicketUpdated)),
            ("ticket.assignee_changed", Some(TicketUpdated)),
            ("ticket.resolution_notes_changed", Some(TicketUpdated)),
            ("ticket.merged_into", Some(TicketUpdated)),
            ("ticket.merged", None),
            ("ticket.deleted", Some(TicketDeleted)),
            ("comment.created", Some(CommentAdded)),
            ("attachment.created", Some(AttachmentAdded)),
            ("attachment.attached", Some(AttachmentAdded)),
            // A merge moving a reply's file adds nothing.
            ("attachment.moved", None),
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
            // Ticket<->asset and ticket<->ticket links are their own
            // sync aggregates now; the external webhook names are kept
            // stable by mapping the new event types.
            ("ticket_asset.added", Some(AssetLinked)),
            ("ticket_asset.removed", Some(AssetUnlinked)),
            ("linked_ticket.added", Some(TicketLinked)),
            ("linked_ticket.removed", Some(TicketUnlinked)),
            ("ticket.sla_breached", Some(TicketSlaBreached)),
            // Old discrete-SSE event names no longer ride sync_actions.
            ("asset.linked", None),
            ("ticket.linked", None),
            // Not webhook events / no source.
            ("documentation_page.visibility_changed", None),
            ("workflow_state.created", None),
            ("cycle.created", None),
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
