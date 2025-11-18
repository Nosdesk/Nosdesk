use actix_web::{web, HttpResponse, Responder, Error, HttpRequest};
use actix_web_actors::ws;
use actix::{Actor, StreamHandler, ActorContext, Running, AsyncContext, Handler, Message, Addr};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use yrs::{Doc, Transact, ReadTxn, WriteTxn, StateVector, Update, GetString, XmlFragment};
use yrs::sync::{Awareness, Protocol, DefaultProtocol};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use bytes::Bytes;
use uuid::Uuid;
use base64::{Engine as _, engine::general_purpose};

use crate::repository;
use crate::models::{NewArticleContent, NewArticleContentRevision};
use crate::utils::redis_yjs_cache::RedisYjsCache;

// How often heartbeat checks are performed (server-side connection health monitoring)
// Note: y-websocket client maintains its own keepalive via resyncInterval (20s)
// This server-side heartbeat is for detecting truly dead connections
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
// How often to create automatic snapshots (every 500 updates - Yrs community recommendation)
const SNAPSHOT_INTERVAL: u32 = 500;

// Document type enum to distinguish between tickets and documentation
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocumentType {
    Ticket(i32),
    Documentation(i32),
}

impl DocumentType {
    // Parse doc_id format: "ticket-123" or "doc-456"
    fn from_doc_id(doc_id: &str) -> Option<Self> {
        if let Some(id_str) = doc_id.strip_prefix("ticket-") {
            id_str.parse::<i32>().ok().map(DocumentType::Ticket)
        } else if let Some(id_str) = doc_id.strip_prefix("doc-") {
            id_str.parse::<i32>().ok().map(DocumentType::Documentation)
        } else {
            None
        }
    }

    fn to_string(&self) -> String {
        match self {
            DocumentType::Ticket(id) => format!("ticket-{}", id),
            DocumentType::Documentation(id) => format!("doc-{}", id),
        }
    }
}

