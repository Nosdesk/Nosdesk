use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};
use walkdir::WalkDir;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use ring::pbkdf2;

use crate::db::DbConnection;
use crate::models::{
    BackupManifest, TableManifest, FilesManifest, EncryptionManifest, RestorePreview,
    BackupJobUpdate,
};
use crate::repository::backup as backup_repo;

// Encryption constants
const SALT_LENGTH: usize = 32;
const PBKDF2_ITERATIONS: u32 = 100_000;

/// Sensitive fields to exclude or encrypt per table
const SENSITIVE_FIELDS: &[(&str, &[&str])] = &[
    ("users", &["mfa_secret", "mfa_backup_codes"]),
    ("user_auth_identities", &["password_hash", "metadata"]),
    ("refresh_tokens", &["token_hash"]),
    ("reset_tokens", &["token_hash", "metadata"]),
];

/// Tables to export in backup
const BACKUP_TABLES: &[&str] = &[
    "users",
    "user_emails",
    "user_auth_identities",
    "devices",
    "tickets",
    "ticket_devices",
    "comments",
    "attachments",
    "projects",
    "project_tickets",
    "documentation_pages",
    "documentation_revisions",
    "article_contents",
    "article_content_revisions",
    "linked_tickets",
    "site_settings",
    "sync_history",
    "active_sessions",
    "refresh_tokens",
    "reset_tokens",
    "security_events",
    "user_ticket_views",
];

/// Error type for backup operations
#[derive(Debug)]
pub enum BackupError {
    IoError(std::io::Error),
    ZipError(zip::result::ZipError),
    JsonError(serde_json::Error),
    DatabaseError(diesel::result::Error),
    EncryptionError(String),
    InvalidPassword,
    CorruptedBackup(String),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupError::IoError(e) => write!(f, "IO error: {e}"),
            BackupError::ZipError(e) => write!(f, "ZIP error: {e}"),
            BackupError::JsonError(e) => write!(f, "JSON error: {e}"),
            BackupError::DatabaseError(e) => write!(f, "Database error: {e}"),
            BackupError::EncryptionError(e) => write!(f, "Encryption error: {e}"),
            BackupError::InvalidPassword => write!(f, "Invalid password"),
            BackupError::CorruptedBackup(e) => write!(f, "Corrupted backup: {e}"),
        }
    }
}

impl From<std::io::Error> for BackupError {
    fn from(e: std::io::Error) -> Self {
        BackupError::IoError(e)
    }
}

impl From<zip::result::ZipError> for BackupError {
    fn from(e: zip::result::ZipError) -> Self {
        BackupError::ZipError(e)
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(e: serde_json::Error) -> Self {
        BackupError::JsonError(e)
    }
}

impl From<diesel::result::Error> for BackupError {
    fn from(e: diesel::result::Error) -> Self {
        BackupError::DatabaseError(e)
    }
}

/// Derive encryption key from password using PBKDF2
fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        std::num::NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
        salt,
        password.as_bytes(),
        &mut key,
    );
    key
}

/// Encrypt data using AES-256-GCM (same pattern as MFA encryption)
fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<(Vec<u8>, [u8; NONCE_LEN]), BackupError> {
    let unbound_key = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| BackupError::EncryptionError("Failed to create encryption key".to_string()))?;
    let sealing_key = LessSafeKey::new(unbound_key);

    // Generate random nonce
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| BackupError::EncryptionError("Failed to generate nonce".to_string()))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    // Encrypt the data
    let mut in_out = data.to_vec();
    sealing_key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| BackupError::EncryptionError("Encryption failed".to_string()))?;

    Ok((in_out, nonce_bytes))
}

/// Decrypt data using AES-256-GCM
fn decrypt_data(encrypted_data: &[u8], key: &[u8; 32], nonce_bytes: &[u8; NONCE_LEN]) -> Result<Vec<u8>, BackupError> {
    let unbound_key = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|_| BackupError::EncryptionError("Failed to create decryption key".to_string()))?;
    let opening_key = LessSafeKey::new(unbound_key);

    let nonce = Nonce::try_assume_unique_for_key(nonce_bytes)
        .map_err(|_| BackupError::EncryptionError("Invalid nonce".to_string()))?;

    // Decrypt
    let mut in_out = encrypted_data.to_vec();
    let plaintext = opening_key.open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| BackupError::InvalidPassword)?;

    Ok(plaintext.to_vec())
}

