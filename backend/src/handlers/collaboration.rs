use actix_web::{web, HttpResponse, Responder, Error, HttpRequest};
use actix_web_actors::ws;
use actix::{Actor, StreamHandler, ActorContext, Running, AsyncContext, Handler, Message, Addr};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use yrs::{Doc, Transact, ReadTxn, StateVector, Update};
use yrs::sync::{Awareness, Protocol, DefaultProtocol};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use bytes::Bytes;
use uuid::Uuid;
use base64::{Engine as _, engine::general_purpose};

use crate::repository;
use crate::models::NewArticleContent;

// How often heartbeat pings are sent
// IMPORTANT: Must be less than 30 seconds because y-websocket client has a hardcoded
// 30-second timeout that closes the connection if no messages are received
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
// Minimum time between saves for the same document
const MIN_SAVE_INTERVAL: Duration = Duration::from_secs(5);
// Maximum time a document can have pending changes before forcing a save
const MAX_PENDING_DURATION: Duration = Duration::from_secs(120);
// How long to wait before doing final save on empty room
const EMPTY_ROOM_FINAL_SAVE_DELAY: Duration = Duration::from_secs(2);
// How long to keep document state after room becomes empty
const EMPTY_ROOM_CLEANUP_DELAY: Duration = Duration::from_secs(300); // 5 minutes

// Simple handler to get article content by ticket ID
pub async fn get_article_content(
    pool: web::Data<crate::db::Pool>,
    doc_id: web::Path<String>,
) -> impl Responder {
    let doc_id = doc_id.into_inner();
    let clean_doc_id = doc_id.replace("/", "_");
    
    // Extract ticket ID from doc_id (format: "ticket-123")
    let ticket_id = match clean_doc_id.strip_prefix("ticket-").and_then(|id| id.parse::<i32>().ok()) {
        Some(id) => id,
        None => {
            println!("Invalid ticket ID format: {}", clean_doc_id);
            return HttpResponse::BadRequest().json("Invalid ticket ID format");
        }
    };
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(article_content) => {
            println!("Retrieved article content for ticket {}", ticket_id);
            
            // Encode binary content as base64
            let content_base64 = general_purpose::STANDARD.encode(&article_content.content);
            
            HttpResponse::Ok().json(json!({
                "content": content_base64,
                "ticket_id": ticket_id
            }))
        },
        Err(e) => {
            println!("No article content found for ticket {}: {}", ticket_id, e);
            HttpResponse::Ok().json(json!({
                "content": "",
                "ticket_id": ticket_id
            }))
        }
    }
}

// ============= WebSocket implementation =============

// Document state tracking
#[derive(Clone)]
struct DocumentState {
    awareness: Arc<Awareness>,
    last_saved: Instant,
    has_pending_changes: bool,
    pending_since: Option<Instant>,
    sync_message_count: u32,
    room_empty_since: Option<Instant>, // Track when room became empty
    final_save_completed: bool, // Track if final save was done
}

impl DocumentState {
    fn new(awareness: Arc<Awareness>) -> Self {
        Self {
            awareness,
            last_saved: Instant::now(),
            has_pending_changes: false,
            pending_since: None,
            sync_message_count: 0,
            room_empty_since: None,
            final_save_completed: false,
        }
    }
    
    fn mark_changed(&mut self) {
        if !self.has_pending_changes {
            self.has_pending_changes = true;
            self.pending_since = Some(Instant::now());
        }
        self.sync_message_count += 1;
        
        // Reset room empty tracking since there's activity
        self.room_empty_since = None;
        self.final_save_completed = false;
    }
    
    fn mark_saved(&mut self) {
        self.last_saved = Instant::now();
        self.has_pending_changes = false;
        self.pending_since = None;
        self.sync_message_count = 0;
    }
    
    fn mark_room_empty(&mut self) {
        if self.room_empty_since.is_none() {
            self.room_empty_since = Some(Instant::now());
            self.final_save_completed = false;
        }
    }
    
    fn mark_room_active(&mut self) {
        self.room_empty_since = None;
        self.final_save_completed = false;
    }
    
    fn mark_final_save_completed(&mut self) {
        self.final_save_completed = true;
    }
    
