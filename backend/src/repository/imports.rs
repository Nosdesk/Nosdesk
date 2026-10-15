//! `import_jobs` CRUD. Companion to `services::imports`.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{ImportJob, ImportJobUpdate, NewImportJob};

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
