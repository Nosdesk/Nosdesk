//! Full-text search service using Tantivy
//!
//! This module provides full-text search across all major entities in Nosdesk:
//! - Tickets (title, description content)
//! - Comments (content)
//! - Documentation pages (title, content)
//! - Attachments (name, transcription if available)
//! - Devices (name, hostname, serial number, manufacturer, model)
//! - Users (name, email, department, title)

pub mod extractors;
pub mod indexer;
pub mod indexing_tasks;
pub mod schema;
pub mod searcher;
pub mod types;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy};
use tracing::{debug, info, warn};

use crate::db::{DbConnection, Pool};
use crate::models;

pub use types::{EntityType, IndexDocument, SearchQuery, SearchResponse};
use schema::SearchSchema;

/// Memory budget for the index writer (50MB)
const INDEX_WRITER_MEMORY_BYTES: usize = 50_000_000;

/// Search service that manages the Tantivy index
pub struct SearchService {
    _index: Index,
    schema: SearchSchema,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    is_rebuilding: AtomicBool,
}

impl SearchService {
    /// Create a new search service with a disk-based index.
    /// Automatically rebuilds the index from the database if empty.
    pub fn new(index_path: &Path, pool: &Pool) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create index directory if it doesn't exist
        std::fs::create_dir_all(index_path)?;

        let (index, schema) = if index_path.join("meta.json").exists() {
            info!(path = ?index_path, "Opening existing search index");
            let idx = Index::open_in_dir(index_path)?;

            // Check if the existing index has a compatible schema
            if SearchSchema::is_compatible_with_index(&idx) {
                match SearchSchema::from_index(&idx) {
                    Ok(sch) => {
                        debug!("Using existing index schema");
                        (idx, sch)
                    }
                    Err(e) => {
                        warn!(error = ?e, "Failed to extract schema from existing index, recreating");
                        drop(idx);
                        Self::recreate_index(index_path)?
                    }
                }
            } else {
                warn!("Existing index schema is incompatible, recreating index");
                drop(idx);
                Self::recreate_index(index_path)?
            }
        } else {
            info!(path = ?index_path, "Creating new search index");
            let sch = SearchSchema::new();
            let idx = Index::create_in_dir(index_path, sch.schema.clone())?;
            (idx, sch)
        };


        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = index.writer(INDEX_WRITER_MEMORY_BYTES)?;

        let service = Self {
            _index: index,
            schema,
            reader,
            writer: Arc::new(RwLock::new(writer)),
            is_rebuilding: AtomicBool::new(false),
        };

        // Auto-populate if the index is empty
        let doc_count = service.reader.searcher().num_docs();
        if doc_count == 0 {
            info!("Search index is empty, rebuilding from database");
            match pool.get() {
                Ok(mut conn) => match service.rebuild_index(&mut conn) {
                    Ok(stats) => info!(total = stats.total(), "Initial index build complete"),
                    Err(e) => warn!(error = ?e, "Initial index build failed, search will populate incrementally"),
                },
                Err(e) => warn!(error = ?e, "Could not connect to database for initial index build"),
            }
        } else {
            info!(documents = doc_count, "Search index loaded");
        }