    fn should_save(&self) -> bool {
        if !self.has_pending_changes {
            return false;
        }
        
        let now = Instant::now();
        
        // Save if enough time has passed since last save
        if now.duration_since(self.last_saved) >= MIN_SAVE_INTERVAL {
            return true;
        }
        
        // Force save if changes have been pending too long
        if let Some(pending_since) = self.pending_since {
            if now.duration_since(pending_since) >= MAX_PENDING_DURATION {
                return true;
            }
        }
        
        // Force save after 10 sync messages to prevent data loss
        if self.sync_message_count >= 10 {
            return true;
        }
        
        false
    }
    
    fn should_do_final_save(&self) -> bool {
        // Only do final save if room has been empty for a bit, changes exist, and we haven't done it yet
        if let Some(empty_since) = self.room_empty_since {
            let now = Instant::now();
            return !self.final_save_completed && 
                   (self.has_pending_changes || now.duration_since(empty_since) < Duration::from_secs(5)) &&
                   now.duration_since(empty_since) >= EMPTY_ROOM_FINAL_SAVE_DELAY;
        }
        false
    }
    
    fn should_cleanup(&self) -> bool {
        // Clean up document state after room has been empty for the cleanup delay and final save is done
        if let Some(empty_since) = self.room_empty_since {
            let now = Instant::now();
            return self.final_save_completed && 
                   now.duration_since(empty_since) >= EMPTY_ROOM_CLEANUP_DELAY;
        }
        false
    }
}

// Create app state to manage active documents and awareness
type DocumentId = String;
type SessionId = String;
type SessionInfo = (Addr<YjsWebSocket>, Instant); // (Socket address, last activity timestamp)
type RoomSessions = HashMap<DocumentId, HashMap<SessionId, SessionInfo>>;
type RoomSessionStore = Arc<RwLock<RoomSessions>>;
type DocumentStore = Arc<RwLock<HashMap<DocumentId, DocumentState>>>;

// Define shared app state for WebSocket connections
#[derive(Clone)]
pub struct YjsAppState {
    documents: DocumentStore,
    sessions: RoomSessionStore,
    pool: web::Data<crate::db::Pool>,
}

impl YjsAppState {
    pub fn new(pool: web::Data<crate::db::Pool>) -> Self {
        let state = YjsAppState {
            documents: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pool,
        };
        // Start the periodic cleanup and save task
        let state_clone = state.clone();
        actix::spawn(async move {
            use actix::clock::interval;
            let mut interval = interval(Duration::from_secs(30)); // Check every 30 seconds (was 10)
            loop {
                interval.tick().await;
                state_clone.cleanup_stale_sessions().await;
                state_clone.save_all_active_documents().await;
            }
        });
        state
    }

    // Save all active documents
    async fn save_all_active_documents(&self) {
        let mut documents = self.documents.write().await;
        let mut saved_count = 0;
        let mut final_saved_count = 0;
        let mut cleaned_up_count = 0;
        let mut docs_to_remove = Vec::new();

        for (doc_id, doc_state) in documents.iter_mut() {
            // Regular saves for active documents
            if doc_state.should_save() {
                println!("Saving document {} with pending changes", doc_id);
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
                saved_count += 1;
            }
            
            // Final save for empty rooms
            if doc_state.should_do_final_save() {
                println!("Performing final save for empty room: {}", doc_id);
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
                doc_state.mark_final_save_completed();
                final_saved_count += 1;
            }
            
            // Clean up old empty documents
            if doc_state.should_cleanup() {
                println!("Cleaning up old document state: {}", doc_id);
                docs_to_remove.push(doc_id.clone());
                cleaned_up_count += 1;
            }
        }
        
        // Remove cleaned up documents
        for doc_id in docs_to_remove {
            documents.remove(&doc_id);
        }
        
        if saved_count > 0 || final_saved_count > 0 || cleaned_up_count > 0 {
            println!("Periodic maintenance completed: {} regular saves, {} final saves, {} cleanups", 
                    saved_count, final_saved_count, cleaned_up_count);
        }
    }

