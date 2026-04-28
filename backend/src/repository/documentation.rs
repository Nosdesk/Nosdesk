use diesel::prelude::*;
use diesel::result::Error;
use diesel::sql_types::{Integer, Nullable};

use crate::db::DbConnection;
use crate::models::{
    DocumentationPage, DocumentationPageWithChildren,
    NewDocumentationPage, DocumentationPageUpdate, PageOrder,
    DocumentationStatus,
};
use crate::schema::documentation_pages;

/// Observer fired after a documentation page's Yjs blob is
/// successfully saved by the collaborative editor. Mirrors
/// `UserCreatedObserver` and `ArticleContentSavedObserver`: defined
/// at the repo layer, implemented elsewhere by the search service
/// so the index reflects body edits without each save site needing
/// to wire up indexing manually.
pub trait DocumentationSavedObserver: Send + Sync {
    fn documentation_saved(&self, page: &DocumentationPage);
}

// Get all documentation pages (excludes archived and deleted)
pub fn get_documentation_pages(conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::status.eq_any([
            DocumentationStatus::Draft,
            DocumentationStatus::Published,
        ]))
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get a specific documentation page by ID
pub fn get_documentation_page(id: i32, conn: &mut DbConnection) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .find(id)
        .first::<DocumentationPage>(conn)
}

// Get a documentation page by its UUID
pub fn get_documentation_page_by_uuid(uuid: &uuid::Uuid, conn: &mut DbConnection) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .filter(documentation_pages::uuid.eq(uuid))
        .first::<DocumentationPage>(conn)
}

// Get a documentation page by its slug
pub fn get_documentation_page_by_slug(slug: &str, conn: &mut DbConnection) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .filter(documentation_pages::slug.eq(slug))
        .first::<DocumentationPage>(conn)
}

// Create a new documentation page
pub fn create_documentation_page(
    page: NewDocumentationPage,
    conn: &mut DbConnection,
) -> Result<DocumentationPage, Error> {
    diesel::insert_into(documentation_pages::table)
        .values(page)
        .get_result(conn)
}

// Update an existing documentation page
pub fn update_documentation_page(
    conn: &mut DbConnection,
    page_id: i32,
    page_update: &DocumentationPageUpdate,
) -> Result<DocumentationPage, Error> {
    diesel::update(documentation_pages::table.find(page_id))
        .set(page_update)
        .get_result(conn)
}

// Delete a documentation page
pub fn delete_documentation_page(id: i32, conn: &mut DbConnection) -> Result<usize, Error> {
    diesel::delete(documentation_pages::table.find(id)).execute(conn)
}

// Get top-level documentation pages (excludes archived and deleted)
pub fn get_top_level_pages(conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.is_null())
        .filter(documentation_pages::status.eq_any([
            DocumentationStatus::Draft,
            DocumentationStatus::Published,
        ]))
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get documentation pages by parent ID (excludes archived and deleted)
pub fn get_pages_by_parent_id(parent_id: i32, conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.eq(parent_id))
        .filter(documentation_pages::status.eq_any([
            DocumentationStatus::Draft,
            DocumentationStatus::Published,
        ]))
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get documentation pages by ticket ID
pub fn get_documentation_pages_by_ticket_id(conn: &mut DbConnection, ticket_id: i32) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::ticket_id.eq(ticket_id))
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Define a SQL function for coalesce
diesel::define_sql_function! {
    fn coalesce(x: Nullable<Integer>, y: Integer) -> Integer;
}

// Get ordered top-level documentation pages
pub fn get_ordered_top_level_pages(
    conn: &mut DbConnection,
) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.is_null())
        .order_by(coalesce(documentation_pages::display_order, 0).asc())
        .load::<DocumentationPage>(conn)
}

// Get ordered documentation pages by parent ID
pub fn get_ordered_pages_by_parent_id(
    conn: &mut DbConnection,
    parent_id: i32,
) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.eq(parent_id))
        .order_by(coalesce(documentation_pages::display_order, 0).asc())
        .load::<DocumentationPage>(conn)
}

