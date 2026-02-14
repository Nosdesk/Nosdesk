use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::Utc;
use diesel::result::Error;
use serde::Deserialize;
use serde_json::json;
use tracing::error;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::sse::SseState;
use crate::models::{
    NewDocumentationCollection, DocumentationCollectionUpdate, NewDocumentationCollectionPage,
    NewDocumentationPage, DocumentationStatus, DocumentationCollection,
};
use crate::repository;
use crate::utils;
use crate::utils::rbac::{require_auth, require_technician_or_admin, require_admin};
use crate::utils::sse::SseBroadcaster;

// ============================================================================
// Root Page Helper
// ============================================================================

/// Create or find a root page for a collection and link it
fn create_root_page_for_collection(
    conn: &mut crate::db::DbConnection,
    collection: &DocumentationCollection,
    user_uuid: Uuid,
) -> Result<i32, Error> {
    let root_slug = utils::slug::generate_unique_slug(&collection.name, conn);

    // Check if root page already exists (e.g. from a previous partial creation)
    let page = match repository::documentation::get_documentation_page_by_slug(&root_slug, conn) {
        Ok(existing) => existing,
        Err(Error::NotFound) => {
            let new_page = NewDocumentationPage {
                uuid: Uuid::now_v7(),
                title: collection.name.clone(),
                slug: root_slug,
                icon: collection.icon.clone(),
                cover_image: None,
                status: DocumentationStatus::Published,
                created_by: user_uuid,
                last_edited_by: user_uuid,
                parent_id: None,
                ticket_id: None,
                display_order: None,
                is_public: false,
                is_template: false,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: false,
            };
            repository::documentation::create_documentation_page(new_page, conn)?
        }
        Err(e) => return Err(e),
    };

    // Update collection to set root_page_id
    let update = DocumentationCollectionUpdate {
        name: None,
        slug: None,
        description: None,
        icon: None,
        color: None,
        updated_at: Some(Utc::now().naive_utc()),
        root_page_id: Some(Some(page.id)),
    };
    repository::documentation_collections::update_collection(conn, collection.id, update)?;

    // Add root page to the collection's page list (on_conflict_do_nothing handles duplicates)
    let entry = NewDocumentationCollectionPage {
        collection_id: collection.id,
        page_id: page.id,
        created_by: Some(user_uuid),
    };
    let _ = repository::documentation_collections::add_page_to_collection(conn, entry);

    Ok(page.id)
}

// ============================================================================
// Collection Endpoints
// ============================================================================

/// List collections visible to the current user
pub async fn get_collections(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    let claims = match require_auth(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    let is_admin = claims.role == "admin";

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::documentation_collections::get_collections_for_user(&mut conn, &user_uuid, is_admin) {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to get collections");
            HttpResponse::InternalServerError().json("Failed to get collections")
        }
    }
}

/// Get a single collection by ID with its page list
pub async fn get_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let claims = match require_auth(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let collection_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let mut collection = match repository::documentation_collections::get_collection(&mut conn, collection_id) {
        Ok(c) => c,
        Err(Error::NotFound) => return HttpResponse::NotFound().json("Collection not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Failed to get collection"),
    };

    // Lazy-create root page if missing
    if collection.root_page_id.is_none() {
        let user_uuid = Uuid::parse_str(&claims.sub).unwrap_or_default();
        match create_root_page_for_collection(&mut conn, &collection, user_uuid) {
            Ok(page_id) => {
                collection.root_page_id = Some(page_id);
            }
            Err(e) => {
                error!(error = ?e, "Failed to create root page for collection {}", collection_id);
            }
        }
    }

    let pages = repository::documentation_collections::get_pages_in_collection(&mut conn, collection_id)
        .unwrap_or_default();

    // Filter out the root page from the pages list
    let root_page_id = collection.root_page_id;
    let filtered_pages: Vec<_> = pages.into_iter()
        .filter(|p| Some(p.id) != root_page_id)
        .collect();

    let visible_groups = repository::documentation_collections::get_visible_groups_for_collection(&mut conn, collection_id)
        .unwrap_or_default();

    let visible_users = repository::documentation_collections::get_visible_users_for_collection(&mut conn, collection_id)
        .unwrap_or_default();

    let is_public = visible_groups.is_empty() && visible_users.is_empty();

    HttpResponse::Ok().json(json!({
        "id": collection.id,
        "uuid": collection.uuid,
        "name": collection.name,
        "slug": collection.slug,
        "description": collection.description,
        "icon": collection.icon,
        "color": collection.color,
        "is_system": collection.is_system,
        "created_by": collection.created_by,
        "created_at": collection.created_at,
        "updated_at": collection.updated_at,
        "root_page_id": collection.root_page_id,
        "pages": filtered_pages,
        "visible_to_groups": visible_groups,
        "visible_to_users": visible_users,
        "is_public": is_public,
        "page_count": filtered_pages.len(),
    }))
}