    // Get or create awareness for a document
    async fn get_or_create_awareness(&self, doc_id: &str) -> Arc<Awareness> {
        let mut documents = self.documents.write().await;
        
        if let Some(doc_state) = documents.get(doc_id) {
            // Only log when document is accessed for the first time in a while, not on every message
            Arc::clone(&doc_state.awareness)
        } else {
            println!("Creating new awareness for document: {}", doc_id);
            let doc = Doc::new();
            let mut awareness = Awareness::new(doc);
            
            // Load existing content from database if available
            if let Some(ticket_id_str) = doc_id.strip_prefix("ticket-") {
                if let Ok(ticket_id) = ticket_id_str.parse::<i32>() {
                    match self.pool.get() {
                        Ok(mut conn) => {
                            match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
                                Ok(article_content) => {
                                    if !article_content.content.is_empty() {
                                        println!("Loading existing content for ticket {} ({} bytes)", 
                                                ticket_id, article_content.content.len());
                                        
                                        // Apply the saved state to the document
                                        if let Ok(update) = Update::decode_v1(article_content.content.as_bytes()) {
                                            if let Err(e) = awareness.doc_mut().transact_mut().apply_update(update) {
                                                println!("Error applying saved state: {:?}", e);
                                            }
                                        } else {
                                            println!("Failed to decode saved state for ticket {}", ticket_id);
                                        }
                                    }
                                },
                                Err(e) => {
                                    println!("No existing content found for ticket {}: {:?}", ticket_id, e);
                                }
                            }
                        },
                        Err(e) => {
                            println!("Database connection error while loading document: {:?}", e);
                        }
                    }
                }
            }
            
            let awareness_arc = Arc::new(awareness);
            let doc_state = DocumentState::new(Arc::clone(&awareness_arc));
            documents.insert(doc_id.to_string(), doc_state);
            awareness_arc
        }
    }

    // Mark document as having pending changes
    async fn mark_document_changed(&self, doc_id: &str) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.mark_changed();
        }
    }

    // Register session
    async fn register_session(&self, doc_id: &str, session_id: &str, addr: Addr<YjsWebSocket>) {
        let mut sessions = self.sessions.write().await;

        // Get or create the room for this document
        let room = sessions.entry(doc_id.to_string())
            .or_insert_with(HashMap::new);

        // Add this session to the room with current timestamp
        room.insert(session_id.to_string(), (addr, Instant::now()));
        let room_size = room.len();

        // Release sessions lock before acquiring documents lock
        drop(sessions);

        // Mark document as having active sessions
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.mark_room_active();
        }

        println!("Session {} joined document {} (room now has {} users)", session_id, doc_id, room_size);
    }

    // Update session activity timestamp
    async fn update_session_activity(&self, doc_id: &str, session_id: &str) {
        let mut sessions = self.sessions.write().await;

        if let Some(room) = sessions.get_mut(doc_id) {
            if let Some(session_info) = room.get_mut(session_id) {
                // Update the timestamp
                session_info.1 = Instant::now();
            }
        }
    }
    
    // Remove session
    async fn remove_session(&self, doc_id: &str, session_id: &str) {
        let mut sessions = self.sessions.write().await;

        if let Some(room) = sessions.get_mut(doc_id) {
            room.remove(session_id);
            let room_size = room.len();
            let is_empty = room.is_empty();
            println!("Session {} left document {} (room now has {} users)", session_id, doc_id, room_size);

            // If room is empty, mark it as empty but don't save immediately
            if is_empty {
                println!("Room for document {} is now empty, will save after delay", doc_id);
                // Release the sessions lock to avoid deadlock
                drop(sessions);

                // Mark the document as having an empty room
                let mut documents = self.documents.write().await;
                if let Some(doc_state) = documents.get_mut(doc_id) {
                    doc_state.mark_room_empty();
                }
            }
        }
    }
    
    // Clean up stale sessions
    async fn cleanup_stale_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();
        let mut stale_session_count = 0;
        let mut newly_empty_rooms = Vec::new();

        // First pass: collect stale sessions
        for (doc_id, room) in sessions.iter_mut() {
            let mut stale_sessions = Vec::new();
            let was_empty = room.is_empty();

            for (session_id, (_, last_active)) in room.iter() {
                if now.duration_since(*last_active) > CLIENT_TIMEOUT * 5 {
                    stale_sessions.push(session_id.clone());
                }
            }

            stale_session_count += stale_sessions.len();

            // Remove stale sessions from the room
            for session_id in stale_sessions.iter() {
                println!("Removing stale session {} from document {}", session_id, doc_id);
                room.remove(session_id);
            }

            // If room just became empty, mark it
            if !was_empty && room.is_empty() {
                newly_empty_rooms.push(doc_id.clone());
            }
        }

        // Log cleanup summary
        if stale_session_count > 0 {
            println!("Cleaned up {} stale sessions", stale_session_count);
        }

        // Release the sessions lock before updating document states
        drop(sessions);

        // Mark newly empty rooms
        if !newly_empty_rooms.is_empty() {
            let mut documents = self.documents.write().await;
            for doc_id in newly_empty_rooms {
                if let Some(doc_state) = documents.get_mut(&doc_id) {
                    println!("Marking room {} as empty due to stale session cleanup", doc_id);
                    doc_state.mark_room_empty();
                }
            }
        }
    }

    // Force save a document immediately, ignoring timing constraints
    async fn force_save_document(&self, doc_id: &str) {
        let documents = self.documents.read().await;
        if let Some(doc_state) = documents.get(doc_id) {
            println!("Force saving document {} on disconnect", doc_id);
            self.save_document_internal(doc_id, &doc_state.awareness);
            // Note: We don't mark as saved here because the lock is read-only
            // The next save cycle will update the saved timestamp
        }
    }

    // Helper method to save a document by ID
    async fn save_document_by_id(&self, doc_id: &str) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            // Force save regardless of timing constraints when explicitly called
            if doc_state.has_pending_changes {
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
            }
        }
    }

    // Broadcast update to all sessions in a room except sender
    async fn broadcast(&self, doc_id: &str, sender_id: &str, msg: &[u8]) {
        if msg.is_empty() {
            return;
        }

        // Collect addresses while holding lock
        let recipients: Vec<Addr<YjsWebSocket>> = {
            let sessions = self.sessions.read().await;

            if let Some(room) = sessions.get(doc_id) {
                room.iter()
                    .filter(|(id, _)| *id != sender_id)
                    .map(|(_, (addr, _))| addr.clone())
                    .collect()
            } else {
                Vec::new()
            }
        }; // Lock released here

        // Send to all recipients without holding lock
        let msg_bytes = Bytes::copy_from_slice(msg);
        for addr in recipients {
            addr.do_send(YjsMessage(msg_bytes.clone()));
        }
    }

    // Save document state to the database from awareness
    fn save_document_internal(&self, doc_id: &str, awareness: &Awareness) {
        // Check if it's a ticket document
        if let Some(ticket_id_str) = doc_id.strip_prefix("ticket-") {
            if let Ok(ticket_id) = ticket_id_str.parse::<i32>() {
                // Get binary content from the document
                let binary_content = {
                    let doc = awareness.doc();
                    let txn = doc.transact();
                    txn.encode_state_as_update_v1(&StateVector::default())
                };
                
                println!("Saving document content for ticket {} ({} bytes)", ticket_id, binary_content.len());
                
                // Save to database in a separate thread
                let pool = self.pool.clone();
                let content = binary_content.clone(); // Already Vec<u8>
                
                // Use actix to spawn a blocking operation
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            let new_content = NewArticleContent {
                                ticket_id,
                                content: general_purpose::STANDARD.encode(&content), // Convert binary to base64 string for storage
                            };
                            
                            match repository::update_article_content(&mut conn, ticket_id, new_content) {
                                Ok(_) => println!("Successfully saved document for ticket {}", ticket_id),
                                Err(e) => println!("Failed to save document for ticket {}: {:?}", ticket_id, e),
                            }
                        },
                        Err(e) => println!("Database connection error when saving document: {:?}", e),
                    }
                });
            }
        }
    }
}

