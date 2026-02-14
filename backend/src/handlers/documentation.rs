use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use serde::Deserialize;
use serde_json::json;
use std::panic;
use std::sync::Arc;
use tracing::{debug, error, info};
use uuid::Uuid;
use yrs::{Doc, Transact, ReadTxn, WriteTxn, GetString, Options, updates::decoder::Decode, Update, XmlFragment, XmlOut};
use regex::Regex;

use crate::db::{Pool, DbConnection};
use crate::models::{Claims, NewDocumentationPage, DocumentationPageWithChildren, DocumentationStatus, DocumentationPage, DocumentationPageResponse, UserInfoWithAvatar};
use crate::repository;
use crate::utils;
use crate::utils::rbac::{is_admin, is_technician_or_admin};
use crate::services::search::SearchService;
use crate::services::search::indexing_tasks;

/// Recursively extract plain text from an XmlOut node
fn extract_text_from_xml_node(node: &XmlOut, txn: &yrs::Transaction) -> String {
    match node {
        XmlOut::Text(text_ref) => {
            // XmlTextRef::get_string returns the text content
            match panic::catch_unwind(panic::AssertUnwindSafe(|| {
                text_ref.get_string(txn)
            })) {
                Ok(s) => s,
                Err(_) => String::new(),
            }
        }
        XmlOut::Element(elem_ref) => {
            // Recursively extract text from element's children
            let mut text = String::new();
            for child in elem_ref.children(txn) {
                let child_text = extract_text_from_xml_node(&child, txn);
                if !child_text.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&child_text);
                }
            }
            text
        }
        XmlOut::Fragment(frag_ref) => {
            // Recursively extract text from fragment's children
            let mut text = String::new();
            for child in frag_ref.children(txn) {
                let child_text = extract_text_from_xml_node(&child, txn);
                if !child_text.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&child_text);
                }
            }
            text
        }
    }
}

/// Extract text content from a Yjs document binary blob
/// Returns the plain text content extracted from the ProseMirror XmlFragment
fn extract_yjs_content(yjs_document: &[u8]) -> Option<String> {
    // Create a new Yjs document with GC disabled for reading
    let options = Options {
        skip_gc: true,
        ..Default::default()
    };
    let doc = Doc::with_options(options);

    // Initialize the prosemirror XmlFragment before applying update
    {
        let mut txn = doc.transact_mut();
        let _ = txn.get_or_insert_xml_fragment("prosemirror");
    }

    // Decode and apply the update
    let update = match Update::decode_v1(yjs_document) {
        Ok(u) => u,
        Err(_) => return None,
    };

    {
        let mut txn = doc.transact_mut();
        if txn.apply_update(update).is_err() {
            return None;
        }
    }

    // Extract text content from the prosemirror fragment by traversing children
    let txn = doc.transact();
    if let Some(fragment) = txn.get_xml_fragment("prosemirror") {
        let mut text_parts = Vec::new();

        // Iterate through top-level children (paragraphs, headings, etc.)
        for child in fragment.children(&txn) {
            let child_text = extract_text_from_xml_node(&child, &txn);
            if !child_text.is_empty() {
                text_parts.push(child_text);
            }
        }

        if text_parts.is_empty() {
            None
        } else {
            let joined = text_parts.join(" ");
            // Strip any remaining XML/HTML tags (e.g., <strong>, <em>, etc.)
            let tag_regex = Regex::new(r"<[^>]+>").unwrap();
            let clean_text = tag_regex.replace_all(&joined, "").to_string();
            // Normalize whitespace
            let whitespace_regex = Regex::new(r"\s+").unwrap();
            let normalized = whitespace_regex.replace_all(&clean_text, " ").trim().to_string();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        }
    } else {
        None
    }
}

// DTO for creating documentation pages (fields that frontend should send)
#[derive(Debug, Deserialize)]
pub struct CreateDocumentationPageRequest {
    pub title: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: Option<String>,
    pub parent_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: Option<bool>,
    pub is_template: Option<bool>,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: Option<bool>,
}

