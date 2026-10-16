use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::services::imports::history::{state_of, JobState};
use crate::services::imports::previous::PreviousValues;
use crate::services::imports::remove::window_closes_at;

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
    /// When Remove imported assets ran on the job; it runs once.
    pub removed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub removed_by: Option<Uuid>,
    /// `services::imports::remove::RemoveResult`: each created asset
    /// deleted or kept with its reasons, and the catalog entries removed.
    pub remove_result: Option<serde_json::Value>,
    /// For a job made by Import the next sheet, the job its workbook was
    /// first uploaded to; `None` for that first job.
    pub upload_of: Option<Uuid>,
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
    /// See [`ImportJob::upload_of`].
    pub upload_of: Option<Uuid>,
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
    pub removed_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub removed_by: Option<Option<Uuid>>,
    pub remove_result: Option<Option<serde_json::Value>>,
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
    /// For a committed asset import, when Remove imported assets closes:
    /// 30 days after the commit.
    pub removal_closes_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When Remove imported assets ran, by whom, and what it did
    /// (`services::imports::remove::RemoveResult`).
    pub removed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub removed_by: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_by_name: Option<String>,
    pub remove_result: Option<serde_json::Value>,
    /// For a committed asset import whose details are kept, the size of
    /// its previous values file; filled where a page shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_values: Option<PreviousValues>,
    /// For a committed workbook import, the next visible sheet with rows
    /// that nothing has imported from this upload yet; filled where a page
    /// shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_sheet: Option<String>,
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
        // Where the grid is stored stays on the server.
        lift("grid");
        let removal_closes_at = match (job.job_type.as_str(), job.status.as_str()) {
            ("assets", "done") => job.completed_at.map(window_closes_at),
            _ => None,
        };
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
            removal_closes_at,
            removed_at: job.removed_at,
            removed_by: job.removed_by,
            removed_by_name: None,
            remove_result: job.remove_result,
            previous_values: None,
            next_sheet: None,
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
    /// Once Remove imported assets ran: how many assets it deleted of
    /// those the import created.
    pub removed: Option<RemovedCounts>,
}

/// "398 of 407 assets removed".
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RemovedCounts {
    pub deleted: i32,
    pub created: i32,
}

/// New, Updated, Unchanged and Skipped, as the tiles count them.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ImportCounts {
    pub new: i32,
    pub updated: i32,
    pub unchanged: i32,
    pub skipped: i32,
}
