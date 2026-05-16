use actix::{Actor, ActorContext, Addr, AsyncContext, Handler, Message, Running, StreamHandler};
use actix_web::{web, Error, HttpRequest, HttpResponse, Responder};
use actix_web_actors::ws;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::panic;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;
use yrs::sync::{Awareness, DefaultProtocol, Protocol};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, ReadTxn, StateVector, Transact, Update, WriteTxn, XmlFragment};

use crate::handlers::errors;
use crate::handlers::helpers;
use crate::repository;

/// Safely get string content from a Yjs XmlFragment
/// Returns None if the fragment contains invalid UTF-8 data (which can cause yrs to panic)
fn safe_get_fragment_string(
    fragment: &yrs::XmlFragmentRef,
    txn: &yrs::Transaction,
) -> Option<String> {
    panic::catch_unwind(panic::AssertUnwindSafe(|| fragment.get_string(txn))).ok()
}

/// Get a preview of document content for logging
fn get_content_preview(awareness: &Awareness, max_chars: usize) -> String {
    let txn = awareness.doc().transact();
    if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
        // Get children count for diagnostic purposes
        let children_count = fragment.len(&txn);
        let text_content = match safe_get_fragment_string(&fragment, &txn) {
            Some(s) => s.chars().take(max_chars).collect::<String>(),
            None => "(invalid char data)".to_string(),
        };

        // Empty text with children - log structure info
        if text_content.is_empty() && children_count > 0 {
            format!("[{children_count} children, text: '']")
        } else if text_content.is_empty() {
            "[0 children]".to_string()
        } else {
            text_content
        }
    } else {
        "(no fragment)".to_string()
    }
}

/// Log all root-level types in a Yjs document for debugging
fn log_document_root_types(awareness: &Awareness, doc_id: &str) {
    let doc = awareness.doc();
    let txn = doc.transact();

    // Get all root-level type names using root_refs iterator
    let root_names: Vec<String> = txn.root_refs().map(|(name, _)| name.to_string()).collect();

    trace!(doc_id = %doc_id, root_types = ?root_names, "Root types in document");

    // Check prosemirror fragment specifically
    if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
        // XmlFragment children count using both methods
        let children_iter: usize = fragment.children(&txn).count();
        let children_len = fragment.len(&txn);
        trace!(doc_id = %doc_id, children_iter, children_len, "prosemirror XmlFragment");

        // Try to iterate and describe children (only log count, not individual items in trace)
        let child_count = fragment.children(&txn).take(6).count();
        if child_count > 5 {
            trace!(doc_id = %doc_id, child_count = "5+", "prosemirror children");
        }

        // Try get_string
        let text = fragment.get_string(&txn);
        if text.is_empty() {
            trace!(doc_id = %doc_id, "prosemirror get_string() is empty");
        } else {
            let preview: String = text.chars().take(100).collect();
            trace!(doc_id = %doc_id, preview = %preview, "prosemirror content preview");
        }
    } else {
        trace!(doc_id = %doc_id, "prosemirror fragment not found");
    }

    // Log state vector to see client contributions
    let sv = txn.state_vector();
    trace!(doc_id = %doc_id, state_vector = ?sv, "Document state vector");
}
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
// Document type enum: distinguishes ticket articles, doc pages,
// and collection descriptions. The collection variant binds to
// the new `documentation_collections.description_yjs` column —
// collections own their overview content directly instead of
// pointing at a sentinel "main page".
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocumentType {
    Ticket(i32),
    Documentation(i32),
    Collection(i32),
}

