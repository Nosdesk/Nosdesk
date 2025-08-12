// Removed unused import: use serde_json::json;
use crate::handlers::sse::{SseState, TicketEvent};
use crate::utils::jwt::JwtUtils;
use actix_web::web;

/// SSE broadcasting utilities for real-time ticket updates
pub struct SseBroadcaster;

impl SseBroadcaster {
    /// Broadcast a ticket field update to all connected clients
    pub fn broadcast_ticket_updated(
        state: &web::Data<SseState>,
        ticket_id: i32,
        field: &str,
        value: serde_json::Value,
        updated_by: &str,
    ) {
        let event = TicketEvent::TicketUpdated {
            ticket_id,
            field: field.to_string(),
            value,
            updated_by: updated_by.to_string(),
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a comment addition to all connected clients
    pub fn broadcast_comment_added(
        state: &web::Data<SseState>,
        ticket_id: i32,
        comment: serde_json::Value,
    ) {
        let event = TicketEvent::CommentAdded {
            ticket_id,
            comment,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a comment deletion to all connected clients
    pub fn broadcast_comment_deleted(
        state: &web::Data<SseState>,
        ticket_id: i32,
        comment_id: i32,
    ) {
        let event = TicketEvent::CommentDeleted {
            ticket_id,
            comment_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a device linking event to all connected clients
    pub fn broadcast_device_linked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        device_id: i32,
    ) {
        let event = TicketEvent::DeviceLinked {
            ticket_id,
            device_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a device unlinking event to all connected clients
    pub fn broadcast_device_unlinked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        device_id: i32,
    ) {
        let event = TicketEvent::DeviceUnlinked {
            ticket_id,
            device_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a ticket linking event to all connected clients
    pub fn broadcast_ticket_linked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        linked_ticket_id: i32,
    ) {
        let event = TicketEvent::TicketLinked {
            ticket_id,
            linked_ticket_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a ticket unlinking event to all connected clients
    pub fn broadcast_ticket_unlinked(
        state: &web::Data<SseState>,
        ticket_id: i32,
        linked_ticket_id: i32,
    ) {
        let event = TicketEvent::TicketUnlinked {
            ticket_id,
            linked_ticket_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a project assignment event to all connected clients
    pub fn broadcast_project_assigned(
        state: &web::Data<SseState>,
        ticket_id: i32,
        project_id: i32,
    ) {
        let event = TicketEvent::ProjectAssigned {
            ticket_id,
            project_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Broadcast a project unassignment event to all connected clients
    pub fn broadcast_project_unassigned(
        state: &web::Data<SseState>,
        ticket_id: i32,
        project_id: i32,
    ) {
        let event = TicketEvent::ProjectUnassigned {
            ticket_id,
            project_id,
            timestamp: chrono::Utc::now(),
        };
        state.broadcast_event(event);
    }

    /// Get the number of currently connected SSE clients
    pub fn get_connected_clients_count(state: &web::Data<SseState>) -> usize {
        state.get_client_count()
    }

    /// Validate SSE authentication token from query parameters
    pub async fn validate_sse_token(
        token: Option<&String>,
        conn: &mut crate::db::DbConnection,
    ) -> Result<(crate::models::Claims, crate::models::User), crate::utils::jwt::JwtError> {
        let token_str = JwtUtils::extract_token_from_query(token)?;
        JwtUtils::validate_token_with_user_check(token_str, conn).await
    }
}

/// Macro for easy SSE broadcasting with error handling
#[macro_export]
macro_rules! broadcast_sse_event {
    ($state:expr, $method:ident, $($arg:expr),*) => {
        if let Some(state) = $state.as_ref() {
            crate::utils::sse::SseBroadcaster::$method(state, $($arg),*);
        }
    };
} 