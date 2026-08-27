//! Self-serve workspace export jobs (Owner-gated). Tracks one export request,
//! its storage-backed artifact, and a bounded download window. All access is
//! isolated by `workspace_id` (RLS for the tenant-facing reads; an explicit
//! predicate on `get_owned` so a guessed id from another tenant is a clean miss).

use crate::db::DbConnection;
use crate::models::{NewWorkspaceExportJob, WorkspaceExportJob, WorkspaceExportJobUpdate};
use crate::schema::workspace_export_jobs;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use uuid::Uuid;

// sync-audit-only: Operational / bespoke tables
/// Create an export job (status defaults to `pending`).
pub fn create(
    conn: &mut DbConnection,
    new: NewWorkspaceExportJob,
) -> QueryResult<WorkspaceExportJob> {
    diesel::insert_into(workspace_export_jobs::table)
        .values(&new)
        .get_result(conn)
}

/// Fetch a job by id, scoped to a workspace. The `workspace_id` predicate is the
/// isolation for reads that run through the BYPASSRLS role.
pub fn get_owned(
    conn: &mut DbConnection,
    id: Uuid,
    workspace_id: i32,
) -> QueryResult<Option<WorkspaceExportJob>> {
    workspace_export_jobs::table
        .filter(workspace_export_jobs::id.eq(id))
        .filter(workspace_export_jobs::workspace_id.eq(workspace_id))
        .first(conn)
        .optional()
}

/// Whether the workspace has an in-flight (pending/processing) export.
pub fn has_active(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        workspace_export_jobs::table
            .filter(workspace_export_jobs::workspace_id.eq(workspace_id))
            .filter(workspace_export_jobs::status.eq_any(vec!["pending", "processing"])),
    ))
    .get_result(conn)
}

/// The most recent export job for a workspace (any status), so the UI can show
/// an in-flight or ready export after a reload rather than losing track of it.
pub fn latest_for_workspace(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<Option<WorkspaceExportJob>> {
    workspace_export_jobs::table
        .filter(workspace_export_jobs::workspace_id.eq(workspace_id))
        .order(workspace_export_jobs::created_at.desc())
        .first(conn)
        .optional()
}

/// The most recent completed export's `created_at` (drives the per-day limit).
pub fn last_completed_at(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<Option<NaiveDateTime>> {
    workspace_export_jobs::table
        .filter(workspace_export_jobs::workspace_id.eq(workspace_id))
        .filter(workspace_export_jobs::status.eq("completed"))
        .order(workspace_export_jobs::created_at.desc())
        .select(workspace_export_jobs::created_at)
        .first(conn)
        .optional()
}

// sync-audit-only: Operational / bespoke tables
/// Apply a partial update.
pub fn update(
    conn: &mut DbConnection,
    id: Uuid,
    upd: WorkspaceExportJobUpdate,
) -> QueryResult<WorkspaceExportJob> {
    diesel::update(workspace_export_jobs::table.find(id))
        .set(&upd)
        .get_result(conn)
}

/// Completed jobs whose download window has passed (for artifact cleanup).
pub fn list_expired(
    conn: &mut DbConnection,
    now: NaiveDateTime,
) -> QueryResult<Vec<WorkspaceExportJob>> {
    workspace_export_jobs::table
        .filter(workspace_export_jobs::status.eq("completed"))
        .filter(workspace_export_jobs::expires_at.lt(now))
        .load(conn)
}

// sync-audit-only: Operational / bespoke tables
/// Fail pending/processing jobs older than `cutoff` (crash / stuck recovery), so
/// the poller sees a terminal state rather than a job stranded at `processing`.
pub fn fail_stale(conn: &mut DbConnection, cutoff: NaiveDateTime) -> QueryResult<usize> {
    diesel::update(
        workspace_export_jobs::table
            .filter(workspace_export_jobs::status.eq_any(vec!["pending", "processing"]))
            .filter(workspace_export_jobs::created_at.lt(cutoff)),
    )
    .set((
        workspace_export_jobs::status.eq("failed"),
        workspace_export_jobs::error_message.eq(Some("Export timed out".to_string())),
    ))
    .execute(conn)
}

// sync-audit-only: Operational / bespoke tables
/// Delete a job row (after its artifact is removed from storage).
pub fn delete(conn: &mut DbConnection, id: Uuid) -> QueryResult<usize> {
    diesel::delete(workspace_export_jobs::table.find(id)).execute(conn)
}
