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

/// Get backup jobs by type (export or restore)
pub fn get_backup_jobs_by_type(
    conn: &mut DbConnection,
    job_type: &str,
) -> QueryResult<Vec<BackupJob>> {
    backup_jobs::table
        .filter(backup_jobs::job_type.eq(job_type))
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

/// Get pending or processing jobs (for cleanup/monitoring)
pub fn get_active_backup_jobs(
    conn: &mut DbConnection,
) -> QueryResult<Vec<BackupJob>> {
    backup_jobs::table
        .filter(
            backup_jobs::status.eq("pending")
                .or(backup_jobs::status.eq("processing"))
        )
        .order(backup_jobs::created_at.desc())
        .load(conn)
}

/// Get completed export jobs with files (for listing available backups)
pub fn get_completed_exports(
    conn: &mut DbConnection,
) -> QueryResult<Vec<BackupJob>> {
    backup_jobs::table
        .filter(backup_jobs::job_type.eq("export"))
        .filter(backup_jobs::status.eq("completed"))
        .filter(backup_jobs::file_path.is_not_null())
        .order(backup_jobs::created_at.desc())
        .load(conn)
}
