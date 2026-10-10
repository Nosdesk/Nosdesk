use diesel::prelude::*;
use diesel::result::Error;
use diesel::sql_types::{Integer, Nullable};
use serde_json::json;

use crate::db::DbConnection;
use crate::models::{
    DocumentationPage, DocumentationPageUpdate, DocumentationPageWithChildren, DocumentationStatus,
    NewDocumentationPage, PageOrder, SyncAggregate, SyncOp,
};
use crate::schema::documentation_pages;
use crate::sync::emit::{self, SyncEmit};
// NB: `groups` is fully-qualified at the emit sites as
// `crate::sync::groups::workspace()` because `use crate::schema::{..,
// groups}` lower in this module already binds the `groups` name to
// the Diesel table for the visibility joins.

/// Sync-event payload for a documentation page. Excludes the Yjs
/// binary columns (`yjs_document` / `yjs_state_vector`) and the
/// `has_unsaved_changes` flag, all of which are collaborative-editor
/// state churned on every keystroke-batch, not metadata a sync
/// consumer wants. The document body flows through the Yjs
/// WebSocket channel, not the sync_actions stream.
/// The single collection a page belongs to, or None if uncollected.
/// `UNIQUE(page_id)` on the junction guarantees at most one, so this
/// is a clean denormalisation onto the page sync row.
pub fn collection_id_for_page(conn: &mut DbConnection, page_id: i32) -> Result<Option<i32>, Error> {
    documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq(page_id))
        .select(documentation_collection_pages::collection_id)
        .first::<i32>(conn)
        .optional()
}

/// True when the page's collection has `require_verification` set.
/// A page belongs to at most one collection (UNIQUE(page_id) on the
/// junction), so this gates the never-verified prompt on that one
/// collection's compliance opt-in. False (neutral) by default.
pub fn page_requires_verification(conn: &mut DbConnection, page_id: i32) -> QueryResult<bool> {
    use crate::schema::documentation_collections as dc;

    let collection_ids = documentation_collection_pages::table
        .filter(documentation_collection_pages::page_id.eq(page_id))
        .select(documentation_collection_pages::collection_id);

    diesel::select(diesel::dsl::exists(
        dc::table
            .filter(dc::id.eq_any(collection_ids))
            .filter(dc::require_verification.eq(true)),
    ))
    .get_result(conn)
}

pub(crate) fn page_sync_payload(
    p: &DocumentationPage,
    collection_id: Option<i32>,
) -> serde_json::Value {
    json!({
        "id": p.id,
        "uuid": p.uuid,
        "collection_id": collection_id,
        "title": p.title,
        "slug": p.slug,
        "icon": p.icon,
        "cover_image": p.cover_image,
        "status": p.status,
        "parent_id": p.parent_id,
        "display_order": p.display_order,
        "is_public": p.is_public,
        "is_template": p.is_template,
        "restricted": p.restricted,
        "archived_at": p.archived_at,
        "deleted_at": p.deleted_at,
        "created_by": p.created_by,
        "last_edited_by": p.last_edited_by,
        "verified_by": p.verified_by,
        "verified_at": p.verified_at,
        "verify_interval_days": p.verify_interval_days,
        "created_at": p.created_at,
        "updated_at": p.updated_at,
    })
}

/// Observer fired after a documentation page's Yjs blob is
/// successfully saved by the collaborative editor. Mirrors
/// `UserCreatedObserver` and `ArticleContentSavedObserver`: defined
/// at the repo layer, implemented elsewhere by the search service
/// so the index reflects body edits without each save site needing
/// to wire up indexing manually.
pub trait DocumentationSavedObserver: Send + Sync {
    fn documentation_saved(&self, page: &DocumentationPage);
}

/// Observer fired after a documentation page is hard-deleted.
/// Implementor removes the page from the search index.
pub trait DocumentationDeletedObserver: Send + Sync {
    fn documentation_deleted(&self, page_id: i32);
}

// Get all documentation pages (excludes archived and deleted)
pub fn get_documentation_pages(conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(
            documentation_pages::status
                .eq_any([DocumentationStatus::Draft, DocumentationStatus::Published]),
        )
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get a specific documentation page by ID
pub fn get_documentation_page(
    id: i32,
    conn: &mut DbConnection,
) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .find(id)
        .first::<DocumentationPage>(conn)
}

// Get a documentation page by its UUID
pub fn get_documentation_page_by_uuid(
    uuid: &uuid::Uuid,
    conn: &mut DbConnection,
) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .filter(documentation_pages::uuid.eq(uuid))
        .first::<DocumentationPage>(conn)
}

/// Resolve a page's immutable UUID to its integer id, or `None` if no
/// live page has it. Used by the collab layer to map a UUID-keyed
/// doc_id to the integer id the persistence layer uses.
pub fn page_id_by_uuid(conn: &mut DbConnection, uuid: uuid::Uuid) -> QueryResult<Option<i32>> {
    documentation_pages::table
        .filter(documentation_pages::uuid.eq(uuid))
        .select(documentation_pages::id)
        .first::<i32>(conn)
        .optional()
}

/// The workspace that owns the resource with this UUID, or `None` when no row
/// has it. Backs the collab image serve route, which derives the workspace
/// from the resource because a direct browser file load carries no workspace
/// selection header. Intended to be called elevated (BYPASSRLS): it reveals
/// only a workspace id, and the caller gates on membership plus the document
/// ACL under that workspace's pin.
pub fn page_workspace_id_by_uuid(
    conn: &mut DbConnection,
    uuid: uuid::Uuid,
) -> QueryResult<Option<i32>> {
    documentation_pages::table
        .filter(documentation_pages::uuid.eq(uuid))
        .select(documentation_pages::workspace_id)
        .first::<i32>(conn)
        .optional()
}

/// Inverse of [`page_id_by_uuid`]: the immutable UUID for an integer
/// page id, or `None` if no live page has it. Used when building a
/// UUID-keyed collab doc_id from an integer id (revision restore).
pub fn page_uuid_by_id(conn: &mut DbConnection, id: i32) -> QueryResult<Option<uuid::Uuid>> {
    documentation_pages::table
        .filter(documentation_pages::id.eq(id))
        .select(documentation_pages::uuid)
        .first::<uuid::Uuid>(conn)
        .optional()
}

// Get a documentation page by its slug
pub fn get_documentation_page_by_slug(
    slug: &str,
    conn: &mut DbConnection,
) -> Result<DocumentationPage, Error> {
    documentation_pages::table
        .filter(documentation_pages::slug.eq(slug))
        .first::<DocumentationPage>(conn)
}

// Create a new documentation page
pub fn create_documentation_page(
    page: NewDocumentationPage,
    conn: &mut DbConnection,
) -> Result<DocumentationPage, Error> {
    conn.transaction(|conn| {
        if let Some(parent_id) = page.parent_id {
            check_parent(conn, None, parent_id)?;
        }
        let page: DocumentationPage = diesel::insert_into(documentation_pages::table)
            .values(page)
            .get_result(conn)?;
        let collection_id = collection_id_for_page(conn, page.id)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationPage,
                aggregate_id: page.id.to_string(),
                op: SyncOp::Insert,
                event_type: "documentation_page.created",
                data: page_sync_payload(&page, collection_id),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(page)
    })
}

