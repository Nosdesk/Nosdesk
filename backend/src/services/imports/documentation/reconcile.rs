//! A background check or commit whose process died leaves its job in
//! `checking` or `committing` with no heartbeat. Once it has been silent
//! for [`STALLED_AFTER`] the job is put back, on the next request for it
//! or by the daily prune:
//!
//! - a check goes back to `parsed`, to be checked again;
//! - a commit is a clean yes or no, because its pages and its `done` are
//!   one transaction. Pages it recorded that exist mean it committed (the
//!   job reads `done`); otherwise the images it recorded are deleted and
//!   the job goes back on Review with "Nothing was imported."

use std::sync::Arc;

use uuid::Uuid;

use super::commit::delete_keys;
use super::{nothing_imported, CommitRecord, JOB_TYPE, STALLED_AFTER};
use crate::db::DbConnection;
use crate::models::{ImportJob, ImportJobUpdate};
use crate::repository::documentation_imports as docs_repo;
use crate::repository::imports as repo;
use crate::utils::storage::Storage;

/// The message a stalled check leaves.
pub const CHECK_STOPPED: &str = "The check stopped before it finished. Check the pages again.";

/// Whether the job's background run has gone silent.
pub fn stalled(job: &ImportJob, now: chrono::DateTime<chrono::Utc>) -> bool {
    job.job_type == JOB_TYPE
        && matches!(job.status.as_str(), "checking" | "committing")
        && now - job.updated_at
            > chrono::Duration::from_std(STALLED_AFTER).unwrap_or(chrono::Duration::minutes(5))
}

/// Put a stalled job back, under its row lock, in the caller's
/// transaction. Returns the storage keys to delete once that transaction
/// commits (a commit that didn't finish).
pub fn settle(conn: &mut DbConnection, id: Uuid) -> diesel::QueryResult<Vec<String>> {
    let job = repo::get_for_update(conn, id)?;
    if !stalled(&job, chrono::Utc::now()) {
        return Ok(Vec::new());
    }
    if job.status == "checking" {
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("parsed".to_string()),
                summary: Some(None),
                row_results: Some(None),
                error_message: Some(Some(CHECK_STOPPED.to_string())),
                ..Default::default()
            },
        )?;
        tracing::warn!(id = %id, "stalled documentation check put back");
        return Ok(Vec::new());
    }
    let record = CommitRecord::from_value(job.record_ids.clone()).unwrap_or_default();
    if !record.page_uuids.is_empty()
        && !docs_repo::existing_page_uuids(conn, &record.page_uuids)?.is_empty()
    {
        // The transaction committed; only the job's update was lost.
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("done".to_string()),
                completed_at: Some(Some(chrono::Utc::now())),
                ..Default::default()
            },
        )?;
        return Ok(Vec::new());
    }
    repo::update(
        conn,
        id,
        ImportJobUpdate {
            status: Some("dry_run_done".to_string()),
            error_message: Some(Some(nothing_imported(
                "The import stopped before it finished.",
            ))),
            record_ids: Some(None),
            records_committed: Some(None),
            ..Default::default()
        },
    )?;
    tracing::warn!(id = %id, "stalled documentation commit put back");
    Ok(record.storage_keys)
}

/// After [`settle`]'s transaction commits: delete what the commit stored.
pub async fn remove_stored(storage: Arc<dyn Storage>, keys: Vec<String>) {
    if !keys.is_empty() {
        delete_keys(storage.as_ref(), &keys).await;
    }
}