// Reorder documentation pages
pub fn reorder_pages(
    conn: &mut DbConnection,
    parent_id: Option<i32>,
    page_orders: &[PageOrder],
) -> Result<Vec<DocumentationPage>, Error> {
    // Begin transaction
    conn.transaction(|conn| {
        let mut updated_pages = Vec::new();
        
        for order in page_orders {
            // Update the page's display_order and ensure it has the correct parent_id
            let updated_page = diesel::update(documentation_pages::table.find(order.page_id))
                .set((
                    documentation_pages::display_order.eq(order.display_order),
                    documentation_pages::parent_id.eq(parent_id),
                ))
                .get_result::<DocumentationPage>(conn)?;
                
            updated_pages.push(updated_page);
        }
        
        Ok(updated_pages)
    })
}

// Check if a page is a descendant of another page (to prevent circular references)
fn is_descendant_of(
    conn: &mut DbConnection,
    page_id: i32,
    potential_ancestor_id: i32,
) -> Result<bool, Error> {
    // Get all descendants of the potential ancestor recursively
    let descendants = get_all_descendant_ids(conn, potential_ancestor_id)?;
    Ok(descendants.contains(&page_id))
}

// Get all descendant IDs of a page recursively
fn get_all_descendant_ids(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<i32>, Error> {
    let mut all_descendants = Vec::new();
    let mut pages_to_check = vec![page_id];

    while !pages_to_check.is_empty() {
        // Get direct children of all pages in the current batch
        let children: Vec<DocumentationPage> = documentation_pages::table
            .filter(documentation_pages::parent_id.eq_any(&pages_to_check))
            .load(conn)?;

        // Clear the pages to check and add the children's IDs
        pages_to_check.clear();
        for child in children {
            all_descendants.push(child.id);
            pages_to_check.push(child.id);
        }
    }

    Ok(all_descendants)
}

// Move a page to a new parent
pub fn move_page_to_parent(
    conn: &mut DbConnection,
    page_id: i32,
    new_parent_id: Option<i32>,
    display_order: i32,
) -> Result<DocumentationPage, Error> {
    // Begin transaction
    conn.transaction(|conn| {
        // Validation 1: Cannot move a page to be its own parent
        if new_parent_id == Some(page_id) {
            return Err(Error::RollbackTransaction);
        }

        // Validation 2: Cannot move a page to be a child of its own descendant
        // (this would create a circular reference)
        if let Some(parent_id) = new_parent_id {
            if is_descendant_of(conn, parent_id, page_id)? {
                return Err(Error::RollbackTransaction);
            }
        }

        // Update the page's parent_id and display_order
        let updated_page = diesel::update(documentation_pages::table.find(page_id))
            .set((
                documentation_pages::parent_id.eq(new_parent_id),
                documentation_pages::display_order.eq(display_order),
            ))
            .get_result::<DocumentationPage>(conn)?;

        // Re-parenting under a page in a different collection
        // pulls this page (and only this page; descendants are
        // moved by the recursive walker if needed) into the
        // parent's collection. Same-collection moves and moves to
        // root (`new_parent_id == None`) are no-ops.
        if let Some(parent_id) = new_parent_id {
            crate::repository::documentation_collections::cascade_collection_membership(
                conn,
                parent_id,
                page_id,
                None,
            )?;
        }

        Ok(updated_page)
    })
}

// Get page with ordered children
pub fn get_page_with_ordered_children(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<DocumentationPageWithChildren, Error> {
    let page = get_documentation_page(page_id, conn)?;
    let children = get_ordered_pages_by_parent_id(conn, page_id)?;

    Ok(DocumentationPageWithChildren {
        page,
        children,
    })
}

// ============= Yjs Collaboration Methods =============

// Update documentation page Yjs state (for WebSocket sync auto-save)
pub fn update_documentation_yjs_state(
    conn: &mut DbConnection,
    page_id: i32,
    yjs_document: Vec<u8>,
    observer: Option<&dyn DocumentationSavedObserver>,
) -> Result<DocumentationPage, Error> {
    use crate::schema::documentation_pages::dsl;

    let result: DocumentationPage = diesel::update(dsl::documentation_pages.find(page_id))
        .set((
            dsl::yjs_document.eq(Some(yjs_document)),
            dsl::updated_at.eq(diesel::dsl::now),
        ))
        .get_result(conn)?;

    if let Some(observer) = observer {
        observer.documentation_saved(&result);
    }

    Ok(result)
}

// Create a documentation revision snapshot
// Note: This is simplified - the schema doesn't have a revision number or contributed_by
// Creates a basic revision with just the snapshot and metadata
pub fn create_documentation_revision(
    conn: &mut DbConnection,
    page_id: i32,
    yjs_state_vector: Vec<u8>,
    yjs_document_content: Vec<u8>,
    contributed_by: Vec<Option<uuid::Uuid>>,
) -> Result<i32, Error> {
    use crate::schema::documentation_pages::dsl as doc_dsl;
    use crate::schema::documentation_revisions;

    conn.transaction(|conn| {
        // Get current revision number from the page and created_by user
        let page: DocumentationPage = doc_dsl::documentation_pages
            .find(page_id)
            .first(conn)?;

        // Get the latest revision number for this page
        let latest_revision: i32 = documentation_revisions::table
            .filter(documentation_revisions::page_id.eq(page_id))
            .select(diesel::dsl::max(documentation_revisions::revision_number))
            .first::<Option<i32>>(conn)?
            .unwrap_or(0);

        let new_revision_number = latest_revision + 1;

        // Use the first contributor or the created_by from the page
        let created_by = contributed_by.first()
            .and_then(|opt_uuid| *opt_uuid)
            .unwrap_or(page.created_by);

        // Insert new revision (schema has different fields than article_content_revisions)
        diesel::insert_into(documentation_revisions::table)
            .values((
                documentation_revisions::page_id.eq(page_id),
                documentation_revisions::revision_number.eq(new_revision_number),
                documentation_revisions::title.eq(&page.title), // Snapshot the title
                documentation_revisions::yjs_document_snapshot.eq(yjs_document_content),
                documentation_revisions::yjs_state_vector.eq(yjs_state_vector),
                documentation_revisions::created_by.eq(created_by),
            ))
            .execute(conn)?;

        Ok(new_revision_number)
    })
}

// Get all revisions for a documentation page
pub fn get_documentation_revisions(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<crate::models::DocumentationRevision>, Error> {
    use crate::schema::documentation_revisions::dsl;

    dsl::documentation_revisions
        .filter(dsl::page_id.eq(page_id))
        .order_by(dsl::revision_number.desc())
        .load(conn)
}

// Get a specific revision for a documentation page
pub fn get_documentation_revision(
    conn: &mut DbConnection,
    page_id: i32,
    revision_number: i32,
) -> Result<crate::models::DocumentationRevision, Error> {
    use crate::schema::documentation_revisions::dsl;

    dsl::documentation_revisions
        .filter(dsl::page_id.eq(page_id))
        .filter(dsl::revision_number.eq(revision_number))
        .first(conn)
}

// Get the latest revision for a documentation page
pub fn get_latest_documentation_revision(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<crate::models::DocumentationRevision, Error> {
    use crate::schema::documentation_revisions::dsl;

    dsl::documentation_revisions
        .filter(dsl::page_id.eq(page_id))
        .order_by(dsl::revision_number.desc())
        .first(conn)
}

// ===== Documentation Page Embeddings =====

/// Sync the embedding relationships for a source page.
/// Deletes existing embeddings and replaces with the new set.
pub fn sync_page_embeddings(
    conn: &mut DbConnection,
    source_page_id: i32,
    target_page_ids: &[i32],
) -> Result<(), Error> {
    use crate::schema::documentation_page_embeddings;
    use crate::models::NewDocumentationPageEmbedding;

    // Delete all existing embeddings for this source page
    diesel::delete(
        documentation_page_embeddings::table
            .filter(documentation_page_embeddings::source_page_id.eq(source_page_id))
    ).execute(conn)?;

    if target_page_ids.is_empty() {
        return Ok(());
    }

    // Insert the new embeddings
    let new_embeddings: Vec<NewDocumentationPageEmbedding> = target_page_ids
        .iter()
        .map(|&target_id| NewDocumentationPageEmbedding {
            source_page_id,
            target_page_id: target_id,
        })
        .collect();

    diesel::insert_into(documentation_page_embeddings::table)
        .values(&new_embeddings)
        .on_conflict_do_nothing()
        .execute(conn)?;

    Ok(())
}

/// Batch-fetch page-level group visibility overrides for a set of page IDs.
/// Returns (page_id, group_id, group_name) tuples.
pub fn get_page_visibility_overrides_batch(
    conn: &mut DbConnection,
    page_ids: &[i32],
) -> Result<Vec<(i32, i32, String)>, Error> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }

    documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq_any(page_ids))
        .filter(documentation_page_visibility::group_id.is_not_null())
        .inner_join(groups::table.on(
            groups::id.nullable().eq(documentation_page_visibility::group_id),
        ))
        .select((
            documentation_page_visibility::page_id,
            groups::id,
            groups::name,
        ))
        .load::<(i32, i32, String)>(conn)
}

/// Batch-fetch page-level user visibility overrides for a set of page IDs.
/// Returns (page_id, user_uuid, user_name) tuples.
pub fn get_page_user_visibility_overrides_batch(
    conn: &mut DbConnection,
    page_ids: &[i32],
) -> Result<Vec<(i32, uuid::Uuid, String)>, Error> {
    use crate::schema::users;

    if page_ids.is_empty() {
        return Ok(Vec::new());
    }

    documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq_any(page_ids))
        .filter(documentation_page_visibility::user_uuid.is_not_null())
        .inner_join(users::table.on(
            users::uuid.nullable().eq(documentation_page_visibility::user_uuid),
        ))
        .select((
            documentation_page_visibility::page_id,
            users::uuid,
            users::name,
        ))
        .load::<(i32, uuid::Uuid, String)>(conn)
}

