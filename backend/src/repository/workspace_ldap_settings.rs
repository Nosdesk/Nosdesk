//! Per-workspace LDAP/directory configuration (the `workspace_ldap_settings`
//! table). One row per workspace, RLS-isolated by `workspace_id`, so every
//! function here MUST run workspace-scoped (`app.workspace_id` set via
//! `TenantConn` / `with_actor_context` / `run_in_workspace`). That GUC both
//! scopes the read and fills the `workspace_id` default on insert.
//!
//! The bind password is stored KEK-encrypted exactly like the SMTP password in
//! `workspace_email_settings`: a framed AES-256-GCM blob plus a `kek_id`
//! sidecar, with the `workspace_id` bound into the AAD so a blob can't be
//! swapped between workspaces by anyone with raw SQL write. The plaintext only
//! ever exists transiently in `set_bind_password` / `decrypt_bind_password`.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{UpsertWorkspaceLdapSettings, WorkspaceLdapSettings, WorkspaceLdapSyncState};
use crate::repository::channels::CredentialError;
use crate::utils::encryption;

/// AAD purpose tag for the workspace LDAP bind password. Combined with the
/// workspace id (RFC 5116 §1.2 bind-context) so a ciphertext lifted into a
/// different workspace's row fails to decrypt; distinct from the email/DKIM tags
/// so a blob can't be swapped across credential slots.
const WS_LDAP_AAD_TAG: &[u8] = b".nosdesk.workspace.ldap.v1";

fn ws_ldap_aad(workspace_id: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + WS_LDAP_AAD_TAG.len());
    buf.extend_from_slice(&workspace_id.to_be_bytes());
    buf.extend_from_slice(WS_LDAP_AAD_TAG);
    buf
}

/// The current workspace's LDAP settings, or `None` if never configured. RLS
/// scopes the read to the request's workspace.
pub fn get(conn: &mut DbConnection) -> QueryResult<Option<WorkspaceLdapSettings>> {
    use crate::schema::workspace_ldap_settings::dsl as w;
    w::workspace_ldap_settings
        .select(WorkspaceLdapSettings::as_select())
        .first(conn)
        .optional()
}

