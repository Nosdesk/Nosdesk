use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Sync History Models
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct SyncHistory {
    pub id: i32,
    pub sync_type: String,
    pub status: String,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
    pub tenant_id: Option<String>,
    pub initiated_by: Option<Uuid>,
    pub is_delta: bool,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct NewSyncHistory {
    pub sync_type: String,
    pub status: String,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
    pub tenant_id: Option<String>,
    pub is_delta: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct SyncHistoryUpdate {
    pub status: Option<String>,
    pub completed_at: Option<Option<NaiveDateTime>>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
}

// Delta tokens for incremental sync (Microsoft Graph delta queries)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct SyncDeltaToken {
    pub id: i32,
    pub provider_type: String,
    pub entity_type: String,
    pub delta_link: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct NewSyncDeltaToken {
    pub provider_type: String,
    pub entity_type: String,
    pub delta_link: String,
}