// Update an existing documentation page
pub fn update_documentation_page(
    conn: &mut DbConnection,
    page_id: i32,
    page_update: &DocumentationPageUpdate,
) -> Result<DocumentationPage, Error> {
    // A verify action (setting verified_at to a timestamp) surfaces
    // as the distinct documentation_page.verified event the tier
    // classification names; every other field change is
    // metadata_changed.
    let is_verify = matches!(page_update.verified_at, Some(Some(_)));
    conn.transaction(|conn| {
        if let Some(Some(parent_id)) = page_update.parent_id {
            check_parent(conn, Some(page_id), parent_id)?;
        }
        let page: DocumentationPage = diesel::update(documentation_pages::table.find(page_id))
            .set(page_update)
            .get_result(conn)?;
        // Gaps drafting on this page follow its status (publish resolves,
        // delete or archive reopens). Every status change comes through here.
        if let Some(status) = &page_update.status {
            crate::repository::knowledge_gaps::on_page_status_changed(conn, page.id, status, None)?;
        }
        let collection_id = collection_id_for_page(conn, page.id)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationPage,
                aggregate_id: page.id.to_string(),
                op: SyncOp::Update,
                event_type: if is_verify {
                    "documentation_page.verified"
                } else {
                    "documentation_page.metadata_changed"
                },
                data: page_sync_payload(&page, collection_id),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(page)
    })
}

// Delete a documentation page
pub fn delete_documentation_page(
    id: i32,
    conn: &mut DbConnection,
    observer: Option<&dyn DocumentationDeletedObserver>,
) -> Result<usize, Error> {
    let count = conn.transaction(|conn| {
        let count = diesel::delete(documentation_pages::table.find(id)).execute(conn)?;
        if count > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationPage,
                    aggregate_id: id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "documentation_page.deleted",
                    data: json!({ "id": id }),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok::<usize, Error>(count)
    })?;
    if count > 0 {
        if let Some(observer) = observer {
            observer.documentation_deleted(id);
        }
    }
    Ok(count)
}

// Get top-level documentation pages (excludes archived and deleted)
pub fn get_top_level_pages(conn: &mut DbConnection) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.is_null())
        .filter(
            documentation_pages::status
                .eq_any([DocumentationStatus::Draft, DocumentationStatus::Published]),
        )
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get documentation pages by parent ID (excludes archived and deleted)
pub fn get_pages_by_parent_id(
    parent_id: i32,
    conn: &mut DbConnection,
) -> Result<Vec<DocumentationPage>, Error> {
    documentation_pages::table
        .filter(documentation_pages::parent_id.eq(parent_id))
        .filter(
            documentation_pages::status
                .eq_any([DocumentationStatus::Draft, DocumentationStatus::Published]),
        )
        .order_by(documentation_pages::title.asc())
        .load::<DocumentationPage>(conn)
}

// Get documentation pages linked to a ticket via the page<->ticket
// join. Both 'resolves' and 'references' link types are returned;
// the caller can filter further if needed.
pub fn get_documentation_pages_by_ticket_id(
    conn: &mut DbConnection,
    ticket_id_arg: i32,
) -> Result<Vec<DocumentationPage>, Error> {
    use crate::schema::documentation_page_tickets;

    documentation_pages::table
        .inner_join(
            documentation_page_tickets::table
                .on(documentation_page_tickets::page_id.eq(documentation_pages::id)),
        )
        .filter(documentation_page_tickets::ticket_id.eq(ticket_id_arg))
        .filter(documentation_pages::deleted_at.is_null())
        .select(documentation_pages::all_columns)
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
            if let Some(parent_id) = parent_id {
                check_parent(conn, Some(order.page_id), parent_id)?;
            }
            // Update the page's display_order and ensure it has the correct parent_id
            let updated_page = diesel::update(documentation_pages::table.find(order.page_id))
                .set((
                    documentation_pages::display_order.eq(order.display_order),
                    documentation_pages::parent_id.eq(parent_id),
                ))
                .get_result::<DocumentationPage>(conn)?;

            let collection_id = collection_id_for_page(conn, updated_page.id)?;
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationPage,
                    aggregate_id: updated_page.id.to_string(),
                    op: SyncOp::Update,
                    event_type: "documentation_page.metadata_changed",
                    data: page_sync_payload(&updated_page, collection_id),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;

            updated_pages.push(updated_page);
        }

        Ok(updated_pages)
    })
}

/// Refuse `parent_id` as the parent of `page_id` (`None` for a page being
/// created) unless it is a page the connection can see, so one in the same
/// workspace, and neither the page itself nor one of its descendants, which
/// would make a cycle. Refusal is `RollbackTransaction`, which the handlers
/// answer with 400.
fn check_parent(
    conn: &mut DbConnection,
    page_id: Option<i32>,
    parent_id: i32,
) -> Result<(), Error> {
    let parent_exists = documentation_pages::table
        .find(parent_id)
        .select(documentation_pages::id)
        .first::<i32>(conn)
        .optional()?
        .is_some();
    let makes_cycle = match page_id {
        Some(page_id) => {
            parent_id == page_id || get_all_descendant_ids(conn, page_id)?.contains(&parent_id)
        }
        None => false,
    };
    if !parent_exists || makes_cycle {
        return Err(Error::RollbackTransaction);
    }
    Ok(())
}

/// Every page below `page_id`. Each page is visited once, so a parent cycle
/// already in the data can't loop it.
fn get_all_descendant_ids(conn: &mut DbConnection, page_id: i32) -> Result<Vec<i32>, Error> {
    let mut seen = std::collections::HashSet::from([page_id]);
    let mut all_descendants = Vec::new();
    let mut pages_to_check = vec![page_id];

    while !pages_to_check.is_empty() {
        let children: Vec<i32> = documentation_pages::table
            .filter(documentation_pages::parent_id.eq_any(&pages_to_check))
            .select(documentation_pages::id)
            .load(conn)?;
        pages_to_check = children.into_iter().filter(|id| seen.insert(*id)).collect();
        all_descendants.extend(&pages_to_check);
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
        if let Some(parent_id) = new_parent_id {
            check_parent(conn, Some(page_id), parent_id)?;
        }

        // Update the page's parent_id and display_order
        let updated_page = diesel::update(documentation_pages::table.find(page_id))
            .set((
                documentation_pages::parent_id.eq(new_parent_id),
                documentation_pages::display_order.eq(display_order),
            ))
            .get_result::<DocumentationPage>(conn)?;

        let collection_id = collection_id_for_page(conn, updated_page.id)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationPage,
                aggregate_id: updated_page.id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_page.metadata_changed",
                data: page_sync_payload(&updated_page, collection_id),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

        // Re-parenting under a page in a different collection
        // pulls this page (and only this page; descendants are
        // moved by the recursive walker if needed) into the
        // parent's collection. Same-collection moves and moves to
        // root (`new_parent_id == None`) are no-ops.
        if let Some(parent_id) = new_parent_id {
            crate::repository::documentation_collections::cascade_collection_membership(
                conn, parent_id, page_id, None,
            )?;
        }

        Ok(updated_page)
    })
}