// Message type for WebSocket communications
#[derive(Message)]
#[rtype(result = "()")]
struct YjsMessage(Bytes);

// WebSocket actor
struct YjsWebSocket {
    id: String,
    doc_id: String,
    app_state: YjsAppState,
    hb: Instant,
    protocol: DefaultProtocol,
    // Statistics for debugging
    messages_received: u32,
    pings_sent: u32,
    pongs_received: u32,
    started_at: Instant,
}

impl YjsWebSocket {
    fn new(doc_id: String, app_state: YjsAppState) -> Self {
        let id = Uuid::new_v4().to_string();
        let now = Instant::now();

        YjsWebSocket {
            id,
            doc_id,
            app_state,
            hb: now,
            protocol: DefaultProtocol,
            messages_received: 0,
            pings_sent: 0,
            pongs_received: 0,
            started_at: now,
        }
    }
    
    // Handle heartbeat
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            let time_since_last_hb = Instant::now().duration_since(act.hb);

            println!("WebSocket heartbeat check for session {}: {} seconds since last activity",
                    act.id, time_since_last_hb.as_secs());

            // Add grace period: warn at CLIENT_TIMEOUT, disconnect at CLIENT_TIMEOUT + 30s
            if time_since_last_hb > CLIENT_TIMEOUT + Duration::from_secs(30) {
                println!("❌ WebSocket Client heartbeat TIMEOUT after {} seconds, disconnecting session {}",
                        time_since_last_hb.as_secs(), act.id);

                // Spawn async removal
                let app_state = act.app_state.clone();
                let doc_id = act.doc_id.clone();
                let session_id = act.id.clone();
                actix::spawn(async move {
                    app_state.remove_session(&doc_id, &session_id).await;
                });

                ctx.stop();
                return;
            }