// Helper function to convert DocumentationPage to DocumentationPageResponse with user info
fn to_page_response(
    page: DocumentationPage,
    conn: &mut DbConnection,
) -> Result<DocumentationPageResponse, String> {
    // Fetch user info for created_by
    let created_by_user = repository::get_user_by_uuid(&page.created_by, conn)
        .map_err(|_| "Failed to fetch created_by user")?;

    // Fetch user info for last_edited_by
    let last_edited_by_user = repository::get_user_by_uuid(&page.last_edited_by, conn)
        .map_err(|_| "Failed to fetch last_edited_by user")?;

    // Extract content from Yjs document if available
    // First try the page's own yjs_document, then fall back to linked ticket's article content
    let content = page.yjs_document.as_ref()
        .and_then(|doc| extract_yjs_content(doc))
        .or_else(|| {
            // If page is linked to a ticket, try to get content from article_contents
            page.ticket_id.and_then(|ticket_id| {
                repository::get_article_content_by_ticket_id(conn, ticket_id)
                    .ok()
                    .and_then(|article| article.yjs_document)
                    .and_then(|doc| extract_yjs_content(&doc))
            })
        });

    Ok(DocumentationPageResponse {
        id: page.id,
        uuid: page.uuid,
        title: page.title,
        slug: page.slug,
        icon: page.icon,
        cover_image: page.cover_image,
        status: page.status,
        created_at: page.created_at,
        updated_at: page.updated_at,
        created_by: UserInfoWithAvatar {
            uuid: created_by_user.uuid,
            name: created_by_user.name,
            avatar_url: created_by_user.avatar_url,
            avatar_thumb: created_by_user.avatar_thumb,
        },
        last_edited_by: UserInfoWithAvatar {
            uuid: last_edited_by_user.uuid,
            name: last_edited_by_user.name,
            avatar_url: last_edited_by_user.avatar_url,
            avatar_thumb: last_edited_by_user.avatar_thumb,
        },
        parent_id: page.parent_id,
        ticket_id: page.ticket_id,
        display_order: page.display_order,
        is_public: page.is_public,
        is_template: page.is_template,
        archived_at: page.archived_at,
        deleted_at: page.deleted_at,
        has_unsaved_changes: page.has_unsaved_changes,
        children: None,
        content,
    })
}

// Helper function to convert multiple DocumentationPages to DocumentationPageResponses
fn to_page_responses(
    pages: Vec<DocumentationPage>,
    conn: &mut DbConnection,
) -> Result<Vec<DocumentationPageResponse>, String> {
    pages
        .into_iter()
        .map(|page| to_page_response(page, conn))
        .collect()
}

// Get all documentation pages
pub async fn get_documentation_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_documentation_pages(&mut conn) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch pages"),
    }
}

// Get a single documentation page by ID
pub async fn get_documentation_page(
    req: HttpRequest,
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(page) => {
            match repository::can_user_access_page(&mut conn, page.id, &user_uuid, is_admin(&claims)) {
                Ok(true) => {},
                Ok(false) => return HttpResponse::NotFound().json("Page not found"),
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            }
            match to_page_response(page, &mut conn) {
                Ok(response) => HttpResponse::Ok().json(response),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::NotFound().json("Page not found"),
    }
}

// Get a documentation page by its slug
pub async fn get_documentation_page_by_slug(
    req: HttpRequest,
    slug: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let page_slug = slug.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_documentation_page_by_slug(&page_slug, &mut conn) {
        Ok(page) => {
            match repository::can_user_access_page(&mut conn, page.id, &user_uuid, is_admin(&claims)) {
                Ok(true) => {},
                Ok(false) => return HttpResponse::NotFound().json("Page not found"),
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            }
            match to_page_response(page, &mut conn) {
                Ok(response) => HttpResponse::Ok().json(response),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::NotFound().json("Page not found"),
    }
}

// Get a documentation page's content by UUID (for embedding)
// Returns the Yjs document as base64 + metadata
pub async fn get_documentation_page_content_by_uuid(
    req: HttpRequest,
    uuid_path: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    let uuid_str = uuid_path.into_inner();
    let page_uuid = match Uuid::parse_str(&uuid_str) {
        Ok(u) => u,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid UUID"})),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"})),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json(json!({"error": "Invalid user UUID"})),
    };

    match repository::get_documentation_page_by_uuid(&page_uuid, &mut conn) {
        Ok(page) => {
            match repository::can_user_access_page(&mut conn, page.id, &user_uuid, is_admin(&claims)) {
                Ok(true) => {},
                Ok(false) => return HttpResponse::NotFound().json(json!({"error": "Page not found"})),
                Err(_) => return HttpResponse::InternalServerError().json(json!({"error": "Failed to check page visibility"})),
            }

            use base64::{Engine as _, engine::general_purpose};

            // Get yjs_document - try page's own, then fall back to linked ticket
            let yjs_document = page.yjs_document.as_ref()
                .cloned()
                .or_else(|| {
                    page.ticket_id.and_then(|ticket_id| {
                        repository::get_article_content_by_ticket_id(&mut conn, ticket_id)
                            .ok()
                            .and_then(|article| article.yjs_document)
                    })
                });

            let yjs_b64 = yjs_document.map(|doc| general_purpose::STANDARD.encode(&doc));

            HttpResponse::Ok().json(json!({
                "uuid": page.uuid,
                "title": page.title,
                "icon": page.icon,
                "status": page.status,
                "yjs_document": yjs_b64,
            }))
        },
        Err(_) => HttpResponse::NotFound().json(json!({"error": "Page not found"})),
    }
}

