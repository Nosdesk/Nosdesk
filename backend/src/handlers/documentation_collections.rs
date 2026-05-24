use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use diesel::result::Error;
use serde::Deserialize;
use serde_json::json;
use tracing::error;
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::handlers::sse::{SseEvent, SseState};
use crate::models::{
    DocumentationCollectionUpdate, NewDocumentationCollection, NewDocumentationCollectionPage,
};
use crate::repository;

// ============================================================================
// Collection Endpoints
// ============================================================================

/// List collections visible to the current user
pub async fn get_collections(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    let user_uuid = auth.user_uuid;
    let is_admin = auth.is_admin();

    match tc.run(|conn| {
        repository::documentation_collections::get_collections_for_user(conn, &user_uuid, is_admin)
    }) {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to get collections");
            errors::internal("Failed to get collections")
        }
    }
}

/// Outcome of fetching a collection plus its render payload.
enum CollectionFetchOutcome {
    Ok(serde_json::Value),
    NotFound,
    Failed,
}

/// Get a single collection by ID with its page list. The
/// collection owns its rich description directly via
/// `description_yjs`; the FE binds the editor to the
/// `collection-${id}` Yjs room rather than to a sentinel page.
pub async fn get_collection(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let collection_id = path.into_inner();

    let outcome = tc.run(|conn| {
        let collection =
            match repository::documentation_collections::get_collection(conn, collection_id) {
                Ok(c) => c,
                Err(Error::NotFound) => return Ok(CollectionFetchOutcome::NotFound),
                Err(_) => return Ok(CollectionFetchOutcome::Failed),
            };
        Ok::<_, diesel::result::Error>(CollectionFetchOutcome::Ok(collection_response(
            conn, collection,
        )))
    });

    match outcome {
        Ok(CollectionFetchOutcome::Ok(payload)) => HttpResponse::Ok().json(payload),
        Ok(CollectionFetchOutcome::NotFound) => errors::not_found_msg("Collection not found"),
        Ok(CollectionFetchOutcome::Failed) => errors::internal("Failed to get collection"),
        Err(_) => errors::internal("Failed to get collection"),
    }
}

/// Get a single collection by slug.
pub async fn get_collection_by_slug(
    mut tc: TenantConn,
    path: web::Path<String>,
    _auth: AuthContext,
) -> impl Responder {
    let slug = path.into_inner();

    let outcome = tc.run(|conn| {
        let collection =
            match repository::documentation_collections::get_collection_by_slug(conn, &slug) {
                Ok(c) => c,
                Err(Error::NotFound) => return Ok(CollectionFetchOutcome::NotFound),
                Err(_) => return Ok(CollectionFetchOutcome::Failed),
            };
        Ok::<_, diesel::result::Error>(CollectionFetchOutcome::Ok(collection_response(
            conn, collection,
        )))
    });

    match outcome {
        Ok(CollectionFetchOutcome::Ok(payload)) => HttpResponse::Ok().json(payload),
        Ok(CollectionFetchOutcome::NotFound) => errors::not_found_msg("Collection not found"),
        Ok(CollectionFetchOutcome::Failed) => errors::internal("Failed to get collection"),
        Err(_) => errors::internal("Failed to get collection"),
    }
}