impl DocumentType {
    /// Parse doc_id format: "ticket-N", "doc-N", or "collection-N".
    fn from_doc_id(doc_id: &str) -> Option<Self> {
        if let Some(id_str) = doc_id.strip_prefix("ticket-") {
            id_str.parse::<i32>().ok().map(DocumentType::Ticket)
        } else if let Some(id_str) = doc_id.strip_prefix("doc-") {
            id_str.parse::<i32>().ok().map(DocumentType::Documentation)
        } else if let Some(id_str) = doc_id.strip_prefix("collection-") {
            id_str.parse::<i32>().ok().map(DocumentType::Collection)
        } else {
            None
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
            warn!(doc_id = %clean_doc_id, "Invalid document ID format");
            return errors::bad_request(
                "Invalid document ID format (expected 'ticket-N' or 'doc-N')",
            );
        }
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match doc_type {
        DocumentType::Ticket(ticket_id) => {
            // Load Yjs document snapshot from article_contents table (snapshot-based persistence)
            match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
                Ok(article_content) => {
                    debug!(ticket_id, "Retrieved article content");

                    // If yjs_document snapshot exists, encode as base64, otherwise return empty
                    let content_base64 = if let Some(yjs_doc) = article_content.yjs_document {
                        if !yjs_doc.is_empty() {
                            debug!(
                                ticket_id,
                                bytes = yjs_doc.len(),
                                "Loading snapshot from PostgreSQL"
                            );
                            general_purpose::STANDARD.encode(&yjs_doc)
                        } else {
                            debug!(ticket_id, "Empty Yjs document");
                            String::new()
                        }
                    } else {
                        debug!(ticket_id, "No Yjs document snapshot");
                        String::new()
                    };

                    HttpResponse::Ok().json(json!({
                        "content": content_base64,
                        "ticket_id": ticket_id
                    }))
                }
                Err(e) => {
                    debug!(ticket_id, error = ?e, "No article content found");
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "ticket_id": ticket_id
                    }))
                }
            }
        }
        DocumentType::Documentation(doc_id) => {
            match repository::get_documentation_page(doc_id, &mut conn) {
                Ok(doc_page) => {
                    debug!(doc_id, "Retrieved documentation page");

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
                }
                Err(e) => {
                    debug!(doc_id, error = %e, "No documentation page found");
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "doc_id": doc_id
                    }))
                }
            }
        }
        DocumentType::Collection(collection_id) => {
            match repository::documentation_collections::get_collection(&mut conn, collection_id) {
                Ok(c) => {
                    let content_base64 = c
                        .description_yjs
                        .as_ref()
                        .map(|d| general_purpose::STANDARD.encode(d))
                        .unwrap_or_default();
                    HttpResponse::Ok().json(json!({
                        "content": content_base64,
                        "collection_id": collection_id
                    }))
                }
                Err(e) => {
                    debug!(collection_id, error = %e, "No collection found");
                    HttpResponse::Ok().json(json!({
                        "content": "",
                        "collection_id": collection_id
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
    final_save_completed: bool,        // Track if final save was done
    // Snapshot tracking (for version history)
    update_counter: u32,   // Total updates since document creation
    last_snapshot_at: u32, // Update count when last snapshot created
    contributors: std::collections::HashSet<Uuid>, // Contributors since last snapshot (only added on actual content changes)
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
                                  // Note: has_changes_since_last_revision is set separately only when content actually changes

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
        // Only do final save if room has been empty for a bit, changes exist, and final save not yet done
        if let Some(empty_since) = self.room_empty_since {
            let now = Instant::now();
            return !self.final_save_completed
                && (self.has_pending_changes
                    || now.duration_since(empty_since) < Duration::from_secs(5))
                && now.duration_since(empty_since) >= EMPTY_ROOM_FINAL_SAVE_DELAY;
        }
        false
    }

    // Snapshot management methods
    fn should_create_snapshot(&self) -> bool {
        // Session-based revisions: snapshots are only created when editing sessions end
        // (when room becomes empty), not based on update count thresholds.
        // This provides more meaningful revision history based on actual editing sessions.
        false
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

/// Per-session bookkeeping kept on the collaboration side. The
/// user identity + ticket presence live in
/// `services::presence::PresenceRegistry`; this map only carries
/// what the transport needs (the actor address and the last
/// activity Instant for stale-cleanup).
struct SessionInfo {
    addr: Addr<YjsWebSocket>,
    last_active: Instant,
    /// User who owns this session. Forwarded to the presence
    /// registry on add / remove so the registry can deduplicate
    /// multi-tab.
    user_uuid: Uuid,
}

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
    sse_state: web::Data<crate::handlers::sse::SseState>,
    /// Search service handle so the periodic + on-disconnect Yjs
    /// saves can fire the indexing observers and keep the search
    /// index in sync with body edits typed into the collaborative
    /// editor. Without this every Yjs save would leave the index
    /// stale until a metadata change triggered a reindex.
    search_service: Arc<crate::services::search::SearchService>,
    /// Per-(user, ticket) presence state. Single source of truth
    /// for the avatar stack on the ticket detail page; gates the
    /// per-user "appear away" preference via its visibility
    /// resolver.
    presence: Arc<crate::services::presence::PresenceRegistry>,
}

impl YjsAppState {
    pub fn new(
        pool: web::Data<crate::db::Pool>,
        redis_cache: Arc<RedisYjsCache>,
        sse_state: web::Data<crate::handlers::sse::SseState>,
        search_service: Arc<crate::services::search::SearchService>,
    ) -> Self {
        let state = YjsAppState {
            documents: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pool,
            redis_cache,
            sse_state,
            search_service,
            presence: Arc::new(
                crate::services::presence::PresenceRegistry::with_default_resolver(),
            ),
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
                debug!(doc_id = %doc_id, "Saving document with pending changes");
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
                saved_count += 1;
            }

            // Check for snapshot creation (every 500 updates)
            if doc_state.should_create_snapshot() {
                debug!(doc_id = %doc_id, updates_since_snapshot = doc_state.update_counter - doc_state.last_snapshot_at,
                    "Snapshot threshold reached");

                // Clone contributors before passing to async function
                let contributors = doc_state.contributors.clone();
                self.create_snapshot_revision(doc_id, &doc_state.awareness, contributors);
                doc_state.reset_snapshot_tracking();
                snapshot_count += 1;
            }

            // Final save for empty rooms
            if doc_state.should_do_final_save() {
                debug!(doc_id = %doc_id, "Performing final save for empty room");
                self.save_document_internal(doc_id, &doc_state.awareness);
                doc_state.mark_saved();
                doc_state.mark_final_save_completed();
                final_saved_count += 1;

                // Create revision at end of editing session if there were content changes
                if !doc_state.contributors.is_empty() {
                    debug!(doc_id = %doc_id, "Creating session-end revision");
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
            info!(
                saves = saved_count,
                final_saves = final_saved_count,
                snapshots = snapshot_count,
                "Periodic maintenance completed"
            );
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
            debug!(doc_id = %doc_id, "Document not in memory - checking Redis cache");

            // Create Doc with GC disabled and a consistent server-side client ID
            // CRITICAL: Use a deterministic client ID based on the document ID to ensure
            // consistency across backend restarts. This prevents state vector mismatches.
            let mut options = yrs::Options::default();
            options.skip_gc = true; // CRITICAL: Disable garbage collection

            // Generate a consistent client ID from the document ID hash
            // This ensures the same document always gets the same server client ID
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            doc_id.hash(&mut hasher);
            let client_id = hasher.finish() | 1; // Ensure it's non-zero

            options.client_id = client_id;
            debug!(doc_id = %doc_id, client_id, "Creating document with consistent client ID");

            let doc = Doc::with_options(options);

            // CRITICAL: Initialize the "prosemirror" XmlFragment root type BEFORE creating Awareness
            // This MUST be done before any sync operations to ensure the backend and frontend
            // are working with the same document structure. The yrs documentation says:
            // "It's highly recommended for all collaborating clients to define all root level types
            // they are going to use up front, during document creation."
            // When data is loaded later via apply_update(), it will be merged into this structure.
            {
                let mut txn = doc.transact_mut();
                let _ = txn.get_or_insert_xml_fragment("prosemirror");
                debug!(doc_id = %doc_id, "Pre-initialized 'prosemirror' XmlFragment");
            }

            let mut awareness = Awareness::new(doc);

            let mut loaded_from_redis = false;
            let mut loaded_from_postgres = false;

            // STEP 1: Try to load from Redis (hot cache - survives restarts)
            if let Some(redis_data) = self.redis_cache.get_document(doc_id).await {
                debug!(doc_id = %doc_id, bytes = redis_data.len(), "Attempting to load document from Redis");

                if let Ok(update) = Update::decode_v1(&redis_data) {
                    let apply_result = {
                        let mut txn = awareness.doc_mut().transact_mut();
                        txn.apply_update(update)
                    };

                    if let Err(e) = apply_result {
                        error!(doc_id = %doc_id, error = ?e, "Error applying Redis state");
                        // Delete corrupted entry from Redis
                        warn!(doc_id = %doc_id, "Deleting corrupted Redis entry");
                        self.redis_cache.delete_document(doc_id).await;
                    } else {
                        debug!(doc_id = %doc_id, "Successfully loaded document from Redis cache");
                        loaded_from_redis = true;

                        // Diagnostic: Verify content
                        let preview = get_content_preview(&awareness, 50);
                        trace!(doc_id = %doc_id, preview = %preview, "Redis content loaded");
                        log_document_root_types(&awareness, doc_id);
                    }
                } else {
                    warn!(doc_id = %doc_id, "Failed to decode Redis data - deleting corrupted entry");
                    // Delete corrupted entry from Redis so it doesn't block future loads
                    self.redis_cache.delete_document(doc_id).await;
                }
            }

            // STEP 2: Fall back to PostgreSQL (cold storage) if Redis didn't have it
            if !loaded_from_redis {
                debug!(doc_id = %doc_id, "Redis cache miss - checking PostgreSQL");

                // Parse document type
                if let Some(doc_type) = DocumentType::from_doc_id(doc_id) {
                    trace!(doc_id = %doc_id, "Parsed doc_type successfully");
                    match self.pool.get() {
                        Ok(mut conn) => {
                            // PHASE 2: Load from PostgreSQL
                            match doc_type {
                                DocumentType::Ticket(ticket_id) => {
                                    // Load Yjs document snapshot from article_contents table (snapshot-based persistence)
                                    match repository::get_article_content_by_ticket_id(
                                        &mut conn, ticket_id,
                                    ) {
                                        Ok(article_content) => {
                                            if let Some(yjs_doc) = article_content.yjs_document {
                                                if !yjs_doc.is_empty() {
                                                    debug!(
                                                        ticket_id,
                                                        bytes = yjs_doc.len(),
                                                        "Loading snapshot from PostgreSQL"
                                                    );

                                                    if let Ok(update) = Update::decode_v1(&yjs_doc)
                                                    {
                                                        let apply_result = {
                                                            let mut txn =
                                                                awareness.doc_mut().transact_mut();
                                                            txn.apply_update(update)
                                                        };

                                                        if let Err(e) = apply_result {
                                                            error!(ticket_id, error = ?e, "Error applying PostgreSQL snapshot");
                                                        } else {
                                                            debug!(ticket_id, "Successfully loaded snapshot from PostgreSQL");
                                                            loaded_from_postgres = true;

                                                            // Cache in Redis for future fast access
                                                            self.redis_cache
                                                                .set_document(doc_id, &yjs_doc)
                                                                .await;

                                                            // Diagnostic: Check content
                                                            let preview = get_content_preview(
                                                                &awareness, 100,
                                                            );
                                                            trace!(ticket_id, preview = %preview, "PostgreSQL content loaded");
                                                            log_document_root_types(
                                                                &awareness, doc_id,
                                                            );
                                                        }
                                                    } else {
                                                        error!(
                                                            ticket_id,
                                                            "Failed to decode PostgreSQL snapshot"
                                                        );
                                                    }
                                                } else {
                                                    debug!(ticket_id, "Empty Yjs document");
                                                }
                                            } else {
                                                debug!(ticket_id, "No Yjs document snapshot");
                                            }
                                        }
                                        Err(e) => {
                                            debug!(ticket_id, error = ?e, "No article content found");
                                        }
                                    }
                                }
                                DocumentType::Documentation(doc_page_id) => {
                                    // Load Yjs document snapshot from documentation_pages table (snapshot-based persistence)
                                    match repository::get_documentation_page(doc_page_id, &mut conn)
                                    {
                                        Ok(doc_page) => {
                                            if let Some(yjs_doc) = doc_page.yjs_document {
                                                if !yjs_doc.is_empty() {
                                                    debug!(
                                                        doc_page_id,
                                                        bytes = yjs_doc.len(),
                                                        "Loading from PostgreSQL"
                                                    );

                                                    if let Ok(update) = Update::decode_v1(&yjs_doc)
                                                    {
                                                        let apply_result = {
                                                            let mut txn =
                                                                awareness.doc_mut().transact_mut();
                                                            txn.apply_update(update)
                                                        };

                                                        if let Err(e) = apply_result {
                                                            error!(doc_page_id, error = ?e, "Error applying PostgreSQL state");
                                                        } else {
                                                            debug!(doc_page_id, "Successfully loaded documentation from PostgreSQL");
                                                            loaded_from_postgres = true;

                                                            // Cache in Redis
                                                            self.redis_cache
                                                                .set_document(doc_id, &yjs_doc)
                                                                .await;

                                                            // Diagnostic: Check what's actually in the document
                                                            let preview = get_content_preview(
                                                                &awareness, 100,
                                                            );
                                                            trace!(doc_page_id, preview = %preview, "PostgreSQL content loaded");
                                                        }
                                                    } else {
                                                        error!(doc_page_id, "Failed to decode Yjs update from PostgreSQL");
                                                    }
                                                } else {
                                                    debug!(doc_page_id, "New documentation page - no existing Yjs content");
                                                }
                                            } else {
                                                debug!(doc_page_id, "New documentation page - no existing Yjs content");
                                            }
                                        }
                                        Err(e) => {
                                            debug!(doc_page_id, error = ?e, "No existing documentation page in PostgreSQL");
                                        }
                                    }
                                }
                                DocumentType::Collection(collection_id) => {
                                    // Load Yjs snapshot from documentation_collections.description_yjs.
                                    match repository::documentation_collections::get_collection(
                                        &mut conn,
                                        collection_id,
                                    ) {
                                        Ok(c) => {
                                            if let Some(yjs_doc) = c.description_yjs {
                                                if !yjs_doc.is_empty() {
                                                    debug!(collection_id, bytes = yjs_doc.len(), "Loading collection description from PostgreSQL");
                                                    if let Ok(update) = Update::decode_v1(&yjs_doc)
                                                    {
                                                        let apply_result = {
                                                            let mut txn =
                                                                awareness.doc_mut().transact_mut();
                                                            txn.apply_update(update)
                                                        };
                                                        if let Err(e) = apply_result {
                                                            error!(collection_id, error = ?e, "Error applying collection description state");
                                                        } else {
                                                            loaded_from_postgres = true;
                                                            self.redis_cache
                                                                .set_document(doc_id, &yjs_doc)
                                                                .await;
                                                        }
                                                    } else {
                                                        error!(collection_id, "Failed to decode Yjs update for collection");
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!(collection_id, error = ?e, "No collection in PostgreSQL");
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!(doc_id = %doc_id, error = ?e, "Database connection error");
                        }
                    }
                } else {
                    warn!(doc_id = %doc_id, "Could not parse doc_id format (expected 'ticket-N' or 'doc-N')");
                }
            }

            // For NEW documents only (no existing data), initialize the prosemirror XmlFragment
            // This ensures new documents have the proper root type structure for ProseMirror
            if !loaded_from_redis && !loaded_from_postgres {
                let mut txn = awareness.doc_mut().transact_mut();
                let _ = txn.get_or_insert_xml_fragment("prosemirror");
                debug!(doc_id = %doc_id, "Initialized 'prosemirror' XmlFragment for NEW document");
            }

            // Log final state after loading attempts
            let preview = get_content_preview(&awareness, 100);
            if loaded_from_redis || loaded_from_postgres {
                debug!(doc_id = %doc_id, preview = %preview, "Document loaded");
                log_document_root_types(&awareness, doc_id);
            } else {
                debug!(doc_id = %doc_id, preview = %preview, "New document created");
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

    /// Replace the document with a new one (used for restoring revisions)
    /// This creates a new Awareness with the new Doc and replaces the existing one
    async fn replace_document(&self, doc_id: &str, new_doc: Doc) {
        let mut documents = self.documents.write().await;

        // Create new Awareness with the new Doc
        let awareness = Awareness::new(new_doc);

        // Initialize awareness with basic server info
        let local_state = r#"{"server": true, "name": "Server"}"#;
        let _ = awareness.set_local_state(local_state);

        let awareness = Arc::new(awareness);

        if let Some(doc_state) = documents.get_mut(doc_id) {
            // Replace the awareness with the new one
            doc_state.awareness = Arc::clone(&awareness);
            doc_state.mark_changed();
            info!(doc_id = %doc_id, "Replaced document with restored revision");
        } else {
            // Document doesn't exist in memory, create it
            let doc_state = DocumentState::new(Arc::clone(&awareness));
            documents.insert(doc_id.to_string(), doc_state);
            info!(doc_id = %doc_id, "Created new document from restored revision");
        }
    }

    // Track contributor for version history
    async fn add_contributor(&self, doc_id: &str, user_uuid: Uuid) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.add_contributor(user_uuid);
        }
    }

    /// Emit a `ViewersChanged` SSE event for one ticket with the
    /// current viewer set from the presence registry. Visibility
    /// resolution is applied inside the registry, so any "appear
    /// away" hidden users are already filtered.
    async fn emit_viewers_changed(&self, ticket_id: i32) {
        let viewers = self.presence.viewers_on_ticket(ticket_id);
        self.sse_state
            .broadcast_event(crate::handlers::sse::SseEvent::ViewersChanged {
                ticket_id,
                viewers,
                timestamp: chrono::Utc::now(),
            })
            .await;
    }

    // Register session
    async fn register_session(
        &self,
        doc_id: &str,
        session_id: &str,
        addr: Addr<YjsWebSocket>,
        user_uuid: Uuid,
    ) {
        let mut sessions = self.sessions.write().await;

        // Get or create the room for this document
        let room = sessions
            .entry(doc_id.to_string())
            .or_insert_with(HashMap::new);

        // Add this session to the room with current timestamp
        room.insert(
            session_id.to_string(),
            SessionInfo {
                addr,
                last_active: Instant::now(),
                user_uuid,
            },
        );
        let room_size = room.len();

        // Release sessions lock before acquiring documents lock
        drop(sessions);

        // Mark document as having active sessions
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            doc_state.mark_room_active();
        }
        drop(documents);

        debug!(session_id = %session_id, doc_id = %doc_id, room_size, "Session joined document");

        // Feed the presence registry. The registry deduplicates
        // multi-tab from the same user, so the SSE event only fires
        // on a real "user joined" delta.
        if let Some(DocumentType::Ticket(ticket_id)) = DocumentType::from_doc_id(doc_id) {
            let delta = self
                .presence
                .add_session(user_uuid, ticket_id, session_id.to_string());
            if delta.changed {
                self.emit_viewers_changed(ticket_id).await;
            }
        }
    }

    // Update session activity timestamp
    async fn update_session_activity(&self, doc_id: &str, session_id: &str) {
        let touched_user: Option<Uuid> = {
            let mut sessions = self.sessions.write().await;
            sessions.get_mut(doc_id).and_then(|room| {
                room.get_mut(session_id).map(|info| {
                    info.last_active = Instant::now();
                    info.user_uuid
                })
            })
        };

        // Keep presence's last-active in sync so the avatar stack's
        // recency ordering reflects what the transport sees. No SSE
        // emission: touches never change the viewer set.
        if let (Some(user_uuid), Some(DocumentType::Ticket(ticket_id))) =
            (touched_user, DocumentType::from_doc_id(doc_id))
        {
            self.presence.touch_session(user_uuid, ticket_id);
        }
    }

    // Remove session
    async fn remove_session(&self, doc_id: &str, session_id: &str) {
        let mut sessions = self.sessions.write().await;

        if let Some(room) = sessions.get_mut(doc_id) {
            let removed_user = room.remove(session_id).map(|info| info.user_uuid);
            let room_size = room.len();
            let is_empty = room.is_empty();
            debug!(session_id = %session_id, doc_id = %doc_id, room_size, "Session left document");

            // Release the sessions lock before any async operations
            drop(sessions);

            // Mirror the removal into the presence registry. The
            // registry only reports `changed = true` when this was
            // the user's last tab on the ticket, so multi-tab close
            // doesn't spam the wire.
            if let (Some(user_uuid), Some(DocumentType::Ticket(ticket_id))) =
                (removed_user, DocumentType::from_doc_id(doc_id))
            {
                let delta = self
                    .presence
                    .remove_session(user_uuid, ticket_id, session_id);
                if delta.changed {
                    self.emit_viewers_changed(ticket_id).await;
                }
            }

            // If room is empty, mark it as empty but don't save immediately
            if is_empty {
                debug!(doc_id = %doc_id, "Room is now empty, will save after delay");

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
        // (ticket_id, user_uuid, session_id) tuples to drop from the
        // presence registry after we release the sessions lock.
        let mut presence_drops: Vec<(i32, Uuid, String)> = Vec::new();

        // First pass: collect stale sessions
        for (doc_id, room) in sessions.iter_mut() {
            let mut stale_sessions = Vec::new();
            let was_empty = room.is_empty();
            let ticket_id = match DocumentType::from_doc_id(doc_id) {
                Some(DocumentType::Ticket(id)) => Some(id),
                _ => None,
            };

            for (session_id, info) in room.iter() {
                if now.duration_since(info.last_active) > CLIENT_TIMEOUT * 5 {
                    stale_sessions.push((session_id.clone(), info.user_uuid));
                }
            }

            stale_session_count += stale_sessions.len();

            for (session_id, user_uuid) in stale_sessions.iter() {
                debug!(session_id = %session_id, doc_id = %doc_id, "Removing stale session");
                room.remove(session_id);
                if let Some(tid) = ticket_id {
                    presence_drops.push((tid, *user_uuid, session_id.clone()));
                }
            }

            // If room just became empty, mark it
            if !was_empty && room.is_empty() {
                newly_empty_rooms.push(doc_id.clone());
            }
        }

        // Log cleanup summary
        if stale_session_count > 0 {
            info!(count = stale_session_count, "Cleaned up stale sessions");
        }

        // Release the sessions lock before updating document states
        drop(sessions);

        // Mark newly empty rooms
        if !newly_empty_rooms.is_empty() {
            let mut documents = self.documents.write().await;
            for doc_id in newly_empty_rooms {
                if let Some(doc_state) = documents.get_mut(&doc_id) {
                    debug!(doc_id = %doc_id, "Marking room empty due to stale session cleanup");
                    doc_state.mark_room_empty();
                }
            }
        }

        // Mirror drops into the presence registry and emit one
        // `ViewersChanged` per ticket whose viewer set actually
        // changed (i.e. a user lost their last session). Per-ticket
        // dedup means a user closing N tabs in a row produces at
        // most one wire event.
        let mut tickets_to_notify: std::collections::HashSet<i32> =
            std::collections::HashSet::new();
        for (ticket_id, user_uuid, session_id) in presence_drops {
            let delta = self
                .presence
                .remove_session(user_uuid, ticket_id, &session_id);
            if delta.changed {
                tickets_to_notify.insert(ticket_id);
            }
        }
        for ticket_id in tickets_to_notify {
            self.emit_viewers_changed(ticket_id).await;
        }
    }

    // Force save a document immediately and create revision if this is end of editing session
    async fn force_save_document(&self, doc_id: &str) {
        let mut documents = self.documents.write().await;
        if let Some(doc_state) = documents.get_mut(doc_id) {
            debug!(doc_id = %doc_id, "Force saving document on disconnect");
            self.save_document_internal(doc_id, &doc_state.awareness);
            doc_state.mark_saved();

            // Create revision at end of editing session if there were actual content changes
            // Contributors are only added when content actually changes, so this is sufficient
            if !doc_state.contributors.is_empty() {
                info!(doc_id = %doc_id, contributors = doc_state.contributors.len(),
                    "Creating session-end revision");
                let contributors = doc_state.contributors.clone();
                self.create_snapshot_revision(doc_id, &doc_state.awareness, contributors);
                doc_state.reset_snapshot_tracking();
            } else {
                debug!(doc_id = %doc_id, "Skipping revision - no content changes in session");
            }

            // Mark final save completed so periodic task doesn't duplicate
            doc_state.mark_final_save_completed();
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
                    .map(|(_, info)| info.addr.clone())
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
                warn!(doc_id = %doc_id, "Cannot save - invalid document ID format");
                return;
            }
        };

        // Get binary content from the document
        let binary_content = {
            let doc = awareness.doc();
            let txn = doc.transact();

            // DIAGNOSTIC: Show ALL root types in the document
            let root_names: Vec<String> =
                txn.root_refs().map(|(name, _)| name.to_string()).collect();
            trace!(doc_id = %doc_id, root_types = ?root_names, "SAVE - root types");

            // Log state vector to see which clients have contributed
            let state_vec = txn.state_vector();
            trace!(doc_id = %doc_id, state_vector = ?state_vec, "SAVE - state vector");

            // Log content preview before saving
            if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                let child_count = fragment.len(&txn);
                let preview = safe_get_fragment_string(&fragment, &txn)
                    .map(|s| s.chars().take(50).collect::<String>())
                    .unwrap_or_else(|| "(invalid chars)".to_string());
                debug!(doc_id = %doc_id, child_count, preview = %preview, "Saving document");
            } else {
                warn!(doc_id = %doc_id, "Saving document: NO 'prosemirror' fragment found");
            }

            txn.encode_state_as_update_v1(&StateVector::default())
        };

        debug!(doc_id = %doc_id, bytes = binary_content.len(), "Saving document content");

        // CRITICAL: Save to Redis first (hot cache - survives restarts)
        // This ensures the latest state is always in Redis for fast recovery
        let redis_cache = self.redis_cache.clone();
        let doc_id_clone = doc_id.to_string();
        let content_for_redis = binary_content.clone();
        actix::spawn(async move {
            redis_cache
                .set_document(&doc_id_clone, &content_for_redis)
                .await;
            // Also refresh TTL to keep active documents cached longer
            redis_cache.refresh_ttl(&doc_id_clone).await;
        });

        // Save to database in a separate thread (cold storage - permanent backup)
        let pool = self.pool.clone();
        let content = binary_content.clone(); // Already Vec<u8>

        let search = self.search_service.clone();
        match doc_type {
            DocumentType::Ticket(ticket_id) => {
                // Save ticket article content Yjs snapshot to PostgreSQL (snapshot-based persistence)
                // Note: This does NOT update the ticket's modified timestamp - that only happens
                // when revisions are created (indicating actual content changes)
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            match repository::update_article_yjs_state(
                                &mut conn,
                                ticket_id,
                                content,
                                Some(&search),
                            ) {
                                Ok(_) => {
                                    debug!(ticket_id, "Successfully saved Yjs snapshot for ticket");
                                }
                                Err(e) => {
                                    error!(ticket_id, error = ?e, "Failed to save Yjs snapshot for ticket")
                                }
                            }
                        }
                        Err(e) => {
                            error!(ticket_id, error = ?e, "Database connection error when saving ticket")
                        }
                    }
                });
            }
            DocumentType::Documentation(doc_page_id) => {
                // Save documentation page Yjs state
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            // Update only the Yjs-related fields
                            match repository::update_documentation_yjs_state(
                                &mut conn,
                                doc_page_id,
                                content,
                                Some(&search),
                            ) {
                                Ok(_) => debug!(
                                    doc_page_id,
                                    "Successfully saved Yjs state for documentation page"
                                ),
                                Err(e) => {
                                    error!(doc_page_id, error = ?e, "Failed to save Yjs state for documentation page")
                                }
                            }
                        }
                        Err(e) => {
                            error!(doc_page_id, error = ?e, "Database connection error when saving documentation")
                        }
                    }
                });
            }
            DocumentType::Collection(collection_id) => {
                // Save collection description Yjs state
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            match repository::documentation_collections::update_collection_description_yjs(
                                &mut conn,
                                collection_id,
                                content,
                            ) {
                                Ok(_) => debug!(collection_id, "Saved Yjs state for collection description"),
                                Err(e) => error!(collection_id, error = ?e, "Failed to save Yjs state for collection description"),
                            }
                        }
                        Err(e) => error!(collection_id, error = ?e, "Database connection error when saving collection"),
                    }
                });
            }
        }
    }

    // Create a snapshot revision for version history using native Yrs encoding
    fn create_snapshot_revision(
        &self,
        doc_id: &str,
        awareness: &Awareness,
        contributors: HashSet<Uuid>,
    ) {
        // Parse document type
        let doc_type = match DocumentType::from_doc_id(doc_id) {
            Some(dt) => dt,
            None => {
                warn!(doc_id = %doc_id, "Skipping snapshot - invalid document ID format");
                return;
            }
        };

        // Encode document state using native Yrs functions
        let (state_vector_bytes, full_update_bytes) = {
            let doc = awareness.doc();
            let txn = doc.transact();

            // Use Yrs native encoding
            let state_vector = txn.state_vector();
            let full_update = txn.encode_state_as_update_v1(&StateVector::default());

            (state_vector.encode_v1(), full_update)
        };

        debug!(doc_id = %doc_id, bytes = full_update_bytes.len(), "Creating snapshot");

        // Save to database asynchronously
        let pool = self.pool.clone();
        let contributor_vec: Vec<Option<Uuid>> = contributors.into_iter().map(Some).collect();

        match doc_type {
            DocumentType::Ticket(ticket_id) => {
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            // Get or create article_content record
                            let article_content = match repository::get_article_content_by_ticket_id(
                                &mut conn, ticket_id,
                            ) {
                                Ok(ac) => ac,
                                Err(_) => {
                                    // Create if doesn't exist
                                    let new_content = NewArticleContent {
                                        ticket_id,
                                        yjs_state_vector: None,
                                        yjs_document: None,
                                        yjs_client_id: None,
                                    };
                                    match repository::create_article_content(&mut conn, new_content)
                                    {
                                        Ok(ac) => ac,
                                        Err(e) => {
                                            error!(ticket_id, error = ?e, "Failed to create article_content for snapshot");
                                            return;
                                        }
                                    }
                                }
                            };

                            // Check if content is the same as the last revision
                            if let Ok(last_revision) =
                                repository::get_latest_article_content_revision(
                                    &mut conn,
                                    article_content.id,
                                )
                            {
                                if last_revision.yjs_document_content == full_update_bytes {
                                    debug!(
                                        ticket_id,
                                        revision = last_revision.revision_number,
                                        "Skipping revision - content unchanged"
                                    );
                                    return;
                                }
                            }

                            // Create new revision with simplified schema (no redundant snapshot field!)
                            let new_revision = NewArticleContentRevision {
                                article_content_id: article_content.id,
                                revision_number: article_content.current_revision_number,
                                yjs_state_vector: state_vector_bytes,
                                yjs_document_content: full_update_bytes,
                                contributed_by: contributor_vec.clone(),
                            };

                            match repository::create_article_content_revision(
                                &mut conn,
                                new_revision,
                            ) {
                                Ok(revision) => {
                                    // Increment revision number in article_content
                                    match repository::increment_article_content_revision(
                                        &mut conn,
                                        article_content.id,
                                    ) {
                                        Ok(_) => {
                                            info!(
                                                ticket_id,
                                                revision = revision.revision_number,
                                                contributors = contributor_vec.len(),
                                                "Snapshot created for ticket"
                                            );

                                            // Update ticket's modified timestamp since content actually changed
                                            if let Err(e) =
                                                repository::update_ticket_modified_timestamp(
                                                    &mut conn, ticket_id,
                                                )
                                            {
                                                warn!(ticket_id, error = ?e, "Failed to update ticket modified timestamp");
                                            }
                                        }
                                        Err(e) => {
                                            error!(ticket_id, error = ?e, "Failed to increment revision number")
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!(ticket_id, error = ?e, "Failed to create revision")
                                }
                            }
                        }
                        Err(e) => {
                            error!(ticket_id, error = ?e, "Database connection error during snapshot")
                        }
                    }
                });
            }
            DocumentType::Documentation(doc_page_id) => {
                actix::spawn(async move {
                    match pool.get() {
                        Ok(mut conn) => {
                            // Check if content is the same as the last revision
                            if let Ok(last_revision) = repository::get_latest_documentation_revision(
                                &mut conn,
                                doc_page_id,
                            ) {
                                if last_revision.yjs_document_snapshot == full_update_bytes {
                                    debug!(
                                        doc_page_id,
                                        revision = last_revision.revision_number,
                                        "Skipping revision - content unchanged"
                                    );
                                    return;
                                }
                            }

                            // Create documentation revision snapshot
                            match repository::create_documentation_revision(
                                &mut conn,
                                doc_page_id,
                                state_vector_bytes,
                                full_update_bytes,
                                contributor_vec.clone(),
                            ) {
                                Ok(revision_number) => {
                                    info!(
                                        doc_page_id,
                                        revision = revision_number,
                                        contributors = contributor_vec.len(),
                                        "Snapshot created for documentation page"
                                    );
                                }
                                Err(e) => {
                                    error!(doc_page_id, error = ?e, "Failed to create documentation revision")
                                }
                            }
                        }
                        Err(e) => {
                            error!(doc_page_id, error = ?e, "Database connection error during snapshot")
                        }
                    }
                });
            }
            DocumentType::Collection(collection_id) => {
                // Collections don't have a per-revision history
                // table yet — the live save in `save_document_internal`
                // already lands the latest state. Skip the snapshot
                // pass without warning. If revision history is added
                // later, mirror the documentation pattern above.
                let _ = (
                    collection_id,
                    contributor_vec,
                    state_vector_bytes,
                    full_update_bytes,
                );
                debug!(
                    collection_id,
                    "Collection description snapshot skipped (no revision history)"
                );
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
    user_uuid: Uuid,            // User UUID for contributor tracking
    yjs_client_id: Option<u64>, // Yjs clientID from awareness, used for cleanup on disconnect
    // Statistics for debugging
    messages_received: u32,
    pings_sent: u32,
    pongs_received: u32,
    started_at: Instant,
}

impl YjsWebSocket {
    fn new(doc_id: String, app_state: YjsAppState, user_uuid: Uuid) -> Self {
        let id = Uuid::now_v7().to_string();
        let now = Instant::now();

        YjsWebSocket {
            id,
            doc_id,
            app_state,
            hb: now,
            user_uuid,
            yjs_client_id: None,
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

            trace!(session_id = %act.id, idle_secs = time_since_last_hb.as_secs(),
                "WebSocket heartbeat check");

            // Add grace period: warn at CLIENT_TIMEOUT, disconnect at CLIENT_TIMEOUT + 30s
            if time_since_last_hb > CLIENT_TIMEOUT + Duration::from_secs(30) {
                warn!(session_id = %act.id, idle_secs = time_since_last_hb.as_secs(),
                    "WebSocket Client heartbeat TIMEOUT, disconnecting");

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
            trace!(session_id = %act.id, ping_num = act.pings_sent + 1,
                idle_secs = time_since_last_hb.as_secs(), "WebSocket sending PING");
            act.pings_sent += 1;
            ctx.ping(b"");

            if time_since_last_hb > CLIENT_TIMEOUT {
                warn!(session_id = %act.id, idle_secs = time_since_last_hb.as_secs(),
                    "WebSocket Client heartbeat WARNING");
            }
        });
    }

    // Process incoming messages using the built-in protocol
    // Simplified to match the working nosdesk-old version - let yrs do the heavy lifting!
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
        let is_sync_message = msg.first() == Some(&0); // MESSAGE_SYNC
        let user_uuid = self.user_uuid; // Capture for contributor tracking

        // Spawn async work
        let addr = ctx.address();
        actix::spawn(async move {
            // Update session activity
            app_state
                .update_session_activity(&doc_id, &session_id)
                .await;

            // Get the awareness for this document
            let awareness = app_state.get_or_create_awareness(&doc_id).await;

            // DIAGNOSTIC: Check content BEFORE processing message
            let content_before = {
                let txn = awareness.doc().transact();
                if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                    fragment.get_string(&txn)
                } else {
                    String::from("(no fragment)")
                }
            };

            // Use the built-in protocol handler to process the message
            // DefaultProtocol is stateless - create new instance
            let protocol = DefaultProtocol;

            // DIAGNOSTIC: Log incoming message details
            let msg_type = if msg_vec.is_empty() { 255 } else { msg_vec[0] };
            trace!(msg_type, bytes = msg_vec.len(), "Processing message");

            // Log sync message type for debugging
            if msg_type == 0 && msg_vec.len() > 1 {
                let sync_step = msg_vec[1];
                match sync_step {
                    0 => trace!("SYNC_STEP_1 (state vector request)"),
                    1 => trace!("SYNC_STEP_2 (state response)"),
                    2 => trace!(
                        bytes = msg_vec.len() - 2,
                        "SYNC_UPDATE (incremental change)"
                    ),
                    _ => trace!(sync_step, "Unknown sync step"),
                }
            }

            match protocol.handle(&awareness, &msg_vec) {
                Ok(messages) => {
                    trace!(
                        response_count = messages.len(),
                        "protocol.handle() succeeded"
                    );

                    // DIAGNOSTIC: Check content AFTER processing message
                    let content_after = {
                        let txn = awareness.doc().transact();
                        if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
                            fragment.get_string(&txn)
                        } else {
                            String::from("(no fragment)")
                        }
                    };

                    let content_changed = content_before != content_after;
                    if content_changed {
                        debug!(before = %crate::utils::utf8_trunc::char_prefix(&content_before, 50),
                            after = %crate::utils::utf8_trunc::char_prefix(&content_after, 50),
                            "Content changed");
                    } else if msg_type == 0 && msg_vec.len() > 1 && msg_vec[1] == 2 {
                        // SYNC_UPDATE didn't apply - request full state from client
                        // This happens when state vectors are misaligned (e.g., after server restart)
                        debug!(
                            "SYNC_UPDATE did not change content - requesting client's full state"
                        );
                        use yrs::sync::Message;
                        let sync_message = Message::Sync(yrs::sync::SyncMessage::SyncStep1(
                            StateVector::default(),
                        ));
                        let encoded = sync_message.encode_v1();
                        addr.do_send(YjsMessage(Bytes::from(encoded)));
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

                    // Track contributor only when content actually changed
                    // This ensures revisions are only created for sessions with real edits
                    if content_changed {
                        app_state.add_contributor(&doc_id, user_uuid).await;
                    }
                }
                Err(e) => {
                    error!(error = ?e, "Error handling protocol message");
                }
            }
        });
    }
}