        Ok(service)
    }

    /// Delete and recreate the index with a fresh schema
    fn recreate_index(index_path: &Path) -> Result<(Index, SearchSchema), Box<dyn std::error::Error + Send + Sync>> {
        // Delete the old index directory contents
        if index_path.exists() {
            for entry in std::fs::read_dir(index_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    std::fs::remove_file(&path)?;
                } else if path.is_dir() {
                    std::fs::remove_dir_all(&path)?;
                }
            }
        }

        // Create fresh index with new schema
        let schema = SearchSchema::new();
        let index = Index::create_in_dir(index_path, schema.schema.clone())?;
        info!(path = ?index_path, "Created fresh search index with new schema");
        Ok((index, schema))
    }

    /// Execute a search query.
    ///
    /// `include_internal` is gated by the caller's role: staff
    /// (Admin / Technician) pass `true`; non-staff pass `false` so
    /// internal-note comments are filtered out of the result set.
    pub fn search(
        &self,
        query: &SearchQuery,
        include_internal: bool,
    ) -> Result<SearchResponse, Box<dyn std::error::Error + Send + Sync>> {
        let entity_types = query.entity_types();
        let entity_types_ref = entity_types.as_deref();

        searcher::execute_search(
            &self.reader,
            &self.schema,
            &query.q,
            query.limit,
            entity_types_ref,
            include_internal,
        )
    }

    /// Index a ticket with its optional article content
    pub fn index_ticket(
        &self,
        ticket: &models::Ticket,
        article_content: Option<&models::ArticleContent>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let doc = indexer::index_document_from_ticket(ticket, article_content);
        self.index_document(&doc)
    }

    /// Index a comment
    pub fn index_comment(&self, comment: &models::Comment, ticket_title: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let doc = indexer::index_document_from_comment(comment, ticket_title);
        self.index_document(&doc)
    }

    /// Index a documentation page
    pub fn index_documentation(&self, doc_page: &models::DocumentationPage) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let doc = indexer::index_document_from_documentation(doc_page);
        self.index_document(&doc)
    }

    /// Index a device
    pub fn index_device(&self, device: &models::Device) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let doc = indexer::index_document_from_device(device);
        self.index_document(&doc)
    }

    /// Index a user with optional primary email
    pub fn index_user(&self, user: &models::User, primary_email: Option<&str>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let doc = indexer::index_document_from_user(user, primary_email);
        self.index_document(&doc)
    }

    /// Delete a ticket from the index
    pub fn delete_ticket(&self, ticket_id: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.delete_by_key(EntityType::Ticket, &ticket_id.to_string())
    }

    /// Delete a comment from the index
    pub fn delete_comment(&self, comment_id: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.delete_by_key(EntityType::Comment, &comment_id.to_string())
    }

    /// Delete a documentation page from the index
    pub fn delete_documentation(&self, doc_id: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.delete_by_key(EntityType::Documentation, &doc_id.to_string())
    }

    /// Delete a device from the index
    pub fn delete_device(&self, device_id: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.delete_by_key(EntityType::Device, &device_id.to_string())
    }

    /// Delete a user from the index
    pub fn delete_user(&self, user_uuid: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.delete_by_key(EntityType::User, user_uuid)
    }

    /// Rebuild the entire index from the database
    pub fn rebuild_index(&self, conn: &mut DbConnection) -> Result<indexer::IndexStats, Box<dyn std::error::Error + Send + Sync>> {
        if self.is_rebuilding.swap(true, Ordering::SeqCst) {
            return Err("Index rebuild already in progress".into());
        }

        let result = {
            let mut writer = self.writer.write().map_err(|e| format!("Lock error: {}", e))?;

            // Delete all existing documents
            writer.delete_all_documents()?;

            // Rebuild from database
            let stats = indexer::rebuild_index(conn, &writer, &self.schema)?;

            // Commit changes
            writer.commit()?;

            Ok(stats)
        };

        self.is_rebuilding.store(false, Ordering::SeqCst);
        result
    }

    /// Check if the index is currently being rebuilt
    pub fn is_rebuilding(&self) -> bool {
        self.is_rebuilding.load(Ordering::SeqCst)
    }

    /// Commit pending changes to the index
    pub fn commit(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut writer = self.writer.write().map_err(|e| format!("Lock error: {}", e))?;
        writer.commit()?;
        Ok(())
    }

    /// Get index statistics
    pub fn get_stats(&self) -> Result<types::IndexStats, Box<dyn std::error::Error + Send + Sync>> {
        let searcher = self.reader.searcher();
        let total_documents = searcher.num_docs();

        // Index size is not easily accessible in Tantivy 0.22's managed directory
        // Return 0 for now - could be computed from segment metadata if needed
        let index_size_bytes = 0;

        Ok(types::IndexStats {
            total_documents,
            by_type: std::collections::HashMap::new(), // Could be computed with faceted search
            index_size_bytes,
            is_rebuilding: self.is_rebuilding(),
        })
    }

    // Internal helper methods

    fn index_document(&self, doc: &IndexDocument) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let writer = self.writer.write().map_err(|e| format!("Lock error: {}", e))?;
        indexer::add_document_to_index(&writer, &self.schema, doc)?;
        // Note: We don't commit here for performance. The caller should commit periodically
        // or use a background commit task.
        Ok(())
    }

    fn delete_by_key(&self, entity_type: EntityType, key: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let writer = self.writer.write().map_err(|e| format!("Lock error: {}", e))?;
        indexer::delete_document_from_index(&writer, &self.schema, entity_type, key)?;
        Ok(())
    }
}

