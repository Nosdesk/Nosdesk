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

/// Which source instance a lookup is about, in which workspace:
/// `("zendesk", "acme")` brought into workspace 3. Every read and write
/// names the workspace itself rather than leaving it to row security.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub workspace_id: i32,
    pub source: &'a str,
    pub instance: &'a str,
}

/// The person id the source's placeholder author is mapped under: not a
/// shape any source uses for its own ids.
pub const PLACEHOLDER_PERSON: &str = "~placeholder";

#[derive(Debug, Insertable)]
#[diesel(table_name = import_source_ids)]
struct NewSourceId<'a> {
    workspace_id: i32,
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
        .filter(import_source_ids::workspace_id.eq(scope.workspace_id))
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
        .filter(import_source_ids::workspace_id.eq(scope.workspace_id))
        .filter(import_source_ids::source.eq(scope.source))
        .filter(import_source_ids::source_scope.eq(scope.instance))
        .filter(import_source_ids::entity.eq(entity.as_str()))
        .filter(import_source_ids::source_id.eq_any(source_ids))
        .select((import_source_ids::source_id, import_source_ids::target_id))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    n: i64,
}

// sync-audit-only: read-only (counts the workspace's tickets the map doesn't name)
/// How many of the workspace's tickets this source didn't bring: its own,
/// or another import's. Zero means the source's numbers can't collide with
/// anything but themselves, however many of its tickets were deleted here.
pub fn other_tickets(conn: &mut DbConnection, scope: Scope<'_>) -> QueryResult<i64> {
    diesel::sql_query(
        "SELECT count(*) AS n FROM tickets t \
         WHERE t.workspace_id = $1 AND NOT EXISTS ( \
             SELECT 1 FROM import_source_ids m \
             WHERE m.workspace_id = $1 AND m.source = $2 AND m.source_scope = $3 \
               AND m.entity = 'ticket' AND m.target_id = t.id::text)",
    )
    .bind::<diesel::sql_types::Integer, _>(scope.workspace_id)
    .bind::<diesel::sql_types::Text, _>(scope.source)
    .bind::<diesel::sql_types::Text, _>(scope.instance)
    .get_result::<Count>(conn)
    .map(|c| c.n)
}

/// A ticket a source brought across before, as it stands here now.
#[derive(Debug, Clone, QueryableByName)]
pub struct ImportedTicket {
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub source_id: String,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    pub ticket_id: i32,
    /// Merged into another ticket here: closed to new replies for good.
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub merged: bool,
    /// Nobody has changed it here since the import last wrote it (see
    /// [`UNTOUCHED`]). Read without a lock: [`lock_imported_ticket`] asks
    /// again before anything is written.
    #[diesel(sql_type = diesel::sql_types::Bool)]
    pub untouched: bool,
    /// When the source last changed it, as of that write.
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>)]
    pub source_updated_at: Option<NaiveDateTime>,
}

/// Whether ticket `t` is as the import (map row `m`) last left it: its
/// `updated_at` no later than the map's `written_at`, and nothing in its
/// activity since then but import writes. The activity log catches what
/// leaves `updated_at` alone: a tag, watcher, file or link change, a merge
/// into it, a reorder.
const UNTOUCHED: &str = "(t.updated_at <= m.written_at AND NOT EXISTS ( \
     SELECT 1 FROM sync_actions s \
     WHERE s.workspace_id = m.workspace_id \
       AND s.groups @> ARRAY['ticket:' || t.id::text] \
       AND s.occurred_at > m.written_at \
       AND (s.actor_ref IS NULL OR s.actor_ref NOT LIKE 'import:%')))";

// sync-audit-only: read-only (the tickets a source brought, as they stand here)
/// Those of `source_ids` brought across before whose ticket is still in
/// the workspace, keyed by source id. A mapped ticket missing from the
/// answer was deleted here.
pub fn imported_tickets(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    source_ids: &[String],
) -> QueryResult<HashMap<String, ImportedTicket>> {
    if source_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<ImportedTicket> = diesel::sql_query(format!(
        "SELECT m.source_id, t.id AS ticket_id, \
                EXISTS (SELECT 1 FROM ticket_merges g WHERE g.ticket_id = t.id) AS merged, \
                {UNTOUCHED} AS untouched, m.source_updated_at \
         FROM import_source_ids m \
         JOIN tickets t ON t.id::text = m.target_id AND t.workspace_id = m.workspace_id \
         WHERE m.workspace_id = $1 AND m.source = $2 AND m.source_scope = $3 \
           AND m.entity = 'ticket' AND m.source_id = ANY($4)"
    ))
    .bind::<diesel::sql_types::Integer, _>(scope.workspace_id)
    .bind::<diesel::sql_types::Text, _>(scope.source)
    .bind::<diesel::sql_types::Text, _>(scope.instance)
    .bind::<diesel::sql_types::Array<diesel::sql_types::Text>, _>(source_ids)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| (r.source_id.clone(), r)).collect())
}