// Simple handler to get article content by ticket ID or documentation page ID
pub async fn get_article_content(
    pool: web::Data<crate::db::Pool>,
    doc_id: web::Path<String>,
) -> impl Responder {
    let doc_id = doc_id.into_inner();
    let clean_doc_id = doc_id.replace("/", "_");

    // Parse document type and ID
    let doc_type = match DocumentType::from_doc_id(&clean_doc_id) {
        Some(dt) => dt,
        None => {
            println!("Invalid document ID format: {}", clean_doc_id);
            return HttpResponse::BadRequest().json("Invalid document ID format (expected 'ticket-N' or 'doc-N')");
        }
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match doc_type {
        DocumentType::Ticket(ticket_id) => {
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
        },
        DocumentType::Documentation(doc_id) => {
            match repository::get_documentation_page(doc_id, &mut conn) {
                Ok(doc_page) => {
                    println!("Retrieved documentation page {}", doc_id);

                    // If yjs_document exists, encode as base64, otherwise return empty
                    let content_base64 = if let Some(yjs_doc) = doc_page.yjs_document {
                        general_purpose::STANDARD.encode(&yjs_doc)
                    } else {
                        String::new()
                    };

                    HttpResponse::Ok().json(json!({
                        "content": content_base64,
                        "doc_id": doc_id
                    }))
                },
                Err(e) => {
                    println!("No documentation page found with ID {}: {}", doc_id, e);
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "doc_id": doc_id
                    }))
                }
            }
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
    // Snapshot tracking (for version history)
    update_counter: u32,                    // Total updates since document creation
    last_snapshot_at: u32,                  // Update count when last snapshot created
    contributors: std::collections::HashSet<Uuid>, // Contributors since last snapshot
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
            // Initialize snapshot tracking
            update_counter: 0,
            last_snapshot_at: 0,
            contributors: std::collections::HashSet::new(),
        }
    }
    
    fn mark_changed(&mut self) {
        if !self.has_pending_changes {
            self.has_pending_changes = true;
            self.pending_since = Some(Instant::now());
        }
        self.sync_message_count += 1;
        self.update_counter += 1; // Track total updates for snapshot scheduling

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

    // Snapshot management methods
    fn should_create_snapshot(&self) -> bool {
        // Create snapshot every SNAPSHOT_INTERVAL updates (Yrs community recommendation)
        // Only if there are contributors and meaningful changes
        let updates_since_snapshot = self.update_counter - self.last_snapshot_at;
        updates_since_snapshot >= SNAPSHOT_INTERVAL && !self.contributors.is_empty()
    }

    fn add_contributor(&mut self, user_uuid: Uuid) {
        self.contributors.insert(user_uuid);
    }

    fn reset_snapshot_tracking(&mut self) {
        self.last_snapshot_at = self.update_counter;
        self.contributors.clear();
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
    redis_cache: Arc<RedisYjsCache>,
}

impl YjsAppState {
    pub fn new(pool: web::Data<crate::db::Pool>, redis_cache: Arc<RedisYjsCache>) -> Self {
        let state = YjsAppState {
            documents: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pool,
            redis_cache,
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
        let mut snapshot_count = 0;

        for (doc_id, doc_state) in documents.iter_mut() {
            // Regular saves for active documents
            if doc_state.should_save() {
                println!("Saving document {} with pending changes", doc_id);
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
                saved_count += 1;
            }

            // Check if we should create a snapshot (every 500 updates)
            if doc_state.should_create_snapshot() {
                println!("📸 Snapshot threshold reached for {} ({} updates since last snapshot)",
                    doc_id, doc_state.update_counter - doc_state.last_snapshot_at);

                // Clone contributors before passing to async function
                let contributors = doc_state.contributors.clone();
                self.create_snapshot_revision(doc_id, &doc_state.awareness, contributors);
                doc_state.reset_snapshot_tracking();
                snapshot_count += 1;
            }

            // Final save for empty rooms
            if doc_state.should_do_final_save() {
                println!("Performing final save for empty room: {}", doc_id);
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
                doc_state.mark_final_save_completed();
                final_saved_count += 1;

                // Also create final snapshot if there are contributors
                if !doc_state.contributors.is_empty() {
                    println!("📸 Creating final snapshot before cleanup: {}", doc_id);
                    let contributors = doc_state.contributors.clone();
                    self.create_snapshot_revision(doc_id, &doc_state.awareness, contributors);
                    doc_state.reset_snapshot_tracking();
                    snapshot_count += 1;
                }
            }

            // YIJS BEST PRACTICE: Keep documents in memory indefinitely
            // Never remove documents from memory - they contain the authoritative live state
            // Database is only for cold storage (server restart recovery)
            // This prevents race conditions where user reconnects before async save completes
            // See: https://discuss.yjs.dev/t/correct-way-to-implement-version-history-like-google-doc/1691
        }

        if saved_count > 0 || final_saved_count > 0 || snapshot_count > 0 {
            println!("Periodic maintenance: {} saves, {} final saves, {} snapshots",
                    saved_count, final_saved_count, snapshot_count);
        }
    }

    // Get or create awareness for a document
    async fn get_or_create_awareness(&self, doc_id: &str) -> Arc<Awareness> {
        let mut documents = self.documents.write().await;

        if let Some(doc_state) = documents.get_mut(doc_id) {
            // Document exists in memory - reuse it (this is the live state!)
            // Reset the empty room timer since there's activity
            doc_state.mark_room_active();
            Arc::clone(&doc_state.awareness)
        } else {
            println!("Document not in memory: {} - checking Redis cache", doc_id);

            // Create Doc with GC disabled and a consistent server-side client ID
            // CRITICAL: Use a deterministic client ID based on the document ID to ensure
            // consistency across backend restarts. This prevents state vector mismatches.
            let mut options = yrs::Options::default();
            options.skip_gc = true;  // CRITICAL: Disable garbage collection

            // Generate a consistent client ID from the document ID hash
            // This ensures the same document always gets the same server client ID
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            doc_id.hash(&mut hasher);
            let client_id = hasher.finish() | 1; // Ensure it's non-zero

            options.client_id = client_id;
            println!("📋 Creating document with consistent client ID: {} (from doc_id: {})", client_id, doc_id);

            let doc = Doc::with_options(options);

            // CRITICAL: Initialize the "prosemirror" XmlFragment root type
            // This MUST be done before any sync operations to ensure the backend
            // and frontend are working with the same document structure
            {
                let mut txn = doc.transact_mut();
                let _ = txn.get_or_insert_xml_fragment("prosemirror");
                println!("🎯 Initialized 'prosemirror' XmlFragment for document: {}", doc_id);
            }

            let mut awareness = Awareness::new(doc);

            let mut loaded_from_redis = false;

            // STEP 1: Try to load from Redis (hot cache - survives restarts)
            if let Some(redis_data) = self.redis_cache.get_document(doc_id).await {
                println!("Attempting to load document from Redis: {} ({} bytes)", doc_id, redis_data.len());

                if let Ok(update) = Update::decode_v1(&redis_data) {
                    let apply_result = {
                        let mut txn = awareness.doc_mut().transact_mut();
                        txn.apply_update(update)
                    };

                    if let Err(e) = apply_result {
                        println!("❌ Error applying Redis state: {:?}", e);
                        // Delete corrupted entry from Redis
                        println!("🗑️ Deleting corrupted Redis entry for {}", doc_id);
                        self.redis_cache.delete_document(doc_id).await;
                    } else {
                        println!("✅ Successfully loaded document from Redis cache");
                        loaded_from_redis = true;

                        // Diagnostic: Verify content
                        use yrs::{GetString, XmlFragment};
                        let txn = awareness.doc().transact();
                        if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                            let child_count = fragment.children(&txn).count();
                            println!("📄 Redis content: {} children", child_count);
                        }
                    }
                } else {
                    println!("⚠️ Failed to decode Redis data - deleting corrupted entry");
                    // Delete corrupted entry from Redis so it doesn't block future loads
                    self.redis_cache.delete_document(doc_id).await;
                }
            }

            // STEP 2: Fall back to PostgreSQL (cold storage) if Redis didn't have it
            if !loaded_from_redis {
                println!("Redis cache miss - checking PostgreSQL for {}", doc_id);

                // Parse document type
                if let Some(doc_type) = DocumentType::from_doc_id(doc_id) {
                    match self.pool.get() {
                        Ok(mut conn) => {
                            let binary_content_opt: Option<Vec<u8>> = match doc_type {
                                DocumentType::Ticket(ticket_id) => {
                                    match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
                                        Ok(article_content) if !article_content.content.is_empty() => {
                                            println!("📦 Loading from PostgreSQL: ticket {} ({} bytes base64)",
                                                    ticket_id, article_content.content.len());

                                            // Decode base64 to get binary Yjs update data
                                            use base64::{Engine as _, engine::general_purpose};
                                            match general_purpose::STANDARD.decode(&article_content.content) {
                                                Ok(binary) => {
                                                    println!("Decoded base64 to {} bytes binary", binary.len());
                                                    Some(binary)
                                                },
                                                Err(e) => {
                                                    println!("Failed to decode base64 from PostgreSQL: {:?}", e);
                                                    None
                                                }
                                            }
                                        },
                                        Ok(_) => {
                                            println!("📝 New ticket - no existing content in PostgreSQL");
                                            None
                                        },
                                        Err(e) => {
                                            println!("📝 No existing ticket content in PostgreSQL: {:?}", e);
                                            None
                                        }
                                    }
                                },
                                DocumentType::Documentation(doc_page_id) => {
                                    match repository::get_documentation_page(doc_page_id, &mut conn) {
                                        Ok(doc_page) => {
                                            if let Some(yjs_doc) = doc_page.yjs_document {
                                                if !yjs_doc.is_empty() {
                                                    println!("📦 Loading from PostgreSQL: doc page {} ({} bytes binary)",
                                                            doc_page_id, yjs_doc.len());
                                                    Some(yjs_doc)
                                                } else {
                                                    println!("📝 New documentation page - no existing Yjs content");
                                                    None
                                                }
                                            } else {
                                                println!("📝 New documentation page - no existing Yjs content");
                                                None
                                            }
                                        },
                                        Err(e) => {
                                            println!("📝 No existing documentation page in PostgreSQL: {:?}", e);
                                            None
                                        }
                                    }
                                }
                            };

                            // If we got binary content, apply it to the document
                            if let Some(binary_content) = binary_content_opt {
                                if let Ok(update) = Update::decode_v1(&binary_content) {
                                    let apply_result = {
                                        let mut txn = awareness.doc_mut().transact_mut();
                                        txn.apply_update(update)
                                    };

                                    if let Err(e) = apply_result {
                                        println!("❌ Error applying PostgreSQL state: {:?}", e);
                                    } else {
                                        println!("✅ Successfully loaded content from PostgreSQL");

                                        // IMPORTANT: Cache in Redis for future restarts
                                        self.redis_cache.set_document(doc_id, &binary_content).await;

                                        // Diagnostic: Check what's actually in the document
                                        use yrs::{GetString, XmlFragment};
                                        let txn = awareness.doc().transact();
                                        if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                                            let child_count = fragment.children(&txn).count();
                                            println!("📄 PostgreSQL content: {} children", child_count);
                                            let content_str = fragment.get_string(&txn);
                                            println!("Fragment preview: {}",
                                                if content_str.len() > 200 { &content_str[..200] } else { &content_str });
                                        } else {
                                            println!("⚠️ WARNING: 'prosemirror' fragment not found after PostgreSQL load!");
                                        }
                                    }
                                } else {
                                    println!("Failed to decode Yjs update from PostgreSQL");
                                }
                            }
                        },
                        Err(e) => {
                            println!("❌ Database connection error: {:?}", e);
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

    // Track contributor for version history
    async fn add_contributor(&self, doc_id: &str, user_uuid: Uuid) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.add_contributor(user_uuid);
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
        // Parse document type
        let doc_type = match DocumentType::from_doc_id(doc_id) {
            Some(dt) => dt,
            None => {
                println!("⚠️ Cannot save - invalid document ID format: {}", doc_id);
                return;
            }
        };

        // Get binary content from the document
        let binary_content = {
            let doc = awareness.doc();
            let txn = doc.transact();

            // DIAGNOSTIC: Check what's actually in the document before saving
            use yrs::{GetString, XmlFragment};
            if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                let child_count = fragment.children(&txn).count();
                let content_str = fragment.get_string(&txn);
                println!("💾 BEFORE SAVE - {}: Fragment has {} children, content preview: {}",
                    doc_id, child_count,
                    if content_str.len() > 100 { &content_str[..100] } else { &content_str });

                // Also log the state vector to see what client IDs we have
                let state_vec = txn.state_vector();
                println!("💾 BEFORE SAVE - State vector: {:?}", state_vec);
            } else {
                println!("⚠️ BEFORE SAVE - {}: NO 'prosemirror' fragment found in document!", doc_id);
            }

            txn.encode_state_as_update_v1(&StateVector::default())
        };

        println!("Saving document content for {} ({} bytes)", doc_id, binary_content.len());

        // CRITICAL: Save to Redis first (hot cache - survives restarts)
        // This ensures the latest state is always in Redis for fast recovery
        let redis_cache = self.redis_cache.clone();
        let doc_id_clone = doc_id.to_string();
        let content_for_redis = binary_content.clone();
        actix::spawn(async move {
            redis_cache.set_document(&doc_id_clone, &content_for_redis).await;
            // Also refresh TTL to keep active documents cached longer
            redis_cache.refresh_ttl(&doc_id_clone).await;
        });

        // Save to database in a separate thread (cold storage - permanent backup)
        let pool = self.pool.clone();
        let content = binary_content.clone(); // Already Vec<u8>

        match doc_type {
            DocumentType::Ticket(ticket_id) => {
                // Use actix to spawn a blocking operation
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            let new_content = NewArticleContent {
                                ticket_id,
                                content: general_purpose::STANDARD.encode(&content), // Convert binary to base64 string for storage
                            };

                            match repository::update_article_content(&mut conn, ticket_id, new_content) {
                                Ok(_) => println!("✅ Successfully saved document for ticket {}", ticket_id),
                                Err(e) => println!("❌ Failed to save document for ticket {}: {:?}", ticket_id, e),
                            }
                        },
                        Err(e) => println!("❌ Database connection error when saving document: {:?}", e),
                    }
                });
            },
            DocumentType::Documentation(doc_page_id) => {
                // Save documentation page Yjs state
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            // Update only the Yjs-related fields
                            match repository::update_documentation_yjs_state(&mut conn, doc_page_id, content) {
                                Ok(_) => println!("✅ Successfully saved Yjs state for documentation page {}", doc_page_id),
                                Err(e) => println!("❌ Failed to save Yjs state for documentation page {}: {:?}", doc_page_id, e),
                            }
                        },
                        Err(e) => println!("❌ Database connection error when saving documentation: {:?}", e),
                    }
                });
            }
        }
    }

    // Create a snapshot revision for version history using native Yrs encoding
    fn create_snapshot_revision(&self, doc_id: &str, awareness: &Awareness, contributors: HashSet<Uuid>) {
        // Parse document type
        let doc_type = match DocumentType::from_doc_id(doc_id) {
            Some(dt) => dt,
            None => {
                println!("⚠️ Skipping snapshot - invalid document ID format: {}", doc_id);
                return;
            }
        };

        // Encode document state using native Yrs functions (DRY - no manual serialization!)
        let (state_vector_bytes, full_update_bytes) = {
            let doc = awareness.doc();
            let txn = doc.transact();

            // Use Yrs native encoding - no manual snapshot struct encoding needed
            let state_vector = txn.state_vector();
            let full_update = txn.encode_state_as_update_v1(&StateVector::default());

            (state_vector.encode_v1(), full_update)
        };

        println!("📸 Creating snapshot for {}: {} bytes",
            doc_id, full_update_bytes.len());

        // Save to database asynchronously
        let pool = self.pool.clone();
        let contributor_vec: Vec<Option<Uuid>> = contributors.into_iter().map(Some).collect();

        match doc_type {
            DocumentType::Ticket(ticket_id) => {
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            // Get or create article_content record
                            let article_content = match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
                                Ok(ac) => ac,
                                Err(_) => {
                                    // Create if doesn't exist
                                    let new_content = NewArticleContent {
                                        ticket_id,
                                        content: String::new(), // Placeholder
                                    };
                                    match repository::create_article_content(&mut conn, new_content) {
                                        Ok(ac) => ac,
                                        Err(e) => {
                                            println!("❌ Failed to create article_content for snapshot: {:?}", e);
                                            return;
                                        }
                                    }
                                }
                            };

                            // Create new revision with simplified schema (no redundant snapshot field!)
                            let new_revision = NewArticleContentRevision {
                                article_content_id: article_content.id,
                                revision_number: article_content.current_revision_number,
                                yjs_state_vector: state_vector_bytes,
                                yjs_document_content: full_update_bytes,
                                contributed_by: contributor_vec.clone(),
                            };

                            match repository::create_article_content_revision(&mut conn, new_revision) {
                                Ok(revision) => {
                                    // Increment revision number in article_content
                                    match repository::increment_article_content_revision(&mut conn, article_content.id) {
                                        Ok(_) => {
                                            println!("✅ Snapshot created: ticket {} revision {} ({} contributors)",
                                                ticket_id, revision.revision_number, contributor_vec.len());
                                        },
                                        Err(e) => println!("❌ Failed to increment revision number: {:?}", e),
                                    }
                                },
                                Err(e) => println!("❌ Failed to create revision: {:?}", e),
                            }
                        },
                        Err(e) => println!("❌ Database connection error during snapshot: {:?}", e),
                    }
                });
            },
            DocumentType::Documentation(doc_page_id) => {
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            // Create documentation revision snapshot
                            match repository::create_documentation_revision(&mut conn, doc_page_id, state_vector_bytes, full_update_bytes, contributor_vec.clone()) {
                                Ok(revision_number) => {
                                    println!("✅ Snapshot created: documentation page {} revision {} ({} contributors)",
                                        doc_page_id, revision_number, contributor_vec.len());
                                },
                                Err(e) => println!("❌ Failed to create documentation revision: {:?}", e),
                            }
                        },
                        Err(e) => println!("❌ Database connection error during snapshot: {:?}", e),
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
    user_uuid: Uuid, // User UUID for contributor tracking
    // Statistics for debugging
    messages_received: u32,
    pings_sent: u32,
    pongs_received: u32,
    started_at: Instant,
}