/// Export table data as JSON using raw SQL.
///
/// `table_name` is interpolated into the query directly because
/// Postgres doesn't accept identifiers as bound parameters. Callers
/// today only pass values from the compile-time `BACKUP_TABLES`
/// list, but we re-validate here as defence in depth so a future
/// caller can't accidentally widen the surface.
fn export_table_data(
    conn: &mut DbConnection,
    table_name: &str,
    include_sensitive: bool,
) -> Result<(serde_json::Value, i64), BackupError> {
    use diesel::sql_query;
    use diesel::sql_types::Text;

    if !BACKUP_TABLES.contains(&table_name) {
        return Err(BackupError::CorruptedBackup(format!(
            "Refusing to export unknown table: {table_name}"
        )));
    }

    let query = format!("SELECT row_to_json(t) FROM {table_name} t");

    #[derive(QueryableByName)]
    struct JsonRow {
        #[diesel(sql_type = Text)]
        row_to_json: String,
    }

    let results: Vec<JsonRow> = sql_query(&query).load(conn)?;

    let mut rows: Vec<serde_json::Value> = Vec::new();
    for row in results {
        let mut json_value: serde_json::Value = serde_json::from_str(&row.row_to_json)?;

        // If not including sensitive data, remove sensitive fields
        if !include_sensitive {
            if let Some(fields) = SENSITIVE_FIELDS.iter()
                .find(|(t, _)| *t == table_name)
                .map(|(_, fields)| *fields)
            {
                if let serde_json::Value::Object(ref mut map) = json_value {
                    for field in fields {
                        map.remove(*field);
                    }
                }
            }
        }

        rows.push(json_value);
    }

    let count = rows.len() as i64;
    Ok((serde_json::Value::Array(rows), count))
}

/// Get the uploads directory path
fn get_uploads_dir() -> PathBuf {
    std::env::var("UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/app/uploads"))
}

/// Get the backups directory path
fn get_backups_dir() -> PathBuf {
    let base = std::env::var("UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/app/uploads"));
    base.join("backups")
}