/// Get a single collection by slug
pub async fn get_collection_by_slug(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let claims = match require_auth(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let slug = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let mut collection = match repository::documentation_collections::get_collection_by_slug(&mut conn, &slug) {
        Ok(c) => c,
        Err(Error::NotFound) => return HttpResponse::NotFound().json("Collection not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Failed to get collection"),
    };

    // Lazy-create root page if missing
    if collection.root_page_id.is_none() {
        let user_uuid = Uuid::parse_str(&claims.sub).unwrap_or_default();
        match create_root_page_for_collection(&mut conn, &collection, user_uuid) {
            Ok(page_id) => {
                collection.root_page_id = Some(page_id);
            }
            Err(e) => {
                error!(error = ?e, "Failed to create root page for collection {}", collection.id);
            }
        }
    }

    let pages = repository::documentation_collections::get_pages_in_collection(&mut conn, collection.id)
        .unwrap_or_default();

    // Filter out the root page from the pages list
    let root_page_id = collection.root_page_id;
    let filtered_pages: Vec<_> = pages.into_iter()
        .filter(|p| Some(p.id) != root_page_id)
        .collect();

    let visible_groups = repository::documentation_collections::get_visible_groups_for_collection(&mut conn, collection.id)
        .unwrap_or_default();

    let visible_users = repository::documentation_collections::get_visible_users_for_collection(&mut conn, collection.id)
        .unwrap_or_default();

    let is_public = visible_groups.is_empty() && visible_users.is_empty();

    HttpResponse::Ok().json(json!({
        "id": collection.id,
        "uuid": collection.uuid,
        "name": collection.name,
        "slug": collection.slug,
        "description": collection.description,
        "icon": collection.icon,
        "color": collection.color,
        "is_system": collection.is_system,
        "created_by": collection.created_by,
        "created_at": collection.created_at,
        "updated_at": collection.updated_at,
        "root_page_id": collection.root_page_id,
        "pages": filtered_pages,
        "visible_to_groups": visible_groups,
        "visible_to_users": visible_users,
        "is_public": is_public,
        "page_count": filtered_pages.len(),
    }))
}

/// Get pages that don't belong to any collection
pub async fn get_uncollected_pages(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_auth(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::documentation_collections::get_uncollected_pages(&mut conn) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(e) => {
            error!(error = ?e, "Failed to get uncollected pages");
            HttpResponse::InternalServerError().json("Failed to get uncollected pages")
        }
    }
}

// ============================================================================
// Collection CRUD (technician+ / admin)
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateCollectionRequest {
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub visible_to_group_ids: Option<Vec<i32>>,
}

/// Create a new collection (technician+)
pub async fn create_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<CreateCollectionRequest>,
) -> impl Responder {
    let claims = match require_technician_or_admin(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Generate slug from name if not provided
    let slug = body.slug.clone().unwrap_or_else(|| {
        body.name.to_lowercase().replace(' ', "-")
    });

    let new_collection = NewDocumentationCollection {
        uuid: Uuid::now_v7(),
        name: body.name.clone(),
        slug,
        description: body.description.clone(),
        icon: body.icon.clone(),
        color: body.color.clone(),
        is_system: false,
        created_by,
        root_page_id: None,
    };

    match repository::documentation_collections::create_collection(&mut conn, new_collection) {
        Ok(mut collection) => {
            // Set visibility if specified
            if let Some(ref group_ids) = body.visible_to_group_ids {
                if !group_ids.is_empty() {
                    if let Err(e) = repository::documentation_collections::set_collection_visibility(
                        &mut conn,
                        collection.id,
                        group_ids.clone(),
                        Vec::new(),
                        created_by,
                    ) {
                        error!(error = ?e, "Failed to set collection visibility");
                    }
                }
            }

            // Auto-create root page
            if let Some(user_uuid) = created_by {
                match create_root_page_for_collection(&mut conn, &collection, user_uuid) {
                    Ok(page_id) => {
                        collection.root_page_id = Some(page_id);
                    }
                    Err(e) => {
                        error!(error = ?e, "Failed to create root page for collection {}", collection.id);
                    }
                }
            }

            HttpResponse::Created().json(collection)
        }
        Err(e) => {
            error!(error = ?e, "Failed to create collection");
            HttpResponse::InternalServerError().json("Failed to create collection")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateCollectionRequest {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

/// Update a collection (technician+, blocks system collection rename)
pub async fn update_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateCollectionRequest>,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let collection_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Check if system collection
    let collection = match repository::documentation_collections::get_collection(&mut conn, collection_id) {
        Ok(c) => c,
        Err(Error::NotFound) => return HttpResponse::NotFound().json("Collection not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Failed to get collection"),
    };

    if collection.is_system && (body.name.is_some() || body.slug.is_some()) {
        return HttpResponse::Forbidden().json("Cannot rename system collections");
    }

    let update = DocumentationCollectionUpdate {
        name: body.name.clone(),
        slug: body.slug.clone(),
        description: body.description.clone(),
        icon: body.icon.clone(),
        color: body.color.clone(),
        updated_at: Some(Utc::now().naive_utc()),
        root_page_id: None,
    };

    match repository::documentation_collections::update_collection(&mut conn, collection_id, update) {
        Ok(updated) => {
            // Broadcast SSE events for each updated field
            if let Some(ref name) = body.name {
                SseBroadcaster::broadcast_collection_updated(
                    &sse_state, collection_id, "name", serde_json::json!(name),
                ).await;
            }
            if let Some(ref icon) = body.icon {
                SseBroadcaster::broadcast_collection_updated(
                    &sse_state, collection_id, "icon", serde_json::json!(icon),
                ).await;
            }
            if let Some(ref description) = body.description {
                SseBroadcaster::broadcast_collection_updated(
                    &sse_state, collection_id, "description", serde_json::json!(description),
                ).await;
            }
            HttpResponse::Ok().json(updated)
        }
        Err(Error::NotFound) => HttpResponse::NotFound().json("Collection not found"),
        Err(e) => {
            error!(error = ?e, "Failed to update collection");
            HttpResponse::InternalServerError().json("Failed to update collection")
        }
    }
}

/// Delete a collection (admin only, blocks system collections)
pub async fn delete_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let collection_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Check if system collection
    match repository::documentation_collections::get_collection(&mut conn, collection_id) {
        Ok(c) if c.is_system => {
            return HttpResponse::Forbidden().json("Cannot delete system collections");
        }
        Ok(_) => {}
        Err(Error::NotFound) => return HttpResponse::NotFound().json("Collection not found"),
        Err(_) => return HttpResponse::InternalServerError().json("Failed to get collection"),
    }

    match repository::documentation_collections::delete_collection(&mut conn, collection_id) {
        Ok(0) => HttpResponse::NotFound().json("Collection not found"),
        Ok(_) => HttpResponse::Ok().json(json!({"success": true, "message": "Collection deleted"})),
        Err(e) => {
            error!(error = ?e, "Failed to delete collection");
            HttpResponse::InternalServerError().json("Failed to delete collection")
        }
    }
}

// ============================================================================
// Collection Page Management
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct AddPageRequest {
    pub page_id: i32,
}

/// Add a page to a collection (technician+)
pub async fn add_page_to_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<AddPageRequest>,
) -> impl Responder {
    let claims = match require_technician_or_admin(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let collection_id = path.into_inner();
    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let new_entry = NewDocumentationCollectionPage {
        collection_id,
        page_id: body.page_id,
        created_by,
    };

    match repository::documentation_collections::add_page_to_collection(&mut conn, new_entry) {
        Ok(entry) => HttpResponse::Created().json(entry),
        Err(e) => {
            error!(error = ?e, "Failed to add page to collection");
            HttpResponse::InternalServerError().json("Failed to add page to collection")
        }
    }
}

/// Remove a page from a collection (technician+)
pub async fn remove_page_from_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let (collection_id, page_id) = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::documentation_collections::remove_page_from_collection(&mut conn, collection_id, page_id) {
        Ok(0) => HttpResponse::NotFound().json("Page not in collection"),
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to remove page from collection");
            HttpResponse::InternalServerError().json("Failed to remove page from collection")
        }
    }
}

/// Get collections for a specific page
pub async fn get_collections_for_page(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_auth(&req) {
        return e;
    }

    let page_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::documentation_collections::get_collections_for_page(&mut conn, page_id) {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to get collections for page");
            HttpResponse::InternalServerError().json("Failed to get collections for page")
        }
    }
}