/// Re-emit a page's `metadata_changed` sync event. Call after a
/// change that alters the page's denormalised `collection_id`
/// (collection add / remove / move) but not the page row itself, so
/// the sync pool's page row reflects its new collection membership.
pub fn emit_page_membership_changed(conn: &mut DbConnection, page_id: i32) -> Result<(), Error> {
    emit_page_row(conn, page_id, "documentation_page.metadata_changed")
}

/// Emit the page's whole sync row under `event_type`. Who may open a page
/// is decided as each reader receives the row, so re-emitting it after a
/// change to its rules (its own, or its collection's) sends it to whoever
/// can open it now and a delete to whoever no longer can.
pub fn emit_page_row(
    conn: &mut DbConnection,
    page_id: i32,
    event_type: &'static str,
) -> Result<(), Error> {
    emit_page_row_to(conn, page_id, event_type, crate::sync::groups::workspace())
}

/// [`emit_page_row`] to the sync `groups` given.
pub fn emit_page_row_to(
    conn: &mut DbConnection,
    page_id: i32,
    event_type: &'static str,
    groups: Vec<String>,
) -> Result<(), Error> {
    let page: DocumentationPage = documentation_pages::table.find(page_id).first(conn)?;
    let collection_id = collection_id_for_page(conn, page_id)?;
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::DocumentationPage,
            aggregate_id: page.id.to_string(),
            op: SyncOp::Update,
            event_type,
            data: page_sync_payload(&page, collection_id),
            groups,
            causation_id: None,
        },
    )?;
    Ok(())
}

/// The records whose rules name one of `group_ids`: collections and pages.
/// Read before a change that drops those grants (deleting a group), and
/// passed to [`emit_records`] after it.
pub fn records_granted_to_groups(
    conn: &mut DbConnection,
    group_ids: &[i32],
) -> Result<(Vec<i32>, Vec<i32>), Error> {
    if group_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let collections: Vec<i32> = documentation_collection_visibility::table
        .filter(documentation_collection_visibility::group_id.eq_any(group_ids))
        .select(documentation_collection_visibility::collection_id)
        .distinct()
        .load(conn)?;
    let pages: Vec<i32> = documentation_page_visibility::table
        .filter(documentation_page_visibility::group_id.eq_any(group_ids))
        .select(documentation_page_visibility::page_id)
        .distinct()
        .load(conn)?;
    Ok((collections, pages))
}

/// Re-emit `collections` (each with every page in it) and `pages` to the sync
/// `groups`, after a change to who may open them that touched none of their
/// rows: group membership, a group's deletion, a role. Each reader receives
/// what they may open now and a delete for what they no longer may, and an
/// editor open on one of them is checked again.
pub fn emit_records(
    conn: &mut DbConnection,
    collections: &[i32],
    pages: &[i32],
    groups: Vec<String>,
) -> Result<(), Error> {
    let mut page_ids: std::collections::BTreeSet<i32> = pages.iter().copied().collect();
    for &collection_id in collections {
        crate::repository::documentation_collections::emit_collection_row_to(
            conn,
            collection_id,
            "documentation_collection.visibility_changed",
            groups.clone(),
        )?;
        page_ids.extend(
            documentation_collection_pages::table
                .filter(documentation_collection_pages::collection_id.eq(collection_id))
                .select(documentation_collection_pages::page_id)
                .load::<i32>(conn)?,
        );
    }
    let existing: Vec<i32> = documentation_pages::table
        .filter(documentation_pages::id.eq_any(page_ids.iter().copied().collect::<Vec<_>>()))
        .select(documentation_pages::id)
        .load(conn)?;
    for page_id in existing {
        emit_page_row_to(
            conn,
            page_id,
            "documentation_page.visibility_changed",
            groups.clone(),
        )?;
    }
    Ok(())
}

/// After a change to who is in `group_ids` (or to what they include): re-emit
/// every record whose rules name one of them, or a group that includes one.
pub fn emit_records_granted_to_groups(
    conn: &mut DbConnection,
    group_ids: &[i32],
) -> Result<(), Error> {
    let affected = crate::repository::groups::with_including_groups(conn, group_ids)?;
    let (collections, pages) = records_granted_to_groups(conn, &affected)?;
    emit_records(conn, &collections, &pages, crate::sync::groups::workspace())
}

/// What decides which documentation a person may open in a workspace: whether
/// they are a member, whether they are an admin (workspace owner or admin, or
/// platform admin), and every group they are in, the including groups too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocAccess {
    member: bool,
    admin: bool,
    groups: std::collections::BTreeSet<i32>,
}

/// Run `f` pinned to `workspace_id`, inside a transaction (a savepoint when
/// nested) so the pin holds even when the caller has none open, and put the
/// previous pin back after. For a write that may run outside that
/// workspace's pin (a control-plane role change).
fn in_workspace<T>(
    conn: &mut DbConnection,
    workspace_id: i32,
    f: impl FnOnce(&mut DbConnection) -> Result<T, Error>,
) -> Result<T, Error> {
    conn.transaction(|conn| {
        let previous = crate::sync::session::current_workspace_id(conn)?;
        crate::sync::session::pin_workspace(conn, workspace_id)?;
        let out = f(conn);
        crate::sync::session::restore_workspace_pin(conn, previous)?;
        out
    })
}

/// [`DocAccess`] for `user_uuid` in `workspace_id`. Read it before a write that
/// can change it (a role, a membership, a group) and hand it to
/// [`emit_if_access_changed`] after.
pub fn doc_access(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: uuid::Uuid,
) -> Result<DocAccess, Error> {
    in_workspace(conn, workspace_id, |conn| {
        use crate::schema::{users, workspace_members};
        let role: Option<String> = workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq(user_uuid))
            .filter(workspace_members::removed_at.is_null())
            .select(workspace_members::role)
            .first(conn)
            .optional()?;
        let platform_admin = users::table
            .find(user_uuid)
            .select(users::platform_role)
            .first::<String>(conn)
            .optional()?
            .is_some_and(|r| crate::models::PlatformRole::from_db(&r).is_platform_admin());
        let groups = if role.is_some() {
            crate::repository::groups::get_group_ids_for_user(conn, &user_uuid)?
                .into_iter()
                .collect()
        } else {
            Default::default()
        };
        Ok(DocAccess {
            member: role.is_some(),
            admin: platform_admin
                || role
                    .as_deref()
                    .is_some_and(|r| matches!(r, "owner" | "admin")),
            groups,
        })
    })
}