            // Always send ping on every heartbeat check to keep y-websocket client alive
            // The y-websocket library has a hardcoded 30s timeout, so we must send
            // messages more frequently than that
            println!("📤 WebSocket sending PING to session {} (ping #{}, {}s since last activity)",
                    act.id, act.pings_sent + 1, time_since_last_hb.as_secs());
            act.pings_sent += 1;
            ctx.ping(b"");

            if time_since_last_hb > CLIENT_TIMEOUT {
                println!("⚠️ WebSocket Client heartbeat WARNING for session {} ({} seconds since last activity)",
                        act.id, time_since_last_hb.as_secs());
            }
        });
    }
    
    // Process incoming messages using the built-in protocol
    fn process_message(&mut self, msg: &[u8], ctx: &mut ws::WebsocketContext<Self>) {
        if msg.is_empty() {
            return;
        }

        // CRITICAL: Update heartbeat timestamp BEFORE spawning async work
        // Otherwise the heartbeat checker thinks the connection is idle
        self.hb = Instant::now();

        let app_state = self.app_state.clone();
        let doc_id = self.doc_id.clone();
        let session_id = self.id.clone();
        let msg_vec = msg.to_vec();
        let is_sync_message = msg.get(0) == Some(&0); // MESSAGE_SYNC

        // Spawn async work
        let addr = ctx.address();
        actix::spawn(async move {
            // Update session activity
            app_state.update_session_activity(&doc_id, &session_id).await;

            // Get the awareness for this document
            let awareness = app_state.get_or_create_awareness(&doc_id).await;

            // Use the built-in protocol handler to process the message
            // DefaultProtocol is stateless, so we can create a new instance
            let protocol = DefaultProtocol;
            match protocol.handle(&awareness, &msg_vec) {
                Ok(messages) => {
                    // Send any response messages back to the client
                    for message in messages {
                        let encoded = message.encode_v1();
                        addr.do_send(YjsMessage(Bytes::from(encoded)));
                    }

                    // Broadcast the entire message to other clients
                    app_state.broadcast(&doc_id, &session_id, &msg_vec).await;

                    // Mark document as changed after sync updates
                    if is_sync_message {
                        app_state.mark_document_changed(&doc_id).await;
                    }
                },
                Err(e) => {
                    println!("Error handling protocol message: {:?}", e);
                }
            }
        });
    }
}