// Sync the embedding references for a page
// Called by the frontend after saving, with the list of embedded document UUIDs
pub async fn sync_page_embeddings(
    page_id: web::Path<i32>,
    body: web::Json<SyncEmbeddingsRequest>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let source_page_id = page_id.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"})),
    };

    // Resolve UUIDs to page IDs
    let mut target_page_ids = Vec::new();
    for uuid_str in &body.embedded_uuids {
        if let Ok(uuid) = Uuid::parse_str(uuid_str) {
            if let Ok(page) = repository::get_documentation_page_by_uuid(&uuid, &mut conn) {
                target_page_ids.push(page.id);
            }
        }
    }

    match repository::sync_page_embeddings(&mut conn, source_page_id, &target_page_ids) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!("Failed to sync page embeddings: {}", e);
            HttpResponse::InternalServerError().json(json!({"error": "Failed to sync embeddings"}))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SyncEmbeddingsRequest {
    pub embedded_uuids: Vec<String>,
}

// Create a new documentation page
pub async fn create_documentation_page(
    req: HttpRequest,
    page_request: web::Json<CreateDocumentationPageRequest>,
    pool: web::Data<Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can create documentation pages"
        }));
    }

    let request = page_request.into_inner();

    // Get the authenticated user's UUID
    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
    };

    // Parse status string to enum
    let status = match request.status.as_deref() {
        Some("published") => DocumentationStatus::Published,
        Some("archived") => DocumentationStatus::Archived,
        _ => DocumentationStatus::Draft,
    };

    // Build the NewDocumentationPage from request
    let slug = utils::slug::generate_unique_slug(&request.title, &mut conn);
    let new_page = NewDocumentationPage {
        uuid: Uuid::now_v7(),
        title: request.title,
        slug,
        icon: request.icon,
        cover_image: request.cover_image,
        status,
        created_by: user_uuid,
        last_edited_by: user_uuid,
        parent_id: request.parent_id,
        ticket_id: request.ticket_id,
        display_order: request.display_order.or(Some(0)),
        is_public: request.is_public.unwrap_or(false),
        is_template: request.is_template.unwrap_or(false),
        yjs_state_vector: request.yjs_state_vector,
        yjs_document: request.yjs_document,
        yjs_client_id: request.yjs_client_id,
        has_unsaved_changes: request.has_unsaved_changes.unwrap_or(false),
    };

    match repository::create_documentation_page(new_page, &mut conn) {
        Ok(created_page) => {
            // Index the new documentation page in search
            indexing_tasks::spawn_index_documentation(search_service.get_ref().clone(), created_page.clone());

            match to_page_response(created_page.clone(), &mut conn) {
                Ok(response) => {
                    // Broadcast SSE event for documentation creation
                    use crate::utils::sse::SseBroadcaster;
                    SseBroadcaster::broadcast_documentation_created(
                        &sse_state,
                        created_page.id,
                        serde_json::to_value(&response).unwrap_or_default(),
                    ).await;

                    HttpResponse::Created().json(response)
                },
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to create page"),
    }
}

// DTO for updating documentation pages (partial update)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UpdateDocumentationPageRequest {
    pub title: Option<String>,
    pub slug: Option<String>,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: Option<DocumentationStatus>,
    pub parent_id: Option<Option<i32>>,
    pub ticket_id: Option<Option<i32>>,
    pub display_order: Option<i32>,
    pub is_public: Option<bool>,
    pub is_template: Option<bool>,
    pub content: Option<Vec<u8>>,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

// Update an existing documentation page
pub async fn update_documentation_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
    page: web::Json<UpdateDocumentationPageRequest>,
) -> impl Responder {
    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can update documentation pages"
        }));
    }

    // Check if the page exists and get its current state
    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(_existing_page) => {
            // Get the user UUID for last_edited_by
            let user_uuid = match utils::parse_uuid(&claims.sub) {
                Ok(uuid) => uuid,
                Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
            };

            // Create update struct with the fields from the request
            let update_req = page.into_inner();
            let now = chrono::Utc::now().naive_utc();

            // Compute archived_at and deleted_at based on status change
            let (archived_at, deleted_at) = match update_req.status {
                Some(DocumentationStatus::Archived) => (Some(Some(now)), Some(None)),
                Some(DocumentationStatus::Deleted) => (Some(None), Some(Some(now))),
                Some(DocumentationStatus::Draft) | Some(DocumentationStatus::Published) => (Some(None), Some(None)),
                None => (None, None),
            };

            // Auto-regenerate slug when title changes (unless user explicitly provided a slug)
            let slug = if update_req.slug.is_some() {
                update_req.slug.clone()
            } else if let Some(ref new_title) = update_req.title {
                Some(utils::slug::generate_unique_slug(new_title, &mut conn))
            } else {
                None
            };

            let page_update = crate::models::DocumentationPageUpdate {
                title: update_req.title.clone(),
                slug,
                icon: update_req.icon.clone(),
                cover_image: update_req.cover_image,
                status: update_req.status,
                last_edited_by: Some(user_uuid),
                parent_id: update_req.parent_id,
                ticket_id: update_req.ticket_id,
                display_order: update_req.display_order,
                is_public: update_req.is_public,
                is_template: update_req.is_template,
                archived_at,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: None,
                updated_at: Some(now),
                deleted_at,
            };

            // Update the page
            match repository::update_documentation_page(&mut conn, page_id, &page_update) {
                Ok(updated_page) => {
                    debug!(page_id = updated_page.id, "Documentation page updated");

                    // Re-index the updated documentation page in search
                    indexing_tasks::spawn_index_documentation(search_service.get_ref().clone(), updated_page.clone());

                    // Broadcast SSE events for each updated field
                    if let Some(ref title) = update_req.title {
                        crate::utils::sse::SseBroadcaster::broadcast_documentation_updated(
                            &sse_state,
                            page_id,
                            "title",
                            serde_json::json!(title),
                            &claims.sub,
                        ).await;
                    }
                    if let Some(ref slug) = update_req.slug {
                        crate::utils::sse::SseBroadcaster::broadcast_documentation_updated(
                            &sse_state,
                            page_id,
                            "slug",
                            serde_json::json!(slug),
                            &claims.sub,
                        ).await;
                    }
                    if let Some(ref icon) = update_req.icon {
                        crate::utils::sse::SseBroadcaster::broadcast_documentation_updated(
                            &sse_state,
                            page_id,
                            "icon",
                            serde_json::json!(icon),
                            &claims.sub,
                        ).await;
                    }
                    if let Some(ref status) = update_req.status {
                        crate::utils::sse::SseBroadcaster::broadcast_documentation_updated(
                            &sse_state,
                            page_id,
                            "status",
                            serde_json::json!(status),
                            &claims.sub,
                        ).await;
                    }

                    match to_page_response(updated_page, &mut conn) {
                        Ok(response) => HttpResponse::Ok().json(response),
                        Err(err) => HttpResponse::InternalServerError().json(err),
                    }
                },
                Err(e) => {
                    error!(page_id = page_id, error = ?e, "Error updating documentation page");
                    HttpResponse::InternalServerError().json("Failed to update documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}

// Delete a documentation page (soft delete — moves to trash)
pub async fn delete_documentation_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
) -> impl Responder {
    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can delete documentation pages"
        }));
    }

    // Check if the page exists
    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(_) => {
            // Soft delete: update status to Deleted and set deleted_at
            let now = chrono::Utc::now().naive_utc();
            let page_update = crate::models::DocumentationPageUpdate {
                title: None,
                slug: None,
                icon: None,
                cover_image: None,
                status: Some(DocumentationStatus::Deleted),
                last_edited_by: None,
                parent_id: None,
                ticket_id: None,
                display_order: None,
                is_public: None,
                is_template: None,
                archived_at: Some(None),
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: None,
                updated_at: Some(now),
                deleted_at: Some(Some(now)),
            };

            match repository::update_documentation_page(&mut conn, page_id, &page_update) {
                Ok(_) => {
                    // Remove documentation from search index (trashed pages shouldn't appear in search)
                    indexing_tasks::spawn_delete_documentation(search_service.get_ref().clone(), page_id);
                    info!(page_id = page_id, deleted_by = %claims.name, "Documentation page moved to trash");
                    HttpResponse::NoContent().finish()
                },
                Err(e) => {
                    error!(page_id = page_id, error = ?e, "Error soft-deleting documentation page");
                    HttpResponse::InternalServerError().json("Failed to delete documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}

// Get top-level documentation pages
pub async fn get_top_level_documentation_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_top_level_pages(&mut conn) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch top-level pages"),
    }
}

// Get documentation pages by parent ID
pub async fn get_documentation_pages_by_parent_id(
    req: HttpRequest,
    parent_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let parent = parent_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_pages_by_parent_id(parent, &mut conn) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch pages by parent ID"),
    }
}

// Get a page with its children by parent ID
pub async fn get_page_with_children_by_parent_id(
    req: HttpRequest,
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    // First get the page
    let page = match repository::get_documentation_page(page_id, &mut conn) {
        Ok(page) => page,
        Err(_) => return HttpResponse::NotFound().json("Page not found"),
    };

    match repository::can_user_access_page(&mut conn, page.id, &user_uuid, is_admin(&claims)) {
        Ok(true) => {},
        Ok(false) => return HttpResponse::NotFound().json("Page not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
    }

    // Then get its children (filtered by access)
    let children = match repository::get_pages_by_parent_id(page_id, &mut conn) {
        Ok(children) => match repository::filter_pages_for_user(&mut conn, children, &user_uuid, is_admin(&claims)) {
            Ok(c) => c,
            Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
        },
        Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch children"),
    };

    let page_with_children = DocumentationPageWithChildren {
        page,
        children,
    };

    HttpResponse::Ok().json(page_with_children)
}

// Get a page with its ordered children
pub async fn get_page_with_ordered_children(
    req: HttpRequest,
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_page_with_ordered_children(&mut conn, page_id) {
        Ok(mut page_with_children) => {
            match repository::can_user_access_page(&mut conn, page_with_children.page.id, &user_uuid, is_admin(&claims)) {
                Ok(true) => {},
                Ok(false) => return HttpResponse::NotFound().json("Page not found or error fetching children"),
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            }
            // Filter children
            page_with_children.children = match repository::filter_pages_for_user(&mut conn, page_with_children.children, &user_uuid, is_admin(&claims)) {
                Ok(c) => c,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            HttpResponse::Ok().json(page_with_children)
        },
        Err(_) => HttpResponse::NotFound().json("Page not found or error fetching children"),
    }
}

// Get ordered documentation pages by parent ID
pub async fn get_ordered_pages_by_parent_id(
    req: HttpRequest,
    parent_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let parent = parent_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_ordered_pages_by_parent_id(&mut conn, parent) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch ordered pages by parent ID"),
    }
}

#[derive(Deserialize)]
pub struct ReorderPagesRequest {
    pub parent_id: i32,
    pub page_orders: Vec<crate::models::PageOrder>,
}

// Reorder pages under a parent
pub async fn reorder_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
    request: web::Json<ReorderPagesRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can reorder documentation pages"
        }));
    }

    match repository::reorder_pages(&mut conn, Some(request.parent_id), &request.page_orders) {
        Ok(updated_pages) => HttpResponse::Ok().json(updated_pages),
        Err(e) => {
            error!(parent_id = request.parent_id, error = ?e, "Error reordering pages");
            HttpResponse::InternalServerError().json("Failed to reorder pages")
        }
    }
}

