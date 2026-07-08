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
        .get_result(conn)
}

// sync-audit-only: workspace config row, see create
pub fn get(conn: &mut DbConnection, id: Uuid) -> QueryResult<ImportJob> {
    use crate::schema::import_jobs;
    import_jobs::table.find(id).first(conn)
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
        .get_result(conn)
}

// sync-audit-only: workspace config row, see create
pub fn list_recent(conn: &mut DbConnection, limit: i64) -> QueryResult<Vec<ImportJob>> {
    use crate::schema::import_jobs;
    import_jobs::table
        .order(import_jobs::created_at.desc())
        .limit(limit)
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
        .first(conn)
        .optional()
}
