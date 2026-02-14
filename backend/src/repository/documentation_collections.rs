use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::schema::documentation_collection_visibility;

// ============================================================================
// Collection CRUD Operations
// ============================================================================

pub fn create_collection(
    conn: &mut DbConnection,
    new_collection: NewDocumentationCollection,
) -> QueryResult<DocumentationCollection> {
    diesel::insert_into(documentation_collections::table)
        .values(&new_collection)
        .get_result(conn)
}

pub fn get_collection(conn: &mut DbConnection, collection_id: i32) -> QueryResult<DocumentationCollection> {
    documentation_collections::table
        .find(collection_id)
        .first(conn)
}

pub fn get_collection_by_slug(conn: &mut DbConnection, slug: &str) -> QueryResult<DocumentationCollection> {
    documentation_collections::table
        .filter(documentation_collections::slug.eq(slug))
        .first(conn)
}

pub fn get_all_collections(conn: &mut DbConnection) -> QueryResult<Vec<DocumentationCollection>> {
    documentation_collections::table
        .order((documentation_collections::display_order.asc(), documentation_collections::name.asc()))
        .load(conn)
}

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

pub fn update_collection(
    conn: &mut DbConnection,
    collection_id: i32,
    update: DocumentationCollectionUpdate,
) -> QueryResult<DocumentationCollection> {
    diesel::update(documentation_collections::table.find(collection_id))
        .set(&update)
        .get_result(conn)
}

pub fn delete_collection(conn: &mut DbConnection, collection_id: i32) -> QueryResult<usize> {
    diesel::delete(documentation_collections::table.find(collection_id))
        .execute(conn)
}

// ============================================================================
// Collection-Page Operations
// ============================================================================

pub fn add_page_to_collection(
    conn: &mut DbConnection,
    new_entry: NewDocumentationCollectionPage,
) -> QueryResult<DocumentationCollectionPage> {
    diesel::insert_into(documentation_collection_pages::table)
        .values(&new_entry)
        .on_conflict_do_nothing()
        .get_result(conn)
}

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
        .inner_join(documentation_pages::table.on(
            documentation_pages::id.eq(documentation_collection_pages::page_id),
        ))
        .select(documentation_pages::all_columns)
        .order((documentation_pages::display_order.asc(), documentation_pages::title.asc()))
        .load(conn)
}

pub fn get_page_count_in_collection(conn: &mut DbConnection, collection_id: i32) -> QueryResult<i64> {
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
            documentation_collection_pages::table
                .select(documentation_collection_pages::page_id),
        )))
        .order((documentation_pages::display_order.asc(), documentation_pages::title.asc()))
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
        .inner_join(groups::table.on(
            groups::id.nullable().eq(documentation_collection_visibility::group_id),
        ))
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
        .inner_join(users::table.on(
            users::uuid.nullable().eq(documentation_collection_visibility::user_uuid),
        ))
        .select((users::uuid, users::name, users::avatar_url, users::avatar_thumb))
        .load::<(Uuid, String, Option<String>, Option<String>)>(conn)
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
            let has_group_access = user_group_ids.iter().any(|id| collection_group_ids.contains(id));
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
pub fn get_all_collections_with_details(conn: &mut DbConnection) -> Result<Vec<CollectionWithDetails>, Error> {
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
