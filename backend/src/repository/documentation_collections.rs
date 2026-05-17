use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::documentation_collection_visibility;
use crate::schema::*;

// ============================================================================
// Collection CRUD Operations
// ============================================================================

// sync-pending-wire: needs sync aggregate wiring
pub fn create_collection(
    conn: &mut DbConnection,
    new_collection: NewDocumentationCollection,
) -> QueryResult<DocumentationCollection> {
    diesel::insert_into(documentation_collections::table)
        .values(&new_collection)
        .get_result(conn)
}

pub fn get_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<DocumentationCollection> {
    documentation_collections::table
        .find(collection_id)
        .first(conn)
}

pub fn get_collection_by_slug(
    conn: &mut DbConnection,
    slug: &str,
) -> QueryResult<DocumentationCollection> {
    documentation_collections::table
        .filter(documentation_collections::slug.eq(slug))
        .first(conn)
}

pub fn get_all_collections(conn: &mut DbConnection) -> QueryResult<Vec<DocumentationCollection>> {
    documentation_collections::table
        .order((
            documentation_collections::display_order.asc(),
            documentation_collections::name.asc(),
        ))
        .load(conn)
}

// sync-pending-wire: needs sync aggregate wiring
pub fn reorder_collections(
    conn: &mut DbConnection,
    orders: &[CollectionOrder],
) -> Result<Vec<DocumentationCollection>, Error> {
    conn.transaction(|conn| {
        for order in orders {
            diesel::update(documentation_collections::table.find(order.collection_id))
                .set(documentation_collections::display_order.eq(order.display_order))
                .execute(conn)?;
        }
        get_all_collections(conn)
    })
}

