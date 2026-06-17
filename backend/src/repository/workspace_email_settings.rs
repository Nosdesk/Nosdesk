//! Per-workspace outbound email identity (the `workspace_email_settings`
//! table). One row per workspace, RLS-isolated by `workspace_id`, so every
//! function here MUST run workspace-scoped (`app.workspace_id` set via
//! `TenantConn` / `with_actor_context` / `run_in_workspace`). That GUC both
//! scopes the read and fills the `workspace_id` default on insert.
//!
//! The SMTP password is stored KEK-encrypted exactly like
//! `channel_credentials`: a framed AES-256-GCM blob plus a `kek_id` sidecar,
//! with the `workspace_id` bound into the AAD so a blob can't be swapped
//! between workspaces by anyone with raw SQL write. The plaintext only ever
//! exists transiently in `set_password` / `decrypt_password`.

use base64::Engine as _;
use diesel::prelude::*;
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey, LineEnding};
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
use rsa::{RsaPrivateKey, RsaPublicKey};

use crate::db::DbConnection;
use crate::models::{
    workspace_email_sending_mode, workspace_email_verification_status,
    UpsertWorkspaceEmailSettings, WorkspaceEmailSettings,
};
use crate::repository::channels::CredentialError;
use crate::utils::encryption;

/// AAD purpose tag for the workspace SMTP password. Combined with the
/// workspace id (RFC 5116 §1.2 bind-context) so a ciphertext lifted into a
/// different workspace's row fails to decrypt.
const WS_EMAIL_AAD_TAG: &[u8] = b".nosdesk.workspace.email.v1";

fn ws_email_aad(workspace_id: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + WS_EMAIL_AAD_TAG.len());
    buf.extend_from_slice(&workspace_id.to_be_bytes());
    buf.extend_from_slice(WS_EMAIL_AAD_TAG);
    buf
}

/// The current workspace's outbound email settings, or `None` if it has
/// never been configured. RLS scopes the read to the request's workspace.
pub fn get(conn: &mut DbConnection) -> QueryResult<Option<WorkspaceEmailSettings>> {
    use crate::schema::workspace_email_settings::dsl as w;
    w::workspace_email_settings
        .select(WorkspaceEmailSettings::as_select())
        .first(conn)
        .optional()
}