/// Settings for an explicit `workspace_id`, filtering by id rather than relying
/// solely on the RLS GUC, so it is correct on a pinned connection (filter agrees
/// with RLS) and on a bypass connection (the filter does the scoping). The
/// background sync uses it from whatever connection context it holds.
pub fn get_for_workspace(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> QueryResult<Option<WorkspaceLdapSettings>> {
    use crate::schema::workspace_ldap_settings::dsl as w;
    w::workspace_ldap_settings
        .filter(w::workspace_id.eq(workspace_id))
        .select(WorkspaceLdapSettings::as_select())
        .first(conn)
        .optional()
}

// sync-audit-only: Workspace LDAP config; covered by the audit_log trigger on workspace_ldap_settings (bind password redacted), sync clients don't subscribe.
/// Insert or update the editable settings for the current workspace. The
/// encrypted bind-password columns are left untouched (managed by
/// `set_bind_password` / `clear_bind_password`), so saving settings never
/// disturbs a stored password.
pub fn upsert(
    conn: &mut DbConnection,
    fields: UpsertWorkspaceLdapSettings,
) -> QueryResult<WorkspaceLdapSettings> {
    use crate::schema::workspace_ldap_settings::dsl as w;
    use diesel::upsert::excluded;

    diesel::insert_into(w::workspace_ldap_settings)
        .values(&fields)
        .on_conflict(w::workspace_id)
        .do_update()
        .set((
            w::enabled.eq(excluded(w::enabled)),
            w::host.eq(excluded(w::host)),
            w::port.eq(excluded(w::port)),
            w::tls_mode.eq(excluded(w::tls_mode)),
            w::verify_certs.eq(excluded(w::verify_certs)),
            w::ca_cert_pem.eq(excluded(w::ca_cert_pem)),
            w::follow_referrals.eq(excluded(w::follow_referrals)),
            w::connect_timeout_secs.eq(excluded(w::connect_timeout_secs)),
            w::auth_mode.eq(excluded(w::auth_mode)),
            w::bind_dn.eq(excluded(w::bind_dn)),
            w::user_base_dn.eq(excluded(w::user_base_dn)),
            w::username_attribute.eq(excluded(w::username_attribute)),
            w::user_filter.eq(excluded(w::user_filter)),
            w::page_size.eq(excluded(w::page_size)),
            w::attribute_map.eq(excluded(w::attribute_map)),
            w::group_config.eq(excluded(w::group_config)),
            w::provisioning.eq(excluded(w::provisioning)),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .returning(WorkspaceLdapSettings::as_returning())
        .get_result(conn)
}

// sync-audit-only: Workspace LDAP bind password; encrypted at rest, never emitted; the workspace_ldap_settings audit trigger redacts it.
/// Encrypt and store the bind password for the current workspace. The settings
/// row must already exist (call `upsert` first); `workspace_id` is the row's id
/// and is bound into the AAD.
pub fn set_bind_password(
    conn: &mut DbConnection,
    workspace_id: i32,
    plaintext: &str,
) -> Result<(), CredentialError> {
    use crate::schema::workspace_ldap_settings::dsl as w;

    let kr = encryption::keyring();
    let aad = ws_ldap_aad(workspace_id);
    let encrypted = kr
        .encrypt(plaintext.as_bytes(), &aad)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let kek_id = kr.current_version() as i16;

    let updated = diesel::update(w::workspace_ldap_settings)
        .filter(w::workspace_id.eq(workspace_id))
        .set((
            w::encrypted_bind_password.eq(Some(encrypted)),
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

// sync-audit-only: Workspace LDAP bind password clear; covered by the workspace_ldap_settings audit trigger (redacted).
/// Remove any stored bind password for the current workspace.
pub fn clear_bind_password(conn: &mut DbConnection) -> QueryResult<()> {
    use crate::schema::workspace_ldap_settings::dsl as w;
    diesel::update(w::workspace_ldap_settings)
        .set((
            w::encrypted_bind_password.eq::<Option<Vec<u8>>>(None),
            w::encrypted_kek_id.eq::<Option<i16>>(None),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

/// Decrypt the stored bind password for `row`, or `Ok(None)` when none is
/// stored. Verifies the sidecar `kek_id` against the blob header and binds
/// `workspace_id` into the AAD, mirroring `workspace_email_settings`.
pub fn decrypt_bind_password(
    row: &WorkspaceLdapSettings,
) -> Result<Option<String>, CredentialError> {
    let blob = match &row.encrypted_bind_password {
        Some(b) => b,
        None => return Ok(None),
    };
    let sidecar = row.encrypted_kek_id.ok_or_else(|| {
        CredentialError::Crypto("bind password blob present but kek_id sidecar is null".into())
    })?;

    // Authoritative kek_id is the one inside the blob; the sidecar is an indexed
    // mirror. Disagreement means a write skipped the sidecar or the column was
    // patched directly. Reject either.
    let blob_kek_id = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    if blob_kek_id as i16 != sidecar {
        return Err(CredentialError::Crypto(format!(
            "workspace_ldap_settings ws#{} sidecar kek_id ({}) disagrees with blob ({})",
            row.workspace_id, sidecar, blob_kek_id
        )));
    }

    let aad = ws_ldap_aad(row.workspace_id);
    let plaintext_bytes = encryption::keyring()
        .decrypt(blob, &aad)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let s = String::from_utf8(plaintext_bytes.to_vec())
        .map_err(|_| CredentialError::Crypto("bind password is not valid UTF-8".into()))?;
    Ok(Some(s))
}

// ---- DirSync cursor state --------------------------------------------------

/// The DirSync cursor state for the current workspace, if any. RLS scopes it.
pub fn get_sync_state(conn: &mut DbConnection) -> QueryResult<Option<WorkspaceLdapSyncState>> {
    use crate::schema::workspace_ldap_sync_state::dsl as s;
    s::workspace_ldap_sync_state
        .select(WorkspaceLdapSyncState::as_select())
        .first(conn)
        .optional()
}

// sync-audit-only: operational DirSync cursor (opaque cookie); not a sync aggregate
/// Persist the DirSync cookie for the workspace (upsert). `None` clears the
/// cursor so the next run is a full sync.
pub fn set_cookie(
    conn: &mut DbConnection,
    workspace_id: i32,
    mechanism: &str,
    cookie: Option<&[u8]>,
) -> QueryResult<()> {
    use crate::schema::workspace_ldap_sync_state::dsl as s;
    use diesel::upsert::excluded;
    diesel::insert_into(s::workspace_ldap_sync_state)
        .values((
            s::workspace_id.eq(workspace_id),
            s::mechanism.eq(mechanism),
            s::cookie.eq(cookie.map(|c| c.to_vec())),
        ))
        .on_conflict(s::workspace_id)
        .do_update()
        .set((
            s::mechanism.eq(excluded(s::mechanism)),
            s::cookie.eq(excluded(s::cookie)),
            s::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(())
}

// sync-audit-only: operational DirSync reconcile timestamp; not a sync aggregate
/// Stamp the last-full-reconcile time for the workspace. The state row must
/// already exist (set_cookie creates it on the first sync).
pub fn mark_full_reconcile(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<()> {
    use crate::schema::workspace_ldap_sync_state::dsl as s;
    diesel::update(s::workspace_ldap_sync_state.find(workspace_id))
        .set(s::last_full_reconcile_at.eq(diesel::dsl::now))
        .execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;
    use serde_json::json;

    #[test]
    fn sync_state_cookie_roundtrips() {
        let mut conn = setup_test_connection();
        assert!(get_sync_state(&mut conn).unwrap().is_none());

        set_cookie(&mut conn, 1, "dirsync", Some(&[0xca, 0xfe])).unwrap();
        let st = get_sync_state(&mut conn).unwrap().unwrap();
        assert_eq!(st.cookie.as_deref(), Some(&[0xca, 0xfe][..]));
        assert_eq!(st.mechanism, "dirsync");
        assert!(st.last_full_reconcile_at.is_none());

        // Upsert advances the cookie in place.
        set_cookie(&mut conn, 1, "dirsync", Some(&[0xbe, 0xef])).unwrap();
        mark_full_reconcile(&mut conn, 1).unwrap();
        let st = get_sync_state(&mut conn).unwrap().unwrap();
        assert_eq!(st.cookie.as_deref(), Some(&[0xbe, 0xef][..]));
        assert!(st.last_full_reconcile_at.is_some());

        // Clearing the cookie resets to a full sync.
        set_cookie(&mut conn, 1, "dirsync", None).unwrap();
        assert!(get_sync_state(&mut conn).unwrap().unwrap().cookie.is_none());
    }

    fn sample_fields() -> UpsertWorkspaceLdapSettings {
        UpsertWorkspaceLdapSettings {
            enabled: true,
            host: "dc01.acme.test".into(),
            port: 636,
            tls_mode: "ldaps".into(),
            verify_certs: true,
            ca_cert_pem: None,
            follow_referrals: false,
            connect_timeout_secs: 5,
            auth_mode: "simple_bind".into(),
            bind_dn: "cn=svc,ou=svc,dc=acme,dc=test".into(),
            user_base_dn: "ou=people,dc=acme,dc=test".into(),
            username_attribute: "sAMAccountName".into(),
            user_filter: "(&(objectClass=user)(sAMAccountName={username}))".into(),
            page_size: 500,
            attribute_map: json!({ "email": "mail", "external_id": "objectGUID" }),
            group_config: json!({ "membership_mode": "memberOf" }),
            provisioning: json!({ "jit_provision": true }),
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
        assert_eq!(row.host, "dc01.acme.test");
        assert_eq!(row.attribute_map["external_id"], "objectGUID");

        let mut changed = sample_fields();
        changed.enabled = false;
        changed.host = "dc02.acme.test".into();
        let row2 = upsert(&mut conn, changed).unwrap();
        assert_eq!(row2.workspace_id, 1);
        assert!(!row2.enabled);
        assert_eq!(row2.host, "dc02.acme.test");
    }

    #[test]
    fn set_bind_password_roundtrips_through_decrypt() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();

        set_bind_password(&mut conn, 1, "hunter2-bind").unwrap();

        let row = get(&mut conn).unwrap().unwrap();
        assert!(row.encrypted_bind_password.is_some());
        assert_eq!(
            decrypt_bind_password(&row).unwrap().as_deref(),
            Some("hunter2-bind")
        );
    }

    #[test]
    fn upsert_preserves_a_stored_bind_password() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        set_bind_password(&mut conn, 1, "keep-me").unwrap();

        let mut changed = sample_fields();
        changed.bind_dn = "cn=renamed,dc=acme,dc=test".into();
        upsert(&mut conn, changed).unwrap();

        let row = get(&mut conn).unwrap().unwrap();
        assert_eq!(row.bind_dn, "cn=renamed,dc=acme,dc=test");
        assert_eq!(
            decrypt_bind_password(&row).unwrap().as_deref(),
            Some("keep-me")
        );
    }

    #[test]
    fn clear_bind_password_removes_it() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        set_bind_password(&mut conn, 1, "to-be-cleared").unwrap();

        clear_bind_password(&mut conn).unwrap();

        let row = get(&mut conn).unwrap().unwrap();
        assert!(row.encrypted_bind_password.is_none());
        assert!(row.encrypted_kek_id.is_none());
        assert!(decrypt_bind_password(&row).unwrap().is_none());
    }

    #[test]
    fn set_bind_password_errors_when_no_row_exists() {
        let mut conn = setup_test_connection();
        let err = set_bind_password(&mut conn, 1, "orphan").unwrap_err();
        assert!(matches!(
            err,
            CredentialError::Db(diesel::result::Error::NotFound)
        ));
    }

    #[test]
    fn aad_is_workspace_bound_so_a_lifted_blob_fails() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        set_bind_password(&mut conn, 1, "secret").unwrap();
        let row = get(&mut conn).unwrap().unwrap();

        // Decrypting the workspace-1 blob under a different workspace's AAD must
        // fail (the workspace_id is bound into the AAD).
        let lifted = WorkspaceLdapSettings {
            workspace_id: 999,
            ..row
        };
        assert!(decrypt_bind_password(&lifted).is_err());
    }

    #[test]
    fn rls_hides_another_workspace_row() {
        let mut conn = setup_test_connection();
        upsert(&mut conn, sample_fields()).unwrap();
        assert!(get(&mut conn).unwrap().is_some());

        switch_workspace(&mut conn, 424242);
        assert!(
            get(&mut conn).unwrap().is_none(),
            "RLS must scope the read to the pinned workspace"
        );
    }
}
