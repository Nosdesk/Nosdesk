use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

// ── Workspace import jobs (an export imported into an existing workspace) ──

#[derive(Debug, Clone, Serialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::workspace_import_jobs)]
pub struct WorkspaceImportJob {
    pub id: Uuid,
    pub workspace_id: i32,
    pub runner: String,
    pub requested_by: Option<Uuid>,
    pub state: String,
    pub stage: Option<String>,
    pub archive_key: Option<String>,
    pub archive_name: Option<String>,
    pub archive_size: i64,
    pub sealed: bool,
    pub regenerate_uuids: bool,
    pub report: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub created_people: Vec<Option<Uuid>>,
    pub files_restored: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub checked_at: Option<DateTime<Utc>>,
    pub confirmed_by: Option<Uuid>,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub applied_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub reset_by: Option<Uuid>,
    pub reset_at: Option<DateTime<Utc>>,
    pub pruned_at: Option<DateTime<Utc>>,
}

impl WorkspaceImportJob {
    /// The accounts the import created.
    pub fn created(&self) -> Vec<Uuid> {
        self.created_people.iter().flatten().copied().collect()
    }
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::workspace_import_jobs)]
pub struct NewWorkspaceImportJob {
    pub id: Uuid,
    pub workspace_id: i32,
    pub runner: String,
    pub requested_by: Option<Uuid>,
    pub archive_name: Option<String>,
    pub archive_size: i64,
    pub sealed: bool,
}

/// A change to a job. `None` leaves a column as it is; `Some(None)` clears a
/// nullable one.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::workspace_import_jobs)]
pub struct WorkspaceImportJobChange {
    pub state: Option<String>,
    pub stage: Option<Option<String>>,
    pub archive_key: Option<Option<String>>,
    pub regenerate_uuids: Option<bool>,
    pub report: Option<Option<serde_json::Value>>,
    pub error: Option<Option<serde_json::Value>>,
    pub created_people: Option<Vec<Uuid>>,
    pub files_restored: Option<i32>,
    pub updated_at: Option<DateTime<Utc>>,
    pub checked_at: Option<Option<DateTime<Utc>>>,
    pub confirmed_by: Option<Option<Uuid>>,
    pub confirmed_at: Option<Option<DateTime<Utc>>>,
    pub applied_at: Option<Option<DateTime<Utc>>>,
    pub finished_at: Option<Option<DateTime<Utc>>>,
    pub reset_by: Option<Option<Uuid>>,
    pub reset_at: Option<Option<DateTime<Utc>>>,
    pub pruned_at: Option<Option<DateTime<Utc>>>,
}