impl YjsWebSocket {
    fn new(doc_id: String, app_state: YjsAppState, user_uuid: Uuid) -> Self {
        let id = Uuid::new_v4().to_string();
        let now = Instant::now();

        YjsWebSocket {
            id,
            doc_id,
            app_state,
            hb: now,
            protocol: DefaultProtocol,
            user_uuid,
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

            // Send WebSocket PING to verify connection health
            // Note: y-websocket client handles its own keepalive via resyncInterval
            // This PING is for detecting dead connections at the WebSocket protocol level
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
        let user_uuid = self.user_uuid; // Capture for contributor tracking

        // Spawn async work
        let addr = ctx.address();
        actix::spawn(async move {
            // Update session activity
            app_state.update_session_activity(&doc_id, &session_id).await;

            // Get the awareness for this document
            let awareness = app_state.get_or_create_awareness(&doc_id).await;

            // DIAGNOSTIC: Check content BEFORE processing message
            let content_before = {
                use yrs::{GetString, XmlFragment};
                let txn = awareness.doc().transact();
                if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                    fragment.get_string(&txn)
                } else {
                    String::from("(no fragment)")
                }
            };

            // Use the built-in protocol handler to process the message
            // DefaultProtocol is stateless, so we can create a new instance
            let protocol = DefaultProtocol;

            // DIAGNOSTIC: Log incoming message details
            let msg_type = if msg_vec.is_empty() { 255 } else { msg_vec[0] };
            println!("🔍 Processing message: type={}, size={} bytes", msg_type, msg_vec.len());

            // DIAGNOSTIC: Decode sync messages to see what they contain
            if msg_type == 0 && msg_vec.len() > 1 {
                let sync_step = msg_vec[1];
                match sync_step {
                    0 => println!("   📍 SYNC_STEP_1 (state vector request)"),
                    1 => println!("   📍 SYNC_STEP_2 (state response)"),
                    2 => {
                        println!("   📍 SYNC_UPDATE (incremental change)");
                        // Get the doc's current state vector to compare with incoming update
                        let doc_state_vec = {
                            let txn = awareness.doc().transact();
                            txn.state_vector()
                        };
                        println!("      Backend state vector: {:?}", doc_state_vec);

                        // Try to decode the update to see if it's valid
                        if msg_vec.len() > 2 {
                            match Update::decode_v1(&msg_vec[2..]) {
                                Ok(_update) => println!("      ✓ Decoded SYNC_UPDATE successfully"),
                                Err(e) => println!("      ✗ Failed to decode SYNC_UPDATE: {:?}", e),
                            }
                        }
                    },
                    _ => println!("   📍 Unknown sync step: {}", sync_step),
                }
            }

            match protocol.handle(&awareness, &msg_vec) {
                Ok(messages) => {
                    println!("✅ protocol.handle() succeeded, generated {} response message(s)", messages.len());

                    // DIAGNOSTIC: Check content AFTER processing message
                    let content_after = {
                        use yrs::{GetString, XmlFragment};
                        let txn = awareness.doc().transact();
                        if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                            fragment.get_string(&txn)
                        } else {
                            String::from("(no fragment)")
                        }
                    };

                    let content_changed = content_before != content_after;
                    if content_changed {
                        println!("📝 Content changed! Before: '{}' → After: '{}'",
                            if content_before.len() > 50 { &content_before[..50] } else { &content_before },
                            if content_after.len() > 50 { &content_after[..50] } else { &content_after });
                    } else {
                        println!("⚠️ Content UNCHANGED after processing message (still: '{}')",
                            if content_after.len() > 30 { &content_after[..30] } else { &content_after });

                        // WORKAROUND: If SYNC_UPDATE failed to apply, request the frontend to send
                        // its full state by sending a SYNC_STEP_1 (state vector request)
                        if msg_type == 0 && msg_vec.len() > 1 && msg_vec[1] == 2 {
                            println!("🔄 SYNC_UPDATE failed to apply changes - requesting client's full state");
                            use yrs::sync::Message;
                            // Send empty state vector to request full state from client
                            let sync_message = Message::Sync(yrs::sync::SyncMessage::SyncStep1(StateVector::default()));
                            let encoded = sync_message.encode_v1();
                            addr.do_send(YjsMessage(Bytes::from(encoded)));
                            println!("📤 Sent SYNC_STEP_1 request to client - expecting full state in response");
                        }
                    }

                    // Send any response messages back to the client
                    for message in messages {
                        let encoded = message.encode_v1();
                        addr.do_send(YjsMessage(Bytes::from(encoded)));
                    }

                    // Broadcast the entire message to other clients
                    app_state.broadcast(&doc_id, &session_id, &msg_vec).await;

                    // Mark document as changed after sync updates (even if failed)
                    // This ensures the backend saves whatever state it has
                    if is_sync_message || content_changed {
                        app_state.mark_document_changed(&doc_id).await;
                    }

                    // Track contributor for version history if this is a SYNC_UPDATE
                    // Only SYNC_UPDATE messages (step 2) represent actual user edits
                    if msg_type == 0 && msg_vec.len() > 1 && msg_vec[1] == 2 {
                        app_state.add_contributor(&doc_id, user_uuid).await;
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
    
    // Extract and validate JWT token from httpOnly cookie
    

    let token = req.cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No authentication cookie"))?;

    // Validate the token and extract user UUID
    let user_uuid = if let Some(pool) = req.app_data::<web::Data<crate::db::Pool>>() {
        let mut conn = pool.get()
            .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;

        // Use our centralized JWT validation
        use crate::utils::jwt::JwtUtils;

        match JwtUtils::validate_token_with_user_check(token.value(), &mut conn).await {
            Ok((_claims, user)) => user.uuid,
            Err(_) => return Err(actix_web::error::ErrorUnauthorized("Invalid or expired token")),
        }
    } else {
        return Err(actix_web::error::ErrorInternalServerError("Database pool not available"));
    };

    println!("WebSocket authentication successful for document: {} (user: {})", doc_id, user_uuid);
    let actor = YjsWebSocket::new(doc_id, app_state.get_ref().clone(), user_uuid);
    ws::start(actor, &req, stream)
}

// ============= Revision History API Endpoints =============

/// GET /tickets/:id/revisions - List all revisions for a ticket
pub async fn get_ticket_revisions(
    ticket_id: web::Path<i32>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let ticket_id = ticket_id.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get article content for this ticket
    let article_content = match crate::repository::article_content::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(content) => content,
        Err(_) => return HttpResponse::NotFound().json("No article content found for this ticket"),
    };

    // Get all revisions
    match crate::repository::article_content::get_article_content_revisions(&mut conn, article_content.id) {
        Ok(revisions) => {
            let responses: Vec<crate::models::ArticleContentRevisionResponse> = revisions
                .into_iter()
                .map(Into::into)
                .collect();
            HttpResponse::Ok().json(responses)
        },
        Err(_) => HttpResponse::InternalServerError().json("Error retrieving revisions"),
    }
}

/// GET /tickets/:id/revisions/:revision_number - Get a specific revision
pub async fn get_ticket_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let (ticket_id, revision_number) = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get article content for this ticket
    let article_content = match crate::repository::article_content::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(content) => content,
        Err(_) => return HttpResponse::NotFound().json("No article content found for this ticket"),
    };

    // Get the specific revision
    match crate::repository::article_content::get_article_content_revision(&mut conn, article_content.id, revision_number) {
        Ok(revision) => {
            // Encode the Yjs document content as base64 for frontend
            let content_base64 = general_purpose::STANDARD.encode(&revision.yjs_document_content);

            HttpResponse::Ok().json(serde_json::json!({
                "id": revision.id,
                "article_content_id": revision.article_content_id,
                "revision_number": revision.revision_number,
                "yjs_document_content": content_base64,
                "contributed_by": revision.contributed_by,
                "created_at": revision.created_at,
            }))
        },
        Err(_) => HttpResponse::NotFound().json("Revision not found"),
    }
}

/// POST /tickets/:id/restore/:revision_number - Restore ticket to a specific revision
pub async fn restore_ticket_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
    app_state: web::Data<YjsAppState>,
) -> HttpResponse {
    let (ticket_id, revision_number) = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get article content for this ticket
    let article_content = match crate::repository::article_content::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(content) => content,
        Err(_) => return HttpResponse::NotFound().json("No article content found for this ticket"),
    };