/// Build the JSON shape the FE expects. Pulls pages, visibility,
/// and computes `is_public`. Hides the binary Yjs columns from
/// the wire — the FE binds to the `collection-${id}` collab room
/// to read them through the existing snapshot endpoint.
fn collection_response(
    conn: &mut crate::db::DbConnection,
    collection: crate::models::DocumentationCollection,
) -> serde_json::Value {
    let pages = repository::documentation_collections::get_pages_in_collection(conn, collection.id)
        .unwrap_or_default();
    let visible_groups = repository::documentation_collections::get_visible_groups_for_collection(
        conn,
        collection.id,
    )
    .unwrap_or_default();
    let visible_users = repository::documentation_collections::get_visible_users_for_collection(
        conn,
        collection.id,
    )
    .unwrap_or_default();
    let is_public = visible_groups.is_empty() && visible_users.is_empty();

    json!({
        "id": collection.id,
        "uuid": collection.uuid,
        "name": collection.name,
        "slug": collection.slug,
        "description": collection.description,
        "description_text": collection.description_text,
        "description_doc_id": format!("collection-{}", collection.id),
        "hide_titles_from_non_members": collection.hide_titles_from_non_members,
        "icon": collection.icon,
        "color": collection.color,
        "is_system": collection.is_system,
        "created_by": collection.created_by,
        "created_at": collection.created_at,
        "updated_at": collection.updated_at,
        "page_count": pages.len(),
        "pages": pages,
        "visible_to_groups": visible_groups,
        "visible_to_users": visible_users,
        "is_public": is_public,
    })
}

