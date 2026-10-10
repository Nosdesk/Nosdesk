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

/// The workspace that owns the resource with this UUID, or `None` when no row
/// has it. Backs the collab image serve route, which derives the workspace
/// from the resource because a direct browser file load carries no workspace
/// selection header. Intended to be called elevated (BYPASSRLS): it reveals
/// only a workspace id, and the caller gates on membership plus the document
/// ACL under that workspace's pin.
pub fn collection_workspace_id_by_uuid(
    conn: &mut DbConnection,
    uuid: Uuid,
) -> QueryResult<Option<i32>> {
    documentation_collections::table
        .filter(documentation_collections::uuid.eq(uuid))
        .select(documentation_collections::workspace_id)
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
        "require_verification": c.require_verification,
        "restricted": c.restricted,
        "created_at": c.created_at,
        "updated_at": c.updated_at,
    })
}

/// Emit the collection's whole row under `event_type` to the sync `groups`.
/// Who may see a collection is decided as each reader receives the row.
pub fn emit_collection_row_to(
    conn: &mut DbConnection,
    collection_id: i32,
    event_type: &'static str,
    groups: Vec<String>,
) -> QueryResult<()> {
    let Some(collection) = documentation_collections::table
        .find(collection_id)
        .first::<DocumentationCollection>(conn)
        .optional()?
    else {
        return Ok(());
    };
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::DocumentationCollection,
            aggregate_id: collection_id.to_string(),
            op: SyncOp::Update,
            event_type,
            data: collection_sync_payload(&collection),
            groups,
            causation_id: None,
        },
    )?;
    Ok(())
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