impl Actor for YjsWebSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(session_id = %self.id, doc_id = %self.doc_id,
            heartbeat_interval_secs = HEARTBEAT_INTERVAL.as_secs(),
            timeout_secs = (CLIENT_TIMEOUT + Duration::from_secs(30)).as_secs(),
            "WebSocket STARTED");

        self.hb(ctx);

        // Register session and send initial sync + awareness state
        let app_state = self.app_state.clone();
        let doc_id = self.doc_id.clone();
        let session_id = self.id.clone();
        let user_uuid = self.user_uuid;
        let addr = ctx.address();
        actix::spawn(async move {
            app_state
                .register_session(&doc_id, &session_id, addr.clone(), user_uuid)
                .await;

            // Per the yjs sync protocol spec, the server should proactively send
            // SyncStep1 + all known awareness states to newly connected clients.
            // This ensures the client immediately discovers other connected users.
            //
            // IMPORTANT: Each message must be sent as a separate WebSocket frame.
            // y-websocket's readMessage() parses one message per frame, so packing
            // multiple messages into a single buffer (as Protocol::start() does)
            // would cause only the first message to be read.
            let awareness = app_state.get_or_create_awareness(&doc_id).await;
            use yrs::sync::{Message, SyncMessage};

            // 1. Send SyncStep1 with the server's state vector
            let sv = awareness.doc().transact().state_vector();
            let sync_msg = Message::Sync(SyncMessage::SyncStep1(sv));
            addr.do_send(YjsMessage(Bytes::from(sync_msg.encode_v1())));

            // 2. Send all known awareness states (other connected clients)
            match awareness.update() {
                Ok(awareness_update) => {
                    let awareness_msg = Message::Awareness(awareness_update);
                    addr.do_send(YjsMessage(Bytes::from(awareness_msg.encode_v1())));
                    debug!(doc_id = %doc_id,
                        "Sent initial SyncStep1 + awareness to new client");
                }
                Err(e) => {
                    debug!(doc_id = %doc_id, error = ?e,
                        "Sent SyncStep1 but no awareness states to send");
                }
            }
        });
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        let time_since_last_hb = Instant::now().duration_since(self.hb);
        let connection_duration = Instant::now().duration_since(self.started_at);

        info!(session_id = %self.id, doc_id = %self.doc_id,
            connection_duration_secs = connection_duration.as_secs(),
            idle_secs = time_since_last_hb.as_secs(),
            messages_received = self.messages_received,
            pings_sent = self.pings_sent,
            pongs_received = self.pongs_received,
            "WebSocket STOPPING");

        // Spawn async cleanup work
        let app_state = self.app_state.clone();
        let doc_id = self.doc_id.clone();
        let session_id = self.id.clone();
        let yjs_client_id = self.yjs_client_id;

        actix::spawn(async move {
            // Remove the session first
            app_state.remove_session(&doc_id, &session_id).await;

            // Clean up the disconnected client's awareness state and notify remaining clients.
            // Per the yjs protocol, disconnecting clients should propagate state=null.
            // On abrupt disconnects (refresh, network loss), the client can't send this itself,
            // so the server must do it.
            if let Some(client_id) = yjs_client_id {
                let awareness = app_state.get_or_create_awareness(&doc_id).await;
                awareness.remove_state(client_id);

                // Encode and broadcast the removal (state=null) to remaining clients
                if let Ok(update) = awareness.update_with_clients([client_id]) {
                    use yrs::sync::Message;
                    let msg = Message::Awareness(update).encode_v1();
                    app_state.broadcast(&doc_id, &session_id, &msg).await;
                    debug!(doc_id = %doc_id, yjs_client_id = client_id,
                        "Removed awareness state and notified remaining clients");
                }
            }

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
                debug!(doc_id = %doc_id, "Last session for document, performing final save");
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
                trace!(session_id = %self.id, "WebSocket received PING");
                self.hb = Instant::now();
                self.messages_received += 1;
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                trace!(session_id = %self.id, "WebSocket received PONG");
                self.hb = Instant::now();
                self.pongs_received += 1;
                self.messages_received += 1;
            }
            Ok(ws::Message::Binary(bin)) => {
                trace!(session_id = %self.id, bytes = bin.len(), "WebSocket received BINARY message");
                self.hb = Instant::now();
                self.messages_received += 1;

                // Capture the yjs clientID from the first awareness message (msg type 1).
                // This is needed to clean up the awareness state on disconnect.
                if self.yjs_client_id.is_none() && bin.first() == Some(&1) && bin.len() > 1 {
                    use yrs::encoding::read::Cursor;
                    use yrs::sync::AwarenessUpdate;
                    use yrs::updates::decoder::DecoderV1 as ADecV1;
                    if let Ok(update) =
                        AwarenessUpdate::decode(&mut ADecV1::new(Cursor::new(&bin[1..])))
                    {
                        if let Some(&client_id) = update.clients.keys().next() {
                            self.yjs_client_id = Some(client_id);
                            debug!(session_id = %self.id, yjs_client_id = client_id,
                                "Captured yjs clientID from awareness");
                        }
                    }
                }

                self.process_message(&bin, ctx);
            }
            Ok(ws::Message::Close(reason)) => {
                debug!(session_id = %self.id, reason = ?reason, "WebSocket received CLOSE message");
                ctx.close(reason);
                ctx.stop();
            }
            Ok(ws::Message::Text(text)) => {
                warn!(session_id = %self.id, text = %text, "WebSocket received unexpected TEXT message");
            }
            Ok(ws::Message::Continuation(_)) => {
                trace!(session_id = %self.id, "WebSocket received CONTINUATION");
            }
            Ok(ws::Message::Nop) => {
                trace!(session_id = %self.id, "WebSocket received NOP");
            }
            Err(e) => {
                error!(session_id = %self.id, error = ?e, "WebSocket protocol error");
                ctx.stop();
            }
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
    debug!(doc_id = %doc_id, "WebSocket connection request");

    // Validate Origin header to prevent WebSocket hijacking (CSWSH)
    let frontend_url =
        std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:5173".to_string());
    let allowed_origin = frontend_url.trim_end_matches('/');
    let is_production = std::env::var("ENVIRONMENT")
        .map(|v| v.to_lowercase() == "production")
        .unwrap_or(false);

    match req.headers().get("Origin") {
        Some(origin) => {
            let origin_str = origin.to_str().unwrap_or("");
            let origin_normalized = origin_str.trim_end_matches('/');
            if origin_normalized != allowed_origin {
                warn!(origin = %origin_str, expected = %allowed_origin, "WebSocket origin mismatch");
                return Err(actix_web::error::ErrorForbidden("Invalid origin"));
            }
        }
        None => {
            // Origin should always be present from browsers (per spec)
            // In production, require it; in dev, allow for testing tools
            if is_production {
                warn!("WebSocket request missing Origin header in production");
                return Err(actix_web::error::ErrorForbidden("Origin header required"));
            }
            debug!("WebSocket request without Origin header (allowed in non-production)");
        }
    }

    // Extract and validate JWT token from httpOnly cookie
    let token = req
        .cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("No authentication cookie"))?;

    // Validate the token and extract user UUID
    let user_uuid = if let Some(pool) = req.app_data::<web::Data<crate::db::Pool>>() {
        let mut conn = pool.get().map_err(|_| {
            actix_web::error::ErrorInternalServerError("Database connection failed")
        })?;

        // Use our centralized JWT validation
        use crate::utils::jwt::JwtUtils;

        match JwtUtils::validate_token_with_user_check(token.value(), &mut conn).await {
            Ok((_claims, user)) => user.uuid,
            Err(_) => {
                return Err(actix_web::error::ErrorUnauthorized(
                    "Invalid or expired token",
                ))
            }
        }
    } else {
        return Err(actix_web::error::ErrorInternalServerError(
            "Database pool not available",
        ));
    };

    debug!(doc_id = %doc_id, user_uuid = %user_uuid, "WebSocket authentication successful");
    let actor = YjsWebSocket::new(doc_id, app_state.get_ref().clone(), user_uuid);

    // Use WsResponseBuilder to configure larger frame size for Yjs documents
    // Default is 64KB, but Yjs documents with history can grow larger
    ws::WsResponseBuilder::new(actor, &req, stream)
        .frame_size(1024 * 1024) // 1MB max frame size
        .start()
}

