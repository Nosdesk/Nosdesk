//! An import's files: the upload and its normalized grid, kept through the
//! workspace's storage backend (local or S3) so any replica reads what
//! another wrote.
//!
//! Keys are logical, under `imports/{job}/`; the caller passes a
//! workspace-scoped storage, so the physical keys sit under
//! `ws/{workspace}/imports/{job}/` and go with the workspace's other files
//! when it is hard-deleted. No file route serves them: each one reads
//! under its own fixed folder (`tickets/`, `temp/`, `assets/`, ...) and
//! refuses `..` segments.
//!
//! A grid is written once under a fresh version and never changed: a new
//! sheet or header row writes a new grid, the job's stored source switches
//! to it, and the old one is removed.

use uuid::Uuid;

use super::csv_parser::ParsedCsv;
use crate::utils::storage::{Storage, StorageError};

/// The answer to a job whose file or grid is gone.
pub const MISSING: &str = "This import's file is no longer available. Upload the file again.";

/// The machine-readable code of [`MISSING`].
pub const MISSING_CODE: &str = "import_file_missing";

/// Every file of a job sits under this prefix.
pub fn job_prefix(job_id: Uuid) -> String {
    format!("imports/{job_id}")
}

/// The original upload: `csv`, `xlsx`, or `bin` for a file that was refused.
pub fn upload_key(job_id: Uuid, extension: &str) -> String {
    format!("imports/{job_id}/upload.{extension}")
}

/// One version of the job's grid.
pub fn grid_key(job_id: Uuid, version: Uuid) -> String {
    format!("imports/{job_id}/grid.{version}.json")
}

/// The job id an `imports/{job}/...` path belongs to.
pub fn job_of(path: &str) -> Option<Uuid> {
    let rest = path.trim_start_matches('/').strip_prefix("imports/")?;
    Uuid::parse_str(rest.split('/').next()?).ok()
}

/// Why a job's file couldn't be read.
#[derive(Debug)]
pub enum FileError {
    /// Gone, or never recorded: the admin uploads the file again.
    Missing,
    Storage(StorageError),
    Decode(String),
}

impl From<StorageError> for FileError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::NotFound(_) => FileError::Missing,
            e => FileError::Storage(e),
        }
    }
}

fn content_type(extension: &str) -> &'static str {
    match extension {
        "csv" => "text/csv",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

/// Keep the upload. Returns its key.
pub async fn put_upload(
    storage: &dyn Storage,
    job_id: Uuid,
    extension: &str,
    bytes: &[u8],
) -> Result<String, StorageError> {
    let key = upload_key(job_id, extension);
    storage
        .put_file(bytes, &key, content_type(extension))
        .await?;
    Ok(key)
}

pub async fn read_upload(
    storage: &dyn Storage,
    job_id: Uuid,
    extension: &str,
) -> Result<Vec<u8>, FileError> {
    Ok(storage.get_file(&upload_key(job_id, extension)).await?)
}

/// A grid as stored. Run it off the async worker: a grid can be large.
pub fn grid_bytes(grid: &ParsedCsv) -> std::io::Result<Vec<u8>> {
    serde_json::to_vec(grid).map_err(std::io::Error::other)
}

/// Keep a grid under a fresh version, returned.
pub async fn put_grid(
    storage: &dyn Storage,
    job_id: Uuid,
    bytes: &[u8],
) -> Result<Uuid, StorageError> {
    let version = Uuid::now_v7();
    storage
        .put_file(bytes, &grid_key(job_id, version), "application/json")
        .await?;
    Ok(version)
}

/// Read the job's grid at `version`, the one its stored source names.
/// `None` is a job whose source names no grid, which reads as missing.
pub async fn read_grid(
    storage: &dyn Storage,
    job_id: Uuid,
    version: Option<Uuid>,
) -> Result<ParsedCsv, FileError> {
    let version = version.ok_or(FileError::Missing)?;
    let bytes = storage.get_file(&grid_key(job_id, version)).await?;
    // The grid is bounded by the upload caps; decoding it is still CPU work.
    tokio::task::spawn_blocking(move || serde_json::from_slice::<ParsedCsv>(&bytes))
        .await
        .map_err(|e| FileError::Decode(e.to_string()))?
        .map_err(|e| FileError::Decode(e.to_string()))
}

/// Remove one grid version. Gone already is fine.
pub async fn remove_grid(
    storage: &dyn Storage,
    job_id: Uuid,
    version: Uuid,
) -> Result<(), StorageError> {
    storage.delete_file(&grid_key(job_id, version)).await
}

/// Remove every file kept for a job. Returns how many there were.
pub async fn remove_job_files(storage: &dyn Storage, job_id: Uuid) -> Result<usize, StorageError> {
    storage.delete_prefix(&job_prefix(job_id)).await
}

/// The job ids that have files in this storage.
pub async fn stored_jobs(storage: &dyn Storage) -> Result<Vec<Uuid>, StorageError> {
    let mut ids: Vec<Uuid> = storage
        .list_prefix("imports/")
        .await?
        .iter()
        .filter_map(|p| job_of(p))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_stay_under_the_job_prefix() {
        let job = Uuid::now_v7();
        let prefix = format!("{}/", job_prefix(job));
        assert!(upload_key(job, "xlsx").starts_with(&prefix));
        assert!(grid_key(job, Uuid::now_v7()).starts_with(&prefix));
        assert!(crate::utils::storage::is_safe_storage_path(&grid_key(
            job,
            Uuid::now_v7()
        )));
    }

    #[test]
    fn reads_the_job_of_a_path() {
        let job = Uuid::now_v7();
        assert_eq!(job_of(&upload_key(job, "csv")), Some(job));
        assert_eq!(job_of(&format!("imports/{job}")), Some(job));
        assert_eq!(job_of("imports/not-a-uuid/upload.csv"), None);
        assert_eq!(job_of("tickets/1/a.png"), None);
    }
}
