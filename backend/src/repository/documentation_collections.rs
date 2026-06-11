use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::documentation_collection_visibility;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
// NB: `groups` is fully-qualified at the emit sites as
// `crate::sync::groups::workspace()` because `use crate::schema::*`
// already brings the `groups` Diesel table into scope under that name.

/// Resolve a collection's immutable UUID to its integer id, or `None`
/// if no live collection has it. Used by the collab layer to map a
/// UUID-keyed doc_id to the integer id the persistence layer uses.
pub fn collection_id_by_uuid(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Option<i32>> {
    documentation_collections::table
        .filter(documentation_collections::uuid.eq(uuid))
        .select(documentation_collections::id)
        .first::<i32>(conn)
        .optional()
}

/// Sync-event payload for a documentation collection. Excludes the
/// Yjs binary columns (`description_yjs` / `description_state_vector`).
/// The rich description body flows through the collaborative-editor
/// WebSocket channel, not the sync_actions stream. The plain-text
/// projection (`description_text`) is included so consumers have the
/// searchable overview without the CRDT blob.
fn collection_sync_payload(c: &DocumentationCollection) -> serde_json::Value {
    json!({
        "id": c.id,
        "uuid": c.uuid,
        "name": c.name,
        "slug": c.slug,
        "description": c.description,
        "icon": c.icon,
        "color": c.color,
        "is_system": c.is_system,
        "created_by": c.created_by,
        "display_order": c.display_order,
        "description_text": c.description_text,
        "hide_titles_from_non_members": c.hide_titles_from_non_members,
        "created_at": c.created_at,
        "updated_at": c.updated_at,
    })
}

// ============================================================================
// Collection CRUD Operations
// ============================================================================

pub fn create_collection(
    conn: &mut DbConnection,
    new_collection: NewDocumentationCollection,
) -> QueryResult<DocumentationCollection> {
    conn.transaction(|conn| {
        let collection: DocumentationCollection =
            diesel::insert_into(documentation_collections::table)
                .values(&new_collection)
                .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: collection.id.to_string(),
                op: SyncOp::Insert,
                event_type: "documentation_collection.created",
                data: collection_sync_payload(&collection),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(collection)
    })
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

pub fn reorder_collections(
    conn: &mut DbConnection,
    orders: &[CollectionOrder],
) -> Result<Vec<DocumentationCollection>, Error> {
    conn.transaction(|conn| {
        for order in orders {
            let collection: DocumentationCollection =
                diesel::update(documentation_collections::table.find(order.collection_id))
                    .set(documentation_collections::display_order.eq(order.display_order))
                    .get_result(conn)?;
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationCollection,
                    aggregate_id: collection.id.to_string(),
                    op: SyncOp::Update,
                    event_type: "documentation_collection.updated",
                    data: collection_sync_payload(&collection),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        get_all_collections(conn)
    })
}

pub fn update_collection(
    conn: &mut DbConnection,
    collection_id: i32,
    update: DocumentationCollectionUpdate,
) -> QueryResult<DocumentationCollection> {
    conn.transaction(|conn| {
        let collection: DocumentationCollection =
            diesel::update(documentation_collections::table.find(collection_id))
                .set(&update)
                .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: collection.id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_collection.updated",
                data: collection_sync_payload(&collection),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(collection)
    })
}

pub fn delete_collection(conn: &mut DbConnection, collection_id: i32) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let count =
            diesel::delete(documentation_collections::table.find(collection_id)).execute(conn)?;
        if count > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationCollection,
                    aggregate_id: collection_id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "documentation_collection.deleted",
                    data: json!({ "id": collection_id }),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(count)
    })
}

// sync-audit-only: bulk cascade run as part of collection teardown; the documentation_collection.deleted event captures the operation and per-page deleted events would require a fan-out fetch of every member page id
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

// sync-audit-only: collaborative-editor CRDT auto-save for the collection's rich description; the body flows through the Yjs WebSocket channel, not the sync_actions stream
/// Update the Yjs binary state for a collection's rich
/// description. Called from the collaboration handler when the
/// `collection-${id}` editor saves.
pub fn update_collection_description_yjs(
    conn: &mut DbConnection,
    collection_id: i32,
    yjs_document: Vec<u8>,
    // Ownership-claim fencing token (Phase 2 affinity). `Some(f)` gates
    // the write so a stale owner cannot clobber a newer owner's state;
    // `None` (single-instance / Redis-down) writes unconditionally. A
    // stale write affects 0 rows (the returned count reflects it). See
    // docs/realtime-collab-affinity-design.md.
    fence: Option<i64>,
) -> QueryResult<usize> {
    let now = chrono::Utc::now().naive_utc();
    match fence {
        Some(f) => diesel::update(
            documentation_collections::table
                .filter(documentation_collections::id.eq(collection_id))
                .filter(
                    documentation_collections::fence_token
                        .is_null()
                        .or(documentation_collections::fence_token.le(f)),
                ),
        )
        .set((
            documentation_collections::description_yjs.eq(Some(yjs_document)),
            documentation_collections::description_state_vector.eq(None::<Vec<u8>>),
            documentation_collections::fence_token.eq(f),
            documentation_collections::updated_at.eq(now),
        ))
        .execute(conn),
        None => diesel::update(documentation_collections::table.find(collection_id))
            .set(DocumentationCollectionDescriptionYjsUpdate {
                description_yjs: Some(yjs_document),
                description_state_vector: None,
                updated_at: Some(now),
            })
            .execute(conn),
    }
}

// ============================================================================
// Collection-Page Operations
// ============================================================================