    // Get the revision to restore
    let revision = match crate::repository::article_content::get_article_content_revision(&mut conn, article_content.id, revision_number) {
        Ok(rev) => rev,
        Err(_) => return HttpResponse::NotFound().json("Revision not found"),
    };

    // Get the in-memory document for this ticket (if it exists)
    let doc_id = format!("ticket-{}", ticket_id);
    let awareness = app_state.get_or_create_awareness(&doc_id).await;

    // Apply the revision content to the document
    let doc = awareness.doc();

    // Decode the stored Yjs update
    use yrs::updates::decoder::Decode;
    let update = match Update::decode_v1(&revision.yjs_document_content) {
        Ok(upd) => upd,
        Err(e) => {
            println!("Error decoding revision update: {:?}", e);
            return HttpResponse::InternalServerError().json("Error decoding revision");
        }
    };

    // Clear the document and apply the revision
    // Note: This is a destructive operation that replaces the entire document state
    // We create a new document with the revision content
    {
        let mut txn = doc.transact_mut();
        if let Err(e) = txn.apply_update(update) {
            println!("Error applying revision update: {:?}", e);
            return HttpResponse::InternalServerError().json("Error applying revision");
        }
    }

    // Mark document as changed to trigger save
    app_state.mark_document_changed(&doc_id).await;