/// Get all pages that embed a given target page (for cache invalidation)
pub fn get_pages_embedding(
    conn: &mut DbConnection,
    target_page_id: i32,
) -> Result<Vec<i32>, Error> {
    use crate::schema::documentation_page_embeddings;

    documentation_page_embeddings::table
        .filter(documentation_page_embeddings::target_page_id.eq(target_page_id))
        .select(documentation_page_embeddings::source_page_id)
        .load::<i32>(conn)
}

/// Get all pages that a source page embeds
pub fn get_embedded_pages(
    conn: &mut DbConnection,
    source_page_id: i32,
) -> Result<Vec<i32>, Error> {
    use crate::schema::documentation_page_embeddings;

    documentation_page_embeddings::table
        .filter(documentation_page_embeddings::source_page_id.eq(source_page_id))
        .select(documentation_page_embeddings::target_page_id)
        .load::<i32>(conn)
}

// Get documentation pages by status (for archived/trash views)
pub fn get_pages_by_status(
    conn: &mut DbConnection,
    target_status: DocumentationStatus,
) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::status.eq_any([target_status]))
        .order_by(documentation_pages::updated_at.desc())
        .load::<DocumentationPage>(conn)
}

// Permanently delete a documentation page (hard delete for trash emptying)
pub fn permanently_delete_page(id: i32, conn: &mut DbConnection) -> Result<usize, Error> {
    diesel::delete(documentation_pages::table.find(id)).execute(conn)
}