/// Get pages that don't belong to any collection
pub async fn get_uncollected_pages(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(|conn| repository::documentation_collections::get_uncollected_pages(conn)) {
        Ok(pages) => HttpResponse::Ok().json(pages),
        Err(e) => {
            error!(error = ?e, "Failed to get uncollected pages");
            errors::internal("Failed to get uncollected pages")
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
    mut tc: TenantConn,
    body: web::Json<CreateCollectionRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }
    let created_by = Some(auth.user_uuid);
    let body = body.into_inner();

    // Generate slug from name if not provided
    let slug = body
        .slug
        .clone()
        .unwrap_or_else(|| body.name.to_lowercase().replace(' ', "-"));

    let new_collection = NewDocumentationCollection {
        uuid: Uuid::now_v7(),
        name: body.name.clone(),
        slug,
        description: body.description.clone(),
        icon: body.icon.clone(),
        color: body.color.clone(),
        is_system: false,
        created_by,
    };

    let result = tc.run(|conn| {
        let collection =
            repository::documentation_collections::create_collection(conn, new_collection)?;
        if let Some(ref group_ids) = body.visible_to_group_ids {
            if !group_ids.is_empty() {
                if let Err(e) = repository::documentation_collections::set_collection_visibility(
                    conn,
                    collection.id,
                    group_ids.clone(),
                    Vec::new(),
                    created_by,
                ) {
                    error!(error = ?e, "Failed to set collection visibility");
                }
            }
        }
        let payload = collection_response(conn, collection);
        Ok::<_, diesel::result::Error>(payload)
    });

    match result {
        Ok(payload) => HttpResponse::Created().json(payload),
        Err(e) => {
            error!(error = ?e, "Failed to create collection");
            errors::internal("Failed to create collection")
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
    pub hide_titles_from_non_members: Option<bool>,
}

/// Outcome of the update_collection transaction.
enum UpdateCollectionOutcome {
    Ok(crate::models::DocumentationCollection),
    NotFound,
    SystemRenameBlocked,
    UpdateFailed,
}

/// Update a collection (technician+, blocks system collection rename)
pub async fn update_collection(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<UpdateCollectionRequest>,
    sse_state: web::Data<SseState>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }

    let collection_id = path.into_inner();
    let body = body.into_inner();

    let body_name = body.name.clone();
    let body_icon = body.icon.clone();
    let body_description = body.description.clone();

    let outcome = tc.run(|conn| {
        let collection =
            match repository::documentation_collections::get_collection(conn, collection_id) {
                Ok(c) => c,
                Err(Error::NotFound) => return Ok(UpdateCollectionOutcome::NotFound),
                Err(_) => return Ok(UpdateCollectionOutcome::UpdateFailed),
            };

        if collection.is_system && (body.name.is_some() || body.slug.is_some()) {
            return Ok(UpdateCollectionOutcome::SystemRenameBlocked);
        }

        let update = DocumentationCollectionUpdate {
            name: body.name.clone(),
            slug: body.slug.clone(),
            description: body.description.clone(),
            icon: body.icon.clone(),
            color: body.color.clone(),
            updated_at: Some(Utc::now().naive_utc()),
            hide_titles_from_non_members: body.hide_titles_from_non_members,
            ..Default::default()
        };

        match repository::documentation_collections::update_collection(
            conn,
            collection_id,
            update,
        ) {
            Ok(updated) => Ok::<_, diesel::result::Error>(UpdateCollectionOutcome::Ok(updated)),
            Err(Error::NotFound) => Ok(UpdateCollectionOutcome::NotFound),
            Err(e) => {
                error!(error = ?e, "Failed to update collection");
                Ok(UpdateCollectionOutcome::UpdateFailed)
            }
        }
    });

    match outcome {
        Ok(UpdateCollectionOutcome::Ok(updated)) => {
            // Broadcast SSE events for each updated field. One event
            // per field so the frontend can apply the change at field
            // granularity.
            let updates: [(&str, Option<serde_json::Value>); 3] = [
                ("name", body_name.as_ref().map(|v| serde_json::json!(v))),
                ("icon", body_icon.as_ref().map(|v| serde_json::json!(v))),
                (
                    "description",
                    body_description.as_ref().map(|v| serde_json::json!(v)),
                ),
            ];
            for (field, value) in updates.into_iter().filter_map(|(f, v)| v.map(|v| (f, v))) {
                sse_state
                    .broadcast_event(SseEvent::CollectionUpdated {
                        collection_id,
                        field: field.to_string(),
                        value,
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
            HttpResponse::Ok().json(updated)
        }
        Ok(UpdateCollectionOutcome::NotFound) => errors::not_found_msg("Collection not found"),
        Ok(UpdateCollectionOutcome::SystemRenameBlocked) => {
            errors::forbidden("Cannot rename system collections")
        }
        Ok(UpdateCollectionOutcome::UpdateFailed) => errors::internal("Failed to update collection"),
        Err(_) => errors::internal("Failed to update collection"),
    }
}

/// Outcome of the delete_collection transaction.
enum DeleteCollectionOutcome {
    Ok { pages_trashed: usize },
    NotFound,
    SystemDeleteBlocked,
    Failed,
}

/// Delete a collection (admin only, blocks system collections)
pub async fn delete_collection(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Admin required");
    }

    let collection_id = path.into_inner();

    let outcome = tc.run(|conn| {
        match repository::documentation_collections::get_collection(conn, collection_id) {
            Ok(c) if c.is_system => return Ok(DeleteCollectionOutcome::SystemDeleteBlocked),
            Ok(_) => {}
            Err(Error::NotFound) => return Ok(DeleteCollectionOutcome::NotFound),
            Err(_) => return Ok(DeleteCollectionOutcome::Failed),
        }

        // Soft-delete the collection's pages first so they remain
        // restorable from the trash, then drop the collection row
        // itself. The junction rows cascade via FK on collection
        // delete; the soft-delete pass turns the pages into trash
        // entries rather than orphaned/visible rows.
        let soft_deleted =
            match repository::documentation_collections::soft_delete_pages_in_collection(
                conn,
                collection_id,
            ) {
                Ok(n) => n,
                Err(e) => {
                    error!(error = ?e, "Failed to soft-delete pages in collection");
                    return Ok(DeleteCollectionOutcome::Failed);
                }
            };

        match repository::documentation_collections::delete_collection(conn, collection_id) {
            Ok(0) => Ok::<_, diesel::result::Error>(DeleteCollectionOutcome::NotFound),
            Ok(_) => Ok(DeleteCollectionOutcome::Ok {
                pages_trashed: soft_deleted,
            }),
            Err(e) => {
                error!(error = ?e, "Failed to delete collection");
                Ok(DeleteCollectionOutcome::Failed)
            }
        }
    });

    match outcome {
        Ok(DeleteCollectionOutcome::Ok { pages_trashed }) => HttpResponse::Ok().json(json!({
            "success": true,
            "message": "Collection deleted",
            "pages_trashed": pages_trashed,
        })),
        Ok(DeleteCollectionOutcome::NotFound) => errors::not_found_msg("Collection not found"),
        Ok(DeleteCollectionOutcome::SystemDeleteBlocked) => {
            errors::forbidden("Cannot delete system collections")
        }
        Ok(DeleteCollectionOutcome::Failed) => errors::internal("Failed to delete collection"),
        Err(_) => errors::internal("Failed to delete collection"),
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
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<AddPageRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }

    let collection_id = path.into_inner();
    let created_by = Some(auth.user_uuid);

    let new_entry = NewDocumentationCollectionPage {
        collection_id,
        page_id: body.page_id,
        created_by,
    };

    // Use the at-root variant: detaches any previous collection
    // membership AND nulls parent_id so the page anchors at the
    // new collection's root instead of dangling under a parent
    // that's now in a different collection.
    match tc.run(|conn| {
        repository::documentation_collections::add_page_to_collection_at_root(conn, new_entry)
    }) {
        Ok(entry) => HttpResponse::Created().json(entry),
        Err(e) => {
            error!(error = ?e, "Failed to add page to collection");
            errors::internal("Failed to add page to collection")
        }
    }
}

/// Remove a page from a collection (technician+)
pub async fn remove_page_from_collection(
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }

    let (collection_id, page_id) = path.into_inner();

    match tc.run(|conn| {
        repository::documentation_collections::remove_page_from_collection(
            conn,
            collection_id,
            page_id,
        )
    }) {
        Ok(0) => errors::not_found_msg("Page not in collection"),
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to remove page from collection");
            errors::internal("Failed to remove page from collection")
        }
    }
}

/// Get collections for a specific page
pub async fn get_collections_for_page(
    mut tc: TenantConn,
    path: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let page_id = path.into_inner();

    match tc
        .run(|conn| repository::documentation_collections::get_collections_for_page(conn, page_id))
    {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to get collections for page");
            errors::internal("Failed to get collections for page")
        }
    }
}

// ============================================================================
// Collection Visibility
// ============================================================================

/// Get visibility groups for a collection (technician+)
pub async fn get_collection_visibility(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }

    let collection_id = path.into_inner();

    match tc.run(|conn| {
        repository::documentation_collections::get_visible_groups_for_collection(
            conn,
            collection_id,
        )
    }) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(e) => {
            error!(error = ?e, "Failed to get collection visibility");
            errors::internal("Failed to get collection visibility")
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
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SetVisibilityRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_admin() {
        return errors::forbidden("Admin required");
    }

    let collection_id = path.into_inner();
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
        repository::documentation_collections::set_collection_visibility(
            conn,
            collection_id,
            body.group_ids.clone(),
            user_uuids.clone(),
            created_by,
        )
    }) {
        Ok(entries) => HttpResponse::Ok().json(entries),
        Err(e) => {
            error!(error = ?e, "Failed to set collection visibility");
            errors::internal("Failed to set collection visibility")
        }
    }
}