    // Broadcast the change to all connected clients
    // Encode the full document state
    let full_state = {
        let txn = doc.transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    };

    // Broadcast to all sessions
    use yrs::sync::Message;
    let sync_message = Message::Sync(yrs::sync::SyncMessage::Update(full_state.into()));
    let encoded = sync_message.encode_v1();
    app_state.broadcast(&doc_id, "", &encoded).await;

    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": format!("Restored to revision {}", revision_number),
    }))
}

// ============= Documentation Revision History API Endpoints =============

/// GET /docs/:id/revisions - List all revisions for a documentation page
pub async fn get_doc_revisions(
    doc_id: web::Path<i32>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let doc_id = doc_id.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get all revisions
    match crate::repository::documentation::get_documentation_revisions(&mut conn, doc_id) {
        Ok(revisions) => {
            HttpResponse::Ok().json(revisions)
        },
        Err(_) => HttpResponse::InternalServerError().json("Error retrieving revisions"),
    }
}

/// GET /docs/:id/revisions/:revision_number - Get a specific revision
pub async fn get_doc_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let (doc_id, revision_number) = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get the specific revision
    match crate::repository::documentation::get_documentation_revision(&mut conn, doc_id, revision_number) {
        Ok(revision) => {
            // Encode the Yjs document snapshot as base64 for frontend
            let content_base64 = general_purpose::STANDARD.encode(&revision.yjs_document_snapshot);

            HttpResponse::Ok().json(serde_json::json!({
                "id": revision.id,
                "page_id": revision.page_id,
                "revision_number": revision.revision_number,
                "title": revision.title,
                "yjs_document_content": content_base64,
                "created_by": revision.created_by,
                "created_at": revision.created_at,
                "change_summary": revision.change_summary,
            }))
        },
        Err(_) => HttpResponse::NotFound().json("Revision not found"),
    }
}

