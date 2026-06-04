use actix_web::{web, HttpRequest, HttpResponse, Responder};
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use std::panic;
use std::sync::Arc;
use tracing::{debug, error, info};
use uuid::Uuid;
use yrs::{
    updates::decoder::Decode, Doc, GetString, Options, ReadTxn, Transact, Update, WriteTxn,
    XmlFragment, XmlOut,
};

use crate::db::{DbConnection, Pool};
use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{
    DocumentationPage, DocumentationPageResponse, DocumentationPageTicketEmbed,
    DocumentationPageWithChildren, DocumentationStatus, NewDocumentationPage, UserInfoWithAvatar,
};
use crate::repository;
use crate::repository::documentation_starred_pages;
use crate::repository::documentation_subscriptions;
use crate::services::notifications::{
    types::{NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode},
    NotificationService,
};
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;
use crate::utils;

/// Collect text from an iterator of XmlOut children
fn collect_children_text(children: impl Iterator<Item = XmlOut>, txn: &yrs::Transaction) -> String {
    let mut text = String::new();
    for child in children {
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

/// Recursively extract plain text from an XmlOut node
fn extract_text_from_xml_node(node: &XmlOut, txn: &yrs::Transaction) -> String {
    match node {
        XmlOut::Text(text_ref) => {
            let s: String =
                panic::catch_unwind(panic::AssertUnwindSafe(|| text_ref.get_string(txn)))
                    .unwrap_or_default();
            s
        }
        XmlOut::Element(elem_ref) => collect_children_text(elem_ref.children(txn), txn),
        XmlOut::Fragment(frag_ref) => collect_children_text(frag_ref.children(txn), txn),
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
            let normalized = whitespace_regex
                .replace_all(&clean_text, " ")
                .trim()
                .to_string();
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
    /// Target collection for the new page. When omitted, the
    /// page is auto-assigned to its parent's collection (if
    /// `parent_id` is set), otherwise it lands as uncollected.
    /// Both branches preserve the page's `parent_id` — the
    /// "add existing page to collection" flow is the only path
    /// that resets parent_id to root.
    pub collection_id: Option<i32>,
}

/// Resolve the Yjs document for a page: try the page's own yjs_document
/// first, then fall back to the most-recently-linked 'resolves' ticket's
/// article content. Pre-Phase-1 pages were authored from a ticket and
/// stored their content in the ticket's article_content row; the join
/// table now expresses that relationship as a 'resolves' link.
fn resolve_yjs_document(page: &DocumentationPage, conn: &mut DbConnection) -> Option<Vec<u8>> {
    page.yjs_document.clone().or_else(|| {
        repository::documentation_page_tickets::most_recent_resolves_ticket_id(conn, page.id)
            .ok()
            .flatten()
            .and_then(|tid| repository::get_article_content_by_ticket_id(conn, tid).ok())
            .and_then(|a| a.yjs_document)
    })
}

/// True when the page has been verified with an interval set and the
/// interval has elapsed. Pages without an interval are evergreen and
/// never stale.
fn is_page_stale(page: &DocumentationPage) -> bool {
    match (page.verified_at, page.verify_interval_days) {
        (Some(verified_at), Some(days)) => {
            let now = chrono::Utc::now().naive_utc();
            now > verified_at + chrono::Duration::days(days as i64)
        }
        _ => false,
    }
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
    let content = resolve_yjs_document(&page, conn).and_then(|doc| extract_yjs_content(&doc));

    // Verifier user info, only fetched when the page has been
    // verified. The DB stores the uuid; the response embeds the
    // user's display info so the frontend doesn't need a second
    // round-trip to render the banner.
    let verified_by = page.verified_by.and_then(|uuid| {
        repository::get_user_by_uuid(&uuid, conn)
            .ok()
            .map(|u| UserInfoWithAvatar {
                uuid: u.uuid,
                name: u.name,
                avatar_url: u.avatar_url,
                avatar_thumb: u.avatar_thumb,
            })
    });
    let is_stale = is_page_stale(&page);

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
        display_order: page.display_order,
        is_public: page.is_public,
        is_template: page.is_template,
        archived_at: page.archived_at,
        deleted_at: page.deleted_at,
        has_unsaved_changes: page.has_unsaved_changes,
        children: None,
        content,
        verified_by,
        verified_at: page.verified_at,
        verify_interval_days: page.verify_interval_days,
        is_stale,
        linked_tickets: None,
    })
}

/// Hydrate the linked_tickets field on a page response by querying
/// the join table and pulling ticket title + status in one batch.
/// Mirrors the standalone list_page_tickets handler so callers can
/// choose between embed (one round trip) and a separate fetch
/// (cheaper for views that don't always want it).
fn embed_page_tickets(
    response: &mut DocumentationPageResponse,
    conn: &mut DbConnection,
) -> Result<(), String> {
    let links = repository::documentation_page_tickets::links_for_page(conn, response.id)
        .map_err(|e| format!("Failed to load page<->ticket links: {e:?}"))?;

    use crate::schema::tickets;
    use diesel::prelude::*;
    let ticket_ids: Vec<i32> = links.iter().map(|l| l.ticket_id).collect();
    let tickets_meta: std::collections::HashMap<
        i32,
        (String, crate::models::WorkflowStateCategory),
    > = if ticket_ids.is_empty() {
        Default::default()
    } else {
        let rows: Vec<(i32, String, i32)> = tickets::table
            .filter(tickets::id.eq_any(&ticket_ids))
            .select((tickets::id, tickets::title, tickets::workflow_state_id))
            .load(conn)
            .map_err(|e| format!("Failed to hydrate tickets: {e:?}"))?;
        rows.into_iter()
            .map(|(id, title, ws_id)| {
                let cat = crate::repository::workflow_states::category_of(conn, ws_id)
                    .ok()
                    .flatten()
                    .unwrap_or(crate::models::WorkflowStateCategory::Backlog);
                (id, (title, cat))
            })
            .collect()
    };

    let embed: Vec<DocumentationPageTicketEmbed> = links
        .into_iter()
        .map(|l| {
            let meta = tickets_meta.get(&l.ticket_id);
            DocumentationPageTicketEmbed {
                ticket_id: l.ticket_id,
                link_type: l.link_type,
                created_at: l.created_at,
                ticket_title: meta.map(|m| m.0.clone()),
                ticket_category: meta.map(|m| m.1),
            }
        })
        .collect();
    response.linked_tickets = Some(embed);
    Ok(())
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
pub async fn get_documentation_pages(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    let is_admin_user = auth.is_admin();
    let user_uuid = auth.user_uuid;

    let result = tc.run(|conn| {
        let pages = repository::get_documentation_pages(conn)?;
        let pages = repository::filter_pages_for_user(conn, pages, &user_uuid, is_admin_user)?;
        let responses = to_page_responses(pages, conn).map_err(|_| {
            diesel::result::Error::QueryBuilderError("Failed to build page responses".into())
        })?;
        Ok::<_, diesel::result::Error>(responses)
    });

    match result {
        Ok(responses) => HttpResponse::Ok().json(responses),
        Err(_) => errors::internal("Failed to fetch pages"),
    }
}

/// Query params for `GET /documentation/pages/{id}`. The `embed`
/// param is a comma-separated list — currently only `tickets` is
/// recognised, but the parser is forward-compatible.
#[derive(Debug, Deserialize)]
pub struct GetDocumentationPageQuery {
    pub embed: Option<String>,
}

fn embed_includes(embed: &Option<String>, key: &str) -> bool {
    embed
        .as_deref()
        .map(|s| s.split(',').map(str::trim).any(|t| t == key))
        .unwrap_or(false)
}

/// Outcome of loading + access-gating a documentation page.
/// Lets the inner `tc.run` closure surface user-visible HTTP-style
/// branches (404, access-check failure) alongside the success
/// payload without leaking `HttpResponse` into the txn closure.
enum PageLoadOutcome {
    Ok(DocumentationPageResponse),
    NotFound,
    VisibilityCheckFailed,
    ResponseBuildFailed(String),
    EmbedFailed(String),
}

// Get a single documentation page by ID
pub async fn get_documentation_page(
    mut tc: TenantConn,
    id: web::Path<i32>,
    query: web::Query<GetDocumentationPageQuery>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = id.into_inner();
    let want_tickets = embed_includes(&query.embed, "tickets");
    let is_admin_user = auth.is_admin();
    let user_uuid = auth.user_uuid;

    let outcome = tc.run(|conn| {
        let page = match repository::get_documentation_page(page_id, conn) {
            Ok(p) => p,
            Err(_) => return Ok(PageLoadOutcome::NotFound),
        };
        match repository::can_user_access_page(conn, page.id, &user_uuid, is_admin_user) {
            Ok(true) => {}
            Ok(false) => return Ok(PageLoadOutcome::NotFound),
            Err(_) => return Ok(PageLoadOutcome::VisibilityCheckFailed),
        }
        let mut response = match to_page_response(page, conn) {
            Ok(r) => r,
            Err(e) => return Ok(PageLoadOutcome::ResponseBuildFailed(e)),
        };
        if want_tickets {
            if let Err(e) = embed_page_tickets(&mut response, conn) {
                return Ok(PageLoadOutcome::EmbedFailed(e));
            }
        }
        Ok::<_, diesel::result::Error>(PageLoadOutcome::Ok(response))
    });

    match outcome {
        Ok(PageLoadOutcome::Ok(resp)) => HttpResponse::Ok().json(resp),
        Ok(PageLoadOutcome::NotFound) => errors::not_found_msg("Page not found"),
        Ok(PageLoadOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Ok(PageLoadOutcome::ResponseBuildFailed(err)) => {
            HttpResponse::InternalServerError().json(err)
        }
        Ok(PageLoadOutcome::EmbedFailed(err)) => HttpResponse::InternalServerError().json(err),
        Err(_) => errors::internal("Failed to load page"),
    }
}

// Get a documentation page by its slug
pub async fn get_documentation_page_by_slug(
    mut tc: TenantConn,
    slug: web::Path<String>,
    query: web::Query<GetDocumentationPageQuery>,
    auth: AuthContext,
) -> impl Responder {
    let page_slug = slug.into_inner();
    let want_tickets = embed_includes(&query.embed, "tickets");
    let is_admin_user = auth.is_admin();
    let user_uuid = auth.user_uuid;

    let outcome = tc.run(|conn| {
        let page = match repository::get_documentation_page_by_slug(&page_slug, conn) {
            Ok(p) => p,
            Err(_) => return Ok(PageLoadOutcome::NotFound),
        };
        match repository::can_user_access_page(conn, page.id, &user_uuid, is_admin_user) {
            Ok(true) => {}
            Ok(false) => return Ok(PageLoadOutcome::NotFound),
            Err(_) => return Ok(PageLoadOutcome::VisibilityCheckFailed),
        }
        let mut response = match to_page_response(page, conn) {
            Ok(r) => r,
            Err(e) => return Ok(PageLoadOutcome::ResponseBuildFailed(e)),
        };
        if want_tickets {
            if let Err(e) = embed_page_tickets(&mut response, conn) {
                return Ok(PageLoadOutcome::EmbedFailed(e));
            }
        }
        Ok::<_, diesel::result::Error>(PageLoadOutcome::Ok(response))
    });

    match outcome {
        Ok(PageLoadOutcome::Ok(resp)) => HttpResponse::Ok().json(resp),
        Ok(PageLoadOutcome::NotFound) => errors::not_found_msg("Page not found"),
        Ok(PageLoadOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Ok(PageLoadOutcome::ResponseBuildFailed(err)) => {
            HttpResponse::InternalServerError().json(err)
        }
        Ok(PageLoadOutcome::EmbedFailed(err)) => HttpResponse::InternalServerError().json(err),
        Err(_) => errors::internal("Failed to load page"),
    }
}

/// Outcome enum mirroring `PageLoadOutcome` but carrying the UUID-
/// content payload, which is a raw JSON value rather than the
/// hydrated DocumentationPageResponse used elsewhere.
enum PageContentOutcome {
    Ok(serde_json::Value),
    NotFound,
    VisibilityCheckFailed,
}

// Get a documentation page's content by UUID (for embedding)
// Returns the Yjs document as base64 + metadata
pub async fn get_documentation_page_content_by_uuid(
    mut tc: TenantConn,
    uuid_path: web::Path<String>,
    auth: AuthContext,
) -> impl Responder {
    let uuid_str = uuid_path.into_inner();
    let page_uuid = match Uuid::parse_str(&uuid_str) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("Invalid UUID"),
    };
    let is_admin_user = auth.is_admin();
    let user_uuid = auth.user_uuid;

    let outcome = tc.run(|conn| {
        let page = match repository::get_documentation_page_by_uuid(&page_uuid, conn) {
            Ok(p) => p,
            Err(_) => return Ok(PageContentOutcome::NotFound),
        };
        match repository::can_user_access_page(conn, page.id, &user_uuid, is_admin_user) {
            Ok(true) => {}
            Ok(false) => return Ok(PageContentOutcome::NotFound),
            Err(_) => return Ok(PageContentOutcome::VisibilityCheckFailed),
        }

        use base64::{engine::general_purpose, Engine as _};
        let yjs_b64 =
            resolve_yjs_document(&page, conn).map(|doc| general_purpose::STANDARD.encode(&doc));

        Ok::<_, diesel::result::Error>(PageContentOutcome::Ok(json!({
            "uuid": page.uuid,
            "title": page.title,
            "icon": page.icon,
            "status": page.status,
            "yjs_document": yjs_b64,
        })))
    });

    match outcome {
        Ok(PageContentOutcome::Ok(payload)) => HttpResponse::Ok().json(payload),
        Ok(PageContentOutcome::NotFound) => errors::not_found_msg("Page not found"),
        Ok(PageContentOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Err(_) => errors::internal("Failed to load page"),
    }
}

// Sync the embedding references for a page
// Called by the frontend after saving, with the list of embedded document UUIDs
pub async fn sync_page_embeddings(
    mut tc: TenantConn,
    page_id: web::Path<i32>,
    body: web::Json<SyncEmbeddingsRequest>,
) -> impl Responder {
    let source_page_id = page_id.into_inner();
    let body = body.into_inner();

    let result = tc.run(|conn| {
        // Resolve UUIDs to page IDs
        let mut target_page_ids = Vec::new();
        for uuid_str in &body.embedded_uuids {
            if let Ok(uuid) = Uuid::parse_str(uuid_str) {
                if let Ok(page) = repository::get_documentation_page_by_uuid(&uuid, conn) {
                    target_page_ids.push(page.id);
                }
            }
        }
        repository::sync_page_embeddings(conn, source_page_id, &target_page_ids)?;
        Ok::<_, diesel::result::Error>(())
    });

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!("Failed to sync page embeddings: {}", e);
            errors::internal("Failed to sync embeddings")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SyncEmbeddingsRequest {
    pub embedded_uuids: Vec<String>,
}

// Create a new documentation page
pub async fn create_documentation_page(
    mut tc: TenantConn,
    page_request: web::Json<CreateDocumentationPageRequest>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can create documentation pages",
        );
    }

    let user_uuid = auth.user_uuid;
    let request = page_request.into_inner();

    // Parse status string to enum
    let status = match request.status.as_deref() {
        Some("published") => DocumentationStatus::Published,
        Some("archived") => DocumentationStatus::Archived,
        _ => DocumentationStatus::Draft,
    };

    let create_result = tc.run(|conn| {
        // Build the NewDocumentationPage from request
        let slug = utils::slug::generate_unique_slug(&request.title, conn);
        let new_page = NewDocumentationPage {
            uuid: Uuid::now_v7(),
            title: request.title.clone(),
            slug,
            icon: request.icon.clone(),
            cover_image: request.cover_image.clone(),
            status,
            created_by: user_uuid,
            last_edited_by: user_uuid,
            parent_id: request.parent_id,
            display_order: request.display_order.or(Some(0)),
            is_public: request.is_public.unwrap_or(false),
            is_template: request.is_template.unwrap_or(false),
            yjs_state_vector: request.yjs_state_vector.clone(),
            yjs_document: request.yjs_document.clone(),
            yjs_client_id: request.yjs_client_id,
            has_unsaved_changes: request.has_unsaved_changes.unwrap_or(false),
        };

        let created_page = repository::create_documentation_page(new_page, conn)?;

        // If the request named a ticket, record it as a
        // 'resolves' link. This keeps the legacy "create page
        // from ticket" flow one-call rather than forcing the
        // frontend to chase a follow-up POST.
        if let Some(tid) = request.ticket_id {
            if let Err(e) = repository::documentation_page_tickets::upsert_link(
                conn,
                created_page.id,
                tid,
                repository::documentation_page_tickets::LINK_RESOLVES,
                Some(user_uuid),
            ) {
                error!(error = ?e, page_id = created_page.id, ticket_id = tid, "Failed to create page<->ticket link");
            }
        }
        // Resolve target collection: explicit body field wins,
        // otherwise inherit from the parent page's collection.
        // Either way, write the junction row directly without
        // touching parent_id (which the create flow already set
        // correctly).
        let target_collection_id: Option<i32> = match request.collection_id {
            Some(id) => Some(id),
            None => match request.parent_id {
                Some(pid) => {
                    repository::documentation_collections::get_collections_for_page(conn, pid)
                        .ok()
                        .and_then(|cs| cs.first().map(|c| c.id))
                }
                None => None,
            },
        };
        if let Some(cid) = target_collection_id {
            let entry = crate::models::NewDocumentationCollectionPage {
                collection_id: cid,
                page_id: created_page.id,
                created_by: Some(user_uuid),
            };
            if let Err(e) =
                repository::documentation_collections::add_page_to_collection(conn, entry)
            {
                error!(error = ?e, page_id = created_page.id, collection_id = cid, "Failed to assign page to collection");
            }
        }

        let response = to_page_response(created_page.clone(), conn).map_err(|_| {
            diesel::result::Error::QueryBuilderError("Failed to build page response".into())
        })?;
        Ok::<_, diesel::result::Error>((created_page, response))
    });

    match create_result {
        Ok((created_page, response)) => {
            // Index the new documentation page in search
            indexing_tasks::spawn_index_documentation(
                search_service.get_ref().clone(),
                created_page.clone(),
            );

            // Broadcast SSE event for documentation creation
            sse_state
                .broadcast_event(crate::handlers::sse::SseEvent::DocumentationCreated {
                    document_id: created_page.id,
                    document: serde_json::to_value(&response).unwrap_or_default(),
                    timestamp: chrono::Utc::now(),
                })
                .await;

            HttpResponse::Created().json(response)
        }
        Err(_) => errors::internal("Failed to create page"),
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
    pub display_order: Option<i32>,
    pub is_public: Option<bool>,
    pub is_template: Option<bool>,
    pub content: Option<Vec<u8>>,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Outcome of the update_documentation_page transaction.
enum UpdatePageOutcome {
    Ok(DocumentationPage, DocumentationPageResponse),
    NotFound,
    UpdateFailed,
    ResponseBuildFailed(String),
}

// Update an existing documentation page
pub async fn update_documentation_page(
    mut tc: TenantConn,
    pool: web::Data<Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    notification_service: web::Data<NotificationService>,
    path: web::Path<i32>,
    page: web::Json<UpdateDocumentationPageRequest>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();

    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can update documentation pages",
        );
    }

    let user_uuid = auth.user_uuid;
    let actor_name = auth.name.clone();
    let actor_sub = user_uuid.to_string();
    let update_req = page.into_inner();
    let now = chrono::Utc::now().naive_utc();

    // Compute archived_at and deleted_at based on status change
    let (archived_at, deleted_at) = match update_req.status {
        Some(DocumentationStatus::Archived) => (Some(Some(now)), Some(None)),
        Some(DocumentationStatus::Deleted) => (Some(None), Some(Some(now))),
        Some(DocumentationStatus::Draft) | Some(DocumentationStatus::Published) => {
            (Some(None), Some(None))
        }
        None => (None, None),
    };

    let request_title = update_req.title.clone();
    let request_slug = update_req.slug.clone();
    let request_icon = update_req.icon.clone();
    let request_status = update_req.status;

    let outcome = tc.run(|conn| {
        // Check if the page exists
        if repository::get_documentation_page(page_id, conn).is_err() {
            return Ok(UpdatePageOutcome::NotFound);
        }

        // Auto-regenerate slug when title changes (unless user explicitly provided a slug)
        let slug = if update_req.slug.is_some() {
            update_req.slug.clone()
        } else {
            update_req
                .title
                .as_ref()
                .map(|new_title| utils::slug::generate_unique_slug(new_title, conn))
        };

        let page_update = crate::models::DocumentationPageUpdate {
            title: update_req.title.clone(),
            slug,
            icon: update_req.icon.clone(),
            cover_image: update_req.cover_image.clone(),
            status: update_req.status,
            last_edited_by: Some(user_uuid),
            parent_id: update_req.parent_id,
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
            verified_by: None,
            verified_at: None,
            verify_interval_days: None,
        };

        let updated_page = match repository::update_documentation_page(conn, page_id, &page_update)
        {
            Ok(p) => p,
            Err(e) => {
                error!(page_id = page_id, error = ?e, "Error updating documentation page");
                return Ok(UpdatePageOutcome::UpdateFailed);
            }
        };

        let response = match to_page_response(updated_page.clone(), conn) {
            Ok(r) => r,
            Err(err) => return Ok(UpdatePageOutcome::ResponseBuildFailed(err)),
        };

        Ok::<_, diesel::result::Error>(UpdatePageOutcome::Ok(updated_page, response))
    });

    let (updated_page, response) = match outcome {
        Ok(UpdatePageOutcome::Ok(p, r)) => (p, r),
        Ok(UpdatePageOutcome::NotFound) => {
            return errors::not_found_msg("Documentation page not found");
        }
        Ok(UpdatePageOutcome::UpdateFailed) => {
            return errors::internal("Failed to update documentation page");
        }
        Ok(UpdatePageOutcome::ResponseBuildFailed(err)) => {
            return HttpResponse::InternalServerError().json(err);
        }
        Err(_) => return errors::internal("Failed to update documentation page"),
    };

    debug!(page_id = updated_page.id, "Documentation page updated");

    // Re-index the updated documentation page in search
    indexing_tasks::spawn_index_documentation(
        search_service.get_ref().clone(),
        updated_page.clone(),
    );

    // Broadcast SSE events for each updated field. One event per
    // field so the frontend can apply the change at field
    // granularity rather than re-fetching the whole page.
    let updates: [(&str, Option<serde_json::Value>); 4] = [
        (
            "title",
            request_title.as_ref().map(|v| serde_json::json!(v)),
        ),
        ("slug", request_slug.as_ref().map(|v| serde_json::json!(v))),
        ("icon", request_icon.as_ref().map(|v| serde_json::json!(v))),
        (
            "status",
            request_status.as_ref().map(|v| serde_json::json!(v)),
        ),
    ];
    for (field, value) in updates.into_iter().filter_map(|(f, v)| v.map(|v| (f, v))) {
        sse_state
            .broadcast_event(crate::handlers::sse::SseEvent::DocumentationUpdated {
                document_id: page_id,
                field: field.to_string(),
                value,
                updated_by: actor_sub.clone(),
                timestamp: chrono::Utc::now(),
            })
            .await;
    }

    // Notify subscribers about the page update. The spawned task
    // checks out its own pool connection on the legacy (non-TenantConn)
    // path — once the RLS rollout demands a workspace pin for this
    // background query, swap it for a `with_actor_context` wrapper.
    {
        let pool = pool.clone();
        let notification_service = notification_service.clone();
        let page_title = updated_page.title.clone();
        let page_slug = updated_page.slug.clone();
        tokio::spawn(async move {
            let mut conn = match pool.get() {
                Ok(conn) => conn,
                Err(_) => return,
            };
            let subscribers = documentation_subscriptions::get_page_subscribers(&mut conn, page_id);
            let actor = NotificationActor {
                uuid: user_uuid,
                name: actor_name,
                avatar_thumb: None,
            };
            let entity = NotificationEntity::DocumentationPage {
                id: page_id,
                title: page_title.clone(),
                slug: page_slug,
            };
            for subscriber_uuid in subscribers {
                if subscriber_uuid == user_uuid {
                    continue;
                }
                let payload = NotificationPayload::new(
                    NotificationTypeCode::DocPageUpdated,
                    subscriber_uuid,
                    actor.clone(),
                    entity.clone(),
                )
                .with_body(format!("\"{}\" was updated", page_title));

                if let Err(e) = notification_service.notify(payload).await {
                    tracing::warn!(error = %e, "Failed to send doc page update notification");
                }
            }
        });
    }

    HttpResponse::Ok().json(response)
}

/// Outcome of the soft-delete transaction.
enum DeletePageOutcome {
    Ok,
    NotFound,
    UpdateFailed,
}

// Delete a documentation page (soft delete — moves to trash)
pub async fn delete_documentation_page(
    req: HttpRequest,
    mut tc: TenantConn,
    search_service: web::Data<Arc<SearchService>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();

    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can delete documentation pages",
        );
    }

    let actor_sub = auth.user_uuid.to_string();
    let actor_name = auth.name.clone();

    let outcome = tc.run(|conn| {
        if repository::get_documentation_page(page_id, conn).is_err() {
            return Ok(DeletePageOutcome::NotFound);
        }
        // Soft delete: update status to Deleted and set deleted_at
        let now = chrono::Utc::now().naive_utc();
        let page_update = crate::models::DocumentationPageUpdate {
            status: Some(DocumentationStatus::Deleted),
            archived_at: Some(None),
            updated_at: Some(now),
            deleted_at: Some(Some(now)),
            ..Default::default()
        };

        match repository::update_documentation_page(conn, page_id, &page_update) {
            Ok(_) => Ok::<_, diesel::result::Error>(DeletePageOutcome::Ok),
            Err(e) => {
                error!(page_id = page_id, error = ?e, "Error soft-deleting documentation page");
                Ok(DeletePageOutcome::UpdateFailed)
            }
        }
    });

    match outcome {
        Ok(DeletePageOutcome::Ok) => {
            // Remove documentation from search index (trashed pages shouldn't appear in search)
            indexing_tasks::spawn_delete_documentation(search_service.get_ref().clone(), page_id);

            // Broadcast SSE event for status change to deleted
            let source_client_id = req
                .headers()
                .get("X-SSE-Client-Id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            sse_state
                .broadcast_event_from(
                    crate::handlers::sse::SseEvent::DocumentationUpdated {
                        document_id: page_id,
                        field: "status".to_string(),
                        value: serde_json::json!("deleted"),
                        updated_by: actor_sub,
                        timestamp: chrono::Utc::now(),
                    },
                    source_client_id,
                )
                .await;

            info!(page_id = page_id, deleted_by = %actor_name, "Documentation page moved to trash");
            HttpResponse::NoContent().finish()
        }
        Ok(DeletePageOutcome::NotFound) => errors::not_found_msg("Documentation page not found"),
        Ok(DeletePageOutcome::UpdateFailed) => {
            errors::internal("Failed to delete documentation page")
        }
        Err(_) => errors::internal("Failed to delete documentation page"),
    }
}

/// Outcome of a page-list transaction (top-level, by-parent, etc).
enum PageListOutcome {
    Ok(Vec<DocumentationPageResponse>),
    VisibilityCheckFailed,
    ResponseBuildFailed(String),
}

fn run_page_list<F>(
    tc: &mut TenantConn,
    user_uuid: Uuid,
    is_admin_user: bool,
    load: F,
) -> diesel::QueryResult<PageListOutcome>
where
    F: FnOnce(&mut DbConnection) -> diesel::QueryResult<Vec<DocumentationPage>>,
{
    tc.run(|conn| {
        let pages = load(conn)?;
        let pages = match repository::filter_pages_for_user(conn, pages, &user_uuid, is_admin_user)
        {
            Ok(p) => p,
            Err(_) => return Ok(PageListOutcome::VisibilityCheckFailed),
        };
        let responses = match to_page_responses(pages, conn) {
            Ok(r) => r,
            Err(err) => return Ok(PageListOutcome::ResponseBuildFailed(err)),
        };
        Ok::<_, diesel::result::Error>(PageListOutcome::Ok(responses))
    })
}

fn respond_page_list(
    outcome: diesel::QueryResult<PageListOutcome>,
    fetch_err_msg: &'static str,
) -> HttpResponse {
    match outcome {
        Ok(PageListOutcome::Ok(responses)) => HttpResponse::Ok().json(responses),
        Ok(PageListOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Ok(PageListOutcome::ResponseBuildFailed(err)) => {
            HttpResponse::InternalServerError().json(err)
        }
        Err(_) => errors::internal(fetch_err_msg),
    }
}

// Get top-level documentation pages
pub async fn get_top_level_documentation_pages(
    mut tc: TenantConn,
    auth: AuthContext,
) -> impl Responder {
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_top_level_pages(conn)
    });
    respond_page_list(outcome, "Failed to fetch top-level pages")
}

// Get documentation pages by parent ID
pub async fn get_documentation_pages_by_parent_id(
    mut tc: TenantConn,
    parent_id: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let parent = parent_id.into_inner();
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_pages_by_parent_id(parent, conn)
    });
    respond_page_list(outcome, "Failed to fetch pages by parent ID")
}

