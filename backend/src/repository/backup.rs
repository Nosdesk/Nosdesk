use diesel::prelude::*;
use uuid::Uuid;
use crate::db::DbConnection;
use crate::models::{BackupJob, NewBackupJob, BackupJobUpdate};
use crate::schema::backup_jobs;

/// Create a new backup job record
pub fn create_backup_job(
    conn: &mut DbConnection,
    new_job: NewBackupJob,
) -> QueryResult<BackupJob> {
    diesel::insert_into(backup_jobs::table)
        .values(&new_job)
        .get_result(conn)
}

/// Get a backup job by ID
pub fn get_backup_job(
    conn: &mut DbConnection,
    job_id: Uuid,
) -> QueryResult<BackupJob> {
    backup_jobs::table.find(job_id).first(conn)
}

/// Get all backup jobs ordered by creation date (most recent first)
pub fn get_all_backup_jobs(
    conn: &mut DbConnection,
) -> QueryResult<Vec<BackupJob>> {
    backup_jobs::table
        .order(backup_jobs::created_at.desc())
        .load(conn)
}

/// Update a backup job
pub fn update_backup_job(
    conn: &mut DbConnection,
    job_id: Uuid,
    update: BackupJobUpdate,
) -> QueryResult<BackupJob> {
    diesel::update(backup_jobs::table.find(job_id))
        .set(&update)
        .get_result(conn)
}

/// Delete a backup job
pub fn delete_backup_job(
    conn: &mut DbConnection,
    job_id: Uuid,
) -> QueryResult<usize> {
    diesel::delete(backup_jobs::table.find(job_id))
        .execute(conn)
}