#[derive(Deserialize)]
pub struct MovePageRequest {
    pub page_id: i32,
    pub new_parent_id: Option<i32>,
    pub display_order: Option<i32>,
}

// Move a page to a new parent
pub async fn move_page_to_parent(
    req: HttpRequest,
    pool: web::Data<Pool>,
    request: web::Json<MovePageRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can move documentation pages"
        }));
    }

    let display_order = request.display_order.unwrap_or(0);

    // Validation: Cannot move a page to be its own parent
    if request.new_parent_id == Some(request.page_id) {
        return HttpResponse::BadRequest().json(json!({
            "error": "Invalid operation",
            "message": "A page cannot be its own parent"
        }));
    }

    match repository::move_page_to_parent(&mut conn, request.page_id, request.new_parent_id, display_order) {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(diesel::result::Error::RollbackTransaction) => {
            HttpResponse::BadRequest().json(json!({
                "error": "Circular reference",
                "message": "Cannot move a page to be a child of its own descendant"
            }))
        }
        Err(e) => {
            error!(page_id = request.page_id, new_parent_id = ?request.new_parent_id, error = ?e, "Error moving page");
            HttpResponse::InternalServerError().json(json!({
                "error": "Internal server error",
                "message": "Failed to move page to new parent"
            }))
        }
    }
}