/// Outcome of loading a page with its children list.
enum PageWithChildrenOutcome {
    Ok(DocumentationPageWithChildren),
    NotFound,
    VisibilityCheckFailed,
    ChildrenFetchFailed,
}

// Get a page with its children by parent ID
pub async fn get_page_with_children_by_parent_id(
    mut tc: TenantConn,
    id: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = id.into_inner();
    let user_uuid = auth.user_uuid;
    let is_admin_user = auth.is_admin();

    let outcome = tc.run(|conn| {
        let page = match repository::get_documentation_page(page_id, conn) {
            Ok(p) => p,
            Err(_) => return Ok(PageWithChildrenOutcome::NotFound),
        };

        match repository::can_user_access_page(conn, page.id, &user_uuid, is_admin_user) {
            Ok(true) => {}
            Ok(false) => return Ok(PageWithChildrenOutcome::NotFound),
            Err(_) => return Ok(PageWithChildrenOutcome::VisibilityCheckFailed),
        }

        let children = match repository::get_pages_by_parent_id(page_id, conn) {
            Ok(c) => match repository::filter_pages_for_user(conn, c, &user_uuid, is_admin_user) {
                Ok(filtered) => filtered,
                Err(_) => return Ok(PageWithChildrenOutcome::VisibilityCheckFailed),
            },
            Err(_) => return Ok(PageWithChildrenOutcome::ChildrenFetchFailed),
        };

        Ok::<_, diesel::result::Error>(PageWithChildrenOutcome::Ok(DocumentationPageWithChildren {
            page,
            children,
        }))
    });

    match outcome {
        Ok(PageWithChildrenOutcome::Ok(p)) => HttpResponse::Ok().json(p),
        Ok(PageWithChildrenOutcome::NotFound) => errors::not_found_msg("Page not found"),
        Ok(PageWithChildrenOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Ok(PageWithChildrenOutcome::ChildrenFetchFailed) => {
            errors::internal("Failed to fetch children")
        }
        Err(_) => errors::internal("Failed to load page"),
    }
}

// Get a page with its ordered children
pub async fn get_page_with_ordered_children(
    mut tc: TenantConn,
    id: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = id.into_inner();
    let user_uuid = auth.user_uuid;
    let is_admin_user = auth.is_admin();

    let outcome = tc.run(|conn| {
        let mut page_with_children = match repository::get_page_with_ordered_children(conn, page_id)
        {
            Ok(p) => p,
            Err(_) => return Ok(PageWithChildrenOutcome::NotFound),
        };
        match repository::can_user_access_page(
            conn,
            page_with_children.page.id,
            &user_uuid,
            is_admin_user,
        ) {
            Ok(true) => {}
            Ok(false) => return Ok(PageWithChildrenOutcome::NotFound),
            Err(_) => return Ok(PageWithChildrenOutcome::VisibilityCheckFailed),
        }
        page_with_children.children = match repository::filter_pages_for_user(
            conn,
            page_with_children.children,
            &user_uuid,
            is_admin_user,
        ) {
            Ok(c) => c,
            Err(_) => return Ok(PageWithChildrenOutcome::VisibilityCheckFailed),
        };
        Ok::<_, diesel::result::Error>(PageWithChildrenOutcome::Ok(page_with_children))
    });

    match outcome {
        Ok(PageWithChildrenOutcome::Ok(p)) => HttpResponse::Ok().json(p),
        Ok(PageWithChildrenOutcome::NotFound) => {
            errors::not_found_msg("Page not found or error fetching children")
        }
        Ok(PageWithChildrenOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Ok(PageWithChildrenOutcome::ChildrenFetchFailed) => {
            errors::internal("Failed to fetch children")
        }
        Err(_) => errors::not_found_msg("Page not found or error fetching children"),
    }
}

// Get ordered documentation pages by parent ID
pub async fn get_ordered_pages_by_parent_id(
    mut tc: TenantConn,
    parent_id: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let parent = parent_id.into_inner();
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_ordered_pages_by_parent_id(conn, parent)
    });
    respond_page_list(outcome, "Failed to fetch ordered pages by parent ID")
}