impl Actor for YjsWebSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let start_time = chrono::Utc::now();
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🟢 WebSocket STARTED at {}", start_time.format("%H:%M:%S%.3f"));
        println!("   Session ID: {}", self.id);
        println!("   Document: {}", self.doc_id);
        println!("   Heartbeat interval: {}s", HEARTBEAT_INTERVAL.as_secs());
        println!("   Timeout threshold: {}s", (CLIENT_TIMEOUT + Duration::from_secs(30)).as_secs());
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        self.hb(ctx);

        // Register session asynchronously
        let app_state = self.app_state.clone();
        let doc_id = self.doc_id.clone();
        let session_id = self.id.clone();
        let addr = ctx.address();
        actix::spawn(async move {
            app_state.register_session(&doc_id, &session_id, addr).await;
        });

        println!("⏳ Waiting for client sync request for document {}", self.doc_id);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        let stop_time = chrono::Utc::now();
        let time_since_last_hb = Instant::now().duration_since(self.hb);

        let connection_duration = Instant::now().duration_since(self.started_at);

        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🔴 WebSocket STOPPING at {}", stop_time.format("%H:%M:%S%.3f"));
        println!("   Session ID: {}", self.id);
        println!("   Document: {}", self.doc_id);
        println!("   Connection duration: {}s", connection_duration.as_secs());
        println!("   Time since last activity: {}s", time_since_last_hb.as_secs());
        println!("   Statistics:");
        println!("     - Messages received: {}", self.messages_received);
        println!("     - PINGs sent: {}", self.pings_sent);
        println!("     - PONGs received: {}", self.pongs_received);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // Spawn async cleanup work
        let app_state = self.app_state.clone();
        let doc_id = self.doc_id.clone();
        let session_id = self.id.clone();

        actix::spawn(async move {
            // Remove the session first
            app_state.remove_session(&doc_id, &session_id).await;

            // Only force save if this was the last session in the room
            // The periodic save task will handle regular saves
            let should_force_save = {
                let sessions = app_state.sessions.read().await;
                if let Some(room) = sessions.get(&doc_id) {
                    room.is_empty() // Only force save if room is now empty
                } else {
                    true // Room doesn't exist, so it was the last session
                }
            };

            if should_force_save {
                println!("Last session for document {}, performing final save", doc_id);
                app_state.force_save_document(&doc_id).await;
            }
        });

        Running::Stop
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for YjsWebSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                println!("📩 WebSocket received PING for session {}", self.id);
                self.hb = Instant::now();
                self.messages_received += 1;
                ctx.pong(&msg);
            },
            Ok(ws::Message::Pong(_)) => {
                println!("📩 WebSocket received PONG for session {}", self.id);
                self.hb = Instant::now();
                self.pongs_received += 1;
                self.messages_received += 1;
            },
            Ok(ws::Message::Binary(bin)) => {
                println!("📩 WebSocket received BINARY message ({} bytes) for session {}", bin.len(), self.id);
                self.hb = Instant::now();
                self.messages_received += 1;
                self.process_message(&bin, ctx);
            },
            Ok(ws::Message::Close(reason)) => {
                println!("WebSocket received CLOSE message for session {}: {:?}", self.id, reason);
                ctx.close(reason);
                ctx.stop();
            },
            Ok(ws::Message::Text(text)) => {
                println!("WebSocket received unexpected TEXT message for session {}: {}", self.id, text);
            },
            Ok(ws::Message::Continuation(_)) => {
                println!("WebSocket received CONTINUATION for session {}", self.id);
            },
            Ok(ws::Message::Nop) => {
                println!("WebSocket received NOP for session {}", self.id);
            },
            Err(e) => {
                println!("WebSocket protocol error for session {}: {:?}", self.id, e);
                ctx.stop();
            },
        }
    }
}

impl Handler<YjsMessage> for YjsWebSocket {
    type Result = ();

    fn handle(&mut self, msg: YjsMessage, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }
}

// WebSocket connection handler - entry point for WebSocket requests
pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<YjsAppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let doc_id = path.into_inner();
    println!("WebSocket connection request for document: {}", doc_id);
    
    // Extract and validate JWT token from query parameters
    let query_string = req.query_string();
    let token = query_string.split('&')
        .find(|param| param.starts_with("token="))
        .and_then(|param| param.split('=').nth(1))
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No authentication token provided"))?;
    
    // Validate the token using our JWT utilities
    if let Some(pool) = req.app_data::<web::Data<crate::db::Pool>>() {
        let mut conn = pool.get()
            .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;
        
        // Use our centralized JWT validation
        use crate::utils::jwt::JwtUtils;
        
        match JwtUtils::validate_token_with_user_check(token, &mut conn).await {
            Ok((_claims, _user)) => (),
            Err(_) => return Err(actix_web::error::ErrorUnauthorized("Invalid or expired token")),
        }
    } else {
        return Err(actix_web::error::ErrorInternalServerError("Database pool not available"));
    }
    
    println!("WebSocket authentication successful for document: {}", doc_id);
    let actor = YjsWebSocket::new(doc_id, app_state.get_ref().clone());
    ws::start(actor, &req, stream)
}

// Configure routes
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("")
            .route("/article/{doc_id}", web::get().to(get_article_content))
            .route("/ws/{doc_id}", web::get().to(ws_handler))
    );
}