// ============= Revision History API Endpoints =============

/// GET /tickets/:id/revisions - List all revisions for a ticket
pub async fn get_ticket_revisions(
    ticket_id: web::Path<i32>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let ticket_id = ticket_id.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get article content for this ticket
    let article_content = match crate::repository::article_content::get_article_content_by_ticket_id(
        &mut conn, ticket_id,
    ) {
        Ok(content) => content,
        Err(_) => return errors::not_found_msg("No article content found for this ticket"),
    };

    // Get all revisions
    match crate::repository::article_content::get_article_content_revisions(
        &mut conn,
        article_content.id,
    ) {
        Ok(revisions) => {
            let responses: Vec<crate::models::ArticleContentRevisionResponse> =
                revisions.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(_) => errors::internal("Error retrieving revisions"),
    }
}

/// GET /tickets/:id/revisions/:revision_number - Get a specific revision
pub async fn get_ticket_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let (ticket_id, revision_number) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get article content for this ticket
    let article_content = match crate::repository::article_content::get_article_content_by_ticket_id(
        &mut conn, ticket_id,
    ) {
        Ok(content) => content,
        Err(_) => return errors::not_found_msg("No article content found for this ticket"),
    };

    // Get the specific revision
    match crate::repository::article_content::get_article_content_revision(
        &mut conn,
        article_content.id,
        revision_number,
    ) {
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
        }
        Err(_) => errors::not_found_msg("Revision not found"),
    }
}