// ============================================================================
// Collection Visibility
// ============================================================================

/// Get visibility groups for a collection (technician+)
pub async fn get_collection_visibility(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let collection_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::documentation_collections::get_visible_groups_for_collection(&mut conn, collection_id) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(e) => {
            error!(error = ?e, "Failed to get collection visibility");
            HttpResponse::InternalServerError().json("Failed to get collection visibility")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetVisibilityRequest {
    pub group_ids: Vec<i32>,
    pub user_uuids: Option<Vec<String>>,
}

/// Set visibility groups for a collection (admin only)
pub async fn set_collection_visibility(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetVisibilityRequest>,
) -> impl Responder {
    let claims = match require_admin(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let collection_id = path.into_inner();
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

    match repository::documentation_collections::set_collection_visibility(
        &mut conn,
        collection_id,
        body.group_ids.clone(),
        user_uuids,
        created_by,
    ) {
        Ok(entries) => HttpResponse::Ok().json(entries),
        Err(e) => {
            error!(error = ?e, "Failed to set collection visibility");
            HttpResponse::InternalServerError().json("Failed to set collection visibility")
        }
    }
}

/// Get page-level visibility overrides for all pages in a collection (technician+)
pub async fn get_page_overrides_in_collection(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let collection_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get all pages in the collection
    let pages = match repository::documentation_collections::get_pages_in_collection(&mut conn, collection_id) {
        Ok(p) => p,
        Err(e) => {
            error!(error = ?e, "Failed to get pages in collection");
            return HttpResponse::InternalServerError().json("Failed to get pages");
        }
    };

    if pages.is_empty() {
        return HttpResponse::Ok().json(serde_json::Value::Array(vec![]));
    }

    let page_ids: Vec<i32> = pages.iter().map(|p| p.id).collect();

    // Batch-fetch all page-level overrides (groups and users)
    let group_overrides = match repository::documentation::get_page_visibility_overrides_batch(&mut conn, &page_ids) {
        Ok(o) => o,
        Err(e) => {
            error!(error = ?e, "Failed to get page visibility overrides");
            return HttpResponse::InternalServerError().json("Failed to get page overrides");
        }
    };

    let user_overrides = match repository::documentation::get_page_user_visibility_overrides_batch(&mut conn, &page_ids) {
        Ok(o) => o,
        Err(e) => {
            error!(error = ?e, "Failed to get page user visibility overrides");
            return HttpResponse::InternalServerError().json("Failed to get page overrides");
        }
    };

    // Group by page_id
    use std::collections::HashMap;
    let mut page_groups: HashMap<i32, Vec<serde_json::Value>> = HashMap::new();
    for (page_id, group_id, group_name) in &group_overrides {
        page_groups.entry(*page_id).or_default().push(json!({
            "id": group_id,
            "name": group_name,
        }));
    }

    let mut page_users: HashMap<i32, Vec<serde_json::Value>> = HashMap::new();
    for (page_id, user_uuid, user_name) in &user_overrides {
        page_users.entry(*page_id).or_default().push(json!({
            "uuid": user_uuid,
            "name": user_name,
        }));
    }

    // Combine page_ids that have any override
    let mut pages_with_overrides: std::collections::HashSet<i32> = page_groups.keys().copied().collect();
    pages_with_overrides.extend(page_users.keys());

    // Build response — only include pages that have overrides
    let page_map: HashMap<i32, _> = pages.iter().map(|p| (p.id, p)).collect();
    let result: Vec<serde_json::Value> = pages_with_overrides
        .into_iter()
        .filter_map(|pid| {
            page_map.get(&pid).map(|page| json!({
                "page_id": pid,
                "page_title": page.title,
                "page_icon": page.icon,
                "groups": page_groups.get(&pid).cloned().unwrap_or_default(),
                "users": page_users.get(&pid).cloned().unwrap_or_default(),
            }))
        })
        .collect();

    HttpResponse::Ok().json(result)
}

// ============================================================================
// Collection Reordering
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ReorderCollectionsRequest {
    pub collection_orders: Vec<crate::models::CollectionOrder>,
}

/// Reorder collections (technician+)
pub async fn reorder_collections(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<ReorderCollectionsRequest>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::documentation_collections::reorder_collections(&mut conn, &body.collection_orders) {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to reorder collections");
            HttpResponse::InternalServerError().json("Failed to reorder collections")
        }
    }
}

/// Update collections for a specific page (technician+)
/// Replaces all collection memberships for the page
#[derive(Debug, Deserialize)]
pub struct SetPageCollectionsRequest {
    pub collection_ids: Vec<i32>,
}

pub async fn set_page_collections(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetPageCollectionsRequest>,
) -> impl Responder {
    let claims = match require_technician_or_admin(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let page_id = path.into_inner();
    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Get current collections for this page
    let current_collections = match repository::documentation_collections::get_collections_for_page(&mut conn, page_id) {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, "Failed to get current collections");
            return HttpResponse::InternalServerError().json("Failed to update page collections");
        }
    };

    let current_ids: Vec<i32> = current_collections.iter().map(|c| c.id).collect();

    // Remove from collections not in the new list
    for id in &current_ids {
        if !body.collection_ids.contains(id) {
            let _ = repository::documentation_collections::remove_page_from_collection(&mut conn, *id, page_id);
        }
    }

    // Add to collections not in the current list
    for id in &body.collection_ids {
        if !current_ids.contains(id) {
            let entry = NewDocumentationCollectionPage {
                collection_id: *id,
                page_id,
                created_by,
            };
            let _ = repository::documentation_collections::add_page_to_collection(&mut conn, entry);
        }
    }

    // Return updated list
    match repository::documentation_collections::get_collections_for_page(&mut conn, page_id) {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to get updated collections");
            HttpResponse::InternalServerError().json("Failed to update page collections")
        }
    }
}
