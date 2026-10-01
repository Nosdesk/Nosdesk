//! The queue of hard-deleted workspaces whose stored files are still to be
//! removed (`workspace_file_purges`). Platform-level: the workspaces it names no
//! longer exist, so it is read and written on an elevated connection.

use chrono::Utc;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::schema::workspace_file_purges;

/// Queue a hard-deleted workspace's stored files for removal. Idempotent.
// sync-audit-only: platform purge queue, which no client reads
pub fn queue(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<()> {
    diesel::insert_into(workspace_file_purges::table)
        .values(workspace_file_purges::deleted_workspace_id.eq(workspace_id))
        .on_conflict(workspace_file_purges::deleted_workspace_id)
        .do_nothing()
        .execute(conn)?;
    Ok(())
}

/// Workspaces whose stored files haven't been removed yet, oldest first.
pub fn pending(conn: &mut DbConnection) -> QueryResult<Vec<i32>> {
    workspace_file_purges::table
        .filter(workspace_file_purges::completed_at.is_null())
        .order(workspace_file_purges::queued_at.asc())
        .select(workspace_file_purges::deleted_workspace_id)
        .load(conn)
}

/// Record one attempt: complete when `error` is `None`, otherwise still pending
/// with the error kind, for the next run to retry.
// sync-audit-only: platform purge queue, which no client reads
pub fn record_attempt(
    conn: &mut DbConnection,
    workspace_id: i32,
    error: Option<&str>,
) -> QueryResult<()> {
    diesel::update(workspace_file_purges::table.find(workspace_id))
        .set((
            workspace_file_purges::attempts.eq(workspace_file_purges::attempts + 1),
            workspace_file_purges::last_error.eq(error),
            workspace_file_purges::completed_at.eq(error.is_none().then(Utc::now)),
        ))
        .execute(conn)?;
    Ok(())
}