#[derive(Deserialize)]
pub struct ReorderPagesRequest {
    pub parent_id: i32,
    pub page_orders: Vec<crate::models::PageOrder>,
}

// Reorder pages under a parent
pub async fn reorder_pages(
    mut tc: TenantConn,
    request: web::Json<ReorderPagesRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can reorder documentation pages",
        );
    }

    let parent_id = request.parent_id;
    let request = request.into_inner();

    match tc.run(|conn| repository::reorder_pages(conn, Some(parent_id), &request.page_orders)) {
        Ok(updated_pages) => HttpResponse::Ok().json(updated_pages),
        Err(e) => {
            error!(parent_id = parent_id, error = ?e, "Error reordering pages");
            errors::internal("Failed to reorder pages")
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
    mut tc: TenantConn,
    request: web::Json<MovePageRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can move documentation pages",
        );
    }

    let request = request.into_inner();
    let display_order = request.display_order.unwrap_or(0);

    // Validation: Cannot move a page to be its own parent
    if request.new_parent_id == Some(request.page_id) {
        return errors::bad_request("Invalid operation: A page cannot be its own parent");
    }

    let page_id = request.page_id;
    let new_parent_id = request.new_parent_id;

    match tc
        .run(|conn| repository::move_page_to_parent(conn, page_id, new_parent_id, display_order))
    {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(diesel::result::Error::RollbackTransaction) => errors::bad_request(
            "Circular reference: Cannot move a page to be a child of its own descendant",
        ),
        Err(e) => {
            error!(page_id = page_id, new_parent_id = ?new_parent_id, error = ?e, "Error moving page");
            errors::internal("Internal server error: Failed to move page to new parent")
        }
    }
}

