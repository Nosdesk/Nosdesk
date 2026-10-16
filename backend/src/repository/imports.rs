//! `import_jobs` CRUD. Companion to `services::imports`.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    ImportJob, ImportJobUpdate, ImportValueAnswer, NewImportJob, NewImportValueAnswer,
};

// sync-audit-only: admin bulk-import audit log, not a sync aggregate. Lives behind admin auth and the audit_log trigger.
pub fn create(conn: &mut DbConnection, new: NewImportJob) -> QueryResult<ImportJob> {
    use crate::schema::import_jobs;
    diesel::insert_into(import_jobs::table)
        .values(&new)
        .returning(ImportJob::as_returning())
        .get_result(conn)
}

// sync-audit-only: workspace config row, see create
pub fn get(conn: &mut DbConnection, id: Uuid) -> QueryResult<ImportJob> {
    use crate::schema::import_jobs;
    import_jobs::table
        .find(id)
        .select(ImportJob::as_select())
        .first(conn)
}

/// A job's stored row results, `None` until its first dry run.
// sync-audit-only: workspace config row, see create
pub fn row_results(conn: &mut DbConnection, id: Uuid) -> QueryResult<Option<serde_json::Value>> {
    use crate::schema::import_jobs;
    import_jobs::table
        .find(id)
        .select(import_jobs::row_results)
        .first(conn)
}

// sync-audit-only: workspace config row, see create
pub fn update(
    conn: &mut DbConnection,
    id: Uuid,
    mut patch: ImportJobUpdate,
) -> QueryResult<ImportJob> {
    use crate::schema::import_jobs;
    patch.updated_at = Some(chrono::Utc::now());
    diesel::update(import_jobs::table.find(id))
        .set(&patch)
        .returning(ImportJob::as_returning())
        .get_result(conn)
}

/// Remove a job row. The caller decides whether the job may go.
// sync-audit-only: workspace config row, see create
pub fn delete(conn: &mut DbConnection, id: Uuid) -> QueryResult<usize> {
    use crate::schema::import_jobs;
    diesel::delete(import_jobs::table.find(id)).execute(conn)
}

// sync-audit-only: workspace config row, see create
pub fn list_recent(conn: &mut DbConnection, limit: i64) -> QueryResult<Vec<ImportJob>> {
    use crate::schema::import_jobs;
    import_jobs::table
        .order(import_jobs::created_at.desc())
        .limit(limit)
        .select(ImportJob::as_select())
        .load(conn)
}