// Get top-level pages (with ordering)
pub async fn get_ordered_top_level_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_ordered_top_level_pages(&mut conn) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch top-level pages"),
    }
}

// Get documentation page by slug with its children
pub async fn get_documentation_page_by_slug_with_children(
    req: HttpRequest,
    slug: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let page_slug = slug.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    // First get the page by slug
    let page = match repository::get_documentation_page_by_slug(&page_slug, &mut conn) {
        Ok(page) => page,
        Err(_) => return HttpResponse::NotFound().json("Page not found"),
    };

    match repository::can_user_access_page(&mut conn, page.id, &user_uuid, is_admin(&claims)) {
        Ok(true) => {},
        Ok(false) => return HttpResponse::NotFound().json("Page not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
    }

    // Then get its children (filtered by access)
    let children = match repository::get_pages_by_parent_id(page.id, &mut conn) {
        Ok(children) => match repository::filter_pages_for_user(&mut conn, children, &user_uuid, is_admin(&claims)) {
            Ok(c) => c,
            Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
        },
        Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch children"),
    };

    let page_with_children = DocumentationPageWithChildren {
        page,
        children,
    };

    HttpResponse::Ok().json(page_with_children)
}

// Get documentation pages for a ticket
pub async fn get_documentation_pages_by_ticket_id(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let ticket_id = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_documentation_pages_by_ticket_id(&mut conn, ticket_id) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            debug!(ticket_id = ticket_id, count = pages.len(), "Found documentation pages for ticket");
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(e) => {
            error!(ticket_id = ticket_id, error = ?e, "Error fetching documentation pages for ticket");
            HttpResponse::InternalServerError().json("Failed to fetch documentation pages")
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CreateDocPageFromTicket {
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub parent_id: Option<i32>,
}

// Response struct for documentation export (minimal fields needed for markdown export)
#[derive(Debug, serde::Serialize)]
pub struct DocumentationPageExport {
    pub id: i32,
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub status: DocumentationStatus,
    pub yjs_document: Option<Vec<u8>>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

// Export all documentation pages with their Yjs content for markdown export
pub async fn export_documentation_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware (require authentication)
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can export documentation"
        }));
    }

    match repository::get_documentation_pages(&mut conn) {
        Ok(pages) => {
            let export_pages: Vec<DocumentationPageExport> = pages.into_iter().map(|page| {
                DocumentationPageExport {
                    id: page.id,
                    uuid: page.uuid,
                    title: page.title,
                    slug: page.slug,
                    icon: page.icon,
                    parent_id: page.parent_id,
                    display_order: page.display_order,
                    status: page.status,
                    yjs_document: page.yjs_document,
                    created_at: page.created_at,
                    updated_at: page.updated_at,
                }
            }).collect();
            HttpResponse::Ok().json(export_pages)
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch pages for export"),
    }
}

