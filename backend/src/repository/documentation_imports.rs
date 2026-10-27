//! The reads a documentation import makes against the workspace it imports
//! into: what its links and spans may point at, which of its pages were
//! imported before, and where appended pages go. Companion to
//! `services::imports::documentation`. Writes go through the documentation
//! repositories, which emit their own sync events.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::DocumentationStatus;
use crate::schema::{documentation_collection_pages, documentation_pages};

/// A page the workspace holds, with the collection it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingPage {
    pub id: i32,
    pub uuid: Uuid,
    pub collection_id: Option<i32>,
}

/// Of `uuids`, the pages that exist and aren't in Trash.
// sync-audit-only: read-only
pub fn pages_by_uuid(conn: &mut DbConnection, uuids: &[Uuid]) -> QueryResult<Vec<ExistingPage>> {
    if uuids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(i32, Uuid, Option<i32>)> = documentation_pages::table
        .left_join(
            documentation_collection_pages::table
                .on(documentation_collection_pages::page_id.eq(documentation_pages::id)),
        )
        .filter(documentation_pages::uuid.eq_any(uuids))
        .filter(documentation_pages::status.ne(DocumentationStatus::Deleted))
        .filter(documentation_pages::deleted_at.is_null())
        .select((
            documentation_pages::id,
            documentation_pages::uuid,
            documentation_collection_pages::collection_id.nullable(),
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(id, uuid, collection_id)| ExistingPage {
            id,
            uuid,
            collection_id,
        })
        .collect())
}

/// Of `ids`, the pages that exist and aren't in Trash.
// sync-audit-only: read-only
pub fn pages_by_id(conn: &mut DbConnection, ids: &[i32]) -> QueryResult<Vec<ExistingPage>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(i32, Uuid, Option<i32>)> = documentation_pages::table
        .left_join(
            documentation_collection_pages::table
                .on(documentation_collection_pages::page_id.eq(documentation_pages::id)),
        )
        .filter(documentation_pages::id.eq_any(ids))
        .filter(documentation_pages::status.ne(DocumentationStatus::Deleted))
        .filter(documentation_pages::deleted_at.is_null())
        .select((
            documentation_pages::id,
            documentation_pages::uuid,
            documentation_collection_pages::collection_id.nullable(),
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(id, uuid, collection_id)| ExistingPage {
            id,
            uuid,
            collection_id,
        })
        .collect())
}

/// Of `uuids`, those naming a page, in Trash or not: a commit that wrote
/// them committed.
// sync-audit-only: read-only
pub fn existing_page_uuids(conn: &mut DbConnection, uuids: &[Uuid]) -> QueryResult<Vec<Uuid>> {
    if uuids.is_empty() {
        return Ok(Vec::new());
    }
    documentation_pages::table
        .filter(documentation_pages::uuid.eq_any(uuids))
        .select(documentation_pages::uuid)
        .load(conn)
}

/// Whether a `/documentation/{reference}` link names a page here: by id
/// when it is a number, else by slug.
// sync-audit-only: read-only
pub fn page_reference_exists(conn: &mut DbConnection, reference: &str) -> QueryResult<bool> {
    let query = documentation_pages::table
        .filter(documentation_pages::status.ne(DocumentationStatus::Deleted))
        .filter(documentation_pages::deleted_at.is_null())
        .into_boxed();
    let query = match reference.parse::<i32>() {
        Ok(id) => query.filter(documentation_pages::id.eq(id)),
        Err(_) => query.filter(documentation_pages::slug.eq(reference)),
    };
    diesel::select(diesel::dsl::exists(query)).get_result(conn)
}

/// Whether the user is a current member of the workspace.
// sync-audit-only: read-only
pub fn member_exists(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<bool> {
    use crate::schema::workspace_members::dsl as m;
    diesel::select(diesel::dsl::exists(
        m::workspace_members
            .filter(m::user_uuid.eq(user_uuid))
            .filter(m::removed_at.is_null()),
    ))
    .get_result(conn)
}

/// Whether ticket `id` is in the workspace.
// sync-audit-only: read-only
pub fn ticket_exists(conn: &mut DbConnection, id: i32) -> QueryResult<bool> {
    use crate::schema::tickets::dsl as t;
    diesel::select(diesel::dsl::exists(t::tickets.filter(t::id.eq(id)))).get_result(conn)
}

/// Whether the collection exists.
// sync-audit-only: read-only
pub fn collection_exists(conn: &mut DbConnection, id: i32) -> QueryResult<bool> {
    use crate::schema::documentation_collections::dsl as c;
    diesel::select(diesel::dsl::exists(
        c::documentation_collections.filter(c::id.eq(id)),
    ))
    .get_result(conn)
}

/// The highest `display_order` among the pages a new page appended under
/// `parent` (or at the root of `collection` when `parent` is `None`) would
/// sit beside; `None` when there are none.
// sync-audit-only: read-only
pub fn last_sibling_order(
    conn: &mut DbConnection,
    collection: i32,
    parent: Option<i32>,
) -> QueryResult<Option<i32>> {
    let in_collection = documentation_collection_pages::table
        .filter(documentation_collection_pages::collection_id.eq(collection))
        .select(documentation_collection_pages::page_id);
    let query = documentation_pages::table
        .filter(documentation_pages::status.ne(DocumentationStatus::Deleted))
        .filter(documentation_pages::deleted_at.is_null())
        .filter(documentation_pages::id.eq_any(in_collection))
        .into_boxed();
    let query = match parent {
        Some(parent) => query.filter(documentation_pages::parent_id.eq(parent)),
        None => query.filter(documentation_pages::parent_id.is_null()),
    };
    query
        .select(diesel::dsl::max(documentation_pages::display_order))
        .first::<Option<i32>>(conn)
}
