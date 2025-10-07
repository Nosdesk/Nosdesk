use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use futures::Stream;
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
        // Optimized buffer: 1000 events is sufficient for most use cases
        // Larger buffers use more memory and can cause lag detection issues
        let (sender, _) = broadcast::channel(1000);
        Self {
            sender,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn broadcast_event(&self, event: TicketEvent) {
        // Fast, non-blocking broadcast - just send once
        // Log when events are dropped so we can track issues
        match self.sender.send(event.clone()) {
            Ok(receiver_count) => {
                #[cfg(debug_assertions)]
                tracing::debug!("SSE: Event sent to {} receivers", receiver_count);
            }
            Err(_) => {
                // No active receivers - log in debug mode to track dropped events
                #[cfg(debug_assertions)]
                tracing::warn!("SSE: Event dropped - no active receivers: {:?}", event);
            }
        }
    }

    pub fn add_client(&self, client_id: String, user_id: String) {
        let mut clients = self.clients.lock().unwrap();
        clients.insert(
            client_id.clone(),
            ClientInfo {
                user_id: user_id.clone(),
                connected_at: Instant::now(),
                last_ping: Instant::now(),
            },
        );
        #[cfg(debug_assertions)]
        tracing::info!(
            "SSE: Client {} connected for user {} ({} total)",
            client_id,
            user_id,
            clients.len()
        );
    }

    pub fn remove_client(&self, client_id: &str) {
        let mut clients = self.clients.lock().unwrap();
        if clients.remove(client_id).is_some() {
            #[cfg(debug_assertions)]
            tracing::info!(
                "SSE: Client {} disconnected ({} remaining)",
                client_id,
                clients.len()
            );
        }
    }

    pub fn get_client_count(&self) -> usize {
        self.clients.lock().unwrap().len()
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
    pub fn new(receiver: EventReceiver, client_id: String, state: web::Data<SseState>) -> Self {
        // 15 second heartbeat for better connection detection
        let mut heartbeat_interval = interval(Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

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
        // Check for new events FIRST (prioritize real-time updates over heartbeat)
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    // Determine event type string (avoid allocations where possible)
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

                    // Serialize event data
                    let event_data = serde_json::to_string(&event).unwrap_or_default();
                    let sse_data = format!("event: {}\ndata: {}\n\n", event_type, event_data);

                    return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    // No events available right now, check heartbeat
                    break;
                }
                Err(broadcast::error::TryRecvError::Lagged(count)) => {
                    // Client is lagging - close connection so they can reconnect with fresh buffer
                    tracing::warn!("SSE: Client {} lagged by {} events, closing connection", self.client_id, count);
                    return Poll::Ready(None);
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    // Channel closed - end stream
                    tracing::info!("SSE: Channel closed for client {}", self.client_id);
                    return Poll::Ready(None);
                }
            }
        }

        // Check for heartbeat
        if let Poll::Ready(_) = self.heartbeat_interval.poll_tick(cx) {
            let sse_data = "event: heartbeat\ndata: {}\n\n";
            return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
        }

        // Register waker and return pending
        // The broadcast receiver and heartbeat interval will wake us when ready
        Poll::Pending
    }
}

impl Drop for SseStream {
    fn drop(&mut self) {
        self.state.remove_client(&self.client_id);
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
        Err(_) => {
            return Ok(HttpResponse::InternalServerError().json("Database connection error"));
        }
    };

    // Validate SSE token
    let token = match query.sse_token.as_ref() {
        Some(t) => t.as_str(),
        None => {
            return Ok(HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Missing SSE token"
            })));
        }
    };

    // Validate the SSE token
    use crate::utils::jwt::JwtUtils;
    let (user_info, _user) = match JwtUtils::validate_token_with_user_check(token, &mut conn).await
    {
        Ok((claims, user)) => (claims, user),
        Err(e) => {
            return Ok(e.into());
        }
    };

    // Generate client ID and create stream
    let client_id = Uuid::new_v4().to_string();
    state.add_client(client_id.clone(), user_info.sub.clone());
    let receiver = state.sender.subscribe();
    let stream = SseStream::new(receiver, client_id.clone(), state.clone());

    // Return SSE response with optimized headers
    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("X-Accel-Buffering", "no")) // Disable nginx buffering
        .streaming(stream))
}

#[derive(Deserialize)]
pub struct TicketEventsQuery {
    ticket_id: Option<i32>,
    sse_token: Option<String>,
}

// SSE status endpoint
pub async fn sse_status(state: web::Data<SseState>) -> impl actix_web::Responder {
    let client_count = state.get_client_count();

    HttpResponse::Ok().json(json!({
        "connected_clients": client_count,
        "status": "running"
    }))
}

// Secure endpoint to get SSE token
pub async fn get_sse_token(
    auth: actix_web_httpauth::extractors::bearer::BearerAuth,
    pool: web::Data<crate::db::Pool>,
) -> impl actix_web::Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    // Authenticate the request
    use crate::utils::jwt::helpers as jwt_helpers;
    let (user_info, _user) = match jwt_helpers::require_role(&auth, &mut conn, "user").await {
        Ok((claims, user)) => (claims, user),
        Err(e) => {
            return e.into();
        }
    };

    // Generate a short-lived SSE token (1 hour)
    use crate::utils::jwt::JwtUtils;
    let sse_token = match JwtUtils::create_sse_token(&user_info.sub, &user_info.role) {
        Ok(token) => token,
        Err(_) => {
            return HttpResponse::InternalServerError().json("Failed to create SSE token");
        }
    };

    HttpResponse::Ok().json(json!({
        "sse_token": sse_token,
        "expires_in": 3600,
        "user_id": user_info.sub
    }))
}
