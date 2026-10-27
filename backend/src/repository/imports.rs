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

/// A job, its row locked `FOR NO KEY UPDATE` for the rest of the
/// transaction: a change to the job waits, and nothing that references it
/// is blocked.
// sync-audit-only: read-only (takes a row lock, writes nothing)
pub fn get_for_update(conn: &mut DbConnection, id: Uuid) -> QueryResult<ImportJob> {
    use crate::schema::import_jobs;
    import_jobs::table
        .find(id)
        .select(ImportJob::as_select())
        .for_no_key_update()
        .first(conn)
}

/// Record that Remove imported assets ran on the job: when, by whom and
/// what it did. Leaves `updated_at` alone, like the prune, so the job keeps
/// its place in header memory. The job's audit entry carries the result.
// sync-audit-only: admin bulk-import audit log, see create
pub fn record_removal(
    conn: &mut DbConnection,
    id: Uuid,
    by: Uuid,
    result: serde_json::Value,
) -> QueryResult<ImportJob> {
    use crate::schema::import_jobs::dsl as j;
    diesel::update(j::import_jobs.find(id))
        .set((
            j::removed_at.eq(Some(chrono::Utc::now())),
            j::removed_by.eq(Some(by)),
            j::remove_result.eq(Some(result)),
        ))
        .returning(ImportJob::as_returning())
        .get_result(conn)
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

/// A job as Recent imports reads it: no file columns beyond the sheet name,
/// no rows, and the outcome counts lifted out of the summary.
#[derive(Debug, Clone, Queryable)]
pub struct ImportJobListRow {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub filename: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub pruned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sheet: Option<String>,
    pub would_create: Option<i32>,
    pub would_update: Option<i32>,
    pub unchanged: Option<i32>,
    pub skipped: Option<i32>,
    /// From Remove imported assets, once it ran: assets deleted, of those
    /// created.
    pub removed: Option<i32>,
    pub removed_of: Option<i32>,
    pub removed_kept: Option<i32>,
}

/// The workspace's newest jobs that reached Map (the upload read, whatever
/// happened since), newest first. A refused upload or a failed commit
/// isn't listed.
// sync-audit-only: read-only
pub fn list_reached_map(conn: &mut DbConnection, limit: i64) -> QueryResult<Vec<ImportJobListRow>> {
    use crate::schema::import_jobs::dsl as j;
    use diesel::dsl::sql;
    use diesel::sql_types::{Integer, Nullable, Text};
    j::import_jobs
        .filter(j::source.is_not_null())
        .filter(j::status.eq_any(["parsed", "checking", "dry_run_done", "committing", "done"]))
        .order((j::created_at.desc(), j::id.desc()))
        .limit(limit)
        .select((
            j::id,
            j::job_type,
            j::status,
            j::filename,
            j::created_by,
            j::created_at,
            j::updated_at,
            j::completed_at,
            j::pruned_at,
            sql::<Nullable<Text>>("import_jobs.source->>'sheet'"),
            sql::<Nullable<Integer>>("(import_jobs.summary->>'would_create')::int"),
            sql::<Nullable<Integer>>("(import_jobs.summary->>'would_update')::int"),
            sql::<Nullable<Integer>>("(import_jobs.summary->>'unchanged')::int"),
            sql::<Nullable<Integer>>("(import_jobs.summary->>'rows_with_errors')::int"),
            sql::<Nullable<Integer>>("(import_jobs.remove_result->>'deleted')::int"),
            sql::<Nullable<Integer>>("(import_jobs.remove_result->>'created')::int"),
            sql::<Nullable<Integer>>("jsonb_array_length(import_jobs.remove_result->'kept')"),
        ))
        .load(conn)
}

/// A job made from one upload, as Import the next sheet reads it.
#[derive(Debug, Clone, Queryable)]
pub struct UploadJobRow {
    pub id: Uuid,
    pub status: String,
    pub sheet: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub pruned_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Every job made from the upload first sent to job `root`: `root` itself
/// and each job Import the next sheet made from it, with the sheet each
/// one reads. Oldest first.
// sync-audit-only: read-only
pub fn jobs_of_upload(conn: &mut DbConnection, root: Uuid) -> QueryResult<Vec<UploadJobRow>> {
    use crate::schema::import_jobs::dsl as j;
    use diesel::dsl::sql;
    use diesel::sql_types::{Nullable, Text};
    j::import_jobs
        .filter(j::id.eq(root).or(j::upload_of.eq(root)))
        .order((j::created_at.asc(), j::id.asc()))
        .select((
            j::id,
            j::status,
            sql::<Nullable<Text>>("import_jobs.source->>'sheet'"),
            j::updated_at,
            j::pruned_at,
        ))
        .load(conn)
}

/// The records a committed job created and updated
/// (`services::imports::RecordIds`), without the rest of the job; `None`
/// when there is no such job here or it wrote nothing.
// sync-audit-only: read-only
pub fn record_ids(
    conn: &mut DbConnection,
    id: Uuid,
    job_type: &str,
) -> QueryResult<Option<serde_json::Value>> {
    use crate::schema::import_jobs::dsl as j;
    Ok(j::import_jobs
        .find(id)
        .filter(j::job_type.eq(job_type))
        .select(j::record_ids)
        .first::<Option<serde_json::Value>>(conn)
        .optional()?
        .flatten())
}

/// Jobs, across every workspace, whose detail the prune drops: committed
/// before `cutoff`, or drafts last changed before it, not pruned yet. At
/// most `limit`, oldest first, as (workspace, job).
// sync-audit-only: read-only
pub fn prune_due(
    conn: &mut DbConnection,
    cutoff: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, Uuid)>> {
    use crate::schema::import_jobs::dsl as j;
    j::import_jobs
        .filter(j::pruned_at.is_null())
        .filter(
            j::status
                .eq("done")
                .and(j::completed_at.lt(cutoff))
                .or(j::status
                    .eq_any(["parsed", "checking", "dry_run_done"])
                    .and(j::updated_at.lt(cutoff))),
        )
        .order(j::updated_at.asc())
        .limit(limit)
        .select((j::workspace_id, j::id))
        .load(conn)
}

/// Jobs, across every workspace, nothing was imported from and that never
/// reached Map or whose commit failed, last changed before `cutoff`: the
/// prune deletes them with their files. At most `limit`, as
/// (workspace, job).
// sync-audit-only: read-only
pub fn discard_due(
    conn: &mut DbConnection,
    cutoff: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, Uuid)>> {
    use crate::schema::import_jobs::dsl as j;
    j::import_jobs
        .filter(j::status.eq("failed").or(j::source.is_null()))
        .filter(j::status.ne("done"))
        .filter(j::record_ids.is_null())
        .filter(j::records_committed.is_null())
        .filter(j::updated_at.lt(cutoff))
        .order(j::updated_at.asc())
        .limit(limit)
        .select((j::workspace_id, j::id))
        .load(conn)
}

/// Of `ids`, the jobs whose files are still kept: those with a row the
/// prune hasn't reached. The sweep removes the files of the rest.
// sync-audit-only: read-only
pub fn keeping_files(conn: &mut DbConnection, ids: &[Uuid]) -> QueryResult<Vec<Uuid>> {
    use crate::schema::import_jobs::dsl as j;
    j::import_jobs
        .filter(j::id.eq_any(ids))
        .filter(j::pruned_at.is_null())
        .select(j::id)
        .load(conn)
}

/// Drop a job's per-row results and answers, and cut its summary and
/// source to what the prune keeps (`services::imports::history`), marking
/// it pruned. Only a job [`prune_due`] still picks at `cutoff`, read again
/// under a row lock, so a draft changed since the scan is left alone.
/// Leaves `updated_at` alone, so the job keeps its place in header memory
/// and its age. 0 when there was nothing to prune.
// sync-audit-only: admin bulk-import audit log, see create
pub fn prune(
    conn: &mut DbConnection,
    id: Uuid,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> QueryResult<usize> {
    use crate::schema::import_jobs::dsl as j;
    use crate::services::imports::history;
    let Some((job_type, summary, source)) = j::import_jobs
        .find(id)
        .filter(j::pruned_at.is_null())
        .filter(
            j::status
                .eq("done")
                .and(j::completed_at.lt(cutoff))
                .or(j::status
                    .eq_any(["parsed", "checking", "dry_run_done"])
                    .and(j::updated_at.lt(cutoff))),
        )
        .select((j::job_type, j::summary, j::source))
        .for_no_key_update()
        .first::<(String, Option<serde_json::Value>, Option<serde_json::Value>)>(conn)
        .optional()?
    else {
        return Ok(0);
    };
    let (summary, source) = if job_type == crate::services::imports::documentation::JOB_TYPE {
        // The tree and the counts stay; the pages' detail goes with the
        // row results.
        (
            crate::services::imports::documentation::pruned_summary(summary),
            source,
        )
    } else {
        let summary = history::pruned_summary(summary);
        // A source that doesn't read is kept whole: it names the file's
        // columns.
        (summary, history::pruned_source(source.clone()).or(source))
    };
    diesel::update(j::import_jobs.find(id))
        .set((
            j::row_results.eq(None::<serde_json::Value>),
            j::answers.eq(None::<serde_json::Value>),
            j::summary.eq(summary),
            j::source.eq(source),
            j::pruned_at.eq(Some(chrono::Utc::now())),
        ))
        .execute(conn)
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
        "documentation" => 4,
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

/// Mark a background run on the job as alive: `updated_at` now and, when
/// given, the pages written so far. Only while the job is still in
/// `status`; 0 when it has moved on (finished, or reconciled as stopped).
/// The audit trigger skips an update that changes only these two columns
/// (`tr_audit_import_jobs_upd`), so the beats leave no audit rows.
// sync-audit-only: admin bulk-import audit log, see create
pub fn heartbeat(
    conn: &mut DbConnection,
    id: Uuid,
    status: &str,
    progress: Option<i32>,
) -> QueryResult<usize> {
    use crate::schema::import_jobs::dsl as j;
    let now = chrono::Utc::now();
    match progress {
        Some(progress) => diesel::update(j::import_jobs.find(id).filter(j::status.eq(status)))
            .set((
                j::updated_at.eq(now),
                j::records_committed.eq(Some(progress)),
            ))
            .execute(conn),
        None => diesel::update(j::import_jobs.find(id).filter(j::status.eq(status)))
            .set(j::updated_at.eq(now))
            .execute(conn),
    }
}

/// The record of every committed documentation import in the workspace
/// (`services::imports::documentation::CommitRecord`), for spotting pages
/// imported before.
// sync-audit-only: read-only
pub fn documentation_records(conn: &mut DbConnection) -> QueryResult<Vec<serde_json::Value>> {
    use crate::schema::import_jobs::dsl as j;
    Ok(j::import_jobs
        .filter(j::job_type.eq(crate::services::imports::documentation::JOB_TYPE))
        .filter(j::status.eq("done"))
        .filter(j::record_ids.is_not_null())
        .select(j::record_ids)
        .load::<Option<serde_json::Value>>(conn)?
        .into_iter()
        .flatten()
        .collect())
}

/// Whether a documentation import other than `except` is committing in
/// the workspace: one runs at a time.
// sync-audit-only: read-only
pub fn other_documentation_commit(conn: &mut DbConnection, except: Uuid) -> QueryResult<bool> {
    use crate::schema::import_jobs::dsl as j;
    diesel::select(diesel::dsl::exists(
        j::import_jobs
            .filter(j::job_type.eq(crate::services::imports::documentation::JOB_TYPE))
            .filter(j::status.eq("committing"))
            .filter(j::id.ne(except)),
    ))
    .get_result(conn)
}

/// Documentation jobs, across every workspace, whose background check or
/// commit hasn't marked itself alive since `cutoff`: the process running it
/// died. At most `limit`, as (workspace, job).
// sync-audit-only: read-only
pub fn stalled_documentation_jobs(
    conn: &mut DbConnection,
    cutoff: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, Uuid)>> {
    use crate::schema::import_jobs::dsl as j;
    j::import_jobs
        .filter(j::job_type.eq(crate::services::imports::documentation::JOB_TYPE))
        .filter(j::status.eq_any(["checking", "committing"]))
        .filter(j::updated_at.lt(cutoff))
        .order(j::updated_at.asc())
        .limit(limit)
        .select((j::workspace_id, j::id))
        .load(conn)
}