// ===== Page Visibility (Access Control) =====

use crate::models::{
    DocumentationPageVisibility, NewDocumentationPageVisibility, Group, UserInfoWithAvatar,
};
use crate::schema::{documentation_page_visibility, documentation_collection_pages,
    documentation_collection_visibility, groups};

/// Get the groups that have explicit page-level visibility for a page.
pub fn get_visible_groups_for_page(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<Group>, Error> {
    documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq(page_id))
        .filter(documentation_page_visibility::group_id.is_not_null())
        .inner_join(groups::table.on(
            groups::id.nullable().eq(documentation_page_visibility::group_id),
        ))
        .select(groups::all_columns)
        .load(conn)
}

/// Get the users that have explicit page-level visibility for a page.
pub fn get_visible_users_for_page(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<UserInfoWithAvatar>, Error> {
    use crate::schema::users;

    documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq(page_id))
        .filter(documentation_page_visibility::user_uuid.is_not_null())
        .inner_join(users::table.on(
            users::uuid.nullable().eq(documentation_page_visibility::user_uuid),
        ))
        .select((users::uuid, users::name, users::avatar_url, users::avatar_thumb))
        .load::<(uuid::Uuid, String, Option<String>, Option<String>)>(conn)
        .map(|rows| {
            rows.into_iter()
                .map(|(uuid, name, avatar_url, avatar_thumb)| UserInfoWithAvatar {
                    uuid,
                    name,
                    avatar_url,
                    avatar_thumb,
                })
                .collect()
        })
}