/// Convenience: the most recent job a given user has touched.
/// Used to resume a tab the admin navigated away from.
// sync-audit-only: workspace config row, see create
pub fn latest_for_user(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<Option<ImportJob>> {
    use crate::schema::import_jobs;
    import_jobs::table
        .filter(import_jobs::created_by.eq(user_uuid))
        .order(import_jobs::created_at.desc())
        .select(ImportJob::as_select())
        .first(conn)
        .optional()
}

/// The workspace's most recently touched job of `job_type` with a stored
/// mapping, whatever its status, so a file fixed and uploaded again keeps
/// the admin's mapping.
// sync-audit-only: workspace config row, see create
pub fn latest_with_mapping(
    conn: &mut DbConnection,
    job_type: &str,
) -> QueryResult<Option<ImportJob>> {
    use crate::schema::import_jobs;
    import_jobs::table
        .filter(import_jobs::job_type.eq(job_type))
        .filter(import_jobs::mapping.is_not_null())
        .order(import_jobs::updated_at.desc())
        .select(ImportJob::as_select())
        .first(conn)
        .optional()
}

/// Advisory-lock key spaces for import commits (two-int form, so they never
/// meet the single-bigint keys used elsewhere).
const JOB_LOCK_SPACE: i32 = 0x494D_504A;
const COMMIT_LOCK_SPACE: i32 = 0x494D_5043;
const TYPE_LOCK_SPACE: i32 = 0x494D_5054;

/// A job's lock key: its id folded to a non-negative int.
fn job_lock_key(job_id: Uuid) -> i32 {
    let b = job_id.as_bytes();
    let folded = b
        .chunks(4)
        .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .fold(0u32, |acc, x| acc ^ x);
    (folded & 0x7fff_ffff) as i32
}

/// A workspace and import type's lock key.
fn type_lock_key(workspace_id: i32, job_type: &str) -> (i32, i32) {
    let code = match job_type {
        "assets" => 1,
        "users" => 2,
        "tickets" => 3,
        _ => 0,
    };
    (TYPE_LOCK_SPACE + code, workspace_id)
}

#[derive(QueryableByName)]
struct Flag {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    flag: bool,
}

fn try_xact_lock(conn: &mut DbConnection, space: i32, key: i32) -> QueryResult<bool> {
    use diesel::sql_types::Integer;
    diesel::sql_query("SELECT pg_try_advisory_xact_lock($1, $2) AS flag")
        .bind::<Integer, _>(space)
        .bind::<Integer, _>(key)
        .get_result::<Flag>(conn)
        .map(|f| f.flag)
}

/// Take, for the rest of the transaction, the lock on imports of
/// `job_type` into the workspace, without waiting; `false` while another
/// commit of that type runs there, so two can't write over the same
/// records at once.
// sync-audit-only: takes advisory locks, writes nothing
pub fn try_lock_type(
    conn: &mut DbConnection,
    workspace_id: i32,
    job_type: &str,
) -> QueryResult<bool> {
    let (space, key) = type_lock_key(workspace_id, job_type);
    try_xact_lock(conn, space, key)
}

/// Take the job's lock for the rest of the transaction, without waiting;
/// `false` while its commit runs. The commit holds it, and so does a
/// change to the job's file or mapping, so one can't land under the other.
// sync-audit-only: takes advisory locks, writes nothing
pub fn try_lock_job(conn: &mut DbConnection, job_id: Uuid) -> QueryResult<bool> {
    try_xact_lock(conn, JOB_LOCK_SPACE, job_lock_key(job_id))
}

/// Mark, for the rest of the transaction, that the job's commit runs,
/// without waiting; `false` when its commit already runs.
// sync-audit-only: takes advisory locks, writes nothing
pub fn try_mark_commit(conn: &mut DbConnection, job_id: Uuid) -> QueryResult<bool> {
    try_xact_lock(conn, COMMIT_LOCK_SPACE, job_lock_key(job_id))
}

/// Whether another transaction holds the job's commit mark
/// ([`try_mark_commit`]), without taking it: the job's commit is running.
// sync-audit-only: reads pg_locks, writes nothing
pub fn commit_running(conn: &mut DbConnection, job_id: Uuid) -> QueryResult<bool> {
    use diesel::sql_types::Integer;
    diesel::sql_query(
        "SELECT EXISTS (
             SELECT 1 FROM pg_locks
             WHERE locktype = 'advisory'
               AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
               AND classid = $1::oid AND objid = $2::oid AND objsubid = 2
               AND granted AND pid <> pg_backend_pid()
         ) AS flag",
    )
    .bind::<Integer, _>(COMMIT_LOCK_SPACE)
    .bind::<Integer, _>(job_lock_key(job_id))
    .get_result::<Flag>(conn)
    .map(|f| f.flag)
}

/// Save answers for the next import of their type, replacing any answer
/// already saved for the same field, context and value.
// sync-audit-only: workspace import memory, admin-only and audited by tr_audit_import_value_answers
pub fn upsert_value_answers(
    conn: &mut DbConnection,
    answers: &[NewImportValueAnswer],
) -> QueryResult<usize> {
    use crate::schema::import_value_answers::dsl::*;
    use diesel::upsert::{excluded, on_constraint};
    if answers.is_empty() {
        return Ok(0);
    }
    diesel::insert_into(import_value_answers)
        .values(answers)
        .on_conflict(on_constraint("import_value_answers_key"))
        .do_update()
        .set((
            answer.eq(excluded(answer)),
            answered_by.eq(excluded(answered_by)),
            job_id.eq(excluded(job_id)),
            source_text.eq(excluded(source_text)),
            created.eq(excluded(created)),
            updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)
}

/// Every saved answer for imports of `job_type`, by field, context and value.
// sync-audit-only: read-only
pub fn list_value_answers(
    conn: &mut DbConnection,
    job_type: &str,
) -> QueryResult<Vec<ImportValueAnswer>> {
    use crate::schema::import_value_answers::dsl as a;
    a::import_value_answers
        .filter(a::job_type.eq(job_type))
        .order((a::field, a::context, a::normalized_value))
        .select(ImportValueAnswer::as_select())
        .load(conn)
}

/// Forget one saved answer.
// sync-audit-only: workspace import memory, see upsert_value_answers
pub fn delete_value_answer(conn: &mut DbConnection, id: Uuid) -> QueryResult<usize> {
    use crate::schema::import_value_answers::dsl as a;
    diesel::delete(a::import_value_answers.find(id)).execute(conn)
}

/// Forget every saved answer for imports of `job_type`.
// sync-audit-only: workspace import memory, see upsert_value_answers
pub fn delete_value_answers(conn: &mut DbConnection, job_type: &str) -> QueryResult<usize> {
    use crate::schema::import_value_answers::dsl as a;
    diesel::delete(a::import_value_answers.filter(a::job_type.eq(job_type))).execute(conn)
}
