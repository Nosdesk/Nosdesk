use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// CSV import job. Two-phase: rows go through dry-run (parse +
/// validate, write `summary`) before the admin commits. The
/// audit row outlives the request that triggered the upload so
/// the admin UI can resume a job they navigated away from.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct ImportJob {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub filename: String,
    pub file_path: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub summary: Option<serde_json::Value>,
    pub records_committed: Option<i32>,
    pub error_message: Option<String>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct NewImportJob {
    pub job_type: String,
    pub filename: String,
    pub file_path: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct ImportJobUpdate {
    pub status: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub summary: Option<Option<serde_json::Value>>,
    pub records_committed: Option<Option<i32>>,
    pub error_message: Option<Option<String>>,
}

/// An import job as the API returns it: the job row without the server's
/// file path.
#[derive(Debug, Clone, Serialize)]
pub struct ImportJobResponse {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub filename: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub summary: Option<serde_json::Value>,
    pub records_committed: Option<i32>,
    pub error_message: Option<String>,
}

impl From<ImportJob> for ImportJobResponse {
    fn from(job: ImportJob) -> Self {
        Self {
            id: job.id,
            job_type: job.job_type,
            status: job.status,
            filename: job.filename,
            created_by: job.created_by,
            created_at: job.created_at,
            updated_at: job.updated_at,
            completed_at: job.completed_at,
            summary: job.summary,
            records_committed: job.records_committed,
            error_message: job.error_message,
        }
    }
}