// Make SearchService thread-safe
unsafe impl Send for SearchService {}
unsafe impl Sync for SearchService {}

/// Bridge the repository's `UserCreatedObserver` hook to the search
/// index. Implemented on `Arc<SearchService>` so a single shared
/// service handle satisfies the observer trait without an extra
/// wrapper struct — the repository helpers accept `Option<&dyn
/// UserCreatedObserver>` and callers pass `Some(search_service.as_ref())`.
impl crate::repository::user_helpers::UserCreatedObserver for Arc<SearchService> {
    fn user_created(&self, user: &models::User, primary_email: &str) {
        indexing_tasks::spawn_index_user(
            Arc::clone(self),
            user.clone(),
            Some(primary_email.to_string()),
        );
    }
}

/// Re-index a ticket whenever the collaborative editor commits a new
/// Yjs snapshot for its article content. Without this hook the index
/// only saw ticket metadata changes (status, priority, title) and
/// the body text typed into the editor was never searchable.
impl crate::repository::article_content::ArticleContentSavedObserver for Arc<SearchService> {
    fn article_content_saved(
        &self,
        ticket: &models::Ticket,
        article: &models::ArticleContent,
    ) {
        indexing_tasks::spawn_index_ticket(
            Arc::clone(self),
            ticket.clone(),
            Some(article.clone()),
        );
    }
}

/// Re-index a documentation page whenever its Yjs blob is saved by
/// the collaborative editor. Pairs with the page-metadata reindex
/// already wired into the documentation handlers so both title-level
/// and body-level edits land in search.
impl crate::repository::documentation::DocumentationSavedObserver for Arc<SearchService> {
    fn documentation_saved(&self, page: &models::DocumentationPage) {
        indexing_tasks::spawn_index_documentation(Arc::clone(self), page.clone());
    }
}

// ---- Update / delete bridges ----

impl crate::repository::users::UserUpdatedObserver for Arc<SearchService> {
    fn user_updated(&self, user: &models::User, primary_email: Option<&str>) {
        indexing_tasks::spawn_index_user(
            Arc::clone(self),
            user.clone(),
            primary_email.map(|s| s.to_string()),
        );
    }
}

impl crate::repository::users::UserDeletedObserver for Arc<SearchService> {
    fn user_deleted(&self, user_uuid: &uuid::Uuid) {
        indexing_tasks::spawn_delete_user(Arc::clone(self), user_uuid.to_string());
    }
}

impl crate::repository::tickets::TicketUpdatedObserver for Arc<SearchService> {
    fn ticket_updated(
        &self,
        ticket: &models::Ticket,
        article: Option<&models::ArticleContent>,
    ) {
        indexing_tasks::spawn_index_ticket(
            Arc::clone(self),
            ticket.clone(),
            article.cloned(),
        );
    }
}

impl crate::repository::tickets::TicketDeletedObserver for Arc<SearchService> {
    fn ticket_deleted(&self, ticket_id: i32) {
        indexing_tasks::spawn_delete_ticket(Arc::clone(self), ticket_id);
    }
}

impl crate::repository::comments::CommentCreatedObserver for Arc<SearchService> {
    fn comment_created(&self, comment: &models::Comment, ticket_title: &str) {
        indexing_tasks::spawn_index_comment(
            Arc::clone(self),
            comment.clone(),
            ticket_title.to_string(),
        );
    }
}

impl crate::repository::comments::CommentDeletedObserver for Arc<SearchService> {
    fn comment_deleted(&self, comment_id: i32) {
        indexing_tasks::spawn_delete_comment(Arc::clone(self), comment_id);
    }
}

impl crate::repository::devices::DeviceDeletedObserver for Arc<SearchService> {
    fn device_deleted(&self, device_id: i32) {
        indexing_tasks::spawn_delete_device(Arc::clone(self), device_id);
    }
}

impl crate::repository::documentation::DocumentationDeletedObserver for Arc<SearchService> {
    fn documentation_deleted(&self, page_id: i32) {
        indexing_tasks::spawn_delete_documentation(Arc::clone(self), page_id);
    }
}
