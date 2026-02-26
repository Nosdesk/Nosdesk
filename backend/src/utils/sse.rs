use crate::handlers::sse::{SseState, SseEvent};
use actix_web::web;
use chrono::Utc;
use tracing::debug;

/// SSE broadcasting utilities for real-time ticket updates
pub struct SseBroadcaster;

impl SseBroadcaster {
    /// Generic function to broadcast any SSE event
    async fn broadcast_event(state: &web::Data<SseState>, event: SseEvent) {
        debug!(event = ?event, "SSE: Broadcasting event");
        state.broadcast_event(event).await;
        debug!("SSE: Event broadcasted successfully");
    }

    /// Generic function to create and broadcast events with common fields
    async fn broadcast_generic_event<F>(state: &web::Data<SseState>, event_creator: F)
    where
        F: FnOnce(chrono::DateTime<Utc>) -> SseEvent,
    {
        let event = event_creator(Utc::now());
        Self::broadcast_event(state, event).await;
    }

    /// Broadcast a ticket creation to all connected clients
    pub async fn broadcast_ticket_created(
        state: &web::Data<SseState>,
        ticket_id: i32,
        ticket: serde_json::Value,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::TicketCreated {
                ticket_id,
                ticket,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a ticket field update to all connected clients
    pub async fn broadcast_ticket_updated(
        state: &web::Data<SseState>,
        ticket_id: i32,
        field: &str,
        value: serde_json::Value,
        updated_by: &str,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::TicketUpdated {
                ticket_id,
                field: field.to_string(),
                value,
                updated_by: updated_by.to_string(),
                timestamp,
            }
        }).await;
    }

    /// Broadcast a comment addition to all connected clients
    pub async fn broadcast_comment_added(
        state: &web::Data<SseState>,
        ticket_id: i32,
        comment: serde_json::Value,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::CommentAdded {
                ticket_id,
                comment,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a comment deletion to all connected clients
    pub async fn broadcast_comment_deleted(
        state: &web::Data<SseState>,
        ticket_id: i32,
        comment_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::CommentDeleted {
                ticket_id,
                comment_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a device linking event to all connected clients
    pub async fn broadcast_device_linked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        device_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DeviceLinked {
                ticket_id,
                device_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a device unlinking event to all connected clients
    pub async fn broadcast_device_unlinked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        device_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DeviceUnlinked {
                ticket_id,
                device_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a device creation to all connected clients
    pub async fn broadcast_device_created(
        state: &web::Data<SseState>,
        device_id: i32,
        device: serde_json::Value,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DeviceCreated {
                device_id,
                device,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a device update event to all connected clients
    pub async fn broadcast_device_updated(
        state: &web::Data<SseState>,
        device_id: i32,
        field: &str,
        value: serde_json::Value,
        updated_by: &str,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DeviceUpdated {
                device_id,
                field: field.to_string(),
                value,
                updated_by: updated_by.to_string(),
                timestamp,
            }
        }).await;
    }

    /// Broadcast a device deletion to all connected clients
    pub async fn broadcast_device_deleted(
        state: &web::Data<SseState>,
        device_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DeviceDeleted {
                device_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a ticket linking event to all connected clients
    pub async fn broadcast_ticket_linked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        linked_ticket_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::TicketLinked {
                ticket_id,
                linked_ticket_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a ticket unlinking event to all connected clients
    pub async fn broadcast_ticket_unlinked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        linked_ticket_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::TicketUnlinked {
                ticket_id,
                linked_ticket_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a project assignment event to all connected clients
    pub async fn broadcast_project_assigned(
        state: &web::Data<SseState>,
        ticket_id: i32,
        project_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::ProjectAssigned {
                ticket_id,
                project_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a project unassignment event to all connected clients
    pub async fn broadcast_project_unassigned(
        state: &web::Data<SseState>,
        ticket_id: i32,
        project_id: i32,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::ProjectUnassigned {
                ticket_id,
                project_id,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a documentation page creation to all connected clients
    pub async fn broadcast_documentation_created(
        state: &web::Data<SseState>,
        document_id: i32,
        document: serde_json::Value,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DocumentationCreated {
                document_id,
                document,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a documentation page update to all connected clients
    pub async fn broadcast_documentation_updated(
        state: &web::Data<SseState>,
        document_id: i32,
        field: &str,
        value: serde_json::Value,
        updated_by: &str,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::DocumentationUpdated {
                document_id,
                field: field.to_string(),
                value,
                updated_by: updated_by.to_string(),
                timestamp,
            }
        }).await;
    }

    /// Broadcast a collection field update to all connected clients
    pub async fn broadcast_collection_updated(
        state: &web::Data<SseState>,
        collection_id: i32,
        field: &str,
        value: serde_json::Value,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::CollectionUpdated {
                collection_id,
                field: field.to_string(),
                value,
                timestamp,
            }
        }).await;
    }

    /// Broadcast viewer count change for a ticket to all connected clients
    pub async fn broadcast_viewer_count(
        state: &web::Data<SseState>,
        ticket_id: i32,
        count: usize,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::ViewerCountChanged {
                ticket_id,
                count,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a user field update to all connected clients
    pub async fn broadcast_user_updated(
        state: &web::Data<SseState>,
        user_uuid: &str,
        field: &str,
        value: serde_json::Value,
        updated_by: &str,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::UserUpdated {
                user_uuid: user_uuid.to_string(),
                field: field.to_string(),
                value,
                updated_by: updated_by.to_string(),
                timestamp,
            }
        }).await;
    }

    /// Broadcast a user creation to all connected clients
    pub async fn broadcast_user_created(
        state: &web::Data<SseState>,
        user_uuid: &str,
        user: serde_json::Value,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::UserCreated {
                user_uuid: user_uuid.to_string(),
                user,
                timestamp,
            }
        }).await;
    }

    /// Broadcast a user deletion to all connected clients
    pub async fn broadcast_user_deleted(
        state: &web::Data<SseState>,
        user_uuid: &str,
    ) {
        Self::broadcast_generic_event(state, |timestamp| {
            SseEvent::UserDeleted {
                user_uuid: user_uuid.to_string(),
                timestamp,
            }
        }).await;
    }
}

/// Macro for easy SSE broadcasting with error handling
#[macro_export]
macro_rules! broadcast_sse_event {
    ($state:expr, $method:ident, $($arg:expr),*) => {
        if let Some(state) = $state.as_ref() {
            $crate::utils::sse::SseBroadcaster::$method(state, $($arg),*);
        }
    };
} 