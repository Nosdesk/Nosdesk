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

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{UpsertWorkspaceEmailSettings, WorkspaceEmailSettings};
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
}