/// Create a backup export
pub fn create_backup(
    conn: &mut DbConnection,
    job_id: Uuid,
    include_sensitive: bool,
    password: Option<&str>,
) -> Result<PathBuf, BackupError> {
    let backups_dir = get_backups_dir();
    fs::create_dir_all(&backups_dir)?;

    let timestamp = Utc::now().format("%Y-%m-%d-%H%M%S");
    let filename = format!("backup-{timestamp}.zip");
    let backup_path = backups_dir.join(&filename);

    let file = File::create(&backup_path)?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut table_manifests = HashMap::new();
    let mut sensitive_data: HashMap<String, serde_json::Value> = HashMap::new();

    // Export each table
    for table_name in BACKUP_TABLES {
        // Always export without sensitive data first
        let (data, count) = export_table_data(conn, table_name, false)?;

        let json_content = serde_json::to_string_pretty(&data)?;
        let path = format!("data/{table_name}.json");

        zip.start_file(&path, options)?;
        zip.write_all(json_content.as_bytes())?;

        table_manifests.insert(table_name.to_string(), TableManifest { count });

        // If including sensitive data, also export the sensitive fields separately
        if include_sensitive && password.is_some()
            && SENSITIVE_FIELDS.iter().any(|(t, _)| *t == *table_name) {
                let (full_data, _) = export_table_data(conn, table_name, true)?;
                sensitive_data.insert(table_name.to_string(), full_data);
            }
    }

    // Export files
    let uploads_dir = get_uploads_dir();
    let mut file_count = 0i64;
    let mut total_size = 0i64;

    if uploads_dir.exists() {
        let thumbs_dir = uploads_dir.join("users").join("thumbs");
        for entry in WalkDir::new(&uploads_dir)
            .into_iter()
            .filter_entry(|e| {
                let path = e.path();
                // Skip the backups directory and thumbnails (can be regenerated)
                !path.starts_with(get_backups_dir()) && !path.starts_with(&thumbs_dir)
            })
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let file_path = entry.path();
                let relative_path = file_path.strip_prefix(&uploads_dir)
                    .map_err(|e| BackupError::IoError(std::io::Error::other(
                        e.to_string()
                    )))?;

                let archive_path = format!("files/{}", relative_path.display());

                zip.start_file(&archive_path, options)?;
                let mut file = File::open(file_path)?;
                let mut buffer = Vec::new();
                let size = file.read_to_end(&mut buffer)?;
                zip.write_all(&buffer)?;

                file_count += 1;
                total_size += size as i64;
            }
        }
    }

    // Handle sensitive data encryption
    let encryption_manifest = if include_sensitive && password.is_some() && !sensitive_data.is_empty() {
        let password = password.unwrap();

        // Generate salt
        let rng = SystemRandom::new();
        let mut salt = [0u8; SALT_LENGTH];
        rng.fill(&mut salt)
            .map_err(|_| BackupError::EncryptionError("Failed to generate salt".to_string()))?;

        // Derive key and encrypt
        let key = derive_key(password, &salt);
        let sensitive_json = serde_json::to_string(&sensitive_data)?;
        let (encrypted, nonce) = encrypt_data(sensitive_json.as_bytes(), &key)?;

        // Write encrypted sensitive data
        zip.start_file("data/sensitive.json.enc", options)?;
        zip.write_all(&encrypted)?;

        Some(EncryptionManifest {
            algorithm: "AES-256-GCM".to_string(),
            kdf: "PBKDF2-HMAC-SHA256".to_string(),
            salt: hex::encode(salt),
            nonce: hex::encode(nonce),
        })
    } else {
        None
    };

    // Create manifest
    let manifest = BackupManifest {
        version: "1.0".to_string(),
        created_at: Utc::now().to_rfc3339(),
        nosdesk_version: env!("CARGO_PKG_VERSION").to_string(),
        include_sensitive,
        tables: table_manifests,
        files: FilesManifest {
            total_count: file_count,
            total_size_bytes: total_size,
        },
        encryption: encryption_manifest,
    };

    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    zip.start_file("manifest.json", options)?;
    zip.write_all(manifest_json.as_bytes())?;

    zip.finish()?;

    // Update job with file info
    let file_size = fs::metadata(&backup_path)?.len() as i64;
    backup_repo::update_backup_job(conn, job_id, BackupJobUpdate {
        status: Some("completed".to_string()),
        file_path: Some(backup_path.to_string_lossy().to_string()),
        file_size: Some(file_size),
        error_message: None,
        completed_at: Some(Utc::now().naive_utc()),
    })?;

    Ok(backup_path)
}

/// Read and parse a backup archive
pub fn read_backup_manifest(backup_path: &Path) -> Result<BackupManifest, BackupError> {
    let file = File::open(backup_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut manifest_file = archive.by_name("manifest.json")?;
    let mut manifest_content = String::new();
    manifest_file.read_to_string(&mut manifest_content)?;

    let manifest: BackupManifest = serde_json::from_str(&manifest_content)?;
    Ok(manifest)
}

/// Preview what a restore would do
pub fn preview_restore(backup_path: &Path) -> Result<RestorePreview, BackupError> {
    let manifest = read_backup_manifest(backup_path)?;

    let has_encrypted_sensitive = manifest.encryption.is_some();

    // Generate warnings
    let mut warnings = Vec::new();

    // Version mismatch warning
    let current_version = env!("CARGO_PKG_VERSION");
    if manifest.nosdesk_version != current_version {
        warnings.push(format!(
            "Backup was created with Nosdesk v{}, current version is v{}",
            manifest.nosdesk_version, current_version
        ));
    }

    // Large file warning
    if manifest.files.total_size_bytes > 1024 * 1024 * 1024 {
        warnings.push(format!(
            "Backup contains {} GB of files, restore may take a while",
            manifest.files.total_size_bytes / (1024 * 1024 * 1024)
        ));
    }

    Ok(RestorePreview {
        manifest,
        has_encrypted_sensitive,
        warnings,
    })
}

/// Restore files from a backup archive
pub fn restore_backup_files(
    backup_path: &Path,
) -> Result<u64, BackupError> {
    let file = File::open(backup_path)?;
    let mut archive = ZipArchive::new(file)?;

    let uploads_dir = get_uploads_dir();
    let mut restored_count = 0u64;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        if name.starts_with("files/") && !name.ends_with('/') {
            let relative_path = name.strip_prefix("files/").unwrap();
            let dest_path = uploads_dir.join(relative_path);

            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut dest_file = File::create(&dest_path)?;
            std::io::copy(&mut file, &mut dest_file)?;
            restored_count += 1;
        }
    }

    Ok(restored_count)
}