/// Create a collection, restricted to `group_ids` and `user_uuids` (admins
/// only when both are empty) or open to everyone, in one transaction, so it
/// is never open to anyone else in between.
pub fn create_collection_open_to(
    conn: &mut DbConnection,
    new_collection: NewDocumentationCollection,
    restricted: bool,
    group_ids: Vec<i32>,
    user_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<DocumentationCollection> {
    conn.transaction(|conn| {
        let collection = create_collection(conn, new_collection)?;
        if !restricted {
            return Ok(collection);
        }
        set_collection_rules(conn, collection.id, true, group_ids, user_uuids, created_by)?;
        get_collection(conn, collection.id)
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

/// Move every page in this collection to the trash ahead of deleting the
/// collection. The page rows survive (authorship and revision history) and
/// can be restored from the trash. A page that followed a restricted
/// collection takes the collection's rules as its own first, so it stays
/// closed to the same people once the collection is gone: a page with no
/// collection and no rules of its own is open to everyone. Each page's new
/// row is emitted. With `UNIQUE(page_id)` on the junction, "every page in
/// this collection" is unambiguous.
pub fn soft_delete_pages_in_collection(
    conn: &mut DbConnection,
    collection_id: i32,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let collection: DocumentationCollection = documentation_collections::table
            .find(collection_id)
            .first(conn)?;
        let pages: Vec<(i32, bool)> = documentation_pages::table
            .filter(
                documentation_pages::id.eq_any(
                    documentation_collection_pages::table
                        .filter(documentation_collection_pages::collection_id.eq(collection_id))
                        .select(documentation_collection_pages::page_id),
                ),
            )
            .select((documentation_pages::id, documentation_pages::restricted))
            .load(conn)?;

        if collection.restricted {
            let grants: Vec<(Option<i32>, Option<Uuid>)> =
                documentation_collection_visibility::table
                    .filter(documentation_collection_visibility::collection_id.eq(collection_id))
                    .select((
                        documentation_collection_visibility::group_id,
                        documentation_collection_visibility::user_uuid,
                    ))
                    .load(conn)?;
            let groups: Vec<i32> = grants.iter().filter_map(|(g, _)| *g).collect();
            let users: Vec<Uuid> = grants.iter().filter_map(|(_, u)| *u).collect();
            for (page_id, restricted) in &pages {
                if !restricted {
                    crate::repository::documentation::set_page_rules(
                        conn,
                        *page_id,
                        true,
                        groups.clone(),
                        users.clone(),
                        None,
                    )?;
                }
            }
        }

        let now = chrono::Utc::now().naive_utc();
        let ids: Vec<i32> = pages.iter().map(|(id, _)| *id).collect();
        let count =
            diesel::update(documentation_pages::table.filter(documentation_pages::id.eq_any(&ids)))
                .set((
                    documentation_pages::status.eq(DocumentationStatus::Deleted),
                    documentation_pages::archived_at.eq(now),
                    documentation_pages::deleted_at.eq(now),
                    documentation_pages::updated_at.eq(now),
                ))
                .execute(conn)?;
        for page_id in ids {
            let page: DocumentationPage = documentation_pages::table.find(page_id).first(conn)?;
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationPage,
                    aggregate_id: page_id.to_string(),
                    op: SyncOp::Update,
                    event_type: "documentation_page.metadata_changed",
                    // The collection is about to go, and its junction rows
                    // with it.
                    data: crate::repository::documentation::page_sync_payload(&page, None),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(count)
    })
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
    // stale write affects 0 rows (the returned count reflects it).
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

/// Set a collection's rules from grants alone: some grants restrict it to
/// them, none opens it to everyone. A caller that can say "restricted to
/// nobody" uses [`set_collection_rules`].
pub fn set_collection_visibility(
    conn: &mut DbConnection,
    collection_id: i32,
    group_ids: Vec<i32>,
    user_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<DocumentationCollectionVisibility>> {
    let restricted = !(group_ids.is_empty() && user_uuids.is_empty());
    set_collection_rules(
        conn,
        collection_id,
        restricted,
        group_ids,
        user_uuids,
        created_by,
    )
}

/// Set a collection's rules (delete-all + re-insert). Restricted is open
/// only to the grants, or to admins only when there are none; unrestricted
/// drops the grants and is open to everyone in the workspace.
pub fn set_collection_rules(
    conn: &mut DbConnection,
    collection_id: i32,
    restricted: bool,
    group_ids: Vec<i32>,
    user_uuids: Vec<Uuid>,
    created_by: Option<Uuid>,
) -> QueryResult<Vec<DocumentationCollectionVisibility>> {
    let (group_ids, user_uuids) = if restricted {
        (group_ids, user_uuids)
    } else {
        (Vec::new(), Vec::new())
    };
    conn.transaction(|conn| {
        diesel::update(documentation_collections::table.find(collection_id))
            .set(documentation_collections::restricted.eq(restricted))
            .execute(conn)?;
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

        // The whole collection and every page in it: the sync feed sends each
        // to whoever can open it now and a delete to whoever no longer can.
        let collection: DocumentationCollection = documentation_collections::table
            .find(collection_id)
            .first(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationCollection,
                aggregate_id: collection_id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_collection.visibility_changed",
                data: collection_sync_payload(&collection),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        let page_ids: Vec<i32> = documentation_collection_pages::table
            .filter(documentation_collection_pages::collection_id.eq(collection_id))
            .select(documentation_collection_pages::page_id)
            .load(conn)?;
        for page_id in page_ids {
            crate::repository::documentation::emit_page_row(
                conn,
                page_id,
                "documentation_page.visibility_changed",
            )?;
        }

        Ok(entries)
    })
}

/// Get collections visible to a user based on their group memberships or direct user grants
/// Batched `get_visible_groups_for_collection`: one query returning a
/// `collection_id -> visible groups` map for many collections.
pub fn get_visible_groups_for_collections(
    conn: &mut DbConnection,
    collection_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, Vec<Group>>> {
    let rows: Vec<(i32, Group)> = documentation_collection_visibility::table
        .filter(documentation_collection_visibility::collection_id.eq_any(collection_ids))
        .filter(documentation_collection_visibility::group_id.is_not_null())
        .inner_join(
            groups::table.on(groups::id
                .nullable()
                .eq(documentation_collection_visibility::group_id)),
        )
        .select((
            documentation_collection_visibility::collection_id,
            groups::all_columns,
        ))
        .load(conn)?;
    let mut map: std::collections::HashMap<i32, Vec<Group>> = std::collections::HashMap::new();
    for (collection_id, group) in rows {
        map.entry(collection_id).or_default().push(group);
    }
    Ok(map)
}

/// Batched `get_visible_users_for_collection`: one query returning a
/// `collection_id -> visible users` map for many collections.
pub fn get_visible_users_for_collections(
    conn: &mut DbConnection,
    collection_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, Vec<UserInfoWithAvatar>>> {
    let rows: Vec<(i32, Uuid, String, Option<String>, Option<String>)> =
        documentation_collection_visibility::table
            .filter(documentation_collection_visibility::collection_id.eq_any(collection_ids))
            .filter(documentation_collection_visibility::user_uuid.is_not_null())
            .inner_join(
                users::table.on(users::uuid
                    .nullable()
                    .eq(documentation_collection_visibility::user_uuid)),
            )
            .select((
                documentation_collection_visibility::collection_id,
                users::uuid,
                users::name,
                users::avatar_url,
                users::avatar_thumb,
            ))
            .load(conn)?;
    let mut map: std::collections::HashMap<i32, Vec<UserInfoWithAvatar>> =
        std::collections::HashMap::new();
    for (collection_id, uuid, name, avatar_url, avatar_thumb) in rows {
        map.entry(collection_id)
            .or_default()
            .push(UserInfoWithAvatar {
                uuid,
                name,
                avatar_url,
                avatar_thumb,
            });
    }
    Ok(map)
}

/// Batched `get_page_count_in_collection`: one grouped query returning a
/// `collection_id -> page count` map. Collections with no pages are absent
/// (callers default to 0).
pub fn get_page_counts_for_collections(
    conn: &mut DbConnection,
    collection_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, i64>> {
    let rows: Vec<(i32, i64)> = documentation_collection_pages::table
        .filter(documentation_collection_pages::collection_id.eq_any(collection_ids))
        .group_by(documentation_collection_pages::collection_id)
        .select((
            documentation_collection_pages::collection_id,
            diesel::dsl::count_star(),
        ))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// Assemble every collection with its visibility + page count, batching the
/// three per-collection lookups into three queries total. Shared by the
/// user-scoped and admin views, which differ only in how they filter the
/// result.
fn collections_with_details(conn: &mut DbConnection) -> Result<Vec<CollectionWithDetails>, Error> {
    let all_collections = get_all_collections(conn)?;
    let ids: Vec<i32> = all_collections.iter().map(|c| c.id).collect();

    let mut groups_map = get_visible_groups_for_collections(conn, &ids)?;
    let mut users_map = get_visible_users_for_collections(conn, &ids)?;
    let count_map = get_page_counts_for_collections(conn, &ids)?;

    Ok(all_collections
        .into_iter()
        .map(|collection| {
            let visible_groups = groups_map.remove(&collection.id).unwrap_or_default();
            let visible_users = users_map.remove(&collection.id).unwrap_or_default();
            let is_public = !collection.restricted;
            let page_count = count_map.get(&collection.id).copied().unwrap_or(0);
            CollectionWithDetails {
                collection,
                visible_to_groups: visible_groups,
                visible_to_users: visible_users,
                is_public,
                page_count,
            }
        })
        .collect())
}

/// The collections `audience` may see, each with the number of its pages
/// they may open.
pub fn get_collections_for_user(
    conn: &mut DbConnection,
    audience: &crate::repository::documentation::PageAudience,
) -> Result<Vec<CollectionWithDetails>, Error> {
    let all = collections_with_details(conn)?;
    let mut visible = audience.filter_collections(conn, all, |c| c.collection.id)?;

    let ids: Vec<i32> = visible.iter().map(|c| c.collection.id).collect();
    let members: Vec<(i32, i32)> = documentation_collection_pages::table
        .filter(documentation_collection_pages::collection_id.eq_any(&ids))
        .select((
            documentation_collection_pages::collection_id,
            documentation_collection_pages::page_id,
        ))
        .load(conn)?;
    let page_ids: Vec<i32> = members.iter().map(|(_, p)| *p).collect();
    let hidden = audience.hidden_pages(conn, &page_ids)?;
    let mut counts: std::collections::HashMap<i32, i64> = std::collections::HashMap::new();
    for (collection_id, page_id) in members {
        if !hidden.contains(&page_id) {
            *counts.entry(collection_id).or_default() += 1;
        }
    }
    for c in &mut visible {
        c.page_count = counts.get(&c.collection.id).copied().unwrap_or(0);
    }
    Ok(visible)
}

/// Get all collections with visibility details (for admin views)
pub fn get_all_collections_with_details(
    conn: &mut DbConnection,
) -> Result<Vec<CollectionWithDetails>, Error> {
    collections_with_details(conn)
}