#[derive(QueryableByName)]
struct Locked {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    ticket_id: i32,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    untouched: bool,
}

// sync-audit-only: takes a row lock and reads, writes nothing
/// Lock the ticket `source_id` became (`FOR NO KEY UPDATE`, as every ticket
/// write does) for the rest of the transaction and say again whether it is
/// untouched, now that no one can change it until the import's write lands.
/// None when it is gone.
pub fn lock_imported_ticket(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    source_id: &str,
) -> QueryResult<Option<(i32, bool)>> {
    diesel::sql_query(format!(
        "SELECT t.id AS ticket_id, {UNTOUCHED} AS untouched \
         FROM tickets t \
         JOIN import_source_ids m ON t.id::text = m.target_id AND t.workspace_id = m.workspace_id \
         WHERE m.workspace_id = $1 AND m.source = $2 AND m.source_scope = $3 \
           AND m.entity = 'ticket' AND m.source_id = $4 \
         FOR NO KEY UPDATE OF t"
    ))
    .bind::<diesel::sql_types::Integer, _>(scope.workspace_id)
    .bind::<diesel::sql_types::Text, _>(scope.source)
    .bind::<diesel::sql_types::Text, _>(scope.instance)
    .bind::<diesel::sql_types::Text, _>(source_id)
    .get_result::<Locked>(conn)
    .optional()
    .map(|row| row.map(|r| (r.ticket_id, r.untouched)))
}

// sync-audit-only: import bookkeeping no client reads; the ticket write it follows is synced and audited
/// Record that the import has brought ticket `source_id` up to date with the
/// source as it was at `source_updated_at`. `written_at` is the `updated_at`
/// the import's own write left (None when it wrote nothing, which keeps
/// the old mark): a change anyone makes later is later than it.
pub fn touch_ticket(
    conn: &mut DbConnection,
    scope: Scope<'_>,
    source_id: &str,
    source_updated_at: NaiveDateTime,
    written_at: Option<NaiveDateTime>,
) -> QueryResult<()> {
    diesel::sql_query(
        "UPDATE import_source_ids \
         SET written_at = COALESCE($6, written_at), source_updated_at = $5 \
         WHERE workspace_id = $1 AND source = $2 AND source_scope = $3 \
           AND entity = 'ticket' AND source_id = $4",
    )
    .bind::<diesel::sql_types::Integer, _>(scope.workspace_id)
    .bind::<diesel::sql_types::Text, _>(scope.source)
    .bind::<diesel::sql_types::Text, _>(scope.instance)
    .bind::<diesel::sql_types::Text, _>(source_id)
    .bind::<diesel::sql_types::Timestamptz, _>(source_updated_at)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>, _>(written_at)
    .execute(conn)
    .map(|_| ())
}

// sync-audit-only: import bookkeeping no client reads, cleared with the account it names
/// Forget every mapping, in every workspace, to the account `user_uuid`:
/// the account is being purged, so a rerun brings the person across again
/// rather than finding a mapping to nothing. Run on the purge's bypass
/// connection, which sees every workspace's rows.
pub fn forget_account(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<usize> {
    diesel::delete(
        import_source_ids::table
            .filter(import_source_ids::entity.eq(Entity::Person.as_str()))
            .filter(import_source_ids::target_id.eq(user_uuid.to_string())),
    )
    .execute(conn)
}

// sync-audit-only: takes a lock and writes nothing
/// Hold, until the transaction ends, the lock that makes writes from one
/// source instance into this workspace one batch at a time. Two runs of the
/// same import (an operator's retry while the first still runs) then
/// interleave whole batches, each reading the map after the other's commit,
/// instead of both writing the same ticket.
pub fn lock_source(conn: &mut DbConnection, scope: Scope<'_>) -> QueryResult<()> {
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind::<diesel::sql_types::Text, _>(format!(
            "helpdesk-import:{}:{}:{}",
            scope.workspace_id, scope.source, scope.instance
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
            .filter(import_source_ids::workspace_id.eq(scope.workspace_id))
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
            workspace_id: scope.workspace_id,
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