// sync-pending-wire: needs sync aggregate wiring
pub fn update_collection(
    conn: &mut DbConnection,
    collection_id: i32,
    update: DocumentationCollectionUpdate,
) -> QueryResult<DocumentationCollection> {
    diesel::update(documentation_collections::table.find(collection_id))
        .set(&update)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring
pub fn delete_collection(conn: &mut DbConnection, collection_id: i32) -> QueryResult<usize> {
    diesel::delete(documentation_collections::table.find(collection_id)).execute(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Soft-delete every page that lives in this collection. Called
/// before `delete_collection` so the page rows survive (preserving
/// authorship + revision history) but vanish from every tree
/// traversal. Pages can later be permanently deleted from the
/// trash view, or restored into a different collection if the
/// admin changes their mind. With `UNIQUE(page_id)` on the
/// junction, "every page in this collection" is unambiguous: each
/// page belongs to exactly one collection.
pub fn soft_delete_pages_in_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<usize> {
    let now = chrono::Utc::now().naive_utc();
    diesel::update(
        documentation_pages::table.filter(
            documentation_pages::id.eq_any(
                documentation_collection_pages::table
                    .filter(documentation_collection_pages::collection_id.eq(collection_id))
                    .select(documentation_collection_pages::page_id),
            ),
        ),
    )
    .set((
        documentation_pages::status.eq(DocumentationStatus::Deleted),
        documentation_pages::archived_at.eq(now),
        documentation_pages::updated_at.eq(now),
    ))
    .execute(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Update the Yjs binary state for a collection's rich
/// description. Called from the collaboration handler when the
/// `collection-${id}` editor saves.
pub fn update_collection_description_yjs(
    conn: &mut DbConnection,
    collection_id: i32,
    yjs_document: Vec<u8>,
) -> QueryResult<usize> {
    let now = chrono::Utc::now().naive_utc();
    diesel::update(documentation_collections::table.find(collection_id))
        .set(DocumentationCollectionDescriptionYjsUpdate {
            description_yjs: Some(yjs_document),
            description_state_vector: None,
            updated_at: Some(now),
        })
        .execute(conn)
}

// ============================================================================
// Collection-Page Operations
// ============================================================================

// sync-pending-wire: needs sync aggregate wiring
pub fn add_page_to_collection(
    conn: &mut DbConnection,
    new_entry: NewDocumentationCollectionPage,
) -> QueryResult<DocumentationCollectionPage> {
    diesel::insert_into(documentation_collection_pages::table)
        .values(&new_entry)
        .on_conflict_do_nothing()
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Add a page to a collection AND null its parent_id so it lands
/// at the collection's root. The pre-redesign `add_page_to_collection`
/// only wrote the junction row; pages whose parent_id pointed at
/// some unrelated page in another collection then "floated" at the
/// root of the new collection's tree builder, which surfaced as
/// the bug where managed-add-to-collection placed pages in the
/// right collection but in the wrong visual position. With
/// `UNIQUE(page_id)`, this also implicitly *moves* the page out
/// of any previous collection (the unique-violation forces the
/// caller to pre-detach if they want to preserve the old link;
/// in practice we treat add as move).
pub fn add_page_to_collection_at_root(
    conn: &mut DbConnection,
    new_entry: NewDocumentationCollectionPage,
) -> QueryResult<DocumentationCollectionPage> {
    let page_id = new_entry.page_id;
    conn.transaction::<_, Error, _>(|tx| {
        // Detach any existing junction row for this page; UNIQUE
        // would otherwise reject the insert.
        diesel::delete(
            documentation_collection_pages::table
                .filter(documentation_collection_pages::page_id.eq(page_id)),
        )
        .execute(tx)?;
        // Null parent_id so the page anchors at the new
        // collection's root rather than dangling under a parent
        // that's no longer in this collection.
        diesel::update(documentation_pages::table.find(page_id))
            .set(documentation_pages::parent_id.eq::<Option<i32>>(None))
            .execute(tx)?;
        diesel::insert_into(documentation_collection_pages::table)
            .values(&new_entry)
            .get_result(tx)
    })
}

// sync-pending-wire: needs sync aggregate wiring
/// Ensure a child page belongs to the same collection as its
/// new parent. Called from `move_page_to_parent` so re-parenting
/// across collection boundaries automatically pulls the child
/// (and, by recursion at the handler level, its descendants)
/// into the new collection. With UNIQUE(page_id), the child is
/// detached from its previous collection in the same transaction.
pub fn cascade_collection_membership(
    conn: &mut DbConnection,
    parent_page_id: i32,
    child_page_id: i32,
    created_by: Option<Uuid>,
) -> QueryResult<()> {
    let parent_collection: Option<i32> = documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq(parent_page_id))
        .select(documentation_collection_pages::collection_id)
        .first(conn)
        .optional()?;
    let Some(parent_collection_id) = parent_collection else {
        return Ok(());
    };
    let child_collection: Option<i32> = documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq(child_page_id))
        .select(documentation_collection_pages::collection_id)
        .first(conn)
        .optional()?;
    if child_collection == Some(parent_collection_id) {
        return Ok(());
    }
    diesel::delete(
        documentation_collection_pages::table
            .filter(documentation_collection_pages::page_id.eq(child_page_id)),
    )
    .execute(conn)?;
    diesel::insert_into(documentation_collection_pages::table)
        .values(NewDocumentationCollectionPage {
            collection_id: parent_collection_id,
            page_id: child_page_id,
            created_by,
        })
        .execute(conn)?;
    Ok(())
}

// sync-pending-wire: needs sync aggregate wiring
pub fn remove_page_from_collection(
    conn: &mut DbConnection,
    collection_id: i32,
    page_id: i32,
) -> QueryResult<usize> {
    diesel::delete(
        documentation_collection_pages::table
            .filter(documentation_collection_pages::collection_id.eq(collection_id))
            .filter(documentation_collection_pages::page_id.eq(page_id)),
    )
    .execute(conn)
}

pub fn get_pages_in_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<Vec<DocumentationPage>> {
    documentation_collection_pages::table
        .filter(documentation_collection_pages::collection_id.eq(collection_id))
        .inner_join(
            documentation_pages::table
                .on(documentation_pages::id.eq(documentation_collection_pages::page_id)),
        )
        .select(documentation_pages::all_columns)
        .order((
            documentation_pages::display_order.asc(),
            documentation_pages::title.asc(),
        ))
        .load(conn)
}

pub fn get_page_count_in_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<i64> {
    documentation_collection_pages::table
        .filter(documentation_collection_pages::collection_id.eq(collection_id))
        .count()
        .get_result(conn)
}

pub fn get_collections_for_page(
    conn: &mut DbConnection,
    page_id: i32,
) -> QueryResult<Vec<DocumentationCollection>> {
    documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq(page_id))
        .inner_join(documentation_collections::table.on(
            documentation_collections::id.eq(documentation_collection_pages::collection_id),
        ))
        .select(documentation_collections::all_columns)
        .order(documentation_collections::name.asc())
        .load(conn)
}

/// Get pages that don't belong to any collection
pub fn get_uncollected_pages(conn: &mut DbConnection) -> QueryResult<Vec<DocumentationPage>> {
    use diesel::dsl::not;

    documentation_pages::table
        .filter(not(documentation_pages::id.eq_any(
            documentation_collection_pages::table.select(documentation_collection_pages::page_id),
        )))
        .order((
            documentation_pages::display_order.asc(),
            documentation_pages::title.asc(),
        ))
        .load(conn)
}

// ============================================================================
// Collection Visibility Operations
// ============================================================================

pub fn get_visible_groups_for_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<Vec<Group>> {
    documentation_collection_visibility::table
        .filter(documentation_collection_visibility::collection_id.eq(collection_id))
        .filter(documentation_collection_visibility::group_id.is_not_null())
        .inner_join(
            groups::table.on(groups::id
                .nullable()
                .eq(documentation_collection_visibility::group_id)),
        )
        .select(groups::all_columns)
        .load(conn)
}

pub fn get_visible_users_for_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<Vec<UserInfoWithAvatar>> {
    documentation_collection_visibility::table
        .filter(documentation_collection_visibility::collection_id.eq(collection_id))
        .filter(documentation_collection_visibility::user_uuid.is_not_null())
        .inner_join(
            users::table.on(users::uuid
                .nullable()
                .eq(documentation_collection_visibility::user_uuid)),
        )
        .select((
            users::uuid,
            users::name,
            users::avatar_url,
            users::avatar_thumb,
        ))
        .load::<(Uuid, String, Option<String>, Option<String>)>(conn)
        .map(|rows| {
            rows.into_iter()
                .map(
                    |(uuid, name, avatar_url, avatar_thumb)| UserInfoWithAvatar {
                        uuid,
                        name,
                        avatar_url,
                        avatar_thumb,
                    },
                )
                .collect()
        })
}

// sync-pending-wire: needs sync aggregate wiring
pub fn set_collection_visibility(
    conn: &mut DbConnection,
    collection_id: i32,
    group_ids: Vec<i32>,
    user_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<DocumentationCollectionVisibility>> {
    // Delete all existing visibility entries
    diesel::delete(
        documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq(collection_id)),
    )
    .execute(conn)?;

    // If no groups or users specified, the collection becomes public (visible to all)
    if group_ids.is_empty() && user_uuids.is_empty() {
        return Ok(Vec::new());
    }

    let mut new_entries: Vec<NewDocumentationCollectionVisibility> = Vec::new();

    // Add group entries
    for group_id in &group_ids {
        new_entries.push(NewDocumentationCollectionVisibility {
            collection_id,
            group_id: Some(*group_id),
            created_by,
            user_uuid: None,
        });
    }

    // Add user entries
    for user_uuid in &user_uuids {
        new_entries.push(NewDocumentationCollectionVisibility {
            collection_id,
            group_id: None,
            created_by,
            user_uuid: Some(*user_uuid),
        });
    }

    diesel::insert_into(documentation_collection_visibility::table)
        .values(&new_entries)
        .get_results(conn)
}

/// Get collections visible to a user based on their group memberships or direct user grants
pub fn get_collections_for_user(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    is_admin: bool,
) -> Result<Vec<CollectionWithDetails>, Error> {
    let all_collections = get_all_collections(conn)?;

    // Get user's group IDs (only needed for non-admins)
    let user_group_ids: Vec<i32> = if !is_admin {
        crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?
    } else {
        Vec::new()
    };

    let mut visible_collections = Vec::new();

    for collection in all_collections {
        // Get visibility entries for this collection
        let visible_groups = get_visible_groups_for_collection(conn, collection.id)?;
        let visible_users = get_visible_users_for_collection(conn, collection.id)?;
        let is_public = visible_groups.is_empty() && visible_users.is_empty();

        // Admins see all collections
        if !is_admin && !is_public {
            let collection_group_ids: Vec<i32> = visible_groups.iter().map(|g| g.id).collect();
            let has_group_access = user_group_ids
                .iter()
                .any(|id| collection_group_ids.contains(id));
            let has_user_access = visible_users.iter().any(|u| u.uuid == *user_uuid);
            if !has_group_access && !has_user_access {
                continue;
            }
        }

        let page_count = get_page_count_in_collection(conn, collection.id)?;

        visible_collections.push(CollectionWithDetails {
            collection,
            visible_to_groups: visible_groups,
            visible_to_users: visible_users,
            is_public,
            page_count,
        });
    }

    Ok(visible_collections)
}

/// Get all collections with visibility details (for admin views)
pub fn get_all_collections_with_details(
    conn: &mut DbConnection,
) -> Result<Vec<CollectionWithDetails>, Error> {
    let all_collections = get_all_collections(conn)?;
    let mut result = Vec::new();

    for collection in all_collections {
        let visible_groups = get_visible_groups_for_collection(conn, collection.id)?;
        let visible_users = get_visible_users_for_collection(conn, collection.id)?;
        let is_public = visible_groups.is_empty() && visible_users.is_empty();
        let page_count = get_page_count_in_collection(conn, collection.id)?;

        result.push(CollectionWithDetails {
            collection,
            visible_to_groups: visible_groups,
            visible_to_users: visible_users,
            is_public,
            page_count,
        });
    }

    Ok(result)
}
