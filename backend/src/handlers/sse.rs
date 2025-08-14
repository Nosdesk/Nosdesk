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
        // Increase buffer size to handle bursts of events (like comment creation with attachments)
        let (sender, _) = broadcast::channel(5000);
        if cfg!(debug_assertions) {
            println!("SSE: Created broadcast channel with capacity 5000");
        }
        Self {
            sender,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn broadcast_event(&self, event: TicketEvent) {
        // Add retry logic for broadcast failures with exponential backoff
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 3;
        let mut delay = std::time::Duration::from_millis(10);
        
        while retry_count < MAX_RETRIES {
            match self.sender.send(event.clone()) {
                Ok(_) => {
                    if cfg!(debug_assertions) {
                        println!("SSE: Event broadcasted: {:?}", event);
                    }
                    return;
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= MAX_RETRIES {
                        eprintln!("SSE: Failed to broadcast event after {} retries: {:?}", MAX_RETRIES, e);
                    } else {
                        // Use async sleep instead of blocking sleep
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                    }
                }
            }
        }
    }

    pub fn add_client(&self, client_id: String, user_id: String) {
        let mut clients = self.clients.lock().unwrap();
        clients.insert(client_id.clone(), ClientInfo {
            user_id: user_id.clone(),
            connected_at: Instant::now(),
            last_ping: Instant::now(),
        });
        if cfg!(debug_assertions) {
            println!("SSE: Added client {} for user {} (total: {})", 
                    client_id, user_id, clients.len());
        }
    }

    pub fn remove_client(&self, client_id: &str) {
        let mut clients = self.clients.lock().unwrap();
        let was_removed = clients.remove(client_id).is_some();
        if cfg!(debug_assertions) {
            if was_removed {
                println!("SSE: Removed client {} (total: {})", client_id, clients.len());
            }
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
    pub fn new(
        receiver: EventReceiver,
        client_id: String,
        state: web::Data<SseState>,
    ) -> Self {
        let mut heartbeat_interval = interval(Duration::from_secs(30));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        if cfg!(debug_assertions) {
            println!("SSE: Creating stream for client {}", client_id);
        }

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

        // Check for new events with better error handling
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
                Poll::Pending
            }
            Err(broadcast::error::TryRecvError::Lagged(count)) => {
                // Client is lagging behind, send a reconnect message
                if cfg!(debug_assertions) {
                    println!("SSE: Client {} lagging by {} messages, sending reconnect", 
                            self.client_id, count);
                }
                let reconnect_data = format!("event: reconnect\ndata: {}\n\n", 
                    json!({"reason": "lagged", "count": count}));
                Poll::Ready(Some(Ok(actix_web::web::Bytes::from(reconnect_data))))
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                // Channel is closed, end the stream gracefully
                if cfg!(debug_assertions) {
                    println!("SSE: Broadcast channel closed for client {}", self.client_id);
                }
                Poll::Ready(None)
            }
        }
    }
}

impl Drop for SseStream {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            println!("SSE: Stream dropping for client {}", self.client_id);
        }
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
    if cfg!(debug_assertions) {
        println!("SSE: Endpoint called for ticket events");
    }
    
    // Get database connection
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            if cfg!(debug_assertions) {
                println!("SSE: Database connection error");
            }
            return Ok(HttpResponse::InternalServerError().json("Database connection error"));
        }
    };

    // Validate SSE token
    let token = match query.sse_token.as_ref() {
        Some(t) => t.as_str(),
        None => {
            if cfg!(debug_assertions) {
                println!("SSE: Missing SSE token");
            }
            return Ok(HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Missing SSE token. Use /api/events/token to get a secure token."
            })));
        }
    };

    // Validate the SSE token and get user info
    use crate::utils::jwt::JwtUtils;
    let (user_info, _user) = match JwtUtils::validate_token_with_user_check(token, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(e) => {
            if cfg!(debug_assertions) {
                println!("SSE: Token validation failed: {:?}", e);
            }
            return Ok(e.into());
        }
    };

    // Generate a unique client ID and create stream
    let client_id = Uuid::new_v4().to_string();
    state.add_client(client_id.clone(), user_info.sub.clone());
    let receiver = state.sender.subscribe();
    let stream = SseStream::new(receiver, client_id.clone(), state.clone());

    if cfg!(debug_assertions) {
        println!("SSE: Client {} connected for user {} (ticket_id: {:?})", 
                client_id, user_info.sub, query.ticket_id);
    }

    // Return SSE response with improved headers for better connection stability
    Ok(HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
        .append_header(("Pragma", "no-cache"))
        .append_header(("Expires", "0"))
        .append_header(("Connection", "keep-alive"))
        .append_header(("Access-Control-Allow-Origin", "*"))
        .append_header(("Access-Control-Allow-Headers", "Authorization"))
        .append_header(("X-Accel-Buffering", "no")) // Disable nginx buffering if present
        .streaming(stream))
}

#[derive(Deserialize)]
pub struct TicketEventsQuery {
    ticket_id: Option<i32>,
    sse_token: Option<String>,
}

// SSE status endpoint
pub async fn sse_status(state: web::Data<SseState>) -> impl actix_web::Responder {
    if cfg!(debug_assertions) {
        println!("SSE: Status endpoint called");
    }
    
    let client_count = state.get_client_count();
    
    if cfg!(debug_assertions) {
        println!("SSE: Status endpoint returning {} connected clients", client_count);
    }
    
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
    if cfg!(debug_assertions) {
        println!("SSE: Token endpoint called");
    }
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            if cfg!(debug_assertions) {
                println!("SSE: Database connection error in token endpoint");
            }
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    // Use the established authentication pattern
    use crate::utils::jwt::helpers as jwt_helpers;
    let (user_info, _user) = match jwt_helpers::require_role(&auth, &mut conn, "user").await {
        Ok((claims, user)) => (claims, user),
        Err(e) => {
            if cfg!(debug_assertions) {
                println!("SSE: Token validation failed in token endpoint: {:?}", e);
            }
            return e.into();
        }
    };

    if cfg!(debug_assertions) {
        println!("SSE: Token validation successful for user {}", user_info.sub);
    }

    // Generate a short-lived SSE-specific token (valid for 1 hour)
    use crate::utils::jwt::JwtUtils;
    let sse_token = match JwtUtils::create_sse_token(&user_info.sub, &user_info.role) {
        Ok(token) => token,
        Err(e) => {
            if cfg!(debug_assertions) {
                println!("SSE: Failed to create SSE token for user {}: {:?}", user_info.sub, e);
            }
            return HttpResponse::InternalServerError().json("Failed to create SSE token");
        }
    };

    if cfg!(debug_assertions) {
        println!("SSE: Returning SSE token for user {}", user_info.sub);
    }

    HttpResponse::Ok().json(json!({
        "sse_token": sse_token,
        "expires_in": 3600, // 1 hour in seconds
        "user_id": user_info.sub
    }))
} 