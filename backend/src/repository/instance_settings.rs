//! The single `instance_settings` row: instance-wide settings an operator sets
//! from the admin UI (the stored licence, the push mode). Not a tenant table,
//! so any connection reads it; only platform-admin handlers write it.
//!
//! Environment variables win over every value here. Callers resolve that
//! precedence (`license`, the push sender); this module only stores.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::instance_settings::dsl as s;
use crate::utils::encryption;

/// AAD for the licence blob. Binds the ciphertext to this column so a blob
/// copied from another encrypted column does not decrypt here.
const LICENSE_AAD: &[u8] = b"instance_settings.license_key";

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = crate::schema::instance_settings)]
pub struct InstanceSettings {
    pub license_key_encrypted: Option<Vec<u8>>,
    pub license_key_kek_id: Option<i16>,
    pub license_source: Option<String>,
    pub license_installed_at: Option<DateTime<Utc>>,
    pub license_installed_by: Option<Uuid>,
    pub license_last_refresh_at: Option<DateTime<Utc>>,
    pub license_last_refresh_error: Option<String>,
    pub push_mode: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum InstanceSettingsError {
    #[error(transparent)]
    Db(#[from] diesel::result::Error),
    #[error("licence blob could not be decrypted: {0}")]
    Crypto(String),
}

/// The row, or `None` on an instance that has never saved a setting.
pub fn get(conn: &mut DbConnection) -> QueryResult<Option<InstanceSettings>> {
    s::instance_settings
        .select(InstanceSettings::as_select())
        .first(conn)
        .optional()
}

/// Decrypt the stored licence token, if any.
pub fn decrypt_license(row: &InstanceSettings) -> Result<Option<String>, InstanceSettingsError> {
    let Some(blob) = &row.license_key_encrypted else {
        return Ok(None);
    };
    // The kek_id inside the blob is authoritative; the column mirrors it.
    // Disagreement means a write skipped the sidecar or someone patched it.
    let blob_kek_id = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| InstanceSettingsError::Crypto(e.to_string()))?;
    if row.license_key_kek_id != Some(blob_kek_id as i16) {
        return Err(InstanceSettingsError::Crypto(format!(
            "licence kek_id sidecar ({:?}) disagrees with blob ({blob_kek_id})",
            row.license_key_kek_id
        )));
    }
    let bytes = encryption::keyring()
        .decrypt(blob, LICENSE_AAD)
        .map_err(|e| InstanceSettingsError::Crypto(e.to_string()))?;
    String::from_utf8(bytes.to_vec())
        .map(Some)
        .map_err(|_| InstanceSettingsError::Crypto("licence is not valid UTF-8".into()))
}

fn ensure_row(conn: &mut DbConnection) -> QueryResult<()> {
    diesel::sql_query("INSERT INTO instance_settings DEFAULT VALUES ON CONFLICT (id) DO NOTHING")
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: Instance-global licence, not workspace data; the handler records a security_events row.
/// Encrypt and store a licence token. The caller has already verified it.
pub fn set_license(
    conn: &mut DbConnection,
    token: &str,
    source: &str,
    installed_by: Option<Uuid>,
) -> Result<(), InstanceSettingsError> {
    let kr = encryption::keyring();
    let encrypted = kr
        .encrypt(token.as_bytes(), LICENSE_AAD)
        .map_err(|e| InstanceSettingsError::Crypto(e.to_string()))?;
    let kek_id = kr.current_version() as i16;
    ensure_row(conn)?;
    diesel::update(s::instance_settings)
        .set((
            s::license_key_encrypted.eq(Some(encrypted)),
            s::license_key_kek_id.eq(Some(kek_id)),
            s::license_source.eq(Some(source)),
            s::license_installed_at.eq(Some(Utc::now())),
            s::license_installed_by.eq(installed_by),
            s::license_last_refresh_error.eq::<Option<String>>(None),
            s::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: Instance-global licence, not workspace data; the handler records a security_events row.
/// Remove the stored licence.
pub fn clear_license(conn: &mut DbConnection) -> QueryResult<()> {
    diesel::update(s::instance_settings)
        .set((
            s::license_key_encrypted.eq::<Option<Vec<u8>>>(None),
            s::license_key_kek_id.eq::<Option<i16>>(None),
            s::license_source.eq::<Option<String>>(None),
            s::license_installed_at.eq::<Option<DateTime<Utc>>>(None),
            s::license_installed_by.eq::<Option<Uuid>>(None),
            s::license_last_refresh_at.eq::<Option<DateTime<Utc>>>(None),
            s::license_last_refresh_error.eq::<Option<String>>(None),
            s::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: Instance-global licence renewal bookkeeping, not workspace data.
/// Record a renewal attempt: when, and the static error kind if it failed.
pub fn record_refresh(conn: &mut DbConnection, error: Option<&str>) -> QueryResult<()> {
    ensure_row(conn)?;
    diesel::update(s::instance_settings)
        .set((
            s::license_last_refresh_at.eq(Some(Utc::now())),
            s::license_last_refresh_error.eq(error),
            s::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: Instance-global push mode, not workspace data; the handler records a security_events row.
/// Store the push mode (`native`, `relay`, `off`), or `None` to defer to the
/// environment.
pub fn set_push_mode(conn: &mut DbConnection, mode: Option<&str>) -> QueryResult<()> {
    ensure_row(conn)?;
    diesel::update(s::instance_settings)
        .set((s::push_mode.eq(mode), s::updated_at.eq(diesel::dsl::now)))
        .execute(conn)?;
    Ok(())
}
