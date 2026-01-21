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
use tokio_stream::wrappers::BroadcastStream;
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
    TicketCreated {
        ticket_id: i32,
        ticket: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TicketDeleted {
        ticket_id: i32,
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
    DeviceCreated {
        device_id: i32,
        device: serde_json::Value,
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
    DocumentationCreated {
        document_id: i32,
        document: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    DocumentationUpdated {
        document_id: i32,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ViewerCountChanged {
        ticket_id: i32,
        count: usize,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    UserUpdated {
        user_uuid: String,
        field: String,
        value: serde_json::Value,
        updated_by: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    UserCreated {
        user_uuid: String,
        user: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    UserDeleted {
        user_uuid: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    Heartbeat {
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Notification received (targeted to specific user)
    NotificationReceived {
        recipient_uuid: String,
        notification: serde_json::Value,
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
        // Log when events are dropped for tracking issues
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

    /// Broadcast a notification to a specific user via SSE
    pub async fn broadcast_notification(
        &self,
        recipient_uuid: String,
        notification: crate::services::notifications::NotificationEvent,
    ) {
        let event = TicketEvent::NotificationReceived {
            recipient_uuid,
            notification: serde_json::to_value(&notification).unwrap_or_default(),
            timestamp: chrono::Utc::now(),
        };
        self.broadcast_event(event).await;
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
    event_stream: BroadcastStream<TicketEvent>,
    heartbeat_interval: tokio::time::Interval,
    client_id: String,
    state: web::Data<SseState>,
}

impl SseStream {
    pub fn new(receiver: EventReceiver, client_id: String, state: web::Data<SseState>) -> Self {
        // 15 second heartbeat for better connection detection
        let mut heartbeat_interval = interval(Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Wrap the receiver in BroadcastStream for proper waker registration
        let event_stream = BroadcastStream::new(receiver);

        Self {
            event_stream,
            heartbeat_interval,
            client_id,
            state,
        }
    }
}

impl Stream for SseStream {
    type Item = Result<actix_web::web::Bytes, actix_web::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let client_id = this.client_id.clone();

        // Poll the BroadcastStream - this properly maintains waker registration across polls
        match Pin::new(&mut this.event_stream).poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                // Got event - determine event type and serialize
                let event_type = match &event {
                    TicketEvent::TicketUpdated { .. } => "ticket-updated",
                    TicketEvent::TicketCreated { .. } => "ticket-created",
                    TicketEvent::TicketDeleted { .. } => "ticket-deleted",
                    TicketEvent::CommentAdded { .. } => "comment-added",
                    TicketEvent::CommentDeleted { .. } => "comment-deleted",
                    TicketEvent::AttachmentAdded { .. } => "attachment-added",
                    TicketEvent::AttachmentDeleted { .. } => "attachment-deleted",
                    TicketEvent::DeviceLinked { .. } => "device-linked",
                    TicketEvent::DeviceUnlinked { .. } => "device-unlinked",
                    TicketEvent::DeviceCreated { .. } => "device-created",
                    TicketEvent::DeviceUpdated { .. } => "device-updated",
                    TicketEvent::ProjectAssigned { .. } => "project-assigned",
                    TicketEvent::ProjectUnassigned { .. } => "project-unassigned",
                    TicketEvent::TicketLinked { .. } => "ticket-linked",
                    TicketEvent::TicketUnlinked { .. } => "ticket-unlinked",
                    TicketEvent::DocumentationCreated { .. } => "documentation-created",
                    TicketEvent::DocumentationUpdated { .. } => "documentation-updated",
                    TicketEvent::ViewerCountChanged { .. } => "viewer-count-changed",
                    TicketEvent::UserUpdated { .. } => "user-updated",
                    TicketEvent::UserCreated { .. } => "user-created",
                    TicketEvent::UserDeleted { .. } => "user-deleted",
                    TicketEvent::Heartbeat { .. } => "heartbeat",
                    TicketEvent::NotificationReceived { .. } => "notification-received",
                };

                // Serialize event data
                let event_data = serde_json::to_string(&event).unwrap_or_default();
                let sse_data = format!("event: {}\ndata: {}\n\n", event_type, event_data);

                return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
            }
            Poll::Ready(Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(count)))) => {
                // Client is lagging - close connection so they can reconnect with fresh buffer
                tracing::warn!("SSE: Client {} lagged by {} events, closing connection", client_id, count);
                return Poll::Ready(None);
            }
            Poll::Ready(None) => {
                // Stream ended (channel closed)
                tracing::info!("SSE: Channel closed for client {}", client_id);
                return Poll::Ready(None);
            }
            Poll::Pending => {
                // No event yet - check heartbeat before returning Pending
            }
        }

        // Check for heartbeat
        if let Poll::Ready(_) = this.heartbeat_interval.poll_tick(cx) {
            let sse_data = "event: heartbeat\ndata: {}\n\n";
            return Poll::Ready(Some(Ok(actix_web::web::Bytes::from(sse_data))));
        }

        // Both event stream and heartbeat are Pending
        // BroadcastStream properly maintains waker registration, waking when either:
        // 1. A new event is broadcast (stream waker fires immediately)
        // 2. Heartbeat interval elapses (interval waker fires)
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
    let client_id = Uuid::now_v7().to_string();
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
    req: actix_web::HttpRequest,
    pool: web::Data<crate::db::Pool>,
) -> impl actix_web::Responder {
    use actix_web::HttpMessage;

    let conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    // Extract claims from request extensions (set by cookie_auth_middleware)
    let user_info = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json("Authentication required");
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
