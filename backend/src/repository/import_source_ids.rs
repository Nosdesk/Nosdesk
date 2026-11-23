//! The helpdesk import's id map: every record brought across from another
//! helpdesk, and the Nosdesk row it became.
//!
//! It is what makes an import safe to run again. A run that died resumes at
//! the first unmapped ticket; a rerun skips what landed and appends only the
//! new replies; a reply finds its ticket and an author their account. It
//! outlives the import job and its pruning, for the life of the workspace.

use std::collections::HashMap;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use diesel::QueryResult;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::import_source_ids;

/// What a mapped record is. Stored as text; the variants are the whole set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entity {
    Person,
    Ticket,
    Comment,
    Attachment,
    Category,
}

impl Entity {
    pub fn as_str(self) -> &'static str {
        match self {
            Entity::Person => "person",
            Entity::Ticket => "ticket",
            Entity::Comment => "comment",
            Entity::Attachment => "attachment",
            Entity::Category => "category",
        }
    }
}

/// Which source instance a lookup is about: `("zendesk", "acme")`.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub source: &'a str,
    pub instance: &'a str,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = import_source_ids)]
struct NewSourceId<'a> {
    source: &'a str,
    source_scope: &'a str,
    entity: &'a str,
    source_id: &'a str,
    target_id: &'a str,
    job_id: Uuid,
    source_updated_at: Option<NaiveDateTime>,
}

/// The Nosdesk id `source_id` became, if it has been brought across.
pub fn target_of(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    entity: Entity,
    source_id: &str,
) -> QueryResult<Option<String>> {
    import_source_ids::table
        .filter(import_source_ids::source.eq(scope.source))
        .filter(import_source_ids::source_scope.eq(scope.instance))
        .filter(import_source_ids::entity.eq(entity.as_str()))
        .filter(import_source_ids::source_id.eq(source_id))
        .select(import_source_ids::target_id)
        .first(conn)
        .optional()
}

/// The Nosdesk ids of those of `source_ids` already brought across, keyed by
/// source id. One query for a batch.
pub fn targets_of(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    entity: Entity,
    source_ids: &[String],
) -> QueryResult<HashMap<String, String>> {
    if source_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, String)> = import_source_ids::table
        .filter(import_source_ids::source.eq(scope.source))
        .filter(import_source_ids::source_scope.eq(scope.instance))
        .filter(import_source_ids::entity.eq(entity.as_str()))
        .filter(import_source_ids::source_id.eq_any(source_ids))
        .select((import_source_ids::source_id, import_source_ids::target_id))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// How many records of this kind have been brought across from the source.
pub fn count(conn: &mut DbConnection, scope: Scope<'_>, entity: Entity) -> QueryResult<i64> {
    import_source_ids::table
        .filter(import_source_ids::source.eq(scope.source))
        .filter(import_source_ids::source_scope.eq(scope.instance))
        .filter(import_source_ids::entity.eq(entity.as_str()))
        .count()
        .get_result(conn)
}

// sync-audit-only: takes a lock and writes nothing
/// Hold, until the transaction ends, the lock that makes writes from one
/// source instance into this workspace one batch at a time. Two runs of the
/// same import (an operator's retry while the first still runs) then
/// interleave whole batches, each reading the map after the other's commit,
/// instead of both writing the same ticket.
pub fn lock_source(
    conn: &mut DbConnection,
    workspace_id: i32,
    scope: Scope<'_>,
) -> QueryResult<()> {
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind::<diesel::sql_types::Text, _>(format!(
            "helpdesk-import:{workspace_id}:{}:{}",
            scope.source, scope.instance
        ))
        .execute(conn)
        .map(|_| ())
}

// sync-audit-only: import bookkeeping no client reads; the mapped rows are synced and audited as they are written, and the job row records who ran the import
/// Drop a mapping whose Nosdesk row is gone (a person whose account was
/// purged), so the record can be brought across again.
pub fn forget(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    entity: Entity,
    source_id: &str,
) -> QueryResult<()> {
    diesel::delete(
        import_source_ids::table
            .filter(import_source_ids::source.eq(scope.source))
            .filter(import_source_ids::source_scope.eq(scope.instance))
            .filter(import_source_ids::entity.eq(entity.as_str()))
            .filter(import_source_ids::source_id.eq(source_id)),
    )
    .execute(conn)
    .map(|_| ())
}

// sync-audit-only: import bookkeeping no client reads; the mapped rows are synced and audited as they are written, and the job row records who ran the import
/// Record that `source_id` became `target_id`. Written in the transaction
/// that writes the row, so the two land or roll back together. A record
/// mapped already is an error, not a second mapping: it means two writes of
/// the same record, and the transaction must roll back rather than keep a
/// duplicate the map can't see.
pub fn record(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    entity: Entity,
    source_id: &str,
    target_id: &str,
    job_id: Uuid,
    source_updated_at: Option<NaiveDateTime>,
) -> QueryResult<()> {
    diesel::insert_into(import_source_ids::table)
        .values(&NewSourceId {
            source: scope.source,
            source_scope: scope.instance,
            entity: entity.as_str(),
            source_id,
            target_id,
            job_id,
            source_updated_at,
        })
        .execute(conn)
        .map(|_| ())
}
