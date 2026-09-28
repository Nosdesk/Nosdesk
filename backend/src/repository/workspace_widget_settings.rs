//! A workspace's embeddable widget settings (`workspace_widget_settings`). One
//! row per workspace, RLS-isolated, so every call runs workspace-pinned.
//!
//! The signing secret is sealed like the requester-SSO client secret:
//! AES-256-GCM under the keyring, workspace id in the AAD, `kek_id` sidecar.
//! It's generated here, shown to the admin once, and otherwise only unsealed to
//! verify a visitor token.

use diesel::prelude::*;
use rand::RngCore;

use crate::db::DbConnection;
use crate::models::WorkspaceWidgetSettings;
use crate::repository::channels::CredentialError;
use crate::schema::workspace_widget_settings as w;
use crate::utils::encryption;

const AAD_TAG: &[u8] = b".nosdesk.workspace.widget_secret.v1";

fn aad(workspace_id: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + AAD_TAG.len());
    buf.extend_from_slice(&workspace_id.to_be_bytes());
    buf.extend_from_slice(AAD_TAG);
    buf
}

/// The workspace's widget settings, if it has any.
pub fn get(conn: &mut DbConnection) -> QueryResult<Option<WorkspaceWidgetSettings>> {
    w::table
        .select(WorkspaceWidgetSettings::as_select())
        .first(conn)
        .optional()
}

// sync-audit-only: workspace configuration, recorded by the audit trigger (secret redacted)
/// Save the widget settings (not the secret; see [`rotate_secret`]).
pub fn save(
    conn: &mut DbConnection,
    enabled: bool,
    allowed_origins: &[String],
    allow_anonymous: bool,
) -> QueryResult<WorkspaceWidgetSettings> {
    let origins: Vec<Option<String>> = allowed_origins.iter().cloned().map(Some).collect();
    diesel::insert_into(w::table)
        .values((
            w::enabled.eq(enabled),
            w::allowed_origins.eq(&origins),
            w::allow_anonymous.eq(allow_anonymous),
        ))
        .on_conflict(w::workspace_id)
        .do_update()
        .set((
            w::enabled.eq(enabled),
            w::allowed_origins.eq(&origins),
            w::allow_anonymous.eq(allow_anonymous),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .returning(WorkspaceWidgetSettings::as_returning())
        .get_result(conn)
}

// sync-audit-only: workspace configuration, recorded by the audit trigger (secret redacted)
/// Generate a new signing secret (32 random bytes, hex), store it sealed and
/// return the plaintext once. Tokens signed with the old secret stop working.
pub fn rotate_secret(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> Result<String, CredentialError> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let kr = encryption::keyring();
    let blob = kr
        .encrypt(secret.as_bytes(), &aad(workspace_id))
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    let kek_id = kr.current_version() as i16;
    diesel::insert_into(w::table)
        .values((
            w::encrypted_secret.eq(Some(&blob)),
            w::encrypted_kek_id.eq(Some(kek_id)),
        ))
        .on_conflict(w::workspace_id)
        .do_update()
        .set((
            w::encrypted_secret.eq(Some(&blob)),
            w::encrypted_kek_id.eq(Some(kek_id)),
            w::updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)?;
    Ok(secret)
}

/// The unsealed signing secret, or `Ok(None)` when none has been generated.
pub fn secret(row: &WorkspaceWidgetSettings) -> Result<Option<String>, CredentialError> {
    let Some(blob) = &row.encrypted_secret else {
        return Ok(None);
    };
    let sidecar = row.encrypted_kek_id.ok_or_else(|| {
        CredentialError::Crypto("widget secret present but kek_id sidecar is null".into())
    })?;
    let blob_kek = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    if blob_kek as i16 != sidecar {
        return Err(CredentialError::Crypto(
            "widget secret sidecar kek_id disagrees with blob".into(),
        ));
    }
    let bytes = encryption::keyring()
        .decrypt(blob, &aad(row.workspace_id))
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    String::from_utf8(bytes.to_vec())
        .map(Some)
        .map_err(|_| CredentialError::Crypto("widget secret is not valid UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    #[test]
    fn the_secret_is_sealed_and_rotation_replaces_it() {
        let mut conn = setup_test_connection();
        assert!(get(&mut conn).unwrap().is_none());
        save(&mut conn, true, &["https://help.acme.test".into()], false).unwrap();
        let first = rotate_secret(&mut conn, 1).unwrap();
        let row = get(&mut conn).unwrap().unwrap();
        assert_ne!(row.encrypted_secret.as_deref(), Some(first.as_bytes()));
        assert_eq!(secret(&row).unwrap().as_deref(), Some(first.as_str()));
        assert_eq!(row.origins(), vec!["https://help.acme.test".to_string()]);
        assert!(!row.allow_anonymous);

        let second = rotate_secret(&mut conn, 1).unwrap();
        assert_ne!(first, second);
        let row = get(&mut conn).unwrap().unwrap();
        assert_eq!(secret(&row).unwrap().as_deref(), Some(second.as_str()));
        // Saving settings keeps the secret.
        save(&mut conn, false, &[], true).unwrap();
        assert!(get(&mut conn).unwrap().unwrap().encrypted_secret.is_some());
    }
}