/// Set page-level visibility (delete-all + re-insert).
/// Empty group_ids and user_uuids clears the override (page inherits from collections).
pub fn set_page_visibility(
    conn: &mut DbConnection,
    page_id: i32,
    group_ids: Vec<i32>,
    user_uuids: Vec<uuid::Uuid>,
    created_by: Option<uuid::Uuid>,
) -> Result<Vec<DocumentationPageVisibility>, Error> {
    // Delete all existing page-level visibility entries
    diesel::delete(
        documentation_page_visibility::table
            .filter(documentation_page_visibility::page_id.eq(page_id)),
    )
    .execute(conn)?;

    if group_ids.is_empty() && user_uuids.is_empty() {
        return Ok(Vec::new());
    }

    let mut new_entries: Vec<NewDocumentationPageVisibility> = Vec::new();

    for gid in &group_ids {
        new_entries.push(NewDocumentationPageVisibility {
            page_id,
            group_id: Some(*gid),
            created_by,
            user_uuid: None,
        });
    }

    for uid in &user_uuids {
        new_entries.push(NewDocumentationPageVisibility {
            page_id,
            group_id: None,
            created_by,
            user_uuid: Some(*uid),
        });
    }

    diesel::insert_into(documentation_page_visibility::table)
        .values(&new_entries)
        .get_results(conn)
}

/// Check whether a single user can access a page.
/// Logic: admin → true; page has override → check page groups + user;
///        else inherit from collections (no collections = public,
///        any public collection = public, else check group/user intersection).
pub fn can_user_access_page(
    conn: &mut DbConnection,
    page_id: i32,
    user_uuid: &uuid::Uuid,
    is_admin: bool,
) -> Result<bool, Error> {
    if is_admin {
        return Ok(true);
    }

    let user_group_ids = crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;

    // Check page-level override — count total entries to know if override exists
    let page_vis_count: i64 = documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq(page_id))
        .count()
        .get_result(conn)?;

    if page_vis_count > 0 {
        // Page has explicit override — check direct user grant
        let has_user_grant: i64 = documentation_page_visibility::table
            .filter(documentation_page_visibility::page_id.eq(page_id))
            .filter(documentation_page_visibility::user_uuid.eq(user_uuid))
            .count()
            .get_result(conn)?;

        if has_user_grant > 0 {
            return Ok(true);
        }

        // Check group grants
        let page_group_ids: Vec<Option<i32>> = documentation_page_visibility::table
            .filter(documentation_page_visibility::page_id.eq(page_id))
            .filter(documentation_page_visibility::group_id.is_not_null())
            .select(documentation_page_visibility::group_id)
            .load(conn)?;

        let page_group_ids: Vec<i32> = page_group_ids.into_iter().flatten().collect();
        return Ok(user_group_ids.iter().any(|uid| page_group_ids.contains(uid)));
    }

    // Inherit from collections
    let collection_ids: Vec<i32> = documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq(page_id))
        .select(documentation_collection_pages::collection_id)
        .load(conn)?;

    if collection_ids.is_empty() {
        // Page belongs to no collection → public
        return Ok(true);
    }

    // For each collection, check if it's public or the user has access
    for coll_id in &collection_ids {
        let coll_vis_count: i64 = documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq(*coll_id))
            .count()
            .get_result(conn)?;

        if coll_vis_count == 0 {
            // This collection is public
            return Ok(true);
        }

        // Check direct user grant on collection
        let has_user_grant: i64 = documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq(*coll_id))
            .filter(documentation_collection_visibility::user_uuid.eq(user_uuid))
            .count()
            .get_result(conn)?;

        if has_user_grant > 0 {
            return Ok(true);
        }

        // Check group grant on collection
        let coll_group_ids: Vec<Option<i32>> = documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq(*coll_id))
            .filter(documentation_collection_visibility::group_id.is_not_null())
            .select(documentation_collection_visibility::group_id)
            .load(conn)?;

        let coll_group_ids: Vec<i32> = coll_group_ids.into_iter().flatten().collect();
        if user_group_ids.iter().any(|uid| coll_group_ids.contains(uid)) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Batch-filter a list of pages for a user. Uses bulk queries to avoid N+1.
