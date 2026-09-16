use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Bug reports
// ---------------------------------------------------------------------------
//
// User-submitted bug reports from the in-app "Report a problem" modal.
// One row per submission, workspace-scoped via RLS. See
// `repository/bug_reports.rs` and `handlers/bug_reports.rs`.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::bug_reports)]
pub struct BugReport {
    pub id: i64,
    pub workspace_id: i32,
    pub user_uuid: Option<Uuid>,
    pub session_id: Uuid,
    pub description: String,
    pub url: String,
    pub breadcrumbs: serde_json::Value,
    pub build_sha: String,
    pub user_agent: Option<String>,
    pub viewport: Option<serde_json::Value>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::bug_reports)]
pub struct NewBugReport {
    pub session_id: Uuid,
    pub user_uuid: Option<Uuid>,
    pub description: String,
    pub url: String,
    pub breadcrumbs: serde_json::Value,
    pub build_sha: String,
    pub user_agent: Option<String>,
    pub viewport: Option<serde_json::Value>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}