// Export a single documentation page as Markdown
pub async fn export_page_as_markdown(
    page_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let id = page_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"})),
    };

    let page = match repository::get_documentation_page(id, &mut conn) {
        Ok(p) => p,
        Err(_) => return HttpResponse::NotFound().json(json!({"error": "Page not found"})),
    };

    // Get yjs_document - try page's own, then fall back to linked ticket
    let yjs_doc = page.yjs_document.as_ref()
        .cloned()
        .or_else(|| {
            page.ticket_id.and_then(|tid| {
                repository::get_article_content_by_ticket_id(&mut conn, tid)
                    .ok()
                    .and_then(|a| a.yjs_document)
            })
        });

    let markdown = match yjs_doc {
        Some(doc_bytes) => {
            let mut visited = std::collections::HashSet::new();
            utils::markdown_export::yjs_to_markdown_with_embeds(
                &doc_bytes,
                &mut conn,
                &mut visited,
                Some(page.uuid),
                0,
            ).unwrap_or_else(|| String::from("*Empty document*"))
        }
        None => String::from("*Empty document*"),
    };

    // Add title as H1 header
    let full_markdown = format!("# {}\n\n{}", page.title, markdown);

    let filename = format!("{}.md", page.slug);

    HttpResponse::Ok()
        .content_type("text/markdown; charset=utf-8")
        .insert_header(("Content-Disposition", format!("attachment; filename=\"{}\"", filename)))
        .body(full_markdown)
}

