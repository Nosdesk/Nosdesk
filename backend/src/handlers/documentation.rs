use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use chrono::Utc;
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
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_pages(&mut conn) {
        Ok(pages) => {
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
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(page) => {
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
    slug: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_slug = slug.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_page_by_slug(&page_slug, &mut conn) {
        Ok(page) => {
            match to_page_response(page, &mut conn) {
                Ok(response) => HttpResponse::Ok().json(response),
                Err(err) => HttpResponse::InternalServerError().json(err),
            }
        },
        Err(_) => HttpResponse::NotFound().json("Page not found"),
    }
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
    let new_page = NewDocumentationPage {
        uuid: Uuid::now_v7(),
        title: request.title,
        slug: None, // Will be generated by the database or repository
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
        Ok(existing_page) => {
            // Get the user UUID for last_edited_by
            let user_uuid = match utils::parse_uuid(&claims.sub) {
                Ok(uuid) => uuid,
                Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
            };

            // Create update struct with the fields from the request
            let update_req = page.into_inner();
            let page_update = crate::models::DocumentationPageUpdate {
                title: update_req.title.clone(),
                slug: update_req.slug.clone(),
                icon: update_req.icon.clone(),
                cover_image: update_req.cover_image,
                status: update_req.status,
                last_edited_by: Some(user_uuid),
                parent_id: update_req.parent_id,
                ticket_id: update_req.ticket_id,
                display_order: update_req.display_order,
                is_public: update_req.is_public,
                is_template: update_req.is_template,
                archived_at: None,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: None,
                updated_at: Some(chrono::Utc::now().naive_utc()),
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

// Delete a documentation page
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

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can delete documentation pages"
        }));
    }

    // Check if the page exists
    match repository::get_documentation_page(page_id, &mut conn) {
        Ok(_) => {
            // Delete the page
            match repository::delete_documentation_page(page_id, &mut conn) {
                Ok(_) => {
                    // Remove documentation from search index
                    indexing_tasks::spawn_delete_documentation(search_service.get_ref().clone(), page_id);
                    info!(page_id = page_id, deleted_by = %claims.name, "Documentation page deleted");
                    HttpResponse::NoContent().finish()
                },
                Err(e) => {
                    error!(page_id = page_id, error = ?e, "Error deleting documentation page");
                    HttpResponse::InternalServerError().json("Failed to delete documentation page")
                },
            }
        },
        Err(_) => HttpResponse::NotFound().json("Documentation page not found"),
    }
}

// Get top-level documentation pages
pub async fn get_top_level_documentation_pages(
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_top_level_pages(&mut conn) {
        Ok(pages) => {
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
    parent_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let parent = parent_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_pages_by_parent_id(parent, &mut conn) {
        Ok(pages) => {
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
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // First get the page
    let page = match repository::get_documentation_page(page_id, &mut conn) {
        Ok(page) => page,
        Err(_) => return HttpResponse::NotFound().json("Page not found"),
    };

    // Then get its children
    let children = match repository::get_pages_by_parent_id(page_id, &mut conn) {
        Ok(children) => children,
        Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch children"),
    };

    // Combine into a single response
    let page_with_children = DocumentationPageWithChildren {
        page,
        children,
    };

    HttpResponse::Ok().json(page_with_children)
}

// Get a page with its ordered children
pub async fn get_page_with_ordered_children(
    id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_page_with_ordered_children(&mut conn, page_id) {
        Ok(page_with_children) => HttpResponse::Ok().json(page_with_children),
        Err(_) => HttpResponse::NotFound().json("Page not found or error fetching children"),
    }
}

// Get ordered documentation pages by parent ID
pub async fn get_ordered_pages_by_parent_id(
    parent_id: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let parent = parent_id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_ordered_pages_by_parent_id(&mut conn, parent) {
        Ok(pages) => {
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
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get all top-level pages with appropriate ordering
    match repository::get_ordered_top_level_pages(&mut conn) {
        Ok(pages) => {
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
    slug: web::Path<String>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let page_slug = slug.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // First get the page by slug
    let page = match repository::get_documentation_page_by_slug(&page_slug, &mut conn) {
        Ok(page) => page,
        Err(_) => return HttpResponse::NotFound().json("Page not found"),
    };

    // Then get its children
    let children = match repository::get_pages_by_parent_id(page.id, &mut conn) {
        Ok(children) => children,
        Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch children"),
    };

    // Combine into a single response
    let page_with_children = DocumentationPageWithChildren {
        page,
        children,
    };

    HttpResponse::Ok().json(page_with_children)
}

// Get documentation pages for a ticket
pub async fn get_documentation_pages_by_ticket_id(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();

    // Get a connection from the pool
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_documentation_pages_by_ticket_id(&mut conn, ticket_id) {
        Ok(pages) => {
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
    pub slug: Option<String>,
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

    // Get the ticket's article content
    let article_content = match repository::get_article_content_by_ticket_id(&mut conn, ticket_id) {
        Ok(content) => content,
        Err(_) => return HttpResponse::NotFound().json("Article content not found for ticket"),
    };

    // Generate a slug from the title
    let slug = page_data.title.to_lowercase().replace(" ", "-");

    let now = Utc::now().naive_utc();
    
    // Get the user UUID for created_by and last_edited_by
    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
    };

    let new_page = NewDocumentationPage {
        uuid: Uuid::now_v7(),
        title: page_data.title.clone(),
        slug: Some(slug),
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
        yjs_state_vector: None,
        yjs_document: None,
        yjs_client_id: None,
        has_unsaved_changes: false,
    };

    // Create the documentation page
    match repository::create_documentation_page(new_page, &mut conn) {
        Ok(page) => {
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