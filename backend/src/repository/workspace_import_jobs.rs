//! Workspace import jobs: an export imported into an existing workspace
//! (`services::workspace_archive::jobs`). One live job per workspace (a
//! partial unique index); the job keeps the report and the accounts the
//! import created, which a reset removes.
//!
//! Every read and write filters by the workspace explicitly as well as
//! through row security, because the import's own transaction runs on the
//! owner role, which row security doesn't scope.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewWorkspaceImportJob, WorkspaceImportJob, WorkspaceImportJobChange};
use crate::schema::workspace_import_jobs::dsl as j;

/// The states a job is live in: one per workspace at a time.
pub const LIVE: &[&str] = &["uploaded", "checking", "previewed", "importing"];

/// The states a background run holds a job in.
pub const RUNNING: &[&str] = &["checking", "importing"];

// sync-audit-only: import job bookkeeping; no sync client subscribes to it, and its audit trigger records the change
/// Create a job. Fails on the one-live-job index when the workspace has one.
pub fn create(
    conn: &mut DbConnection,
    new: &NewWorkspaceImportJob,
) -> QueryResult<WorkspaceImportJob> {
    diesel::insert_into(j::workspace_import_jobs)
        .values(new)
        .returning(WorkspaceImportJob::as_returning())
        .get_result(conn)
}

/// The job `id` in `workspace_id`.
pub fn get(
    conn: &mut DbConnection,
    workspace_id: i32,
    id: Uuid,
) -> QueryResult<Option<WorkspaceImportJob>> {
    j::workspace_import_jobs
        .filter(j::id.eq(id))
        .filter(j::workspace_id.eq(workspace_id))
        .select(WorkspaceImportJob::as_select())
        .first(conn)
        .optional()
}

/// The job `id` in whichever workspace holds it, for a caller that knows
/// only the job (the CLI). Needs an elevated connection.
pub fn find(conn: &mut DbConnection, id: Uuid) -> QueryResult<Option<WorkspaceImportJob>> {
    j::workspace_import_jobs
        .filter(j::id.eq(id))
        .select(WorkspaceImportJob::as_select())
        .first(conn)
        .optional()
}

/// [`get`], locking the row for the rest of the transaction.
pub fn get_for_update(
    conn: &mut DbConnection,
    workspace_id: i32,
    id: Uuid,
) -> QueryResult<Option<WorkspaceImportJob>> {
    j::workspace_import_jobs
        .filter(j::id.eq(id))
        .filter(j::workspace_id.eq(workspace_id))
        .select(WorkspaceImportJob::as_select())
        .for_update()
        .first(conn)
        .optional()
}