/// POST /tickets/:id/restore/:revision_number - Restore ticket to a specific revision
pub async fn restore_ticket_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
    app_state: web::Data<YjsAppState>,
) -> HttpResponse {
    let (ticket_id, revision_number) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get article content for this ticket
    let article_content = match crate::repository::article_content::get_article_content_by_ticket_id(
        &mut conn, ticket_id,
    ) {
        Ok(content) => content,
        Err(_) => return errors::not_found_msg("No article content found for this ticket"),
    };

    // Get the revision to restore
    let revision = match crate::repository::article_content::get_article_content_revision(
        &mut conn,
        article_content.id,
        revision_number,
    ) {
        Ok(rev) => rev,
        Err(_) => return errors::not_found_msg("Revision not found"),
    };

    // Get the document ID
    let doc_id = format!("ticket-{ticket_id}");

    // Decode the stored Yjs update (this is the full document state at that revision)
    use yrs::updates::decoder::Decode;
    let update = match Update::decode_v1(&revision.yjs_document_content) {
        Ok(upd) => upd,
        Err(e) => {
            error!(ticket_id, revision_number, error = ?e, "Error decoding revision update");
            return errors::internal("Error decoding revision");
        }
    };

    // Restore requires replacing document entirely - Yjs CRDTs merge updates, don't support reverting.
    // Steps: create new doc with revision content, replace existing, broadcast to clients.
    let new_doc = {
        use yrs::{Doc, Options};

        let options = Options {
            client_id: rand::random(),
            skip_gc: false,
            ..Options::default()
        };

        let doc = Doc::with_options(options);

        // Initialize the prosemirror fragment first
        {
            let mut txn = doc.transact_mut();
            let _ = txn.get_or_insert_xml_fragment("prosemirror");
        }

        // Apply the revision update
        {
            let mut txn = doc.transact_mut();
            if let Err(e) = txn.apply_update(update) {
                error!(ticket_id, revision_number, error = ?e, "Error applying revision update to new doc");
                return errors::internal("Error applying revision");
            }
        }

        doc
    };

    // Get the full state from the new document
    let full_state = {
        let txn = new_doc.transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    };

    // Replace the document in app_state with the new one
    // This creates a new Awareness with the restored document
    app_state.replace_document(&doc_id, new_doc).await;

    // Mark document as changed to trigger save
    app_state.mark_document_changed(&doc_id).await;

    // Broadcast the full restored state to all connected clients
    use yrs::sync::Message;
    let sync_message = Message::Sync(yrs::sync::SyncMessage::Update(full_state));
    let encoded = sync_message.encode_v1();
    app_state.broadcast(&doc_id, "", &encoded).await;

    info!(ticket_id, revision_number, "Restored ticket to revision");

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

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get all revisions
    match crate::repository::documentation::get_documentation_revisions(&mut conn, doc_id) {
        Ok(revisions) => HttpResponse::Ok().json(revisions),
        Err(_) => errors::internal("Error retrieving revisions"),
    }
}