// Create a documentation page from a ticket's article content
pub async fn create_documentation_page_from_ticket(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    page_data: web::Json<CreateDocPageFromTicket>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can create documentation pages from tickets"
        }));
    }

    // Check if a documentation page already exists for this ticket
    match repository::get_documentation_pages_by_ticket_id(&mut conn, ticket_id) {
        Ok(existing_pages) => {
            if let Some(existing_page) = existing_pages.into_iter().next() {
                // Return the existing page instead of creating a new one
                return HttpResponse::Ok().json(existing_page);
            }
        }
        Err(_) => {
            // No existing pages found, continue to create a new one
        }
    }

    // Get the ticket's article content (for Yjs document cloning)
    let article_content = match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(content) => Some(content),
        Err(_) => None, // Article content may not exist yet - allow creation without it
    };

    // Generate a unique slug from the title
    let slug = utils::slug::generate_unique_slug(&page_data.title, &mut conn);

    // Get the user UUID for created_by and last_edited_by
    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
    };

    // Clone Yjs document data from ticket's article content if available
    let (yjs_state_vector, yjs_document, yjs_client_id) = match &article_content {
        Some(content) => (
            content.yjs_state_vector.clone(),
            content.yjs_document.clone(),
            content.yjs_client_id,
        ),
        None => (None, None, None),
    };

    let new_page = NewDocumentationPage {
        uuid: Uuid::now_v7(),
        title: page_data.title.clone(),
        slug,
        icon: page_data.icon.clone(),
        cover_image: None,
        status: DocumentationStatus::Draft,
        created_by: user_uuid,
        last_edited_by: user_uuid,
        parent_id: page_data.parent_id,
        ticket_id: Some(ticket_id),
        display_order: Some(0),
        is_public: false,
        is_template: false,
        yjs_state_vector,
        yjs_document,
        yjs_client_id,
        has_unsaved_changes: false,
    };

    // Create the documentation page
    match repository::create_documentation_page(new_page, &mut conn) {
        Ok(page) => {
            // Auto-add to "Tickets" system collection
            if let Ok(tickets_collection) = repository::documentation_collections::get_collection_by_slug(&mut conn, "tickets") {
                let entry = crate::models::NewDocumentationCollectionPage {
                    collection_id: tickets_collection.id,
                    page_id: page.id,
                    created_by: Some(user_uuid),
                };
                if let Err(e) = repository::documentation_collections::add_page_to_collection(&mut conn, entry) {
                    error!(error = ?e, "Failed to add page to Tickets collection");
                }
            }

            // Broadcast SSE event for documentation creation
            use crate::utils::sse::SseBroadcaster;
            SseBroadcaster::broadcast_documentation_created(
                &sse_state,
                page.id,
                serde_json::to_value(&page).unwrap_or_default(),
            ).await;

            HttpResponse::Created().json(page)
        },
        Err(e) => {
            error!(ticket_id = ticket_id, error = ?e, "Error creating documentation page from ticket");
            HttpResponse::InternalServerError().json("Failed to create documentation page")
        }
    }
}

// Get archived documentation pages
pub async fn get_archived_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_pages_by_status(&mut conn, DocumentationStatus::Archived) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch archived pages"),
    }
}