/// Verify if a password can decrypt the sensitive data
pub fn verify_backup_password(backup_path: &Path, password: &str) -> Result<bool, BackupError> {
    let manifest = read_backup_manifest(backup_path)?;

    if manifest.encryption.is_none() {
        return Ok(true); // No encryption, no password needed
    }

    let encryption = manifest.encryption.unwrap();

    // Read encrypted file
    let file = File::open(backup_path)?;
    let mut archive = ZipArchive::new(file)?;

    let mut enc_file = archive.by_name("data/sensitive.json.enc")
        .map_err(|_| BackupError::CorruptedBackup("Missing encrypted sensitive data".to_string()))?;
    let mut encrypted_data = Vec::new();
    enc_file.read_to_end(&mut encrypted_data)?;

    // Decode salt and nonce
    let salt = hex::decode(&encryption.salt)
        .map_err(|e| BackupError::EncryptionError(format!("Invalid salt: {e}")))?;
    let nonce = hex::decode(&encryption.nonce)
        .map_err(|e| BackupError::EncryptionError(format!("Invalid nonce: {e}")))?;

    if salt.len() != SALT_LENGTH || nonce.len() != NONCE_LEN {
        return Err(BackupError::CorruptedBackup("Invalid encryption parameters".to_string()));
    }

    let mut salt_arr = [0u8; SALT_LENGTH];
    let mut nonce_arr = [0u8; NONCE_LEN];
    salt_arr.copy_from_slice(&salt);
    nonce_arr.copy_from_slice(&nonce);

    // Try to decrypt
    let key = derive_key(password, &salt_arr);
    match decrypt_data(&encrypted_data, &key, &nonce_arr) {
        Ok(_) => Ok(true),
        Err(BackupError::InvalidPassword) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Delete a backup file
pub fn delete_backup_file(file_path: &str) -> Result<(), BackupError> {
    let path = Path::new(file_path);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Stats from database restore
#[derive(Debug)]
pub struct RestoreStats {
    pub tables_restored: usize,
    pub records_restored: usize,
}

/// Tables the restore is allowed to write to. Every code path
/// that interpolates a table name into SQL during restore
/// validates against this list, so a poisoned backup can't
/// inject SQL via a hostile table name.
const ALLOWED_RESTORE_TABLES: &[&str] = &[
    "users",
    "user_emails",
    "user_auth_identities",
    "devices",
    "tickets",
    "ticket_devices",
    "comments",
    "attachments",
    "projects",
    "project_tickets",
    "documentation_pages",
    "documentation_revisions",
    "article_contents",
    "article_content_revisions",
    "linked_tickets",
    "site_settings",
    "user_ticket_views",
    "refresh_tokens",
    "reset_tokens",
];

fn assert_table_allowed(table_name: &str) -> Result<(), BackupError> {
    if ALLOWED_RESTORE_TABLES.contains(&table_name) {
        Ok(())
    } else {
        Err(BackupError::CorruptedBackup(format!(
            "backup references disallowed table '{table_name}'"
        )))
    }
}

/// Restore database tables from backup archive
pub fn restore_database(
    conn: &mut DbConnection,
    backup_path: &Path,
    password: Option<&str>,
) -> Result<RestoreStats, BackupError> {
    let file = File::open(backup_path)?;
    let mut archive = ZipArchive::new(file)?;

    // Read manifest
    let manifest: BackupManifest = {
        let mut manifest_file = archive.by_name("manifest.json")?;
        let mut content = String::new();
        manifest_file.read_to_string(&mut content)?;
        serde_json::from_str(&content)?
    };

    // Tables the restore is allowed to write to. Used both as the
    // restore-order list (respecting foreign-key dependencies) and
    // as the table-name allowlist that `restore_table_data` and
    // `update_sensitive_fields` validate against. A poisoned backup
    // file can't pivot to a table outside this list.
    let restore_order = ALLOWED_RESTORE_TABLES;

    let mut tables_restored = 0;
    let mut records_restored = 0;

    // Restore each table
    for table_name in restore_order {
        let data_path = format!("data/{table_name}.json");

        // Try to read the file, skip if not in backup
        let content = match archive.by_name(&data_path) {
            Ok(mut data_file) => {
                let mut content = String::new();
                data_file.read_to_string(&mut content)?;
                content
            }
            Err(_) => continue,
        };

        let rows: Vec<serde_json::Value> = serde_json::from_str(&content)?;

        if rows.is_empty() {
            continue;
        }

        // Insert rows using raw SQL
        let count = restore_table_data(conn, table_name, &rows)?;
        if count > 0 {
            tables_restored += 1;
            records_restored += count;
        }
    }

    // Handle encrypted sensitive data if password provided
    if manifest.encryption.is_some() && password.is_some() {
        // Read encrypted data from archive
        let encrypted_data: Option<Vec<u8>> = {
            let file = File::open(backup_path)?;
            let mut archive = ZipArchive::new(file)?;

            // Check if file exists and read it
            let result = archive.by_name("data/sensitive.json.enc");
            if let Ok(mut enc_file) = result {
                let mut data = Vec::new();
                enc_file.read_to_end(&mut data)?;
                drop(enc_file); // Explicitly drop before archive
                Some(data)
            } else {
                None
            }
        };

        // Process encrypted data after archive is dropped
        if let Some(encrypted_data) = encrypted_data {
            if let Some(enc_info) = &manifest.encryption {
                let password = password.unwrap();
                let salt = hex::decode(&enc_info.salt)
                    .map_err(|e| BackupError::EncryptionError(format!("Invalid salt: {e}")))?;
                let nonce = hex::decode(&enc_info.nonce)
                    .map_err(|e| BackupError::EncryptionError(format!("Invalid nonce: {e}")))?;

                let mut salt_arr = [0u8; SALT_LENGTH];
                let mut nonce_arr = [0u8; NONCE_LEN];
                salt_arr.copy_from_slice(&salt);
                nonce_arr.copy_from_slice(&nonce);

                // Derive key and decrypt
                let key = derive_key(password, &salt_arr);
                let decrypted = decrypt_data(&encrypted_data, &key, &nonce_arr)?;

                let sensitive_tables: std::collections::HashMap<String, Vec<serde_json::Value>> =
                    serde_json::from_slice(&decrypted)?;

                // Update tables with sensitive fields
                for (table_name, rows) in sensitive_tables {
                    update_sensitive_fields(conn, &table_name, &rows)?;
                }
            }
        }
    }

    // Reset all sequences to avoid primary key conflicts
    reset_sequences(conn)?;

    Ok(RestoreStats {
        tables_restored,
        records_restored,
    })
}

/// Reset all sequences to be higher than the max ID in each table
/// This is necessary after restoring data with explicit IDs
fn reset_sequences(conn: &mut DbConnection) -> Result<(), BackupError> {
    use diesel::sql_query;
    use diesel::RunQueryDsl;

    // Tables with serial/bigserial id columns that need sequence reset
    let tables_with_sequences = [
        "tickets",
        "devices",
        "user_emails",
        "user_auth_identities",
        "comments",
        "attachments",
        "projects",
        "documentation_pages",
        "documentation_revisions",
        "article_contents",
        "article_content_revisions",
        "active_sessions",
        "refresh_tokens",
        "security_events",
        "sync_history",
    ];

    for table in tables_with_sequences {
        let seq_name = format!("{table}_id_seq");
        let query = format!(
            "SELECT setval('{seq_name}', COALESCE((SELECT MAX(id) FROM {table}), 0) + 1, false)"
        );

        if let Err(e) = sql_query(&query).execute(conn) {
            // Log but don't fail - some sequences might not exist
            log::warn!("Could not reset sequence {seq_name}: {e}");
        } else {
            log::debug!("Reset sequence {seq_name} for table {table}");
        }
    }

    log::info!("Sequences reset after restore");
    Ok(())
}

/// Restore data to a table.
///
/// AUD-008: column names come from `information_schema.columns` for
/// the target table, never from JSON keys. A poisoned backup can put
/// arbitrary strings in its row keys, but only keys that match a
/// real column on `table_name` are interpolated into the INSERT;
/// everything else is dropped. The table name itself is checked
/// against `ALLOWED_RESTORE_TABLES`. Values still go through
/// `json_to_sql_value`'s quote-doubling escape, but the
/// load-bearing protection is column-name validation against the
/// schema rather than escape correctness.
fn restore_table_data(
    conn: &mut DbConnection,
    table_name: &str,
    rows: &[serde_json::Value],
) -> Result<usize, BackupError> {
    use diesel::sql_query;
    use diesel::RunQueryDsl;

    assert_table_allowed(table_name)?;
    let valid_columns = fetch_table_columns(conn, table_name)?;

    let mut inserted = 0;
    for row in rows {
        let Some(map) = row.as_object() else { continue };
        if map.is_empty() {
            continue;
        }

        let entries: Vec<(&str, String)> = map
            .iter()
            .filter(|(k, _)| valid_columns.contains(k.as_str()))
            .map(|(k, v)| (k.as_str(), json_to_sql_value(v)))
            .collect();
        if entries.is_empty() {
            continue;
        }
        let columns: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
        let values: Vec<&str> = entries.iter().map(|(_, v)| v.as_str()).collect();
        let full_query = format!(
            "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT DO NOTHING",
            table_name,
            columns.join(", "),
            values.join(", ")
        );

        match sql_query(&full_query).execute(conn) {
            Ok(count) => inserted += count,
            Err(e) => {
                log::warn!("Failed to insert into {table_name}: {e}");
            }
        }
    }
    Ok(inserted)
}

/// Convert JSON value to SQL literal. Single-quote escaping via
/// doubling is the standard Postgres pattern and is safe as long as
/// `standard_conforming_strings` is on (default since Postgres 9.1+).
/// The column-name allowlist in [`restore_table_data`] and
/// [`update_sensitive_fields`] is the load-bearing protection;
/// this helper is the second layer.
fn json_to_sql_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            format!("'{}'::jsonb", value.to_string().replace('\'', "''"))
        }
    }
}