pub fn add_page_to_collection(
    conn: &mut DbConnection,
    new_entry: NewDocumentationCollectionPage,
) -> QueryResult<DocumentationCollectionPage> {
    conn.transaction(|conn| {
        let entry: DocumentationCollectionPage =
            diesel::insert_into(documentation_collection_pages::table)
                .values(&new_entry)
                .on_conflict_do_nothing()
                .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: entry.collection_id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_collection.page_added",
                data: json!({ "collection_id": entry.collection_id, "page_id": entry.page_id }),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        // Re-emit the page so the sync pool's page row picks up its
        // new denormalised collection_id.
        crate::repository::documentation::emit_page_membership_changed(conn, entry.page_id)?;
        Ok(entry)
    })
}

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
        let entry: DocumentationCollectionPage =
            diesel::insert_into(documentation_collection_pages::table)
                .values(&new_entry)
                .get_result(tx)?;
        emit::record(
            tx,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: entry.collection_id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_collection.page_added",
                data: json!({ "collection_id": entry.collection_id, "page_id": entry.page_id }),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        // Re-emit the page (its parent_id was nulled above and its
        // collection_id changed) so the sync pool row reflects both.
        crate::repository::documentation::emit_page_membership_changed(tx, entry.page_id)?;
        Ok(entry)
    })
}

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
    // The page leaves its previous collection (if it had one) and
    // joins the parent's, so both sides of the move emit.
    if let Some(old_collection_id) = child_collection {
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: old_collection_id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_collection.page_removed",
                data: json!({ "collection_id": old_collection_id, "page_id": child_page_id }),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
    }
    diesel::insert_into(documentation_collection_pages::table)
        .values(NewDocumentationCollectionPage {
            collection_id: parent_collection_id,
            page_id: child_page_id,
            created_by,
        })
        .execute(conn)?;
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::DocumentationCollection,
            aggregate_id: parent_collection_id.to_string(),
            op: SyncOp::Update,
            event_type: "documentation_collection.page_added",
            data: json!({ "collection_id": parent_collection_id, "page_id": child_page_id }),
            groups: crate::sync::groups::workspace(),
            causation_id: None,
        },
    )?;
    crate::repository::documentation::emit_page_membership_changed(conn, child_page_id)?;
    Ok(())
}

pub fn remove_page_from_collection(
    conn: &mut DbConnection,
    collection_id: i32,
    page_id: i32,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let count = diesel::delete(
            documentation_collection_pages::table
                .filter(documentation_collection_pages::collection_id.eq(collection_id))
                .filter(documentation_collection_pages::page_id.eq(page_id)),
        )
        .execute(conn)?;
        if count > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationCollection,
                    aggregate_id: collection_id.to_string(),
                    op: SyncOp::Update,
                    event_type: "documentation_collection.page_removed",
                    data: json!({ "collection_id": collection_id, "page_id": page_id }),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
            // Page is now uncollected; re-emit so the pool row's
            // collection_id drops to null.
            crate::repository::documentation::emit_page_membership_changed(conn, page_id)?;
        }
        Ok(count)
    })
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

/// Whether a user can see a collection. Mirrors the collection branch of
/// `documentation::can_user_access_page`: admins see all; a collection
/// with no visibility overrides is public; otherwise access needs a direct
/// user grant or membership in a granted group.
pub fn can_user_access_collection(
    conn: &mut DbConnection,
    collection_id: i32,
    user_uuid: &Uuid,
    is_admin: bool,
) -> Result<bool, Error> {
    if is_admin {
        return Ok(true);
    }

    let override_count: i64 = documentation_collection_visibility::table
        .filter(documentation_collection_visibility::collection_id.eq(collection_id))
        .count()
        .get_result(conn)?;
    if override_count == 0 {
        // No override → public.
        return Ok(true);
    }

    let has_user_grant: i64 = documentation_collection_visibility::table
        .filter(documentation_collection_visibility::collection_id.eq(collection_id))
        .filter(documentation_collection_visibility::user_uuid.eq(user_uuid))
        .count()
        .get_result(conn)?;
    if has_user_grant > 0 {
        return Ok(true);
    }

    let user_group_ids = crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?;
    let coll_group_ids: Vec<i32> = documentation_collection_visibility::table
        .filter(documentation_collection_visibility::collection_id.eq(collection_id))
        .filter(documentation_collection_visibility::group_id.is_not_null())
        .select(documentation_collection_visibility::group_id)
        .load::<Option<i32>>(conn)?
        .into_iter()
        .flatten()
        .collect();
    Ok(user_group_ids
        .iter()
        .any(|uid| coll_group_ids.contains(uid)))
}

pub fn set_collection_visibility(
    conn: &mut DbConnection,
    collection_id: i32,
    group_ids: Vec<i32>,
    user_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<DocumentationCollectionVisibility>> {
    conn.transaction(|conn| {
        // Delete all existing visibility entries
        diesel::delete(
            documentation_collection_visibility::table
                .filter(documentation_collection_visibility::collection_id.eq(collection_id)),
        )
        .execute(conn)?;

        let entries: Vec<DocumentationCollectionVisibility> =
            if group_ids.is_empty() && user_uuids.is_empty() {
                // Clearing all entries makes the collection public; this is
                // still a visibility change and emits below.
                Vec::new()
            } else {
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
                    .get_results(conn)?
            };

        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: collection_id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_collection.visibility_changed",
                data: json!({
                    "collection_id": collection_id,
                    "group_ids": group_ids,
                    "user_uuids": user_uuids,
                }),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

        Ok(entries)
    })
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