// Get trashed (soft-deleted) documentation pages
pub async fn get_trashed_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return HttpResponse::InternalServerError().json("Invalid user UUID"),
    };

    match repository::get_pages_by_status(&mut conn, DocumentationStatus::Deleted) {
        Ok(pages) => {
            let pages = match repository::filter_pages_for_user(&mut conn, pages, &user_uuid, is_admin(&claims)) {
                Ok(p) => p,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to check page visibility"),
            };
            match to_page_responses(pages, &mut conn) {
                Ok(responses) => HttpResponse::Ok().json(responses),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to fetch trashed pages"),
    }
}

// ============================================================================
// Page Visibility (Access Control)
// ============================================================================

/// Get visibility groups for a documentation page (technician+)
pub async fn get_page_visibility(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let _claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&_claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can view page visibility"
        }));
    }

    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let groups = match repository::get_visible_groups_for_page(&mut conn, page_id) {
        Ok(g) => g,
        Err(e) => {
            error!(error = ?e, "Failed to get page visibility groups");
            return HttpResponse::InternalServerError().json("Failed to get page visibility");
        }
    };

    let users = match repository::get_visible_users_for_page(&mut conn, page_id) {
        Ok(u) => u,
        Err(e) => {
            error!(error = ?e, "Failed to get page visibility users");
            return HttpResponse::InternalServerError().json("Failed to get page visibility");
        }
    };

    HttpResponse::Ok().json(serde_json::json!({
        "groups": groups,
        "users": users,
    }))
}

#[derive(Deserialize)]
pub struct SetPageVisibilityRequest {
    pub group_ids: Vec<i32>,
    pub user_uuids: Option<Vec<String>>,
}

/// Set visibility groups for a documentation page (admin only)
pub async fn set_page_visibility(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetPageVisibilityRequest>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can set page visibility"
        }));
    }

    let page_id = path.into_inner();
    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Parse user UUIDs
    let user_uuids: Vec<Uuid> = body.user_uuids.as_ref()
        .map(|uuids| {
            uuids.iter()
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect()
        })
        .unwrap_or_default();

    match repository::set_page_visibility(&mut conn, page_id, body.group_ids.clone(), user_uuids, created_by) {
        Ok(entries) => HttpResponse::Ok().json(entries),
        Err(e) => {
            error!(error = ?e, "Failed to set page visibility");
            HttpResponse::InternalServerError().json("Failed to set page visibility")
        }
    }
}

// Restore a page from archive or trash back to draft
pub async fn restore_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
) -> impl Responder {
    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can restore documentation pages"
        }));
    }

    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(_) => {
            let now = chrono::Utc::now().naive_utc();
            let page_update = crate::models::DocumentationPageUpdate {
                title: None,
                slug: None,
                icon: None,
                cover_image: None,
                status: Some(DocumentationStatus::Draft),
                last_edited_by: None,
                parent_id: None,
                ticket_id: None,
                display_order: None,
                is_public: None,
                is_template: None,
                archived_at: Some(None),
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: None,
                updated_at: Some(now),
                deleted_at: Some(None),
            };

            match repository::update_documentation_page(&mut conn, page_id, &page_update) {
                Ok(restored_page) => {
                    // Re-index in search
                    indexing_tasks::spawn_index_documentation(search_service.get_ref().clone(), restored_page.clone());
                    info!(page_id = page_id, restored_by = %claims.name, "Documentation page restored");
                    match to_page_response(restored_page, &mut conn) {
                        Ok(response) => HttpResponse::Ok().json(response),
                        Err(err) => HttpResponse::InternalServerError().json(err),
                    }
                },
                Err(e) => {
                    error!(page_id = page_id, error = ?e, "Error restoring documentation page");
                    HttpResponse::InternalServerError().json("Failed to restore documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}

// Permanently delete a documentation page (hard delete, admin only)
pub async fn permanently_delete_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
) -> impl Responder {
    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can permanently delete documentation pages"
        }));
    }

    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(_) => {
            match repository::permanently_delete_page(page_id, &mut conn) {
                Ok(_) => {
                    indexing_tasks::spawn_delete_documentation(search_service.get_ref().clone(), page_id);
                    info!(page_id = page_id, deleted_by = %claims.name, "Documentation page permanently deleted");
                    HttpResponse::NoContent().finish()
                },
                Err(e) => {
                    error!(page_id = page_id, error = ?e, "Error permanently deleting documentation page");
                    HttpResponse::InternalServerError().json("Failed to permanently delete documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}