/// Settings for an explicit `workspace_id`. Unlike [`get`], this filters by
/// the id rather than relying solely on the RLS GUC, so it is correct on a
/// pinned connection (filter agrees with RLS) and on a bypass connection
/// (the filter does the scoping). The outbound resolver uses it so it works
/// from whatever connection context the caller holds.
pub fn get_for_workspace(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<Option<WorkspaceEmailSettings>> {
    use crate::schema::workspace_email_settings::dsl as w;
    w::workspace_email_settings
        .filter(w::workspace_id.eq(workspace_id))
        .select(WorkspaceEmailSettings::as_select())
        .first(conn)
        .optional()
}

/// Settings for several workspaces in one read. Filters by id rather than
/// the RLS GUC, so the queue worker (which drains under a bypass connection
/// across all workspaces) can resolve a whole drain's identities at once.
pub fn get_for_workspaces(
    conn: &mut DbConnection,
    workspace_ids: &[i32],
) -> QueryResult<Vec<WorkspaceEmailSettings>> {
    use crate::schema::workspace_email_settings::dsl as w;
    w::workspace_email_settings
        .filter(w::workspace_id.eq_any(workspace_ids))
        .select(WorkspaceEmailSettings::as_select())
        .load(conn)
}

/// The ids of every workspace currently in verified-domain mode with status
/// `verified`. A cross-workspace scan for the periodic DKIM re-verification job,
/// so it filters explicitly and is meant to run under a bypass connection.
pub fn verified_domain_workspace_ids(conn: &mut DbConnection) -> QueryResult<Vec<i32>> {
    use crate::schema::workspace_email_settings::dsl as w;
    w::workspace_email_settings
        .filter(w::sending_mode.eq(workspace_email_sending_mode::VERIFIED_DOMAIN))
        .filter(w::verification_status.eq(workspace_email_verification_status::VERIFIED))
        .select(w::workspace_id)
        .load(conn)
}

// sync-audit-only: Workspace outbound email identity; covered by the audit_log trigger on workspace_email_settings, sync clients don't subscribe.
/// Insert or update the editable settings for the current workspace. The
/// password columns are left untouched (managed by `set_password` /
/// `clear_password`), so saving settings never disturbs a stored password.
pub fn upsert(
    conn: &mut DbConnection,
    fields: UpsertWorkspaceEmailSettings,
) -> QueryResult<WorkspaceEmailSettings> {
    use crate::schema::workspace_email_settings::dsl as w;
    use diesel::upsert::excluded;

    diesel::insert_into(w::workspace_email_settings)
        .values(&fields)
        .on_conflict(w::workspace_id)
        .do_update()
        .set((
            w::enabled.eq(excluded(w::enabled)),
            w::from_name.eq(excluded(w::from_name)),
            w::from_email.eq(excluded(w::from_email)),
            w::smtp_host.eq(excluded(w::smtp_host)),
            w::smtp_port.eq(excluded(w::smtp_port)),
            w::smtp_security.eq(excluded(w::smtp_security)),
            w::smtp_username.eq(excluded(w::smtp_username)),
            w::sending_mode.eq(excluded(w::sending_mode)),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .returning(WorkspaceEmailSettings::as_returning())
        .get_result(conn)
}

// sync-audit-only: Workspace outbound SMTP password; encrypted at rest, never emitted; the workspace_email_settings audit trigger redacts it.
/// Encrypt and store the SMTP password for the current workspace. The
/// settings row must already exist (call `upsert` first); `workspace_id` is
/// the row's id and is bound into the AAD.
pub fn set_password(
    conn: &mut DbConnection,
    workspace_id: i32,
    plaintext: &str,
) -> Result<(), CredentialError> {
    use crate::schema::workspace_email_settings::dsl as w;

    let kr = encryption::keyring();
    let aad = ws_email_aad(workspace_id);
    let encrypted = kr
        .encrypt(plaintext.as_bytes(), &aad)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let kek_id = kr.current_version() as i16;

    let updated = diesel::update(w::workspace_email_settings)
        .filter(w::workspace_id.eq(workspace_id))
        .set((
            w::encrypted_smtp_password.eq(Some(encrypted)),
            w::encrypted_kek_id.eq(Some(kek_id)),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;

    if updated == 0 {
        // No row for this workspace (or RLS filtered it out). The caller is
        // expected to upsert the settings row first.
        return Err(CredentialError::Db(diesel::result::Error::NotFound));
    }
    Ok(())
}

// sync-audit-only: Workspace outbound SMTP password clear; covered by the workspace_email_settings audit trigger (redacted).
/// Remove any stored SMTP password for the current workspace.
pub fn clear_password(conn: &mut DbConnection) -> QueryResult<()> {
    use crate::schema::workspace_email_settings::dsl as w;
    diesel::update(w::workspace_email_settings)
        .set((
            w::encrypted_smtp_password.eq::<Option<Vec<u8>>>(None),
            w::encrypted_kek_id.eq::<Option<i16>>(None),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

/// Decrypt the stored SMTP password for `row`, or `Ok(None)` when none is
/// stored. Verifies the sidecar `kek_id` against the blob header and binds
/// `workspace_id` into the AAD, mirroring `channels::get_credential`.
pub fn decrypt_password(row: &WorkspaceEmailSettings) -> Result<Option<String>, CredentialError> {
    let blob = match &row.encrypted_smtp_password {
        Some(b) => b,
        None => return Ok(None),
    };
    let sidecar = row.encrypted_kek_id.ok_or_else(|| {
        CredentialError::Crypto("password blob present but kek_id sidecar is null".into())
    })?;

    // Authoritative kek_id is the one inside the blob; the sidecar is an
    // indexed mirror. Disagreement means a write skipped the sidecar or the
    // column was patched directly. Reject either.
    let blob_kek_id = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    if blob_kek_id as i16 != sidecar {
        return Err(CredentialError::Crypto(format!(
            "workspace_email_settings ws#{} sidecar kek_id ({}) disagrees with blob ({})",
            row.workspace_id, sidecar, blob_kek_id
        )));
    }

    let aad = ws_email_aad(row.workspace_id);
    let plaintext_bytes = encryption::keyring()
        .decrypt(blob, &aad)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let s = String::from_utf8(plaintext_bytes.to_vec())
        .map_err(|_| CredentialError::Crypto("SMTP password is not valid UTF-8".into()))?;
    Ok(Some(s))
}

// ===========================================================================
// Verified-domain DKIM (self-managed keys)
// ===========================================================================

/// AAD purpose tag for the workspace DKIM private key. Distinct from the SMTP
/// password tag so a DKIM blob can't be swapped into the password slot, and
/// workspace-bound so it can't be lifted to another workspace.
const WS_DKIM_AAD_TAG: &[u8] = b".nosdesk.workspace.dkim.v1";

fn ws_dkim_aad(workspace_id: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + WS_DKIM_AAD_TAG.len());
    buf.extend_from_slice(&workspace_id.to_be_bytes());
    buf.extend_from_slice(WS_DKIM_AAD_TAG);
    buf
}

/// RSA-2048 is the v1 default (universal receiver support; ed25519 is a future
/// refinement). The selector is fixed for v1; rotation is a later concern.
const DKIM_RSA_BITS: usize = 2048;
const DKIM_SELECTOR: &str = "nosdesk";

/// The DNS record a workspace admin publishes to authorise our DKIM signing.
pub struct DkimDnsRecord {
    /// Record name: `<selector>._domainkey.<domain>`.
    pub name: String,
    /// TXT value: `v=DKIM1; k=rsa; p=<base64 SPKI DER public key>`.
    pub txt_value: String,
    pub selector: String,
    /// The base64 SPKI public key alone (the `p=` value), for comparing
    /// against what's actually published during verification.
    pub public_b64: String,
}

fn dkim_record(selector: &str, domain: &str, public_b64: &str) -> DkimDnsRecord {
    DkimDnsRecord {
        name: format!("{selector}._domainkey.{domain}"),
        txt_value: format!("v=DKIM1; k=rsa; p={public_b64}"),
        selector: selector.to_string(),
        public_b64: public_b64.to_string(),
    }
}

/// Derive the base64 SPKI public key from a PKCS#1 PEM private key, for the
/// published record. The public key is not secret, so this can run on a
/// decrypted key without further protection.
fn dkim_public_b64(private_pem: &str) -> Result<String, CredentialError> {
    let private = RsaPrivateKey::from_pkcs1_pem(private_pem)
        .map_err(|e| CredentialError::Crypto(format!("DKIM private key parse: {e}")))?;
    let der = RsaPublicKey::from(&private)
        .to_public_key_der()
        .map_err(|e| CredentialError::Crypto(format!("DKIM public key encode: {e}")))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(der.as_bytes()))
}

/// The DKIM private key as base64-encoded PKCS#8 DER, the format Amazon SES
/// BYODKIM (`DomainSigningPrivateKey`) expects. Input is our stored PKCS#1 PEM.
/// The output carries the private key, so handle it like the PEM itself.
pub fn dkim_private_pkcs8_b64(private_pem: &str) -> Result<String, CredentialError> {
    let private = RsaPrivateKey::from_pkcs1_pem(private_pem)
        .map_err(|e| CredentialError::Crypto(format!("DKIM private key parse: {e}")))?;
    let der = private
        .to_pkcs8_der()
        .map_err(|e| CredentialError::Crypto(format!("DKIM PKCS#8 encode: {e}")))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(der.as_bytes()))
}

// sync-audit-only: Workspace DKIM key provisioning; the private key is encrypted at rest and redacted by the workspace_email_settings audit trigger.
/// Generate a fresh RSA-2048 DKIM keypair for `domain`, store the private key
/// KEK-encrypted (AAD-bound to the workspace), switch the workspace into
/// `verified_domain` mode with `pending` status, and return the DNS record to
/// publish. The settings row must already exist (upsert first). Calling again
/// rotates the key and resets verification. Key generation is CPU-bound; async
/// callers should wrap this in `spawn_blocking`.
pub fn provision_dkim(
    conn: &mut DbConnection,
    workspace_id: i32,
    domain: &str,
) -> Result<DkimDnsRecord, CredentialError> {
    use crate::schema::workspace_email_settings::dsl as w;

    let mut rng = rand::thread_rng();
    let private = RsaPrivateKey::new(&mut rng, DKIM_RSA_BITS)
        .map_err(|e| CredentialError::Crypto(format!("DKIM key generation: {e}")))?;
    let private_pem = private
        .to_pkcs1_pem(LineEnding::LF)
        .map_err(|e| CredentialError::Crypto(format!("DKIM private key encode: {e}")))?;
    let public_b64 = dkim_public_b64(&private_pem)?;

    let kr = encryption::keyring();
    let aad = ws_dkim_aad(workspace_id);
    let encrypted = kr
        .encrypt(private_pem.as_bytes(), &aad)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let kek_id = kr.current_version() as i16;

    let updated = diesel::update(w::workspace_email_settings)
        .filter(w::workspace_id.eq(workspace_id))
        .set((
            w::sending_mode.eq(workspace_email_sending_mode::VERIFIED_DOMAIN),
            w::sending_domain.eq(domain),
            w::dkim_selector.eq(DKIM_SELECTOR),
            w::dkim_algorithm.eq("rsa"),
            w::encrypted_dkim_private_key.eq(Some(encrypted)),
            w::dkim_kek_id.eq(Some(kek_id)),
            w::verification_status.eq(workspace_email_verification_status::PENDING),
            w::verified_at.eq::<Option<chrono::NaiveDateTime>>(None),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    if updated == 0 {
        return Err(CredentialError::Db(diesel::result::Error::NotFound));
    }

    Ok(dkim_record(DKIM_SELECTOR, domain, &public_b64))
}

/// Decrypt the stored DKIM private key (PKCS#1 PEM), or `Ok(None)` when none is
/// stored. Verifies the sidecar `dkim_kek_id` and binds `workspace_id` into the
/// AAD, mirroring [`decrypt_password`].
pub fn decrypt_dkim_key(row: &WorkspaceEmailSettings) -> Result<Option<String>, CredentialError> {
    let blob = match &row.encrypted_dkim_private_key {
        Some(b) => b,
        None => return Ok(None),
    };
    let sidecar = row.dkim_kek_id.ok_or_else(|| {
        CredentialError::Crypto("DKIM key present but kek_id sidecar is null".into())
    })?;
    let blob_kek_id = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    if blob_kek_id as i16 != sidecar {
        return Err(CredentialError::Crypto(format!(
            "workspace_email_settings ws#{} DKIM sidecar kek_id ({}) disagrees with blob ({})",
            row.workspace_id, sidecar, blob_kek_id
        )));
    }
    let aad = ws_dkim_aad(row.workspace_id);
    let plaintext = encryption::keyring()
        .decrypt(blob, &aad)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let s = String::from_utf8(plaintext.to_vec())
        .map_err(|_| CredentialError::Crypto("DKIM key is not valid UTF-8".into()))?;
    Ok(Some(s))
}

/// The DNS record to publish for `row`, derived from the stored key, or `None`
/// when the workspace has no DKIM domain/key. Used by the admin UI and the
/// verification check.
pub fn dns_record_for(
    row: &WorkspaceEmailSettings,
) -> Result<Option<DkimDnsRecord>, CredentialError> {
    let (domain, selector) = match (&row.sending_domain, &row.dkim_selector) {
        (Some(d), Some(s)) => (d, s),
        _ => return Ok(None),
    };
    let pem = match decrypt_dkim_key(row)? {
        Some(p) => p,
        None => return Ok(None),
    };
    let public_b64 = dkim_public_b64(&pem)?;
    Ok(Some(dkim_record(selector, domain, &public_b64)))
}

// sync-audit-only: Workspace outbound mode reset; covered by the workspace_email_settings audit trigger.
/// Revert the workspace to `fallback` mode, clearing the verified-domain DKIM
/// material and verification state. The identity (`from_*`) and any `smtp_*`
/// columns are left as-is.
pub fn reset_to_fallback(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<()> {
    use crate::schema::workspace_email_settings::dsl as w;
    diesel::update(w::workspace_email_settings)
        .filter(w::workspace_id.eq(workspace_id))
        .set((
            w::sending_mode.eq(workspace_email_sending_mode::FALLBACK),
            w::verification_status.eq(workspace_email_verification_status::UNVERIFIED),
            w::sending_domain.eq::<Option<String>>(None),
            w::dkim_selector.eq::<Option<String>>(None),
            w::dkim_algorithm.eq::<Option<String>>(None),
            w::encrypted_dkim_private_key.eq::<Option<Vec<u8>>>(None),
            w::dkim_kek_id.eq::<Option<i16>>(None),
            w::verified_at.eq::<Option<chrono::NaiveDateTime>>(None),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: Workspace DKIM verification status; covered by the workspace_email_settings audit trigger.
/// Set the DKIM verification status for the current workspace (and stamp
/// `verified_at` when transitioning to verified). The settings row must exist.
pub fn set_verification_status(
    conn: &mut DbConnection,
    workspace_id: i32,
    status: &str,
    verified_at: Option<chrono::NaiveDateTime>,
) -> QueryResult<()> {
    use crate::schema::workspace_email_settings::dsl as w;
    diesel::update(w::workspace_email_settings)
        .filter(w::workspace_id.eq(workspace_id))
        .set((
            w::verification_status.eq(status),
            w::verified_at.eq(verified_at),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    fn sample_fields() -> UpsertWorkspaceEmailSettings {
        UpsertWorkspaceEmailSettings {
            enabled: true,
            from_name: "Acme Support".into(),
            from_email: "support@acme.test".into(),
            smtp_host: "smtp.acme.test".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            smtp_username: "smtp-user".into(),
            sending_mode: "smtp_relay".into(),
        }
    }

    fn switch_workspace(conn: &mut DbConnection, id: i32) {
        diesel::sql_query(format!(
            "SELECT set_config('app.workspace_id', '{id}', false)"
        ))
        .execute(conn)
        .expect("switch workspace GUC");
    }

    #[test]
    fn get_is_none_when_unconfigured() {
        let mut conn = setup_test_connection();
        assert!(get(&mut conn).unwrap().is_none());
    }

    #[test]
    fn upsert_inserts_then_updates_in_place() {
        let mut conn = setup_test_connection();

        let row = upsert(&mut conn, sample_fields()).unwrap();
        assert_eq!(row.workspace_id, 1);
        assert!(row.enabled);
        assert_eq!(row.from_email, "support@acme.test");

        // A second upsert updates the same row (no duplicate).
        let mut changed = sample_fields();
        changed.enabled = false;
        changed.from_email = "help@acme.test".into();
        let row2 = upsert(&mut conn, changed).unwrap();
        assert_eq!(row2.workspace_id, 1);
        assert!(!row2.enabled);
        assert_eq!(row2.from_email, "help@acme.test");

        // Exactly one row visible for this workspace.
        let fetched = get(&mut conn).unwrap().unwrap();
        assert_eq!(fetched.from_email, "help@acme.test");
    }

    #[test]
    fn set_password_roundtrips_through_decrypt() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();

        set_password(&mut conn, 1, "hunter2-smtp").unwrap();

        let row = get(&mut conn).unwrap().unwrap();
        assert!(row.encrypted_smtp_password.is_some());
        assert_eq!(
            decrypt_password(&row).unwrap().as_deref(),
            Some("hunter2-smtp")
        );
    }

    #[test]
    fn upsert_preserves_a_stored_password() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        set_password(&mut conn, 1, "keep-me").unwrap();

        // Editing settings must not disturb the stored password.
        let mut changed = sample_fields();
        changed.from_name = "Renamed".into();
        upsert(&mut conn, changed).unwrap();

        let row = get(&mut conn).unwrap().unwrap();
        assert_eq!(row.from_name, "Renamed");
        assert_eq!(decrypt_password(&row).unwrap().as_deref(), Some("keep-me"));
    }

    #[test]
    fn clear_password_removes_it() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        set_password(&mut conn, 1, "to-be-cleared").unwrap();

        clear_password(&mut conn).unwrap();

        let row = get(&mut conn).unwrap().unwrap();
        assert!(row.encrypted_smtp_password.is_none());
        assert!(row.encrypted_kek_id.is_none());
        assert!(decrypt_password(&row).unwrap().is_none());
    }

    #[test]
    fn set_password_errors_when_no_row_exists() {
        let mut conn = setup_test_connection();
        // No upsert first: nothing to attach the password to.
        let err = set_password(&mut conn, 1, "orphan").unwrap_err();
        assert!(matches!(
            err,
            CredentialError::Db(diesel::result::Error::NotFound)
        ));
    }

    #[test]
    fn rls_hides_another_workspace_row() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        assert!(get(&mut conn).unwrap().is_some());

        // A different workspace context must not see workspace 1's row.
        switch_workspace(&mut conn, 424242);
        assert!(
            get(&mut conn).unwrap().is_none(),
            "RLS must scope the read to the pinned workspace"
        );
    }

    #[test]
    fn dkim_helpers_are_none_when_unprovisioned() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        let row = get(&mut conn).unwrap().unwrap();
        assert!(decrypt_dkim_key(&row).unwrap().is_none());
        assert!(dns_record_for(&row).unwrap().is_none());
    }

    #[test]
    fn verified_domain_workspace_ids_lists_only_verified() {
        let mut conn = setup_test_connection();
        let mut fields = sample_fields();
        fields.sending_mode = "verified_domain".into();
        upsert(&mut conn, fields).unwrap();

        // A fresh verified-domain row is unverified, so it isn't listed.
        assert!(verified_domain_workspace_ids(&mut conn).unwrap().is_empty());

        set_verification_status(
            &mut conn,
            1,
            "verified",
            Some(chrono::Utc::now().naive_utc()),
        )
        .unwrap();
        assert_eq!(verified_domain_workspace_ids(&mut conn).unwrap(), vec![1]);

        // Reverting drops it from the list (so the re-verify job won't re-check
        // an already-reverted domain).
        set_verification_status(&mut conn, 1, "pending", None).unwrap();
        assert!(verified_domain_workspace_ids(&mut conn).unwrap().is_empty());
    }

    #[test]
    fn set_verification_status_round_trips() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();

        let ts = chrono::Utc::now().naive_utc();
        set_verification_status(&mut conn, 1, "verified", Some(ts)).unwrap();
        let row = get(&mut conn).unwrap().unwrap();
        assert_eq!(row.verification_status, "verified");
        assert!(row.verified_at.is_some());

        set_verification_status(&mut conn, 1, "pending", None).unwrap();
        let row = get(&mut conn).unwrap().unwrap();
        assert_eq!(row.verification_status, "pending");
        assert!(row.verified_at.is_none());
    }

    // One RSA-2048 keypair is generated here (CPU-bound, ~hundreds of ms), so
    // the full provisioning cycle is asserted in a single test.
    #[test]
    fn provision_dkim_stores_key_and_renders_a_usable_record() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();

        let record = provision_dkim(&mut conn, 1, "acme.test").unwrap();
        assert_eq!(record.name, "nosdesk._domainkey.acme.test");
        assert!(record.txt_value.starts_with("v=DKIM1; k=rsa; p="));
        assert!(record.txt_value.len() > 200, "public key looks too short");

        let row = get(&mut conn).unwrap().unwrap();
        assert_eq!(row.sending_mode, "verified_domain");
        assert_eq!(row.verification_status, "pending");
        assert_eq!(row.sending_domain.as_deref(), Some("acme.test"));
        assert_eq!(row.dkim_selector.as_deref(), Some("nosdesk"));
        assert!(row.encrypted_dkim_private_key.is_some());
        assert!(row.dkim_kek_id.is_some());

        // The stored key decrypts to a PKCS#1 PEM that the signing layer (lettre)
        // accepts, and the rendered record is stable across reads.
        let pem = decrypt_dkim_key(&row).unwrap().unwrap();
        assert!(pem.contains("BEGIN RSA PRIVATE KEY"));
        lettre::message::dkim::DkimSigningKey::new(
            &pem,
            lettre::message::dkim::DkimSigningAlgorithm::Rsa,
        )
        .expect("provisioned key must be valid for DKIM signing");
        assert_eq!(
            dns_record_for(&row).unwrap().unwrap().txt_value,
            record.txt_value
        );

        // The same key converts to the base64 PKCS#8 DER that SES BYODKIM wants,
        // and the bytes round-trip back to a parseable PKCS#8 private key.
        use rsa::pkcs8::DecodePrivateKey;
        let b64 = dkim_private_pkcs8_b64(&pem).unwrap();
        let der = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .unwrap();
        rsa::RsaPrivateKey::from_pkcs8_der(&der).expect("SES key must be valid PKCS#8 DER");
    }
}