/// Update sensitive fields in existing rows.
///
/// AUD-008: column names come from the hardcoded `sensitive_fields`
/// list, the primary-key column from the same match arm, and the
/// table name from `ALLOWED_RESTORE_TABLES`. None of these are
/// influenced by the backup contents.
fn update_sensitive_fields(
    conn: &mut DbConnection,
    table_name: &str,
    rows: &[serde_json::Value],
) -> Result<(), BackupError> {
    use diesel::sql_query;
    use diesel::RunQueryDsl;

    assert_table_allowed(table_name)?;

    let (pk_column, sensitive_fields): (&str, &[&str]) = match table_name {
        "users" => ("uuid", &["mfa_secret", "mfa_backup_codes"]),
        "user_auth_identities" => ("id", &["password_hash", "metadata"]),
        "refresh_tokens" => ("id", &["token_hash"]),
        "reset_tokens" => ("id", &["token_hash", "metadata"]),
        _ => return Ok(()),
    };

    for row in rows {
        let Some(map) = row.as_object() else { continue };
        let Some(pk_value) = map.get(pk_column).map(json_to_sql_value) else {
            continue;
        };

        let updates: Vec<String> = sensitive_fields
            .iter()
            .filter_map(|field| {
                map.get(*field)
                    .map(|v| format!("{field} = {}", json_to_sql_value(v)))
            })
            .collect();
        if updates.is_empty() {
            continue;
        }

        let query = format!(
            "UPDATE {table_name} SET {} WHERE {pk_column} = {pk_value}",
            updates.join(", "),
        );
        if let Err(e) = sql_query(&query).execute(conn) {
            log::warn!("Failed to update sensitive fields in {table_name}: {e}");
        }
    }
    Ok(())
}