// Get top-level pages (with ordering)
pub async fn get_ordered_top_level_pages(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_ordered_top_level_pages(conn)
    });
    respond_page_list(outcome, "Failed to fetch top-level pages")
}

// Get documentation page by slug with its children
pub async fn get_documentation_page_by_slug_with_children(
    mut tc: TenantConn,
    slug: web::Path<String>,
    auth: AuthContext,
) -> impl Responder {
    let page_slug = slug.into_inner();
    let user_uuid = auth.user_uuid;
    let is_admin_user = auth.is_admin();

    let outcome = tc.run(|conn| {
        let page = match repository::get_documentation_page_by_slug(&page_slug, conn) {
            Ok(p) => p,
            Err(_) => return Ok(PageWithChildrenOutcome::NotFound),
        };

        match repository::can_user_access_page(conn, page.id, &user_uuid, is_admin_user) {
            Ok(true) => {}
            Ok(false) => return Ok(PageWithChildrenOutcome::NotFound),
            Err(_) => return Ok(PageWithChildrenOutcome::VisibilityCheckFailed),
        }

        let children = match repository::get_pages_by_parent_id(page.id, conn) {
            Ok(c) => match repository::filter_pages_for_user(conn, c, &user_uuid, is_admin_user) {
                Ok(filtered) => filtered,
                Err(_) => return Ok(PageWithChildrenOutcome::VisibilityCheckFailed),
            },
            Err(_) => return Ok(PageWithChildrenOutcome::ChildrenFetchFailed),
        };

        Ok::<_, diesel::result::Error>(PageWithChildrenOutcome::Ok(DocumentationPageWithChildren {
            page,
            children,
        }))
    });

    match outcome {
        Ok(PageWithChildrenOutcome::Ok(p)) => HttpResponse::Ok().json(p),
        Ok(PageWithChildrenOutcome::NotFound) => errors::not_found_msg("Page not found"),
        Ok(PageWithChildrenOutcome::VisibilityCheckFailed) => {
            errors::internal("Failed to check page visibility")
        }
        Ok(PageWithChildrenOutcome::ChildrenFetchFailed) => {
            errors::internal("Failed to fetch children")
        }
        Err(_) => errors::internal("Failed to load page"),
    }
}

