use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Backup Jobs - System Backup and Restore
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct BackupJob {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct NewBackupJob {
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct BackupJobUpdate {
    pub status: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub completed_at: Option<NaiveDateTime>,
}

// API response for backup jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupJobResponse {
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_by: Option<String>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
}

impl From<BackupJob> for BackupJobResponse {
    fn from(job: BackupJob) -> Self {
        BackupJobResponse {
            id: job.id.to_string(),
            job_type: job.job_type,
            status: job.status,
            include_sensitive: job.include_sensitive,
            file_path: job.file_path,
            file_size: job.file_size,
            error_message: job.error_message,
            created_by: job.created_by.map(|u| u.to_string()),
            created_at: job.created_at,
            completed_at: job.completed_at,
        }
    }
}

// Request to start an export backup
#[derive(Debug, Serialize, Deserialize)]
pub struct StartBackupExportRequest {
    pub include_sensitive: bool,
    pub password: Option<String>,
}

// Request to execute a restore
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecuteRestoreRequest {
    pub password: Option<String>,
}

// Backup manifest for archive metadata.
//
// Lives inside the zip (encrypted-or-not) as `manifest.json`.
// The encryption envelope is OUTSIDE the manifest — when a
// backup is password-protected, the entire zip is wrapped in an
// AES-GCM container whose header carries the salt + nonce; this
// struct is read after that decryption.
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Bumped on every breaking change to the on-disk shape.
    /// Restorers refuse archives with an unknown version (the
    /// pg_dump `K_VERS_*` pattern). Starts at 1.
    pub backup_format_version: u32,
    /// `CARGO_PKG_VERSION` of the binary that wrote the backup.
    /// Operator-readable, not gate-load-bearing.
    pub nosdesk_version: String,
    /// `build.rs`'s fingerprint of the migrations (`NOSDESK_SCHEMA_HASH`).
    /// The only schema marker 1.0.x wrote; restore reads it for those
    /// backups alone, since its hasher can change with the toolchain.
    pub schema_hash: String,
    /// The last migration applied when the backup was taken. With
    /// `schema_sha256`, how restore tells which schema the rows are in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_version: Option<String>,
    /// SHA-256 of every migration up to `migration_version` (`build.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_sha256: Option<String>,
    pub created_at: String,
    pub tables: std::collections::HashMap<String, TableManifest>,
    pub files: FilesManifest,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableManifest {
    pub count: i64,
    /// Hex SHA-256 of the table's `data/<name>.json` payload as
    /// stored in the zip. Restore recomputes and refuses on
    /// mismatch — catches truncated downloads, corrupt storage,
    /// and post-creation tampering.
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FilesManifest {
    pub total_count: i64,
    pub total_size_bytes: i64,
}

// Restore preview response
#[derive(Debug, Serialize, Deserialize)]
pub struct RestorePreview {
    pub manifest: BackupManifest,
    /// True when the source file uses the encrypted wrapper.
    /// The preview can still be returned without the password
    /// only for unencrypted archives; encrypted previews
    /// require the password to be passed in to decrypt the
    /// manifest first.
    pub encrypted: bool,
    /// Set when an earlier Nosdesk made the backup: restoring upgrades it.
    pub upgrade: Option<RestoreUpgrade>,
    pub warnings: Vec<String>,
}

/// The versions a restore upgrades a backup between.
#[derive(Debug, Serialize, Deserialize)]
pub struct RestoreUpgrade {
    /// The Nosdesk version that made the backup.
    pub from_version: String,
    /// This server's version.
    pub to_version: String,
    /// Live tables the backup predates that hold rows now. The restore
    /// replaces them with what the upgrade produces, which for settings
    /// means their defaults. Filled where the preview can reach the
    /// database.
    pub replaced_tables: Vec<String>,
}