/// Outcome of the page-overrides aggregation.
enum PageOverridesOutcome {
    Ok(Vec<serde_json::Value>),
    PagesFailed,
    OverridesFailed,
}

/// Get page-level visibility overrides for all pages in a collection (technician+)
pub async fn get_page_overrides_in_collection(
    mut tc: TenantConn,
    path: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }

    let collection_id = path.into_inner();

    let outcome = tc.run(|conn| {
        let pages = match repository::documentation_collections::get_pages_in_collection(
            conn,
            collection_id,
        ) {
            Ok(p) => p,
            Err(e) => {
                error!(error = ?e, "Failed to get pages in collection");
                return Ok(PageOverridesOutcome::PagesFailed);
            }
        };

        if pages.is_empty() {
            return Ok(PageOverridesOutcome::Ok(Vec::new()));
        }

        let page_ids: Vec<i32> = pages.iter().map(|p| p.id).collect();

        let group_overrides = match repository::documentation::get_page_visibility_overrides_batch(
            conn, &page_ids,
        ) {
            Ok(o) => o,
            Err(e) => {
                error!(error = ?e, "Failed to get page visibility overrides");
                return Ok(PageOverridesOutcome::OverridesFailed);
            }
        };

        let user_overrides =
            match repository::documentation::get_page_user_visibility_overrides_batch(
                conn, &page_ids,
            ) {
                Ok(o) => o,
                Err(e) => {
                    error!(error = ?e, "Failed to get page user visibility overrides");
                    return Ok(PageOverridesOutcome::OverridesFailed);
                }
            };

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

        let mut pages_with_overrides: std::collections::HashSet<i32> =
            page_groups.keys().copied().collect();
        pages_with_overrides.extend(page_users.keys());

        let page_map: HashMap<i32, _> = pages.iter().map(|p| (p.id, p)).collect();
        let result: Vec<serde_json::Value> = pages_with_overrides
            .into_iter()
            .filter_map(|pid| {
                page_map.get(&pid).map(|page| {
                    json!({
                        "page_id": pid,
                        "page_title": page.title,
                        "page_icon": page.icon,
                        "groups": page_groups.get(&pid).cloned().unwrap_or_default(),
                        "users": page_users.get(&pid).cloned().unwrap_or_default(),
                    })
                })
            })
            .collect();
        Ok::<_, diesel::result::Error>(PageOverridesOutcome::Ok(result))
    });

    match outcome {
        Ok(PageOverridesOutcome::Ok(payload)) => HttpResponse::Ok().json(payload),
        Ok(PageOverridesOutcome::PagesFailed) => errors::internal("Failed to get pages"),
        Ok(PageOverridesOutcome::OverridesFailed) => errors::internal("Failed to get page overrides"),
        Err(_) => errors::internal("Failed to get page overrides"),
    }
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
    mut tc: TenantConn,
    body: web::Json<ReorderCollectionsRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }
    let body = body.into_inner();

    match tc.run(|conn| {
        repository::documentation_collections::reorder_collections(conn, &body.collection_orders)
    }) {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to reorder collections");
            errors::internal("Failed to reorder collections")
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
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SetPageCollectionsRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden");
    }

    let page_id = path.into_inner();
    let created_by = Some(auth.user_uuid);
    let body = body.into_inner();

    let result = tc.run(|conn| {
        let current_collections =
            match repository::documentation_collections::get_collections_for_page(conn, page_id) {
                Ok(c) => c,
                Err(e) => {
                    error!(error = ?e, "Failed to get current collections");
                    return Err(e);
                }
            };

        let current_ids: Vec<i32> = current_collections.iter().map(|c| c.id).collect();

        // Remove from collections not in the new list
        for id in &current_ids {
            if !body.collection_ids.contains(id) {
                let _ = repository::documentation_collections::remove_page_from_collection(
                    conn, *id, page_id,
                );
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
                let _ = repository::documentation_collections::add_page_to_collection(conn, entry);
            }
        }

        repository::documentation_collections::get_collections_for_page(conn, page_id)
    });

    match result {
        Ok(collections) => HttpResponse::Ok().json(collections),
        Err(e) => {
            error!(error = ?e, "Failed to update page collections");
            errors::internal("Failed to update page collections")
        }
    }
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Collections gate by role + tier:
    //! delete is admin-only, while create/update accept technicians.
    //! We test delete here — accidentally widening that gate from
    //! admin to technician would let any tech wipe documentation.
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new().app_data(web::Data::new(pool)).route(
            "/admin/documentation-collections/{id}",
            web::delete().to(delete_collection),
        )
    }

    #[actix_web::test]
    async fn delete_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/documentation-collections/1")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn delete_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/documentation-collections/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn delete_rejects_technician_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::Technician);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/documentation-collections/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