/// After a write that may have changed what `user_uuid` may open in
/// `workspace_id`: compare with `before` and, only on a real change, re-emit
/// to that person the records it touches. Becoming or no longer being a
/// member or an admin touches every restricted record; joining or leaving
/// groups touches the records those groups are granted. Other readers'
/// access didn't change, so the rows are addressed to this person alone.
pub fn emit_if_access_changed(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: uuid::Uuid,
    before: &DocAccess,
) -> Result<(), Error> {
    // Someone who just joined holds nothing yet: their first sync brings it.
    if !before.member {
        return Ok(());
    }
    let after = doc_access(conn, workspace_id, user_uuid)?;
    if after == *before {
        return Ok(());
    }
    in_workspace(conn, workspace_id, |conn| {
        let (collections, pages) = if after.member != before.member || after.admin != before.admin {
            let collections: Vec<i32> = documentation_collections::table
                .filter(documentation_collections::workspace_id.eq(workspace_id))
                .filter(documentation_collections::restricted.eq(true))
                .select(documentation_collections::id)
                .load(conn)?;
            let pages: Vec<i32> = documentation_pages::table
                .filter(documentation_pages::workspace_id.eq(workspace_id))
                .filter(documentation_pages::restricted.eq(true))
                .select(documentation_pages::id)
                .load(conn)?;
            (collections, pages)
        } else {
            let changed: Vec<i32> = before
                .groups
                .symmetric_difference(&after.groups)
                .copied()
                .collect();
            records_granted_to_groups(conn, &changed)?
        };
        emit_records(
            conn,
            &collections,
            &pages,
            crate::sync::groups::for_user(user_uuid),
        )
    })
}

