use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::services::imports::history::{state_of, JobState};

/// CSV import job. Two-phase: rows go through dry-run (parse +
/// validate, write `summary`) before the admin commits. The
/// audit row outlives the request that triggered the upload so
/// the admin UI can resume a job they navigated away from.
///
/// Selected without `row_results`, which holds every row and is read on
/// its own (`repository::imports::row_results`).
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
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
    /// The uploaded file's headers, samples and suggested mapping
    /// (`services::imports::mapping::StoredSource`).
    pub source: Option<serde_json::Value>,
    /// Field id to column index, set at the Map step.
    pub mapping: Option<serde_json::Value>,
    /// `services::imports::ImportOptions`, set with the mapping.
    pub options: Option<serde_json::Value>,
    /// `services::imports::ValueAnswers`: the admin's answer per distinct
    /// source value.
    pub answers: Option<serde_json::Value>,
    /// `services::imports::RecordIds`, written by the commit.
    pub record_ids: Option<serde_json::Value>,
    /// When the 30-day prune dropped the file and the per-row detail
    /// (`services::imports::history`).
    pub pruned_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct NewImportJob {
    /// Minted by the caller: the job's files are named by it.
    pub id: Uuid,
    pub job_type: String,
    pub filename: String,
    pub file_path: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct ImportJobUpdate {
    pub job_type: Option<String>,
    pub status: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub summary: Option<Option<serde_json::Value>>,
    pub records_committed: Option<Option<i32>>,
    pub error_message: Option<Option<String>>,
    pub source: Option<Option<serde_json::Value>>,
    pub mapping: Option<Option<serde_json::Value>>,
    pub options: Option<Option<serde_json::Value>>,
    /// `services::imports::StoredRowResults`, written by the dry run and
    /// again by the commit.
    pub row_results: Option<Option<serde_json::Value>>,
    pub answers: Option<Option<serde_json::Value>>,
    pub record_ids: Option<Option<serde_json::Value>>,
}

/// A saved match: the answer an admin gave for one source value, which the
/// next import of the type fills in. `context` is empty when the field has
/// none; `answer` is a `services::imports::ValueAnswer`.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::import_value_answers)]
pub struct ImportValueAnswer {
    pub id: Uuid,
    pub workspace_id: i32,
    pub job_type: String,
    pub field: String,
    pub context: String,
    pub normalized_value: String,
    pub answer: serde_json::Value,
    pub answered_by: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// The value as the file first spelled it; `None` for an answer saved
    /// before spellings were kept.
    pub source_text: Option<String>,
    /// The answer links to an entry the import that saved it created.
    pub created: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::import_value_answers)]
pub struct NewImportValueAnswer {
    pub job_type: String,
    pub field: String,
    pub context: String,
    pub normalized_value: String,
    pub answer: serde_json::Value,
    pub answered_by: Option<Uuid>,
    pub job_id: Option<Uuid>,
    pub source_text: Option<String>,
    pub created: bool,
}

/// An import job as the API returns it: the job row without the server's
/// file path or the per-row results, with the suggested mapping and the
/// columns set to Don't import lifted out of `source`.
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
    pub source: Option<serde_json::Value>,
    pub suggested_mapping: serde_json::Value,
    pub mapping: Option<serde_json::Value>,
    /// Column indexes the stored mapping sets to Don't import.
    pub ignored: serde_json::Value,
    pub options: Option<serde_json::Value>,
    pub answers: Option<serde_json::Value>,
    /// Draft, Imported or Expired (`services::imports::history::state_of`);
    /// `None` for a refused upload or a failed commit.
    pub state: Option<JobState>,
    /// When the 30-day prune dropped the file and the per-row detail; from
    /// then the job is counts, options and mapping only.
    pub pruned_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Who uploaded it, as the workspace names them; filled where a page
    /// shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_name: Option<String>,
}

impl From<ImportJob> for ImportJobResponse {
    fn from(job: ImportJob) -> Self {
        let mut source = job.source;
        let mut lift = |key: &str| {
            source
                .as_mut()
                .and_then(serde_json::Value::as_object_mut)
                .and_then(|o| o.remove(key))
                .unwrap_or_else(|| serde_json::json!([]))
        };
        let suggested_mapping = lift("suggested_mapping");
        let ignored = lift("ignored");
        let state = state_of(
            &job.status,
            job.updated_at,
            job.pruned_at,
            chrono::Utc::now(),
        );
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
            source,
            suggested_mapping,
            mapping: job.mapping,
            ignored,
            options: job.options,
            answers: job.answers,
            state,
            pruned_at: job.pruned_at,
            created_by_name: None,
        }
    }
}

/// One job on Recent imports: who, when, which file, its state and its
/// outcome counts, without the file's columns or any row.
#[derive(Debug, Clone, Serialize)]
pub struct ImportJobListItem {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub state: Option<JobState>,
    pub filename: String,
    /// The workbook sheet, for an .xlsx.
    pub sheet: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_by_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub pruned_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Outcome counts once the job has been checked.
    pub counts: Option<ImportCounts>,
}

/// New, Updated, Unchanged and Skipped, as the tiles count them.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ImportCounts {
    pub new: i32,
    pub updated: i32,
    pub unchanged: i32,
    pub skipped: i32,
}
