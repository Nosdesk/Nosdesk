//! Async indexing tasks for incremental search index updates
//!
//! These functions spawn background tasks to update the search index
//! after CRUD operations, ensuring the main request is not blocked.

use std::sync::Arc;
use tracing::{debug, error};

use super::SearchService;
use crate::models;

/// Spawn a background indexing task that commits after completion.
fn spawn_indexing_task(
    search_service: Arc<SearchService>,
    label: &'static str,
    task: impl FnOnce(&SearchService) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
) {
    tokio::spawn(async move {
        if let Err(e) = task(&search_service) {
            error!(error = ?e, "Failed to {label}");
        } else {
            debug!("{label} completed");
        }
        if let Err(e) = search_service.commit() {
            error!(error = ?e, "Failed to commit search index after {label}");
        }
    });
}

/// Index a ticket in the background
pub fn spawn_index_ticket(
    search_service: Arc<SearchService>,
    ticket: models::Ticket,
    article_content: Option<models::ArticleContent>,
) {
    spawn_indexing_task(search_service, "index ticket", move |svc| {
        svc.index_ticket(&ticket, article_content.as_ref())
    });
}

/// Delete a ticket from the index in the background
pub fn spawn_delete_ticket(search_service: Arc<SearchService>, ticket_id: i32) {
    spawn_indexing_task(search_service, "delete ticket", move |svc| {
        svc.delete_ticket(ticket_id)
    });
}

/// Index a comment in the background
pub fn spawn_index_comment(
    search_service: Arc<SearchService>,
    comment: models::Comment,
    ticket_title: String,
) {
    spawn_indexing_task(search_service, "index comment", move |svc| {
        svc.index_comment(&comment, &ticket_title)
    });
}

/// Delete a comment from the index in the background
pub fn spawn_delete_comment(search_service: Arc<SearchService>, comment_id: i32) {
    spawn_indexing_task(search_service, "delete comment", move |svc| {
        svc.delete_comment(comment_id)
    });
}

/// Index a documentation page in the background
pub fn spawn_index_documentation(
    search_service: Arc<SearchService>,
    doc_page: models::DocumentationPage,
) {
    spawn_indexing_task(search_service, "index documentation", move |svc| {
        svc.index_documentation(&doc_page)
    });
}

/// Delete a documentation page from the index in the background
pub fn spawn_delete_documentation(search_service: Arc<SearchService>, doc_id: i32) {
    spawn_indexing_task(search_service, "delete documentation", move |svc| {
        svc.delete_documentation(doc_id)
    });
}

/// Index a device in the background
pub fn spawn_index_device(search_service: Arc<SearchService>, device: models::Device) {
    spawn_indexing_task(search_service, "index device", move |svc| {
        svc.index_device(&device)
    });
}

/// Delete a device from the index in the background
pub fn spawn_delete_device(search_service: Arc<SearchService>, device_id: i32) {
    spawn_indexing_task(search_service, "delete device", move |svc| {
        svc.delete_device(device_id)
    });
}

/// Index a user in the background
pub fn spawn_index_user(
    search_service: Arc<SearchService>,
    user: models::User,
    primary_email: Option<String>,
) {
    spawn_indexing_task(search_service, "index user", move |svc| {
        svc.index_user(&user, primary_email.as_deref())
    });
}

/// Delete a user from the index in the background
pub fn spawn_delete_user(search_service: Arc<SearchService>, user_uuid: String) {
    spawn_indexing_task(search_service, "delete user", move |svc| {
        svc.delete_user(&user_uuid)
    });
}