// Get page with ordered children
pub fn get_page_with_ordered_children(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<DocumentationPageWithChildren, Error> {
    let page = get_documentation_page(page_id, conn)?;
    let children = get_ordered_pages_by_parent_id(conn, page_id)?;

    Ok(DocumentationPageWithChildren { page, children })
}

// ============= Yjs Collaboration Methods =============

// Update documentation page Yjs state (for WebSocket sync auto-save)
// sync-audit-only: collaborative-editor CRDT auto-save fires on every keystroke-batch; the document body flows through the Yjs WebSocket channel, not the sync_actions stream
pub fn update_documentation_yjs_state(
    conn: &mut DbConnection,
    page_id: i32,
    yjs_document: Vec<u8>,
    // Ownership-claim fencing token (Phase 2 affinity). `Some(f)` gates
    // the write so a stale owner cannot clobber a newer owner's state;
    // `None` (single-instance / Redis-down) writes unconditionally.
    fence: Option<i64>,
    observer: Option<&dyn DocumentationSavedObserver>,
) -> Result<DocumentationPage, Error> {
    use crate::schema::documentation_pages::dsl;

    let result: DocumentationPage = match fence {
        Some(f) => {
            let updated = diesel::update(
                dsl::documentation_pages
                    .filter(dsl::id.eq(page_id))
                    .filter(dsl::fence_token.is_null().or(dsl::fence_token.le(f))),
            )
            .set((
                dsl::yjs_document.eq(Some(yjs_document)),
                dsl::fence_token.eq(f),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .get_result::<DocumentationPage>(conn)
            .optional()?;
            match updated {
                Some(p) => p,
                None => {
                    // Stale owner: leave the row unchanged and skip the
                    // observer (nothing changed). Return the current row.
                    tracing::warn!(
                        page_id,
                        fence = f,
                        "Skipped Yjs save: stale ownership fence"
                    );
                    return dsl::documentation_pages.find(page_id).first(conn);
                }
            }
        }
        None => diesel::update(dsl::documentation_pages.find(page_id))
            .set((
                dsl::yjs_document.eq(Some(yjs_document)),
                dsl::updated_at.eq(diesel::dsl::now),
            ))
            .get_result(conn)?,
    };

    if let Some(observer) = observer {
        observer.documentation_saved(&result);
    }

    Ok(result)
}

// Create a documentation revision snapshot
// Note: This is simplified - the schema doesn't have a revision number or contributed_by
// Creates a basic revision with just the snapshot and metadata
// sync-audit-only: revision snapshots are derived versioning state holding Yjs binary blobs, not page metadata a sync consumer projects
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
        let page: DocumentationPage = doc_dsl::documentation_pages.find(page_id).first(conn)?;

        // Get the latest revision number for this page
        let latest_revision: i32 = documentation_revisions::table
            .filter(documentation_revisions::page_id.eq(page_id))
            .select(diesel::dsl::max(documentation_revisions::revision_number))
            .first::<Option<i32>>(conn)?
            .unwrap_or(0);

        let new_revision_number = latest_revision + 1;

        // Use the first contributor or the created_by from the page
        let created_by = contributed_by
            .first()
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

// sync-audit-only: ML link-suggestion projection; the documentation_page_embeddings table is excluded from the sync tier by design
/// Sync the embedding relationships for a source page.
/// Deletes existing embeddings and replaces with the new set.
pub fn sync_page_embeddings(
    conn: &mut DbConnection,
    source_page_id: i32,
    target_page_ids: &[i32],
) -> Result<(), Error> {
    use crate::models::NewDocumentationPageEmbedding;
    use crate::schema::documentation_page_embeddings;

    // Delete all existing embeddings for this source page
    diesel::delete(
        documentation_page_embeddings::table
            .filter(documentation_page_embeddings::source_page_id.eq(source_page_id)),
    )
    .execute(conn)?;

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
        .inner_join(
            groups::table.on(groups::id
                .nullable()
                .eq(documentation_page_visibility::group_id)),
        )
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
        .inner_join(
            users::table.on(users::uuid
                .nullable()
                .eq(documentation_page_visibility::user_uuid)),
        )
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
pub fn get_embedded_pages(conn: &mut DbConnection, source_page_id: i32) -> Result<Vec<i32>, Error> {
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
    conn.transaction(|conn| {
        let count = diesel::delete(documentation_pages::table.find(id)).execute(conn)?;
        if count > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::DocumentationPage,
                    aggregate_id: id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "documentation_page.deleted",
                    data: json!({ "id": id }),
                    groups: crate::sync::groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(count)
    })
}

// ===== Page Visibility (Access Control) =====

use crate::models::{
    DocumentationPageVisibility, Group, NewDocumentationPageVisibility, UserInfoWithAvatar,
};
use crate::schema::{
    documentation_collection_pages, documentation_collection_visibility, documentation_collections,
    documentation_page_visibility, groups,
};

/// Give page `to` the page-level rules of page `from`, if it has any, so a
/// copy is closed to the same people as the original.
pub fn copy_page_rules(
    conn: &mut DbConnection,
    from: i32,
    to: i32,
    created_by: Option<uuid::Uuid>,
) -> Result<(), Error> {
    let restricted: bool = documentation_pages::table
        .find(from)
        .select(documentation_pages::restricted)
        .first(conn)?;
    if !restricted {
        return Ok(());
    }
    let rules: Vec<(Option<i32>, Option<uuid::Uuid>)> = documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq(from))
        .select((
            documentation_page_visibility::group_id,
            documentation_page_visibility::user_uuid,
        ))
        .load(conn)?;
    let groups: Vec<i32> = rules.iter().filter_map(|(g, _)| *g).collect();
    let users: Vec<uuid::Uuid> = rules.iter().filter_map(|(_, u)| *u).collect();
    set_page_rules(conn, to, true, groups, users, created_by)?;
    Ok(())
}

/// Get the groups that have explicit page-level visibility for a page.
pub fn get_visible_groups_for_page(
    conn: &mut DbConnection,
    page_id: i32,
) -> Result<Vec<Group>, Error> {
    documentation_page_visibility::table
        .filter(documentation_page_visibility::page_id.eq(page_id))
        .filter(documentation_page_visibility::group_id.is_not_null())
        .inner_join(
            groups::table.on(groups::id
                .nullable()
                .eq(documentation_page_visibility::group_id)),
        )
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
        .inner_join(
            users::table.on(users::uuid
                .nullable()
                .eq(documentation_page_visibility::user_uuid)),
        )
        .select((
            users::uuid,
            users::name,
            users::avatar_url,
            users::avatar_thumb,
        ))
        .load::<(uuid::Uuid, String, Option<String>, Option<String>)>(conn)
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

/// Set page-level visibility from grants alone: some grants give the page
/// its own rules, none returns it to following its collection. A caller that
/// can say "restricted to nobody" uses [`set_page_rules`].
pub fn set_page_visibility(
    conn: &mut DbConnection,
    page_id: i32,
    group_ids: Vec<i32>,
    user_uuids: Vec<uuid::Uuid>,
    created_by: Option<uuid::Uuid>,
) -> Result<Vec<DocumentationPageVisibility>, Error> {
    let restricted = !(group_ids.is_empty() && user_uuids.is_empty());
    set_page_rules(conn, page_id, restricted, group_ids, user_uuids, created_by)
}

/// Set a page's own rules (delete-all + re-insert). `restricted` gives the
/// page its own rules: open only to the grants, or to admins only when there
/// are none. Unrestricted drops the grants and the page follows its
/// collection.
pub fn set_page_rules(
    conn: &mut DbConnection,
    page_id: i32,
    restricted: bool,
    group_ids: Vec<i32>,
    user_uuids: Vec<uuid::Uuid>,
    created_by: Option<uuid::Uuid>,
) -> Result<Vec<DocumentationPageVisibility>, Error> {
    let (group_ids, user_uuids) = if restricted {
        (group_ids, user_uuids)
    } else {
        (Vec::new(), Vec::new())
    };
    conn.transaction(|conn| {
        // The same rules saved again change no one's access: nothing to write
        // or re-send.
        let current: Vec<DocumentationPageVisibility> = documentation_page_visibility::table
            .filter(documentation_page_visibility::page_id.eq(page_id))
            .load(conn)?;
        let was_restricted: bool = documentation_pages::table
            .find(page_id)
            .select(documentation_pages::restricted)
            .first(conn)?;
        let same_groups = current
            .iter()
            .filter_map(|v| v.group_id)
            .collect::<std::collections::BTreeSet<_>>()
            == group_ids.iter().copied().collect();
        let same_users = current
            .iter()
            .filter_map(|v| v.user_uuid)
            .collect::<std::collections::BTreeSet<_>>()
            == user_uuids.iter().copied().collect();
        if was_restricted == restricted && same_groups && same_users {
            return Ok(current);
        }
        diesel::update(documentation_pages::table.find(page_id))
            .set(documentation_pages::restricted.eq(restricted))
            .execute(conn)?;
        // Delete all existing page-level visibility entries
        diesel::delete(
            documentation_page_visibility::table
                .filter(documentation_page_visibility::page_id.eq(page_id)),
        )
        .execute(conn)?;

        let entries: Vec<DocumentationPageVisibility> =
            if group_ids.is_empty() && user_uuids.is_empty() {
                // Clearing the override is itself a visibility change
                // (page reverts to inheriting from its collections), so it
                // still emits below.
                Vec::new()
            } else {
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
                    .get_results(conn)?
            };

        // The whole page, not just its new rules: the sync feed sends it to
        // whoever can open it now and a delete to whoever no longer can.
        let page: DocumentationPage = documentation_pages::table.find(page_id).first(conn)?;
        let collection_id = collection_id_for_page(conn, page_id)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::DocumentationPage,
                aggregate_id: page_id.to_string(),
                op: SyncOp::Update,
                event_type: "documentation_page.visibility_changed",
                data: page_sync_payload(&page, collection_id),
                groups: crate::sync::groups::workspace(),
                causation_id: None,
            },
        )?;

        Ok(entries)
    })
}

/// Who is reading documentation, and the one place that decides which pages
/// and collections they may open. Every reader asks it: the REST routes, the
/// sync bootstrap and feed, the collab editor and the exporter. A page or
/// collection it says no to reads as absent everywhere.
///
/// The markdown exporter follows `embedded_document` nodes, and a page the
/// caller may read can embed one they may not, so it carries the audience
/// down the recursion and checks every page the output contains.
///
/// The comment-side analogue is `ticket_visibility::CommentAudience`.
#[derive(Clone, Copy)]
pub enum PageAudience {
    /// A specific caller, filtered by the rules on each page and collection.
    User {
        user_uuid: uuid::Uuid,
        is_admin: bool,
    },
    /// No filtering, for callers that have already established authority over
    /// the whole export (nothing uses this yet; it exists so a future system
    /// exporter has to say so rather than pass a borrowed user).
    Unrestricted,
    /// The guest portal: pages published to guests that no restriction
    /// covers, and no collections.
    Guest,
}

impl PageAudience {
    /// The caller of a docs route.
    pub fn from_auth(auth: &crate::extractors::AuthContext) -> Self {
        PageAudience::User {
            user_uuid: auth.user_uuid,
            is_admin: auth.is_workspace_admin(),
        }
    }

    /// The reader `user` is in the connection's workspace.
    pub fn for_user(conn: &mut DbConnection, user: &crate::models::User) -> Self {
        PageAudience::User {
            user_uuid: user.uuid,
            is_admin: crate::repository::user_helpers::user_is_admin(conn, user),
        }
    }

    /// Fails closed: a lookup error reads as "not accessible", matching the
    /// handlers, which turn a visibility-check failure into a refusal rather
    /// than falling through to the content.
    pub fn can_read(&self, conn: &mut DbConnection, page_id: i32) -> bool {
        self.try_can_read(conn, page_id).unwrap_or(false)
    }

    /// Whether this audience may read the page, with a lookup error left to
    /// the caller (a route answers it with a 500, not a 404).
    pub fn try_can_read(&self, conn: &mut DbConnection, page_id: i32) -> Result<bool, Error> {
        Ok(!self.hidden_pages(conn, &[page_id])?.contains(&page_id))
    }

    /// Whether the page exists and this audience may read it. A route answers
    /// a page it may not read like a missing one, with the same 404, and
    /// changes nothing. (An admin may read any id, so the row is checked
    /// too.)
    pub fn can_open_page(&self, conn: &mut DbConnection, page_id: i32) -> Result<bool, Error> {
        let exists = documentation_pages::table
            .find(page_id)
            .select(documentation_pages::id)
            .first::<i32>(conn)
            .optional()?
            .is_some();
        Ok(exists && self.can_read(conn, page_id))
    }

    /// Which of `page_ids` this audience may not read. An id with no row is
    /// hidden: it may have been a page they could not open.
    pub fn hidden_pages(
        &self,
        conn: &mut DbConnection,
        page_ids: &[i32],
    ) -> Result<std::collections::HashSet<i32>, Error> {
        Ok(self.hidden(conn, page_ids, &[])?.0)
    }

    /// The pages in `pages` this audience may read.
    pub fn filter_pages(
        &self,
        conn: &mut DbConnection,
        pages: Vec<DocumentationPage>,
    ) -> Result<Vec<DocumentationPage>, Error> {
        match self {
            PageAudience::Unrestricted => Ok(pages),
            PageAudience::User {
                user_uuid,
                is_admin,
            } => filter_pages_for_user(conn, pages, user_uuid, *is_admin),
            PageAudience::Guest => {
                let ids: Vec<i32> = pages.iter().map(|p| p.id).collect();
                let hidden = self.hidden_pages(conn, &ids)?;
                Ok(pages
                    .into_iter()
                    .filter(|p| !hidden.contains(&p.id))
                    .collect())
            }
        }
    }

    /// Which of `page_ids` and `collection_ids` this audience may not open.
    /// An id with no row is hidden: it may have been one they could not open.
    pub fn hidden(
        &self,
        conn: &mut DbConnection,
        page_ids: &[i32],
        collection_ids: &[i32],
    ) -> Result<
        (
            std::collections::HashSet<i32>,
            std::collections::HashSet<i32>,
        ),
        Error,
    > {
        match self {
            PageAudience::Unrestricted => Ok(Default::default()),
            PageAudience::User {
                user_uuid,
                is_admin,
            } => hidden_documentation_ids(conn, page_ids, collection_ids, user_uuid, *is_admin),
            PageAudience::Guest => {
                hidden_ids(conn, page_ids, collection_ids, Reader::Guest, Trash::Absent)
            }
        }
    }

    /// [`PageAudience::hidden`], but a page in the trash reads as its rules
    /// say rather than as absent: for the trash list, restoring from it, and
    /// the sync rows the trash view is built from. Admins hide nothing.
    pub fn hidden_with_trash(
        &self,
        conn: &mut DbConnection,
        page_ids: &[i32],
        collection_ids: &[i32],
    ) -> Result<
        (
            std::collections::HashSet<i32>,
            std::collections::HashSet<i32>,
        ),
        Error,
    > {
        match self {
            PageAudience::Unrestricted | PageAudience::User { is_admin: true, .. } => {
                Ok(Default::default())
            }
            PageAudience::User { user_uuid, .. } => hidden_ids(
                conn,
                page_ids,
                collection_ids,
                Reader::Member(user_uuid),
                Trash::ByRules,
            ),
            // The portal never shows the trash.
            PageAudience::Guest => self.hidden(conn, page_ids, collection_ids),
        }
    }

    /// The pages in `pages` this audience may see with the trash read by its
    /// rules (see [`PageAudience::hidden_with_trash`]).
    pub fn filter_pages_with_trash(
        &self,
        conn: &mut DbConnection,
        pages: Vec<DocumentationPage>,
    ) -> Result<Vec<DocumentationPage>, Error> {
        let ids: Vec<i32> = pages.iter().map(|p| p.id).collect();
        let (hidden, _) = self.hidden_with_trash(conn, &ids, &[])?;
        Ok(pages
            .into_iter()
            .filter(|p| !hidden.contains(&p.id))
            .collect())
    }

    /// Whether the page exists and this audience may restore it: one in the
    /// trash or archived that they could open under its rules.
    pub fn can_restore_page(&self, conn: &mut DbConnection, page_id: i32) -> Result<bool, Error> {
        let exists = documentation_pages::table
            .find(page_id)
            .select(documentation_pages::id)
            .first::<i32>(conn)
            .optional()?
            .is_some();
        Ok(exists
            && !self
                .hidden_with_trash(conn, &[page_id], &[])?
                .0
                .contains(&page_id))
    }

    /// Fails closed, like [`PageAudience::can_read`].
    pub fn can_read_collection(&self, conn: &mut DbConnection, collection_id: i32) -> bool {
        self.try_can_read_collection(conn, collection_id)
            .unwrap_or(false)
    }

    /// Whether this audience may see the collection, with a lookup error left
    /// to the caller.
    pub fn try_can_read_collection(
        &self,
        conn: &mut DbConnection,
        collection_id: i32,
    ) -> Result<bool, Error> {
        Ok(!self
            .hidden(conn, &[], &[collection_id])?
            .1
            .contains(&collection_id))
    }

    /// The items in `items` whose collection (named by `id`) this audience
    /// may see.
    pub fn filter_collections<T>(
        &self,
        conn: &mut DbConnection,
        items: Vec<T>,
        id: impl Fn(&T) -> i32,
    ) -> Result<Vec<T>, Error> {
        let ids: Vec<i32> = items.iter().map(&id).collect();
        let (_, hidden) = self.hidden(conn, &[], &ids)?;
        Ok(items
            .into_iter()
            .filter(|item| !hidden.contains(&id(item)))
            .collect())
    }

    /// Whether the collection exists and this audience may see it. Same
    /// contract as [`PageAudience::can_open_page`].
    pub fn can_open_collection(
        &self,
        conn: &mut DbConnection,
        collection_id: i32,
    ) -> Result<bool, Error> {
        let exists = documentation_collections::table
            .find(collection_id)
            .select(documentation_collections::id)
            .first::<i32>(conn)
            .optional()?
            .is_some();
        Ok(exists && self.can_read_collection(conn, collection_id))
    }

    /// The collection, if it exists and this audience may see it, for routes
    /// that need the row.
    pub fn readable_collection(
        &self,
        conn: &mut DbConnection,
        collection_id: i32,
    ) -> Result<Option<crate::models::DocumentationCollection>, Error> {
        let collection = documentation_collections::table
            .find(collection_id)
            .first::<crate::models::DocumentationCollection>(conn)
            .optional()?;
        Ok(collection.filter(|c| self.can_read_collection(conn, c.id)))
    }
}

/// Whether a page in the trash reads as absent (every route but the trash
/// itself) or as its rules say (the trash list, restoring from it, and the
/// sync rows the trash view is built from).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trash {
    Absent,
    ByRules,
}

/// Who the documentation rule is deciding for.
#[derive(Clone, Copy)]
enum Reader<'a> {
    /// A workspace user who isn't an admin (admins read everything).
    Member(&'a uuid::Uuid),
    /// The guest portal: anyone, signed in or not.
    Guest,
}

/// Batch-filter a list of pages for a user. Returns only the pages the user
/// can open (see [`hidden_ids`]).
fn filter_pages_for_user(
    conn: &mut DbConnection,
    pages: Vec<DocumentationPage>,
    user_uuid: &uuid::Uuid,
    is_admin: bool,
) -> Result<Vec<DocumentationPage>, Error> {
    if is_admin || pages.is_empty() {
        return Ok(pages);
    }
    let ids: Vec<i32> = pages.iter().map(|p| p.id).collect();
    let (hidden, _) = hidden_ids(conn, &ids, &[], Reader::Member(user_uuid), Trash::Absent)?;
    Ok(pages
        .into_iter()
        .filter(|p| !hidden.contains(&p.id))
        .collect())
}

/// The page and collection ids among these the user may NOT see. Admins
/// hide nothing. Reached through [`PageAudience::hidden`].
fn hidden_documentation_ids(
    conn: &mut DbConnection,
    page_ids: &[i32],
    collection_ids: &[i32],
    user_uuid: &uuid::Uuid,
    is_admin: bool,
) -> Result<
    (
        std::collections::HashSet<i32>,
        std::collections::HashSet<i32>,
    ),
    Error,
> {
    if is_admin {
        return Ok(Default::default());
    }
    hidden_ids(
        conn,
        page_ids,
        collection_ids,
        Reader::Member(user_uuid),
        Trash::Absent,
    )
}

/// The documentation rule. Which of these pages and collections `reader`
/// may not open:
///
/// - An id with no row (deleted for good) is hidden: nothing shows it was
///   open to them, and the sync feed still holds its old rows, which must
///   reach them only as a delete.
/// - A page in the trash is hidden, except where `trash` says it reads as
///   its rules say (the trash list, restoring, and the sync rows the trash
///   view is built from).
/// - A restricted collection is open only to the people and groups it
///   names; one that names nobody is open to admins only. An unrestricted
///   collection is open to everyone in the workspace.
/// - A page with its own rules (restricted) is open only to the people and
///   groups it names, or to admins only when it names nobody. Any other page
///   follows its collection, and a page in no collection is open.
/// - The guest portal reads only pages published to guests that no
///   restriction covers.
///
/// An empty set of grants never means open.
fn hidden_ids(
    conn: &mut DbConnection,
    page_ids: &[i32],
    collection_ids: &[i32],
    reader: Reader<'_>,
    trash: Trash,
) -> Result<
    (
        std::collections::HashSet<i32>,
        std::collections::HashSet<i32>,
    ),
    Error,
> {
    use std::collections::{HashMap, HashSet};

    // The pages, and every collection either named or holding a page.
    let pages: Vec<(i32, bool, DocumentationStatus, bool)> = if page_ids.is_empty() {
        Vec::new()
    } else {
        documentation_pages::table
            .filter(documentation_pages::id.eq_any(page_ids))
            .select((
                documentation_pages::id,
                documentation_pages::restricted,
                documentation_pages::status,
                documentation_pages::is_public,
            ))
            .load(conn)?
    };
    let memberships: Vec<(i32, i32)> = if pages.is_empty() {
        Vec::new()
    } else {
        documentation_collection_pages::table
            .filter(documentation_collection_pages::page_id.eq_any(page_ids))
            .select((
                documentation_collection_pages::page_id,
                documentation_collection_pages::collection_id,
            ))
            .load(conn)?
    };
    let mut wanted: Vec<i32> = collection_ids.to_vec();
    wanted.extend(memberships.iter().map(|(_, c)| *c));
    let collections: HashMap<i32, bool> = if wanted.is_empty() {
        HashMap::new()
    } else {
        documentation_collections::table
            .filter(documentation_collections::id.eq_any(&wanted))
            .select((
                documentation_collections::id,
                documentation_collections::restricted,
            ))
            .load::<(i32, bool)>(conn)?
            .into_iter()
            .collect()
    };

    // Grants, only for what is restricted, and only a member can hold one.
    let (page_grants, collection_grants, user_groups) = match reader {
        Reader::Guest => (Vec::new(), Vec::new(), HashSet::new()),
        Reader::Member(_) => {
            let restricted_pages: Vec<i32> = pages.iter().filter(|p| p.1).map(|p| p.0).collect();
            let restricted_collections: Vec<i32> = collections
                .iter()
                .filter(|(_, restricted)| **restricted)
                .map(|(id, _)| *id)
                .collect();
            let page_grants: Vec<(i32, Option<i32>, Option<uuid::Uuid>)> =
                if restricted_pages.is_empty() {
                    Vec::new()
                } else {
                    documentation_page_visibility::table
                        .filter(documentation_page_visibility::page_id.eq_any(&restricted_pages))
                        .select((
                            documentation_page_visibility::page_id,
                            documentation_page_visibility::group_id,
                            documentation_page_visibility::user_uuid,
                        ))
                        .load(conn)?
                };
            let collection_grants: Vec<(i32, Option<i32>, Option<uuid::Uuid>)> =
                if restricted_collections.is_empty() {
                    Vec::new()
                } else {
                    documentation_collection_visibility::table
                        .filter(
                            documentation_collection_visibility::collection_id
                                .eq_any(&restricted_collections),
                        )
                        .select((
                            documentation_collection_visibility::collection_id,
                            documentation_collection_visibility::group_id,
                            documentation_collection_visibility::user_uuid,
                        ))
                        .load(conn)?
                };
            let user_groups: HashSet<i32> =
                if page_grants.is_empty() && collection_grants.is_empty() {
                    HashSet::new()
                } else if let Reader::Member(user_uuid) = reader {
                    crate::repository::groups::get_group_ids_for_user(conn, user_uuid)?
                        .into_iter()
                        .collect()
                } else {
                    HashSet::new()
                };
            (page_grants, collection_grants, user_groups)
        }
    };
    let granted = |grants: &[(i32, Option<i32>, Option<uuid::Uuid>)], id: i32| match reader {
        Reader::Guest => false,
        Reader::Member(user_uuid) => grants.iter().any(|(on, group, user)| {
            *on == id
                && (user.as_ref() == Some(user_uuid)
                    || group.is_some_and(|g| user_groups.contains(&g)))
        }),
    };
    let collection_open = |id: i32| match collections.get(&id) {
        None => false,
        Some(false) => true,
        Some(true) => granted(&collection_grants, id),
    };

    let mut hidden_collections: HashSet<i32> = collection_ids
        .iter()
        .copied()
        .filter(|id| !collection_open(*id))
        .collect();
    if matches!(reader, Reader::Guest) {
        // The portal lists no collections.
        hidden_collections.extend(collection_ids.iter().copied());
    }

    let open_pages: HashSet<i32> = pages
        .iter()
        .filter(|(id, restricted, status, is_public)| {
            if *status == DocumentationStatus::Deleted && trash == Trash::Absent {
                return false;
            }
            if matches!(reader, Reader::Guest) && !*is_public {
                return false;
            }
            if *restricted {
                return granted(&page_grants, *id);
            }
            let mut in_collections = memberships
                .iter()
                .filter(|(page, _)| page == id)
                .map(|(_, c)| *c)
                .peekable();
            in_collections.peek().is_none() || in_collections.any(&collection_open)
        })
        .map(|p| p.0)
        .collect();
    let hidden_pages: HashSet<i32> = page_ids
        .iter()
        .copied()
        .filter(|id| !open_pages.contains(id))
        .collect();

    Ok((hidden_pages, hidden_collections))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DocumentationStatus;
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
        let user = TestFixtures::create_user(&mut conn, "docuser", "admin");

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let fetched = get_documentation_page(page.id, &mut conn).unwrap();
        assert_eq!(fetched.title, "Test Page");
    }

    #[test]
    fn get_by_slug() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sluguser", "admin");

        create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let fetched = get_documentation_page_by_slug("test-page", &mut conn).unwrap();
        assert_eq!(fetched.title, "Test Page");
    }

    #[test]
    fn update_documentation_page_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "upduser", "admin");

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let update = DocumentationPageUpdate {
            title: Some("Updated Title".to_string()),
            slug: None,
            icon: None,
            cover_image: None,
            status: None,
            last_edited_by: None,
            parent_id: None,
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
            verified_by: None,
            verified_at: None,
            verify_interval_days: None,
        };
        let updated = update_documentation_page(&mut conn, page.id, &update).unwrap();
        assert_eq!(updated.title, "Updated Title");
    }

    #[test]
    fn delete_documentation_page_test() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "deluser", "admin");

        let page = create_documentation_page(make_page(user.uuid), &mut conn).unwrap();
        let rows = delete_documentation_page(page.id, &mut conn, None).unwrap();
        assert_eq!(rows, 1);
        assert!(get_documentation_page(page.id, &mut conn).is_err());
    }

    #[test]
    fn top_level_pages_excludes_children() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "treeuser", "admin");

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

    #[test]
    fn hidden_documentation_ids_respects_page_visibility() {
        let mut conn = setup_test_connection();
        let grantee = TestFixtures::create_user(&mut conn, "doc_grantee", "user");
        let outsider = TestFixtures::create_user(&mut conn, "doc_outsider", "user");

        // A page with no override is public: hidden for nobody.
        let public_page = create_documentation_page(make_page(grantee.uuid), &mut conn).unwrap();
        let (hidden, _) =
            hidden_documentation_ids(&mut conn, &[public_page.id], &[], &outsider.uuid, false)
                .unwrap();
        assert!(
            hidden.is_empty(),
            "page with no visibility override should be public"
        );

        // Restrict the page to the grantee only.
        set_page_visibility(
            &mut conn,
            public_page.id,
            vec![],
            vec![grantee.uuid],
            Some(grantee.uuid),
        )
        .unwrap();
        let page_id = public_page.id;

        // Outsider can no longer see it -> reported hidden.
        let (hidden, _) =
            hidden_documentation_ids(&mut conn, &[page_id], &[], &outsider.uuid, false).unwrap();
        assert!(
            hidden.contains(&page_id),
            "restricted page must be hidden from an outsider"
        );

        // The grantee still sees it -> not hidden.
        let (hidden, _) =
            hidden_documentation_ids(&mut conn, &[page_id], &[], &grantee.uuid, false).unwrap();
        assert!(
            !hidden.contains(&page_id),
            "granted user must still see the page"
        );

        // Admins hide nothing.
        let (hidden, _) =
            hidden_documentation_ids(&mut conn, &[page_id], &[], &outsider.uuid, true).unwrap();
        assert!(hidden.is_empty(), "admin must see all documentation");

        // An id with no row (a hard-deleted page or collection) can't be
        // shown to be open, so it is hidden: its old rows reach a reader
        // only as a delete.
        let (hidden_pages, hidden_collections) = hidden_documentation_ids(
            &mut conn,
            &[page_id + 99999],
            &[i32::MAX],
            &outsider.uuid,
            false,
        )
        .unwrap();
        assert!(
            hidden_pages.contains(&(page_id + 99999)),
            "a page with no row"
        );
        assert!(
            hidden_collections.contains(&i32::MAX),
            "a collection with no row"
        );
    }

    fn page_titled(created_by: Uuid, title: &str, parent_id: Option<i32>) -> NewDocumentationPage {
        NewDocumentationPage {
            title: title.to_string(),
            slug: title.to_lowercase().replace(' ', "-"),
            parent_id,
            ..make_page(created_by)
        }
    }

    fn reparent(parent_id: i32) -> DocumentationPageUpdate {
        DocumentationPageUpdate {
            title: None,
            slug: None,
            icon: None,
            cover_image: None,
            status: None,
            last_edited_by: None,
            parent_id: Some(Some(parent_id)),
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
            verified_by: None,
            verified_at: None,
            verify_interval_days: None,
        }
    }

    fn refused<T: std::fmt::Debug>(result: Result<T, Error>) -> bool {
        matches!(result, Err(Error::RollbackTransaction))
    }

    /// Every path that sets a parent refuses the page itself, one of its
    /// descendants, and a page that doesn't exist; a real move still works.
    #[test]
    fn a_page_cannot_become_its_own_ancestor() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "cycleuser", "admin");
        let top =
            create_documentation_page(page_titled(user.uuid, "Top", None), &mut conn).unwrap();
        let child =
            create_documentation_page(page_titled(user.uuid, "Child", Some(top.id)), &mut conn)
                .unwrap();
        let grandchild = create_documentation_page(
            page_titled(user.uuid, "Grandchild", Some(child.id)),
            &mut conn,
        )
        .unwrap();
        let other =
            create_documentation_page(page_titled(user.uuid, "Other", None), &mut conn).unwrap();

        for parent in [top.id, child.id, grandchild.id] {
            assert!(
                refused(update_documentation_page(
                    &mut conn,
                    top.id,
                    &reparent(parent)
                )),
                "update under {parent}"
            );
            assert!(
                refused(move_page_to_parent(&mut conn, top.id, Some(parent), 0)),
                "move under {parent}"
            );
            let order = [PageOrder {
                page_id: top.id,
                display_order: 0,
            }];
            assert!(
                refused(reorder_pages(&mut conn, Some(parent), &order)),
                "reorder under {parent}"
            );
        }
        let missing = i32::MAX;
        assert!(refused(create_documentation_page(
            page_titled(user.uuid, "Orphan", Some(missing)),
            &mut conn
        )));
        assert!(refused(update_documentation_page(
            &mut conn,
            child.id,
            &reparent(missing)
        )));

        let moved = move_page_to_parent(&mut conn, top.id, Some(other.id), 0).unwrap();
        assert_eq!(moved.parent_id, Some(other.id));
        let moved =
            update_documentation_page(&mut conn, grandchild.id, &reparent(other.id)).unwrap();
        assert_eq!(moved.parent_id, Some(other.id));
    }

    /// A parent cycle already in the data (written before the check existed)
    /// can't hang the descendant walk, and a move into it is still refused.
    #[test]
    fn the_descendant_walk_survives_an_existing_cycle() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "loopuser", "admin");
        let a =
            create_documentation_page(page_titled(user.uuid, "Loop A", None), &mut conn).unwrap();
        let b = create_documentation_page(page_titled(user.uuid, "Loop B", Some(a.id)), &mut conn)
            .unwrap();
        diesel::update(documentation_pages::table.find(a.id))
            .set(documentation_pages::parent_id.eq(b.id))
            .execute(&mut conn)
            .unwrap();

        assert_eq!(get_all_descendant_ids(&mut conn, a.id).unwrap(), vec![b.id]);
        assert!(refused(move_page_to_parent(&mut conn, a.id, Some(b.id), 0)));
    }
}
