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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use crate::models::NewBackupJob;

    fn minimal_job() -> NewBackupJob {
        NewBackupJob {
            job_type: "full".to_string(),
            status: "pending".to_string(),
            include_sensitive: false,
            created_by: None,
        }
    }

    #[test]
    fn create_and_get_backup_job() {
        let mut conn = setup_test_connection();
        let job = create_backup_job(&mut conn, minimal_job()).unwrap();

        let fetched = get_backup_job(&mut conn, job.id).unwrap();
        assert_eq!(fetched.job_type, "full");
        assert_eq!(fetched.status, "pending");
    }

    #[test]
    fn get_all_backup_jobs_test() {
        let mut conn = setup_test_connection();
        let j1 = create_backup_job(&mut conn, minimal_job()).unwrap();
        let j2 = create_backup_job(&mut conn, minimal_job()).unwrap();

        let all = get_all_backup_jobs(&mut conn).unwrap();
        let ids: Vec<Uuid> = all.iter().map(|j| j.id).collect();
        assert!(ids.contains(&j1.id));
        assert!(ids.contains(&j2.id));
    }

    #[test]
    fn delete_backup_job_test() {
        let mut conn = setup_test_connection();
        let job = create_backup_job(&mut conn, minimal_job()).unwrap();

        let count = delete_backup_job(&mut conn, job.id).unwrap();
        assert_eq!(count, 1);

        let result = get_backup_job(&mut conn, job.id);
        assert!(result.is_err());
    }
}