/// The workspace's live job, if it has one.
pub fn live(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<Option<WorkspaceImportJob>> {
    j::workspace_import_jobs
        .filter(j::workspace_id.eq(workspace_id))
        .filter(j::state.eq_any(LIVE))
        .select(WorkspaceImportJob::as_select())
        .first(conn)
        .optional()
}

/// The workspace's jobs, newest first.
pub fn recent(
    conn: &mut DbConnection,
    workspace_id: i32,
    limit: i64,
) -> QueryResult<Vec<WorkspaceImportJob>> {
    j::workspace_import_jobs
        .filter(j::workspace_id.eq(workspace_id))
        .order(j::created_at.desc())
        .limit(limit)
        .select(WorkspaceImportJob::as_select())
        .load(conn)
}

// sync-audit-only: import job bookkeeping, audited by the table's trigger
/// Apply `change` to the job when it is in one of `from`. Returns the job
/// as changed, or `None` when it wasn't in one of them (or doesn't exist).
pub fn transition(
    conn: &mut DbConnection,
    workspace_id: i32,
    id: Uuid,
    from: &[&str],
    change: WorkspaceImportJobChange,
) -> QueryResult<Option<WorkspaceImportJob>> {
    let change = WorkspaceImportJobChange {
        updated_at: Some(change.updated_at.unwrap_or_else(Utc::now)),
        ..change
    };
    diesel::update(
        j::workspace_import_jobs
            .filter(j::id.eq(id))
            .filter(j::workspace_id.eq(workspace_id))
            .filter(j::state.eq_any(from)),
    )
    .set(&change)
    .returning(WorkspaceImportJob::as_returning())
    .get_result(conn)
    .optional()
}

// sync-audit-only: marks a running job alive; the audit trigger skips an update of updated_at, stage and files_restored only
/// Mark the job alive while it is in `state`, and where it is (`stage`,
/// files restored so far). Returns 0 once the job has moved on.
pub fn heartbeat(
    conn: &mut DbConnection,
    id: Uuid,
    state: &str,
    stage: Option<&str>,
    files_restored: Option<i32>,
) -> QueryResult<usize> {
    let target = j::workspace_import_jobs
        .filter(j::id.eq(id))
        .filter(j::state.eq(state));
    let now = Utc::now();
    match (stage, files_restored) {
        (Some(stage), Some(files)) => diesel::update(target)
            .set((
                j::updated_at.eq(now),
                j::stage.eq(Some(stage)),
                j::files_restored.eq(files),
            ))
            .execute(conn),
        (Some(stage), None) => diesel::update(target)
            .set((j::updated_at.eq(now), j::stage.eq(Some(stage))))
            .execute(conn),
        (None, Some(files)) => diesel::update(target)
            .set((j::updated_at.eq(now), j::files_restored.eq(files)))
            .execute(conn),
        (None, None) => diesel::update(target)
            .set(j::updated_at.eq(now))
            .execute(conn),
    }
}

// sync-audit-only: import job bookkeeping, audited by the table's trigger
/// Delete the job.
pub fn delete(conn: &mut DbConnection, workspace_id: i32, id: Uuid) -> QueryResult<usize> {
    diesel::delete(
        j::workspace_import_jobs
            .filter(j::id.eq(id))
            .filter(j::workspace_id.eq(workspace_id)),
    )
    .execute(conn)
}

/// Jobs in a running state whose heartbeat stopped before `before`, across
/// every workspace, as `(workspace_id, id)`. Call it elevated.
pub fn stalled(
    conn: &mut DbConnection,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, Uuid)>> {
    j::workspace_import_jobs
        .filter(j::state.eq_any(RUNNING))
        .filter(j::updated_at.lt(before))
        .order(j::updated_at.asc())
        .limit(limit)
        .select((j::workspace_id, j::id))
        .load(conn)
}

/// Jobs the 30-day prune is due on, across every workspace, as
/// `(workspace_id, id)`: not yet pruned, not running, and last changed before
/// `before`. Call it elevated.
pub fn prune_due(
    conn: &mut DbConnection,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, Uuid)>> {
    j::workspace_import_jobs
        .filter(j::pruned_at.is_null())
        .filter(diesel::dsl::not(j::state.eq_any(RUNNING)))
        .filter(j::updated_at.lt(before))
        .order(j::updated_at.asc())
        .limit(limit)
        .select((j::workspace_id, j::id))
        .load(conn)
}

/// Of `people`, those with a membership (any status) in a workspace other
/// than `workspace_id`.
pub fn members_elsewhere(
    conn: &mut DbConnection,
    workspace_id: i32,
    people: &[Uuid],
) -> QueryResult<Vec<Uuid>> {
    use crate::schema::workspace_members::dsl as m;
    if people.is_empty() {
        return Ok(Vec::new());
    }
    // members-any-status: someone a reset removes keeps their account while any other workspace holds them
    m::workspace_members
        .filter(m::user_uuid.eq_any(people))
        .filter(m::workspace_id.ne(workspace_id))
        .select(m::user_uuid)
        .distinct()
        .load(conn)
}

// sync-audit-only: a reset removing the accounts its import created, in the reset's own transaction, which bumps the workspace's data epoch so every client starts over
/// Remove `people`, the accounts an import into `workspace_id` created,
/// after a reset has cleared the workspace's content: each loses their
/// membership here, and an account no other workspace holds is purged
/// (`users::purge_user`, which handles every reference to it). Returns
/// `(purged, kept)`. Run it on the owner role, in the reset's transaction.
pub fn remove_created_people(
    conn: &mut DbConnection,
    workspace_id: i32,
    people: &[Uuid],
) -> QueryResult<(usize, usize)> {
    use crate::schema::workspace_members::dsl as m;
    if people.is_empty() {
        return Ok((0, 0));
    }
    let elsewhere = members_elsewhere(conn, workspace_id, people)?;
    // members-any-status: the import's people leave this workspace whatever their membership's status
    diesel::delete(
        m::workspace_members
            .filter(m::workspace_id.eq(workspace_id))
            .filter(m::user_uuid.eq_any(people)),
    )
    .execute(conn)?;
    let mut purged = 0;
    for person in people.iter().filter(|p| !elsewhere.contains(p)) {
        // The import's own accounts only: the retention purge's primitive,
        // since nothing else may still name them once the content is gone.
        if crate::repository::users::purge_user(person, conn, None)? > 0 {
            purged += 1;
        }
    }
    Ok((purged, elsewhere.len()))
}