// Get documentation pages for a ticket
pub async fn get_documentation_pages_by_ticket_id(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_documentation_pages_by_ticket_id(conn, ticket_id)
    });
    if let Ok(PageListOutcome::Ok(ref r)) = outcome {
        debug!(
            ticket_id = ticket_id,
            count = r.len(),
            "Found documentation pages for ticket"
        );
    }
    respond_page_list(outcome, "Failed to fetch documentation pages")
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
pub async fn export_documentation_pages(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can export documentation",
        );
    }

    match tc.run(repository::get_documentation_pages) {
        Ok(pages) => {
            let export_pages: Vec<DocumentationPageExport> = pages
                .into_iter()
                .map(|page| DocumentationPageExport {
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
                })
                .collect();
            HttpResponse::Ok().json(export_pages)
        }
        Err(_) => errors::internal("Failed to fetch pages for export"),
    }
}

/// Outcome of building a single page's markdown export.
enum MarkdownOutcome {
    Ok {
        title: String,
        slug: String,
        markdown: String,
    },
    NotFound,
}

// Export a single documentation page as Markdown
pub async fn export_page_as_markdown(
    req: actix_web::HttpRequest,
    mut tc: TenantConn,
    page_id: web::Path<i32>,
) -> impl Responder {
    let id = page_id.into_inner();
    let locale = crate::utils::locale::request_locale(&req);

    let outcome = tc.run(|conn| {
        let page = match repository::get_documentation_page(id, conn) {
            Ok(p) => p,
            Err(_) => return Ok(MarkdownOutcome::NotFound),
        };
        let markdown = match resolve_yjs_document(&page, conn) {
            Some(doc_bytes) => {
                let mut visited = std::collections::HashSet::new();
                utils::markdown_export::yjs_to_markdown_with_embeds(
                    &doc_bytes,
                    conn,
                    &mut visited,
                    Some(page.uuid),
                    0,
                    &locale,
                )
                .unwrap_or_else(|| String::from("*Empty document*"))
            }
            None => String::from("*Empty document*"),
        };
        Ok::<_, diesel::result::Error>(MarkdownOutcome::Ok {
            title: page.title,
            slug: page.slug,
            markdown,
        })
    });

    match outcome {
        Ok(MarkdownOutcome::Ok {
            title,
            slug,
            markdown,
        }) => {
            // Add title as H1 header
            let full_markdown = format!("# {}\n\n{}", title, markdown);
            let filename = format!("{}.md", slug);
            HttpResponse::Ok()
                .content_type("text/markdown; charset=utf-8")
                .insert_header((
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", filename),
                ))
                .body(full_markdown)
        }
        Ok(MarkdownOutcome::NotFound) => errors::not_found_msg("Page not found"),
        Err(_) => errors::internal("Failed to export page"),
    }
}

/// Outcome of the create_from_ticket transaction.
enum CreateFromTicketOutcome {
    /// Existing page is returned without creating a new one.
    Existing(DocumentationPage),
    /// Newly-created page.
    Created(DocumentationPage),
    CreateFailed,
}

