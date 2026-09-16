use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::csp_reports)]
pub struct CspReport {
    pub id: i64,
    pub dedup_hash: String,
    pub effective_directive: String,
    pub blocked_uri: Option<String>,
    pub source_file: Option<String>,
    pub line_number: Option<i32>,
    pub column_number: Option<i32>,
    pub document_uri: String,
    pub referrer: Option<String>,
    pub violated_directive: Option<String>,
    pub original_policy: Option<String>,
    pub disposition: String,
    pub user_agent: Option<String>,
    pub user_uuid: Option<Uuid>,
    pub occurrence_count: i32,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::csp_reports)]
pub struct NewCspReport {
    pub dedup_hash: String,
    pub effective_directive: String,
    pub blocked_uri: Option<String>,
    pub source_file: Option<String>,
    pub line_number: Option<i32>,
    pub column_number: Option<i32>,
    pub document_uri: String,
    pub referrer: Option<String>,
    pub violated_directive: Option<String>,
    pub original_policy: Option<String>,
    pub disposition: String,
    pub user_agent: Option<String>,
    pub user_uuid: Option<Uuid>,
}