/// GET /docs/:id/revisions/:revision_number - Get a specific revision
pub async fn get_doc_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
) -> HttpResponse {
    let (doc_id, revision_number) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get the specific revision
    match crate::repository::documentation::get_documentation_revision(
        &mut conn,
        doc_id,
        revision_number,
    ) {
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
        }
        Err(_) => errors::not_found_msg("Revision not found"),
    }
}

/// POST /docs/:id/restore/:revision_number - Restore documentation page to a specific revision
pub async fn restore_doc_revision(
    path: web::Path<(i32, i32)>,
    pool: web::Data<crate::db::Pool>,
    app_state: web::Data<YjsAppState>,
) -> HttpResponse {
    let (doc_id, revision_number) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get the revision to restore
    let revision = match crate::repository::documentation::get_documentation_revision(
        &mut conn,
        doc_id,
        revision_number,
    ) {
        Ok(rev) => rev,
        Err(_) => return errors::not_found_msg("Revision not found"),
    };

    // Get the document ID string
    let doc_id_str = format!("doc-{doc_id}");

    // Decode the stored Yjs update (this is the full document state at that revision)
    use yrs::updates::decoder::Decode;
    let update = match Update::decode_v1(&revision.yjs_document_snapshot) {
        Ok(upd) => upd,
        Err(e) => {
            error!(doc_id, revision_number, error = ?e, "Error decoding revision update");
            return errors::internal("Error decoding revision");
        }
    };

    // Restore requires replacing document entirely - Yjs CRDTs merge updates, don't support reverting.
    // Steps: create new doc with revision content, replace existing, broadcast to clients.
    let new_doc = {
        use yrs::{Doc, Options};

        let options = Options {
            client_id: rand::random(),
            skip_gc: false,
            ..Options::default()
        };

        let doc = Doc::with_options(options);

        // Initialize the prosemirror fragment first
        {
            let mut txn = doc.transact_mut();
            let _ = txn.get_or_insert_xml_fragment("prosemirror");
        }

        // Apply the revision update
        {
            let mut txn = doc.transact_mut();
            if let Err(e) = txn.apply_update(update) {
                error!(doc_id, revision_number, error = ?e, "Error applying revision update to new doc");
                return errors::internal("Error applying revision");
            }
        }

        doc
    };

    // Get the full state from the new document
    let full_state = {
        let txn = new_doc.transact();
        txn.encode_state_as_update_v1(&StateVector::default())
    };

    // Replace the document in app_state with the new one
    app_state.replace_document(&doc_id_str, new_doc).await;

    // Mark document as changed to trigger save
    app_state.mark_document_changed(&doc_id_str).await;

    // Broadcast the full restored state to all connected clients
    use yrs::sync::Message;
    let sync_message = Message::Sync(yrs::sync::SyncMessage::Update(full_state));
    let encoded = sync_message.encode_v1();
    app_state.broadcast(&doc_id_str, "", &encoded).await;

    info!(
        doc_id,
        revision_number, "Restored documentation page to revision"
    );

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
            .route(
                "/tickets/{ticket_id}/revisions",
                web::get().to(get_ticket_revisions),
            )
            .route(
                "/tickets/{ticket_id}/revisions/{revision_number}",
                web::get().to(get_ticket_revision),
            )
            .route(
                "/tickets/{ticket_id}/restore/{revision_number}",
                web::post().to(restore_ticket_revision),
            )
            .route("/docs/{doc_id}/revisions", web::get().to(get_doc_revisions))
            .route(
                "/docs/{doc_id}/revisions/{revision_number}",
                web::get().to(get_doc_revision),
            )
            .route(
                "/docs/{doc_id}/restore/{revision_number}",
                web::post().to(restore_doc_revision),
            ),
    );
}