/// Returns only the pages the user can access.
pub fn filter_pages_for_user(
    conn: &mut DbConnection,
    pages: Vec<DocumentationPage>,
    user_uuid: &uuid::Uuid,
    is_admin: bool,
) -> Result<Vec<DocumentationPage>, Error> {
    if is_admin || pages.is_empty() {
        return Ok(pages);
    }

    let page_ids: Vec<i32> = pages.iter().map(|p| p.id).collect();

    // 1. User's group IDs (includes composite parent groups)
    let user_group_ids = crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;

    // 2. All page-level visibility entries for these pages (group-based)
    let page_vis_groups: Vec<(i32, Option<i32>)> = documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq_any(&page_ids))
        .filter(documentation_page_visibility::group_id.is_not_null())
        .select((
            documentation_page_visibility::page_id,
            documentation_page_visibility::group_id,
        ))
        .load(conn)?;

    // 2b. All page-level visibility entries for these pages (user-based)
    let page_vis_users: Vec<(i32, Option<uuid::Uuid>)> = documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq_any(&page_ids))
        .filter(documentation_page_visibility::user_uuid.is_not_null())
        .select((
            documentation_page_visibility::page_id,
            documentation_page_visibility::user_uuid,
        ))
        .load(conn)?;

    // 2c. Pages that have ANY visibility override (to know if override exists)
    let pages_with_override: Vec<i32> = documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq_any(&page_ids))
        .select(documentation_page_visibility::page_id)
        .distinct()
        .load(conn)?;

    // 3. All page→collection memberships
    let page_colls: Vec<(i32, i32)> = documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq_any(&page_ids))
        .select((
            documentation_collection_pages::page_id,
            documentation_collection_pages::collection_id,
        ))
        .load(conn)?;

    // Collect all unique collection IDs
    let coll_ids: Vec<i32> = page_colls.iter().map(|(_, cid)| *cid).collect::<std::collections::HashSet<_>>().into_iter().collect();

    // 4. All collection-level visibility for those collections (groups)
    let coll_vis_groups: Vec<(i32, Option<i32>)> = if !coll_ids.is_empty() {
        documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq_any(&coll_ids))
            .filter(documentation_collection_visibility::group_id.is_not_null())
            .select((
                documentation_collection_visibility::collection_id,
                documentation_collection_visibility::group_id,
            ))
            .load(conn)?
    } else {
        Vec::new()
    };

    // 4b. Collection-level visibility (users)
    let coll_vis_users: Vec<(i32, Option<uuid::Uuid>)> = if !coll_ids.is_empty() {
        documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq_any(&coll_ids))
            .filter(documentation_collection_visibility::user_uuid.is_not_null())
            .select((
                documentation_collection_visibility::collection_id,
                documentation_collection_visibility::user_uuid,
            ))
            .load(conn)?
    } else {
        Vec::new()
    };

    // 4c. Collections that have ANY visibility entries
    let colls_with_vis: Vec<i32> = if !coll_ids.is_empty() {
        documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq_any(&coll_ids))
            .select(documentation_collection_visibility::collection_id)
            .distinct()
            .load(conn)?
    } else {
        Vec::new()
    };

    // Build lookup maps
    use std::collections::{HashMap, HashSet};

    let pages_with_override_set: HashSet<i32> = pages_with_override.into_iter().collect();

    // page_id → set of group_ids (page-level override)
    let mut page_override_groups: HashMap<i32, HashSet<i32>> = HashMap::new();
    for (pid, gid) in &page_vis_groups {
        if let Some(gid) = gid {
            page_override_groups.entry(*pid).or_default().insert(*gid);
        }
    }

    // page_id → set of user_uuids (page-level override)
    let mut page_override_users: HashMap<i32, HashSet<uuid::Uuid>> = HashMap::new();
    for (pid, uid) in &page_vis_users {
        if let Some(uid) = uid {
            page_override_users.entry(*pid).or_default().insert(*uid);
        }
    }

    // page_id → set of collection_ids
    let mut page_to_colls: HashMap<i32, HashSet<i32>> = HashMap::new();
    for (pid, cid) in &page_colls {
        page_to_colls.entry(*pid).or_default().insert(*cid);
    }

    // collection_id → set of group_ids
    let mut coll_to_groups: HashMap<i32, HashSet<i32>> = HashMap::new();
    for (cid, gid) in &coll_vis_groups {
        if let Some(gid) = gid {
            coll_to_groups.entry(*cid).or_default().insert(*gid);
        }
    }

    // collection_id → set of user_uuids
    let mut coll_to_users: HashMap<i32, HashSet<uuid::Uuid>> = HashMap::new();
    for (cid, uid) in &coll_vis_users {
        if let Some(uid) = uid {
            coll_to_users.entry(*cid).or_default().insert(*uid);
        }
    }

    let colls_with_vis_set: HashSet<i32> = colls_with_vis.into_iter().collect();
    let user_groups_set: HashSet<i32> = user_group_ids.into_iter().collect();

    // Filter
    let filtered = pages.into_iter().filter(|page| {
        // Page-level override?
        if pages_with_override_set.contains(&page.id) {
            // Check direct user grant
            if let Some(pg_users) = page_override_users.get(&page.id) {
                if pg_users.contains(user_uuid) {
                    return true;
                }
            }
            // Check group grant
            if let Some(pg_groups) = page_override_groups.get(&page.id) {
                if pg_groups.iter().any(|g| user_groups_set.contains(g)) {
                    return true;
                }
            }
            return false;
        }

        // Inherit from collections
        let colls = match page_to_colls.get(&page.id) {
            Some(c) => c,
            None => return true, // no collections → public
        };

        for cid in colls {
            if !colls_with_vis_set.contains(cid) {
                // Collection has no visibility entries → public
                return true;
            }

            // Check direct user grant on collection
            if let Some(cu) = coll_to_users.get(cid) {
                if cu.contains(user_uuid) {
                    return true;
                }
            }

            // Check group grant on collection
            if let Some(cg) = coll_to_groups.get(cid) {
                if cg.iter().any(|g| user_groups_set.contains(g)) {
                    return true;
                }
            }
        }

        false
    }).collect();

    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DocumentationStatus, UserRole};
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use uuid::Uuid;

    fn make_page(created_by: Uuid) -> NewDocumentationPage {
        NewDocumentationPage {
            uuid: Uuid::new_v4(),
            title: "Test Page".to_string(),
            slug: "test-page".to_string(),
            icon: None,
            cover_image: None,
            status: DocumentationStatus::Draft,
            created_by,
            last_edited_by: created_by,
            parent_id: None,
            ticket_id: None,
            display_order: None,
            is_public: false,
            is_template: false,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: false,
        }
    }

    #[test]
    fn create_and_get_documentation_page() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "docuser", UserRole::Admin);

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let fetched = get_documentation_page(page.id, &mut conn).unwrap();
        assert_eq!(fetched.title, "Test Page");
    }

    #[test]
    fn get_by_slug() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sluguser", UserRole::Admin);

        create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let fetched = get_documentation_page_by_slug("test-page", &mut conn).unwrap();
        assert_eq!(fetched.title, "Test Page");
    }

    #[test]
    fn update_documentation_page_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "upduser", UserRole::Admin);

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let update = DocumentationPageUpdate {
            title: Some("Updated Title".to_string()),
            slug: None,
            icon: None,
            cover_image: None,
            status: None,
            last_edited_by: None,
            parent_id: None,
            ticket_id: None,
            display_order: None,
            is_public: None,
            is_template: None,
            archived_at: None,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: None,
            updated_at: None,
            deleted_at: None,
        };
        let updated = update_documentation_page(&mut conn, page.id, &update).unwrap();
        assert_eq!(updated.title, "Updated Title");
    }

    #[test]
    fn delete_documentation_page_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "deluser", UserRole::Admin);

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let rows = delete_documentation_page(page.id, &mut conn).unwrap();
        assert_eq!(rows, 1);
        assert!(get_documentation_page(page.id, &mut conn).is_err());
    }

    #[test]
    fn top_level_pages_excludes_children() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "treeuser", UserRole::Admin);

        let parent = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();

        let mut child_page = make_page(user.uuid);
        child_page.title = "Child Page".to_string();
        child_page.slug = "child-page".to_string();
        child_page.parent_id = Some(parent.id);
        create_documentation_page(child_page, &mut conn).unwrap();

        let top = get_top_level_pages(&mut conn).unwrap();
        assert!(top.iter().all(|p| p.parent_id.is_none()));
        assert!(top.iter().any(|p| p.id == parent.id));
    }
}
