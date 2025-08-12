use actix_web::{web, HttpResponse, Result as ActixResult, HttpRequest};
use futures::stream::{Stream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::interval;
use uuid::Uuid;

// Event types for SSE
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum TicketEvent {
    TicketUpdated {
        ticket_id: i32,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    CommentAdded {
        ticket_id: i32,
        comment: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    CommentDeleted {
        ticket_id: i32,
        comment_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AttachmentAdded {
        ticket_id: i32,
        comment_id: i32,
        attachment: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    AttachmentDeleted {
        ticket_id: i32,
        comment_id: i32,
        attachment_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    DeviceLinked {
        ticket_id: i32,
        device_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    DeviceUnlinked {
        ticket_id: i32,
        device_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    DeviceUpdated {
        device_id: i32,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ProjectAssigned {
        ticket_id: i32,
        project_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ProjectUnassigned {
        ticket_id: i32,
        project_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketLinked {
        ticket_id: i32,
        linked_ticket_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketUnlinked {
        ticket_id: i32,
        linked_ticket_id: i32,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    Heartbeat {
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

// Client connection info
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClientInfo {
    pub user_id: String,
    pub connected_at: Instant,
    pub last_ping: Instant,
}

// Global event broadcaster
type EventSender = broadcast::Sender<TicketEvent>;
type EventReceiver = broadcast::Receiver<TicketEvent>;

// Global state for managing SSE connections
pub struct SseState {
    pub sender: EventSender,
    pub clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
}

impl SseState {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1000); // Buffer up to 1000 events
        Self {
            sender,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn broadcast_event(&self, event: TicketEvent) {
        if let Err(e) = self.sender.send(event.clone()) {
            eprintln!("Failed to broadcast SSE event: {:?}", e);
        } else {
            // Log the event (only in debug mode)
            if cfg!(debug_assertions) {
                println!("Broadcasted SSE event: {:?}", event);
            }
        }
    }

    pub fn add_client(&self, client_id: String, user_id: String) {
        let mut clients = self.clients.lock().unwrap();
        clients.insert(client_id, ClientInfo {
            user_id,
            connected_at: Instant::now(),
            last_ping: Instant::now(),
        });
    }

    pub fn remove_client(&self, client_id: &str) {
        let mut clients = self.clients.lock().unwrap();
        clients.remove(client_id);
    }

    pub fn get_client_count(&self) -> usize {
        let clients = self.clients.lock().unwrap();
        clients.len()
    }
}

// SSE stream implementation
pub struct SseStream {
    receiver: EventReceiver,
    heartbeat_interval: tokio::time::Interval,
    client_id: String,
    state: web::Data<SseState>,
}

impl SseStream {
    pub fn new(
        receiver: EventReceiver,
        client_id: String,
        state: web::Data<SseState>,
    ) -> Self {
        let mut heartbeat_interval = interval(Duration::from_secs(30)); // Send heartbeat every 30 seconds
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        Self {
            receiver,
            heartbeat_interval,
            client_id,
            state,
        }
    }
}

impl Stream for SseStream {
    type Item = Result<actix_web::web::Bytes, actix_web::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Check for heartbeat
        if let Poll::Ready(_) = self.heartbeat_interval.poll_tick(cx) {
            let heartbeat = TicketEvent::Heartbeat {
                timestamp: chrono::Utc::now(),
            };
            
            let event_data = serde_json::to_string(&heartbeat).unwrap_or_default();
            let sse_data = format!("event: heartbeat\ndata: {}\n\n", event_data);
            
            return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
        }

        // Check for new events
        match self.receiver.try_recv() {
            Ok(event) => {
                let event_type = match &event {
                    TicketEvent::TicketUpdated { .. } => "ticket-updated",
                    TicketEvent::CommentAdded { .. } => "comment-added",
                    TicketEvent::CommentDeleted { .. } => "comment-deleted",
                    TicketEvent::AttachmentAdded { .. } => "attachment-added",
                    TicketEvent::AttachmentDeleted { .. } => "attachment-deleted",
                    TicketEvent::DeviceLinked { .. } => "device-linked",
                    TicketEvent::DeviceUnlinked { .. } => "device-unlinked",
                    TicketEvent::DeviceUpdated { .. } => "device-updated",
                    TicketEvent::ProjectAssigned { .. } => "project-assigned",
                    TicketEvent::ProjectUnassigned { .. } => "project-unassigned",
                    TicketEvent::TicketLinked { .. } => "ticket-linked",
                    TicketEvent::TicketUnlinked { .. } => "ticket-unlinked",
                    TicketEvent::Heartbeat { .. } => "heartbeat",
                };

                let event_data = serde_json::to_string(&event).unwrap_or_default();
                let sse_data = format!("event: {}\ndata: {}\n\n", event_type, event_data);
                
                Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))))
            }
            Err(broadcast::error::TryRecvError::Empty) => {
                // No new events, return Pending
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => {
                // Client is lagging behind, send a reconnect message
                let reconnect_data = format!("event: reconnect\ndata: {}\n\n", json!({"reason": "lagged"}));
                Poll::Ready(Some(Ok(actix_web::web::Bytes::from(reconnect_data))))
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                // Channel is closed, end the stream
                Poll::Ready(None)
            }
        }
    }
}

impl Drop for SseStream {
    fn drop(&mut self) {
        self.state.remove_client(&self.client_id);
        if cfg!(debug_assertions) {
            println!("SSE client {} disconnected", self.client_id);
        }
    }
}

// SSE endpoint for ticket updates
pub async fn ticket_events_stream(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    state: web::Data<SseState>,
    query: web::Query<TicketEventsQuery>,
) -> ActixResult<HttpResponse> {
    // Get database connection
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return Ok(HttpResponse::InternalServerError().json("Database connection error")),
    };

    // EventSource doesn't support custom headers, so we must use query parameters
    // The SSE token should be a short-lived token obtained from /api/events/token
    let token = match query.sse_token.as_ref() {
        Some(t) => t.as_str(),
        None => return Ok(HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Missing SSE token. Use /api/events/token to get a secure token."
        }))),
    };

    // Validate the SSE token and get user info
    use crate::utils::jwt::JwtUtils;
    let (user_info, _user) = match JwtUtils::validate_token_with_user_check(token, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(e) => return Ok(e.into()),
    };

    // Generate a unique client ID
    let client_id = Uuid::new_v4().to_string();

    // Add client to the state
    state.add_client(client_id.clone(), user_info.sub.clone());

    // Create a receiver for this client
    let receiver = state.sender.subscribe();

    // Create the SSE stream
    let stream = SseStream::new(receiver, client_id.clone(), state.clone());

    if cfg!(debug_assertions) {
        println!("SSE client {} connected for user {} (ticket_id: {:?})", 
                client_id, user_info.sub, query.ticket_id);
    }

    // Return SSE response
    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("Access-Control-Allow-Origin", "*"))
        .append_header(("Access-Control-Allow-Headers", "Authorization"))
        .streaming(stream))
}

#[derive(Deserialize)]
pub struct TicketEventsQuery {
    ticket_id: Option<i32>,
    sse_token: Option<String>,
}

// Helper function to broadcast ticket update events
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

// Helper function to broadcast comment events
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

// Helper function to broadcast device events
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

pub fn broadcast_device_updated(
    state: &web::Data<SseState>,
    device_id: i32,
    field: &str,
    value: serde_json::Value,
    updated_by: &str,
) {
    let event = TicketEvent::DeviceUpdated {
        device_id,
        field: field.to_string(),
        value,
        updated_by: updated_by.to_string(),
        timestamp: chrono::Utc::now(),
    };
    state.broadcast_event(event);
}

// Helper function to broadcast ticket linking events
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

// Helper function to broadcast project assignment events
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

// SSE status endpoint
pub async fn sse_status(state: web::Data<SseState>) -> impl actix_web::Responder {
    let client_count = state.get_client_count();
    HttpResponse::Ok().json(json!({
        "connected_clients": client_count,
        "status": "running"
    }))
}

// Secure endpoint to get SSE token (requires authentication)
pub async fn get_sse_token(
    auth: actix_web_httpauth::extractors::bearer::BearerAuth,
    pool: web::Data<crate::db::Pool>,
) -> impl actix_web::Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Use the established authentication pattern
    use crate::utils::jwt::helpers as jwt_helpers;
    let (user_info, _user) = match jwt_helpers::require_role(&auth, &mut conn, "user").await {
        Ok((claims, user)) => (claims, user),
        Err(e) => return e.into(),
    };

    // Generate a short-lived SSE-specific token (valid for 1 hour)
    use crate::utils::jwt::JwtUtils;
    let sse_token = match JwtUtils::create_sse_token(&user_info.sub, &user_info.role) {
        Ok(token) => token,
        Err(_) => return HttpResponse::InternalServerError().json("Failed to create SSE token"),
    };

    HttpResponse::Ok().json(json!({
        "sse_token": sse_token,
        "expires_in": 3600, // 1 hour in seconds
        "user_id": user_info.sub
    }))
} 