// Create a documentation page from a ticket's article content
pub async fn create_documentation_page_from_ticket(
    mut tc: TenantConn,
    path: web::Path<i32>,
    page_data: web::Json<CreateDocPageFromTicket>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = path.into_inner();

    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden: Only technicians and administrators can create documentation pages from tickets");
    }

    let user_uuid = auth.user_uuid;
    let page_data = page_data.into_inner();

    let outcome = tc.run(|conn| {
        // Check if a documentation page already exists for this ticket
        if let Ok(existing_pages) =
            repository::get_documentation_pages_by_ticket_id(conn, ticket_id)
        {
            if let Some(existing_page) = existing_pages.into_iter().next() {
                return Ok(CreateFromTicketOutcome::Existing(existing_page));
            }
        }

        // Get the ticket's article content (for Yjs document cloning)
        let article_content =
            repository::get_article_content_by_ticket_id(conn, ticket_id).ok();

        // Generate a unique slug from the title
        let slug = utils::slug::generate_unique_slug(&page_data.title, conn);

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
            display_order: Some(0),
            is_public: false,
            is_template: false,
            yjs_state_vector,
            yjs_document,
            yjs_client_id,
            has_unsaved_changes: false,
        };

        let page = match repository::create_documentation_page(new_page, conn) {
            Ok(p) => p,
            Err(e) => {
                error!(ticket_id = ticket_id, error = ?e, "Error creating documentation page from ticket");
                return Ok(CreateFromTicketOutcome::CreateFailed);
            }
        };

        // Record the page<->ticket linkage in the join table.
        if let Err(e) = repository::documentation_page_tickets::upsert_link(
            conn,
            page.id,
            ticket_id,
            repository::documentation_page_tickets::LINK_RESOLVES,
            Some(user_uuid),
        ) {
            error!(error = ?e, page_id = page.id, ticket_id, "Failed to create page<->ticket link");
        }

        // Auto-add to "Tickets" system collection
        if let Ok(tickets_collection) =
            repository::documentation_collections::get_collection_by_slug(conn, "tickets")
        {
            let entry = crate::models::NewDocumentationCollectionPage {
                collection_id: tickets_collection.id,
                page_id: page.id,
                created_by: Some(user_uuid),
            };
            if let Err(e) =
                repository::documentation_collections::add_page_to_collection(conn, entry)
            {
                error!(error = ?e, "Failed to add page to Tickets collection");
            }
        }

        Ok::<_, diesel::result::Error>(CreateFromTicketOutcome::Created(page))
    });

    match outcome {
        Ok(CreateFromTicketOutcome::Existing(p)) => HttpResponse::Ok().json(p),
        Ok(CreateFromTicketOutcome::Created(page)) => {
            sse_state
                .broadcast_event(crate::handlers::sse::SseEvent::DocumentationCreated {
                    document_id: page.id,
                    document: serde_json::to_value(&page).unwrap_or_default(),
                    timestamp: chrono::Utc::now(),
                })
                .await;
            HttpResponse::Created().json(page)
        }
        Ok(CreateFromTicketOutcome::CreateFailed) => {
            errors::internal("Failed to create documentation page")
        }
        Err(_) => errors::internal("Failed to create documentation page"),
    }
}

// Get archived documentation pages
pub async fn get_archived_pages(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_pages_by_status(conn, DocumentationStatus::Archived)
    });
    respond_page_list(outcome, "Failed to fetch archived pages")
}

// Get trashed (soft-deleted) documentation pages
pub async fn get_trashed_pages(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    let outcome = run_page_list(&mut tc, auth.user_uuid, auth.is_admin(), |conn| {
        repository::get_pages_by_status(conn, DocumentationStatus::Deleted)
    });
    respond_page_list(outcome, "Failed to fetch trashed pages")
}

// ============================================================================
// Page Visibility (Access Control)
// ============================================================================

/// Get visibility groups for a documentation page (technician+)
pub async fn get_page_visibility(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can view page visibility",
        );
    }

    let page_id = path.into_inner();

    let result = tc.run(|conn| {
        let groups = repository::get_visible_groups_for_page(conn, page_id)?;
        let users = repository::get_visible_users_for_page(conn, page_id)?;
        Ok::<_, diesel::result::Error>((groups, users))
    });

    match result {
        Ok((groups, users)) => HttpResponse::Ok().json(serde_json::json!({
            "groups": groups,
            "users": users,
        })),
        Err(e) => {
            error!(error = ?e, "Failed to get page visibility");
            errors::internal("Failed to get page visibility")
        }
    }
}

#[derive(Deserialize)]
pub struct SetPageVisibilityRequest {
    pub group_ids: Vec<i32>,
    pub user_uuids: Option<Vec<String>>,
}

/// Set visibility groups for a documentation page (admin only)
pub async fn set_page_visibility(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SetPageVisibilityRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Forbidden: Only administrators can set page visibility");
    }

    let page_id = path.into_inner();
    let created_by = Some(auth.user_uuid);
    let body = body.into_inner();

    // Parse user UUIDs
    let user_uuids: Vec<Uuid> = body
        .user_uuids
        .as_ref()
        .map(|uuids| {
            uuids
                .iter()
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect()
        })
        .unwrap_or_default();

    match tc.run(|conn| {
        repository::set_page_visibility(
            conn,
            page_id,
            body.group_ids.clone(),
            user_uuids.clone(),
            created_by,
        )
    }) {
        Ok(entries) => HttpResponse::Ok().json(entries),
        Err(e) => {
            error!(error = ?e, "Failed to set page visibility");
            errors::internal("Failed to set page visibility")
        }
    }
}

/// Outcome of the page-restore transaction.
enum RestorePageOutcome {
    Ok(DocumentationPage, DocumentationPageResponse),
    NotFound,
    UpdateFailed,
    ResponseBuildFailed(String),
}

// Restore a page from archive or trash back to draft
pub async fn restore_page(
    req: HttpRequest,
    mut tc: TenantConn,
    search_service: web::Data<Arc<SearchService>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();

    if !auth.is_technician_or_admin() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can restore documentation pages",
        );
    }

    let actor_sub = auth.user_uuid.to_string();
    let actor_name = auth.name.clone();

    let outcome = tc.run(|conn| {
        if repository::get_documentation_page(page_id, conn).is_err() {
            return Ok(RestorePageOutcome::NotFound);
        }
        let now = chrono::Utc::now().naive_utc();
        let page_update = crate::models::DocumentationPageUpdate {
            status: Some(DocumentationStatus::Draft),
            archived_at: Some(None),
            updated_at: Some(now),
            deleted_at: Some(None),
            ..Default::default()
        };
        let restored_page = match repository::update_documentation_page(conn, page_id, &page_update)
        {
            Ok(p) => p,
            Err(e) => {
                error!(page_id = page_id, error = ?e, "Error restoring documentation page");
                return Ok(RestorePageOutcome::UpdateFailed);
            }
        };
        let response = match to_page_response(restored_page.clone(), conn) {
            Ok(r) => r,
            Err(err) => return Ok(RestorePageOutcome::ResponseBuildFailed(err)),
        };
        Ok::<_, diesel::result::Error>(RestorePageOutcome::Ok(restored_page, response))
    });

    match outcome {
        Ok(RestorePageOutcome::Ok(restored_page, response)) => {
            indexing_tasks::spawn_index_documentation(
                search_service.get_ref().clone(),
                restored_page.clone(),
            );

            let source_client_id = req
                .headers()
                .get("X-SSE-Client-Id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            sse_state
                .broadcast_event_from(
                    crate::handlers::sse::SseEvent::DocumentationUpdated {
                        document_id: page_id,
                        field: "status".to_string(),
                        value: serde_json::json!("draft"),
                        updated_by: actor_sub,
                        timestamp: chrono::Utc::now(),
                    },
                    source_client_id,
                )
                .await;

            info!(page_id = page_id, restored_by = %actor_name, "Documentation page restored");
            HttpResponse::Ok().json(response)
        }
        Ok(RestorePageOutcome::NotFound) => errors::not_found_msg("Documentation page not found"),
        Ok(RestorePageOutcome::UpdateFailed) => {
            errors::internal("Failed to restore documentation page")
        }
        Ok(RestorePageOutcome::ResponseBuildFailed(err)) => {
            HttpResponse::InternalServerError().json(err)
        }
        Err(_) => errors::internal("Failed to restore documentation page"),
    }
}

/// Outcome of the hard-delete transaction.
enum HardDeleteOutcome {
    Ok,
    NotFound,
    DeleteFailed,
}

// Permanently delete a documentation page (hard delete, admin only)
pub async fn permanently_delete_page(
    mut tc: TenantConn,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();

    if !auth.is_admin() {
        return errors::forbidden(
            "Forbidden: Only administrators can permanently delete documentation pages",
        );
    }
    let actor_name = auth.name.clone();

    let outcome = tc.run(|conn| {
        if repository::get_documentation_page(page_id, conn).is_err() {
            return Ok(HardDeleteOutcome::NotFound);
        }
        match repository::permanently_delete_page(page_id, conn) {
            Ok(_) => Ok::<_, diesel::result::Error>(HardDeleteOutcome::Ok),
            Err(e) => {
                error!(page_id = page_id, error = ?e, "Error permanently deleting documentation page");
                Ok(HardDeleteOutcome::DeleteFailed)
            }
        }
    });

    match outcome {
        Ok(HardDeleteOutcome::Ok) => {
            indexing_tasks::spawn_delete_documentation(search_service.get_ref().clone(), page_id);
            info!(page_id = page_id, deleted_by = %actor_name, "Documentation page permanently deleted");
            HttpResponse::NoContent().finish()
        }
        Ok(HardDeleteOutcome::NotFound) => errors::not_found_msg("Documentation page not found"),
        Ok(HardDeleteOutcome::DeleteFailed) => {
            errors::internal("Failed to permanently delete documentation page")
        }
        Err(_) => errors::internal("Failed to permanently delete documentation page"),
    }
}