/// POST /docs/:id/restore/:revision_number - Restore documentation page to a specific revision
pub async fn restore_doc_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
    app_state: web::Data<YjsAppState>,
) -> HttpResponse {
    let (doc_id, revision_number) = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get the revision to restore
    let revision = match crate::repository::documentation::get_documentation_revision(&mut conn, doc_id, revision_number) {
        Ok(rev) => rev,
        Err(_) => return HttpResponse::NotFound().json("Revision not found"),
    };

    // Get the in-memory document for this documentation page
    let doc_id_str = format!("doc-{}", doc_id);
    let awareness = app_state.get_or_create_awareness(&doc_id_str).await;

    // Apply the revision content to the document
    let doc = awareness.doc();

    // Decode the stored Yjs update
    use yrs::updates::decoder::Decode;
    let update = match Update::decode_v1(&revision.yjs_document_snapshot) {
        Ok(upd) => upd,
        Err(e) => {
            println!("Error decoding revision update: {:?}", e);
            return HttpResponse::InternalServerError().json("Error decoding revision");
        }
    };

    // Clear the document and apply the revision
    {
        let mut txn = doc.transact_mut();
        if let Err(e) = txn.apply_update(update) {
            println!("Error applying revision update: {:?}", e);
            return HttpResponse::InternalServerError().json("Error applying revision");
        }
    }

    // Mark document as changed to trigger save
    app_state.mark_document_changed(&doc_id_str).await;

    // Broadcast the change to all connected clients
    let full_state = {
        let txn = doc.transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    };

    // Broadcast to all sessions
    use yrs::sync::Message;
    let sync_message = Message::Sync(yrs::sync::SyncMessage::Update(full_state.into()));
    let encoded = sync_message.encode_v1();
    app_state.broadcast(&doc_id_str, "", &encoded).await;

    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": format!("Restored to revision {}", revision_number),
    }))
}

// Configure routes
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("")
            .route("/article/{doc_id}", web::get().to(get_article_content))
            .route("/ws/{doc_id}", web::get().to(ws_handler))
            .route("/tickets/{ticket_id}/revisions", web::get().to(get_ticket_revisions))
            .route("/tickets/{ticket_id}/revisions/{revision_number}", web::get().to(get_ticket_revision))
            .route("/tickets/{ticket_id}/restore/{revision_number}", web::post().to(restore_ticket_revision))
            .route("/docs/{doc_id}/revisions", web::get().to(get_doc_revisions))
            .route("/docs/{doc_id}/revisions/{revision_number}", web::get().to(get_doc_revision))
            .route("/docs/{doc_id}/restore/{revision_number}", web::post().to(restore_doc_revision))
    );
}