/// Look up the column names of a table from `information_schema`.
/// Used to filter JSON keys before they're interpolated into a
/// restore INSERT — a column name that isn't on this list can't
/// reach the SQL layer regardless of what the backup file claims.
fn fetch_table_columns(
    conn: &mut DbConnection,
    table_name: &str,
) -> Result<std::collections::HashSet<String>, BackupError> {
    use diesel::deserialize::QueryableByName;
    use diesel::prelude::*;
    use diesel::sql_query;
    use diesel::sql_types::Text;

    #[derive(QueryableByName)]
    struct ColumnName {
        #[diesel(sql_type = Text)]
        column_name: String,
    }

    let rows: Vec<ColumnName> = sql_query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1",
    )
    .bind::<Text, _>(table_name)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(rows.into_iter().map(|c| c.column_name).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── derive_key ───────────────────────────────────────────────

    #[test]
    fn derive_key_produces_32_bytes() {
        let key = derive_key("password", b"saltsaltsaltsalt");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn derive_key_is_deterministic() {
        let salt = b"fixed-salt-value-for-test1234567";
        let k1 = derive_key("pass", salt);
        let k2 = derive_key("pass", salt);
        assert_eq!(k1, k2);
    }

    #[test]
    fn derive_key_differs_for_different_passwords() {
        let salt = b"fixed-salt-value-for-test1234567";
        let k1 = derive_key("password1", salt);
        let k2 = derive_key("password2", salt);
        assert_ne!(k1, k2);
    }

    // ── encrypt_data / decrypt_data roundtrip ────────────────────

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = derive_key("test-password", b"salt-for-encrypt-decrypt-test!!");
        let plaintext = b"hello world, this is secret data";
        let (ciphertext, nonce) = encrypt_data(plaintext, &key).unwrap();
        assert_ne!(&ciphertext[..plaintext.len()], plaintext);

        let decrypted = decrypt_data(&ciphertext, &key, &nonce).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let key = derive_key("correct", b"salt-for-wrong-key-test!!!!!!!!");
        let wrong_key = derive_key("wrong", b"salt-for-wrong-key-test!!!!!!!!");
        let (ciphertext, nonce) = encrypt_data(b"secret", &key).unwrap();
        assert!(decrypt_data(&ciphertext, &wrong_key, &nonce).is_err());
    }

    // ── table allowlist (AUD-008) ────────────────────────────────

    #[test]
    fn assert_table_allowed_accepts_whitelisted_tables() {
        for &name in ALLOWED_RESTORE_TABLES {
            assert_table_allowed(name).expect("whitelisted table must pass");
        }
    }

    #[test]
    fn assert_table_allowed_rejects_unknown_tables() {
        for hostile in [
            "",
            "pg_authid",
            "users; DROP TABLE users; --",
            "users) RETURNING *; --",
            "USERS",
            "Users",
            "schema.users",
            "secret_table",
        ] {
            assert!(
                assert_table_allowed(hostile).is_err(),
                "expected rejection for {hostile:?}"
            );
        }
    }

    // ── DB-backed: poisoned-backup defence (AUD-008) ─────────────

    #[test]
    fn restore_table_data_ignores_hostile_column_names() {
        // A poisoned backup might ship a row whose JSON keys carry
        // SQL fragments instead of real column names. With the
        // pre-AUD-008 string-concatenation pattern this would have
        // landed straight in the INSERT statement. With
        // jsonb_populate_record, unknown keys are silently dropped
        // and Postgres treats them as no-ops. This test confirms:
        // (a) the legitimate column data is restored intact, and
        // (b) the hostile keys aren't executed (the users table
        //     still exists, no extra rows appear, etc).
        use crate::test_helpers::setup_test_connection;
        use diesel::prelude::*;
        use uuid::Uuid;

        let mut conn = setup_test_connection();

        let uuid = Uuid::new_v4();
        let row = serde_json::json!({
            "uuid": uuid,
            "name": "Restored Alice",
            "role": "user",
            "feature_flag_overrides": {},
            "created_at": "2026-01-01T00:00:00",
            "updated_at": "2026-01-01T00:00:00",
            // Hostile keys that an attacker might smuggle in.
            "name); DROP TABLE users; --": "ignored",
            "id, mfa_enabled) VALUES (1,true": 42,
            "); SELECT pg_sleep(60)--": null,
        });

        let inserted = restore_table_data(&mut conn, "users", &[row])
            .expect("restore_table_data should not fail");
        assert_eq!(inserted, 1, "exactly one row inserted");

        // The legit row landed with the right name.
        let name: String = crate::schema::users::table
            .filter(crate::schema::users::uuid.eq(uuid))
            .select(crate::schema::users::name)
            .first(&mut conn)
            .expect("inserted user should be readable");
        assert_eq!(name, "Restored Alice");

        // The hostile-named columns weren't created; the users table
        // shape is whatever the schema says. A simple count query
        // succeeding confirms the table wasn't dropped or mutated.
        let count: i64 = crate::schema::users::table
            .count()
            .get_result(&mut conn)
            .expect("users table should still be queryable");
        assert!(count >= 1, "users table intact after hostile insert");
    }
}
