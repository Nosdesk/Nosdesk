use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Self-serve workspace export (Owner-gated; account-erasure Phase 3) ──

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::workspace_export_jobs)]
pub struct WorkspaceExportJob {
    pub id: Uuid,
    pub workspace_id: i32,
    pub requested_by: Option<Uuid>,
    pub status: String,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub expires_at: Option<NaiveDateTime>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::workspace_export_jobs)]
pub struct NewWorkspaceExportJob {
    pub workspace_id: i32,
    pub requested_by: Option<Uuid>,
    pub status: String,
}

/// Partial update: a `None` field is left unchanged (cannot NULL a column, which
/// this flow never needs to).
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::workspace_export_jobs)]
pub struct WorkspaceExportJobUpdate {
    pub status: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub completed_at: Option<NaiveDateTime>,
    pub expires_at: Option<NaiveDateTime>,
}