// ============================================================================
// Documentation Page Subscriptions
// ============================================================================

/// Check if the current user is subscribed to a documentation page
pub async fn get_page_subscription(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;

    let result = tc.run(|conn| {
        Ok::<_, diesel::result::Error>(documentation_subscriptions::is_user_subscribed(
            conn, user_uuid, page_id,
        ))
    });

    match result {
        Ok(subscribed) => HttpResponse::Ok().json(json!({ "subscribed": subscribed })),
        Err(_) => errors::internal("Failed to check subscription"),
    }
}

/// Subscribe the current user to a documentation page
pub async fn subscribe_to_page(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;

    enum SubscribeOutcome {
        Ok,
        NotFound,
        Failed,
    }

    let outcome = tc.run(|conn| {
        if repository::get_documentation_page(page_id, conn).is_err() {
            return Ok(SubscribeOutcome::NotFound);
        }
        match documentation_subscriptions::subscribe_user(conn, user_uuid, page_id) {
            Ok(_) => Ok::<_, diesel::result::Error>(SubscribeOutcome::Ok),
            Err(e) => {
                error!(error = ?e, "Failed to subscribe to documentation page");
                Ok(SubscribeOutcome::Failed)
            }
        }
    });

    match outcome {
        Ok(SubscribeOutcome::Ok) => HttpResponse::Ok().json(json!({ "subscribed": true })),
        Ok(SubscribeOutcome::NotFound) => errors::not_found_msg("Documentation page not found"),
        Ok(SubscribeOutcome::Failed) => errors::internal("Failed to subscribe"),
        Err(_) => errors::internal("Failed to subscribe"),
    }
}

/// Unsubscribe the current user from a documentation page
pub async fn unsubscribe_from_page(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;

    match tc.run(|conn| documentation_subscriptions::unsubscribe_user(conn, user_uuid, page_id)) {
        Ok(_) => HttpResponse::Ok().json(json!({ "subscribed": false })),
        Err(e) => {
            error!(error = ?e, "Failed to unsubscribe from documentation page");
            errors::internal("Failed to unsubscribe")
        }
    }
}

// ============================================================================
// Starred Pages
// ============================================================================

/// Get all starred pages for the current user (for sidebar)
pub async fn get_starred_pages(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    let user_uuid = auth.user_uuid;
    match tc.run(|conn| {
        Ok::<_, diesel::result::Error>(documentation_starred_pages::get_user_starred_pages(
            conn, user_uuid,
        ))
    }) {
        Ok(starred) => HttpResponse::Ok().json(starred),
        Err(_) => errors::internal("Failed to load starred pages"),
    }
}

/// Check if the current user has starred a documentation page
pub async fn get_page_starred(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;

    match tc.run(|conn| {
        Ok::<_, diesel::result::Error>(documentation_starred_pages::is_page_starred(
            conn, user_uuid, page_id,
        ))
    }) {
        Ok(starred) => HttpResponse::Ok().json(json!({ "starred": starred })),
        Err(_) => errors::internal("Failed to check starred state"),
    }
}

/// Star a documentation page for the current user
pub async fn star_page(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;

    enum StarOutcome {
        Ok,
        NotFound,
        Failed,
    }

    let outcome = tc.run(|conn| {
        if repository::get_documentation_page(page_id, conn).is_err() {
            return Ok(StarOutcome::NotFound);
        }
        match documentation_starred_pages::star_page(conn, user_uuid, page_id) {
            Ok(_) => Ok::<_, diesel::result::Error>(StarOutcome::Ok),
            Err(e) => {
                error!(error = ?e, "Failed to star documentation page");
                Ok(StarOutcome::Failed)
            }
        }
    });

    match outcome {
        Ok(StarOutcome::Ok) => HttpResponse::Ok().json(json!({ "starred": true })),
        Ok(StarOutcome::NotFound) => errors::not_found_msg("Documentation page not found"),
        Ok(StarOutcome::Failed) => errors::internal("Failed to star page"),
        Err(_) => errors::internal("Failed to star page"),
    }
}

/// Unstar a documentation page for the current user
pub async fn unstar_page(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;

    match tc.run(|conn| documentation_starred_pages::unstar_page(conn, user_uuid, page_id)) {
        Ok(_) => HttpResponse::Ok().json(json!({ "starred": false })),
        Err(e) => {
            error!(error = ?e, "Failed to unstar documentation page");
            errors::internal("Failed to unstar page")
        }
    }
}

// ============================================================================
// Page <-> Ticket links + Verification
// ============================================================================

/// Public DTO returned alongside a link. Embeds ticket title +
/// status so the frontend can render the row without an extra
/// fetch per ticket.
#[derive(Debug, serde::Serialize)]
pub struct PageTicketLinkResponse {
    pub page_id: i32,
    pub ticket_id: i32,
    pub link_type: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::NaiveDateTime,
    pub ticket_title: Option<String>,
    pub ticket_category: Option<crate::models::WorkflowStateCategory>,
}

#[derive(Debug, serde::Serialize)]
pub struct PageDocLinkResponse {
    pub page_id: i32,
    pub ticket_id: i32,
    pub link_type: String,
    pub page_title: String,
    pub page_slug: String,
    pub page_icon: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct CreatePageTicketLinkRequest {
    pub ticket_id: i32,
    /// Defaults to "references" when omitted.
    pub link_type: Option<String>,
}

/// GET /api/documentation/pages/{id}/tickets
pub async fn list_page_tickets(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();

    let result = tc.run(|conn| {
        let links = repository::documentation_page_tickets::links_for_page(conn, page_id)?;

        // Hydrate ticket title + status in one query rather than N+1.
        use crate::schema::tickets;
        use diesel::prelude::*;
        let ticket_ids: Vec<i32> = links.iter().map(|l| l.ticket_id).collect();
        let tickets_meta: std::collections::HashMap<
            i32,
            (String, crate::models::WorkflowStateCategory),
        > = if ticket_ids.is_empty() {
            Default::default()
        } else {
            let rows: Vec<(i32, String, i32)> = tickets::table
                .filter(tickets::id.eq_any(&ticket_ids))
                .select((tickets::id, tickets::title, tickets::workflow_state_id))
                .load(conn)
                .unwrap_or_default();
            rows.into_iter()
                .map(|(id, title, ws_id)| {
                    let cat = crate::repository::workflow_states::category_of(conn, ws_id)
                        .ok()
                        .flatten()
                        .unwrap_or(crate::models::WorkflowStateCategory::Backlog);
                    (id, (title, cat))
                })
                .collect()
        };

        let responses: Vec<PageTicketLinkResponse> = links
            .into_iter()
            .map(|l| {
                let meta = tickets_meta.get(&l.ticket_id);
                PageTicketLinkResponse {
                    page_id: l.page_id,
                    ticket_id: l.ticket_id,
                    link_type: l.link_type,
                    created_by: l.created_by,
                    created_at: l.created_at,
                    ticket_title: meta.map(|m| m.0.clone()),
                    ticket_category: meta.map(|m| m.1),
                }
            })
            .collect();
        Ok::<_, diesel::result::Error>(responses)
    });

    match result {
        Ok(responses) => HttpResponse::Ok().json(responses),
        Err(e) => {
            error!(error = ?e, "Failed to load page<->ticket links");
            errors::internal("Failed to load links")
        }
    }
}

/// POST /api/documentation/pages/{id}/tickets
pub async fn create_page_ticket_link(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<CreatePageTicketLinkRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }
    let user_uuid = auth.user_uuid;
    let page_id = path.into_inner();
    let req_body = body.into_inner();
    let link_type = req_body
        .link_type
        .unwrap_or_else(|| repository::documentation_page_tickets::LINK_REFERENCES.to_string());
    if let Err(msg) = repository::documentation_page_tickets::validate_link_type(&link_type) {
        return HttpResponse::BadRequest().json(json!({"error": msg}));
    }

    match tc.run(|conn| {
        repository::documentation_page_tickets::upsert_link(
            conn,
            page_id,
            req_body.ticket_id,
            &link_type,
            Some(user_uuid),
        )
    }) {
        Ok(row) => HttpResponse::Created().json(row),
        Err(e) => {
            error!(error = ?e, "Failed to create page<->ticket link");
            errors::internal("Failed to create link")
        }
    }
}

/// DELETE /api/documentation/pages/{page_id}/tickets/{ticket_id}
pub async fn delete_page_ticket_link(
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }
    let (page_id, ticket_id) = path.into_inner();
    match tc
        .run(|conn| repository::documentation_page_tickets::delete_link(conn, page_id, ticket_id))
    {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = ?e, "Failed to delete page<->ticket link");
            errors::internal("Failed to delete link")
        }
    }
}

/// Outcome of building the ticket->doc-links response.
enum TicketDocLinksOutcome {
    Ok(Vec<PageDocLinkResponse>),
    HydrateFailed,
    FilterFailed,
}

/// GET /api/tickets/{ticket_id}/documentation-pages
/// Mirror of list_page_tickets from the ticket side. Returns
/// hydrated doc rows so the ticket "See also" panel needs no
/// secondary request.
pub async fn list_ticket_doc_links(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let user_uuid = auth.user_uuid;
    let is_admin_user = auth.is_admin();

    let outcome = tc.run(|conn| {
        let links = repository::documentation_page_tickets::links_for_ticket(conn, ticket_id)?;

        use crate::schema::documentation_pages;
        use diesel::prelude::*;
        let page_ids: Vec<i32> = links.iter().map(|l| l.page_id).collect();
        if page_ids.is_empty() {
            return Ok(TicketDocLinksOutcome::Ok(Vec::new()));
        }

        let pages: Vec<DocumentationPage> = match documentation_pages::table
            .filter(documentation_pages::id.eq_any(&page_ids))
            .filter(documentation_pages::deleted_at.is_null())
            .load(conn)
        {
            Ok(p) => p,
            Err(e) => {
                error!(error = ?e, "Failed to hydrate linked docs");
                return Ok(TicketDocLinksOutcome::HydrateFailed);
            }
        };

        // Apply per-user visibility filtering — the same page_visibility
        // rules that gate page reads must gate this list, otherwise the
        // ticket panel would leak doc titles past their group boundary.
        let pages = match repository::filter_pages_for_user(conn, pages, &user_uuid, is_admin_user)
        {
            Ok(p) => p,
            Err(_) => return Ok(TicketDocLinksOutcome::FilterFailed),
        };
        let pages_by_id: std::collections::HashMap<i32, DocumentationPage> =
            pages.into_iter().map(|p| (p.id, p)).collect();

        let responses: Vec<PageDocLinkResponse> = links
            .into_iter()
            .filter_map(|l| {
                pages_by_id.get(&l.page_id).map(|p| PageDocLinkResponse {
                    page_id: l.page_id,
                    ticket_id: l.ticket_id,
                    link_type: l.link_type,
                    page_title: p.title.clone(),
                    page_slug: p.slug.clone(),
                    page_icon: p.icon.clone(),
                    created_at: l.created_at,
                })
            })
            .collect();
        Ok::<_, diesel::result::Error>(TicketDocLinksOutcome::Ok(responses))
    });

    match outcome {
        Ok(TicketDocLinksOutcome::Ok(rs)) => HttpResponse::Ok().json(rs),
        Ok(TicketDocLinksOutcome::HydrateFailed) => errors::internal("Failed to hydrate docs"),
        Ok(TicketDocLinksOutcome::FilterFailed) => errors::internal("Failed to filter pages"),
        Err(e) => {
            error!(error = ?e, "Failed to load ticket<->doc links");
            errors::internal("Failed to load links")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct VerifyPageRequest {
    /// Days until the verification expires. None means evergreen
    /// (no staleness check). 0 is rejected — that would mark the
    /// page stale immediately, which is never useful.
    pub interval_days: Option<i32>,
}

/// Outcome of the verify/unverify-page transaction.
enum VerifyPageOutcome {
    Ok(DocumentationPage, DocumentationPageResponse),
    UpdateFailed,
    ResponseBuildFailed(String),
}

/// POST /api/documentation/pages/{id}/verification
pub async fn verify_page(
    mut tc: TenantConn,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    path: web::Path<i32>,
    body: web::Json<VerifyPageRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }
    let page_id = path.into_inner();
    let req_body = body.into_inner();
    if matches!(req_body.interval_days, Some(0) | Some(i32::MIN..=-1)) {
        return errors::bad_request("interval_days must be positive");
    }
    let user_uuid = auth.user_uuid;
    let now = chrono::Utc::now().naive_utc();

    let outcome = tc.run(|conn| {
        let update = crate::models::DocumentationPageUpdate {
            verified_by: Some(Some(user_uuid)),
            verified_at: Some(Some(now)),
            verify_interval_days: Some(req_body.interval_days),
            updated_at: Some(now),
            ..Default::default()
        };
        let updated = match repository::update_documentation_page(conn, page_id, &update) {
            Ok(u) => u,
            Err(e) => {
                error!(error = ?e, "Failed to verify page");
                return Ok(VerifyPageOutcome::UpdateFailed);
            }
        };
        // Re-verifying a page closes the editorial loop on
        // any open stale_doc gap that flagged it. Best-effort
        // — a failure here doesn't roll back the verification.
        if let Err(e) =
            repository::knowledge_gaps::dismiss_stale_doc_gaps_for_page(conn, page_id, user_uuid)
        {
            error!(error = ?e, page_id, "Failed to auto-dismiss stale_doc gaps after re-verify");
        }
        let response = match to_page_response(updated.clone(), conn) {
            Ok(r) => r,
            Err(err) => return Ok(VerifyPageOutcome::ResponseBuildFailed(err)),
        };
        Ok::<_, diesel::result::Error>(VerifyPageOutcome::Ok(updated, response))
    });

    match outcome {
        Ok(VerifyPageOutcome::Ok(updated, response)) => {
            sse_state
                .broadcast_event(crate::handlers::sse::SseEvent::DocumentationUpdated {
                    document_id: updated.id,
                    field: "verification".to_string(),
                    value: serde_json::json!({
                        "verified_by": updated.verified_by,
                        "verified_at": updated.verified_at,
                        "verify_interval_days": updated.verify_interval_days,
                    }),
                    updated_by: user_uuid.to_string(),
                    timestamp: chrono::Utc::now(),
                })
                .await;
            HttpResponse::Ok().json(response)
        }
        Ok(VerifyPageOutcome::UpdateFailed) => errors::internal("Failed to verify page"),
        Ok(VerifyPageOutcome::ResponseBuildFailed(err)) => {
            HttpResponse::InternalServerError().json(err)
        }
        Err(_) => errors::internal("Failed to verify page"),
    }
}

/// DELETE /api/documentation/pages/{id}/verification
pub async fn unverify_page(
    mut tc: TenantConn,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }
    let page_id = path.into_inner();
    let user_uuid = auth.user_uuid;
    let now = chrono::Utc::now().naive_utc();

    let outcome = tc.run(|conn| {
        let update = crate::models::DocumentationPageUpdate {
            verified_by: Some(None),
            verified_at: Some(None),
            verify_interval_days: Some(None),
            updated_at: Some(now),
            ..Default::default()
        };
        let updated = match repository::update_documentation_page(conn, page_id, &update) {
            Ok(u) => u,
            Err(e) => {
                error!(error = ?e, "Failed to clear verification");
                return Ok(VerifyPageOutcome::UpdateFailed);
            }
        };
        let response = match to_page_response(updated.clone(), conn) {
            Ok(r) => r,
            Err(err) => return Ok(VerifyPageOutcome::ResponseBuildFailed(err)),
        };
        Ok::<_, diesel::result::Error>(VerifyPageOutcome::Ok(updated, response))
    });

    match outcome {
        Ok(VerifyPageOutcome::Ok(updated, response)) => {
            sse_state
                .broadcast_event(crate::handlers::sse::SseEvent::DocumentationUpdated {
                    document_id: updated.id,
                    field: "verification".to_string(),
                    value: serde_json::Value::Null,
                    updated_by: user_uuid.to_string(),
                    timestamp: chrono::Utc::now(),
                })
                .await;
            HttpResponse::Ok().json(response)
        }
        Ok(VerifyPageOutcome::UpdateFailed) => errors::internal("Failed to clear verification"),
        Ok(VerifyPageOutcome::ResponseBuildFailed(err)) => {
            HttpResponse::InternalServerError().json(err)
        }
        Err(_) => errors::internal("Failed to clear verification"),
    }
}
