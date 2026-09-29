//! A workspace's requester sign-in provider (`workspace_identity_providers`).
//! One row per workspace, RLS-isolated, so every call runs workspace-pinned.
//!
//! The client secret is stored KEK-encrypted exactly like the LDAP bind
//! password: framed AES-256-GCM with the workspace id bound into the AAD, and a
//! `kek_id` sidecar. The plaintext exists only transiently in `save` and
//! `client_secret`.

use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::WorkspaceIdentityProvider;
use crate::repository::channels::CredentialError;
use crate::schema::workspace_identity_providers as p;
use crate::utils::encryption;

/// AAD purpose tag, distinct from the email/LDAP tags so a blob can't be
/// swapped across credential slots.
const AAD_TAG: &[u8] = b".nosdesk.workspace.requester_sso.v1";

fn aad(workspace_id: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + AAD_TAG.len());
    buf.extend_from_slice(&workspace_id.to_be_bytes());
    buf.extend_from_slice(AAD_TAG);
    buf
}

/// The workspace's provider, if one is configured.
pub fn get(conn: &mut DbConnection) -> QueryResult<Option<WorkspaceIdentityProvider>> {
    p::table
        .select(WorkspaceIdentityProvider::as_select())
        .first(conn)
        .optional()
}

/// The fields an admin sets. `client_secret: None` keeps the stored secret,
/// as long as the app (issuer and client id) is the same: a secret belongs to
/// one app, so pointing at another drops it.
pub struct ProviderInput<'a> {
    pub kind: &'a str,
    pub display_name: &'a str,
    pub issuer_url: &'a str,
    pub client_id: &'a str,
    pub client_secret: Option<&'a str>,
    pub allowed_domains: &'a [String],
    pub enabled: bool,
}

// sync-audit-only: workspace configuration, recorded by the audit trigger (secret redacted)
/// Create or replace the workspace's provider.
pub fn save(
    conn: &mut DbConnection,
    workspace_id: i32,
    input: &ProviderInput<'_>,
) -> Result<WorkspaceIdentityProvider, CredentialError> {
    let sealed = match input.client_secret {
        Some(secret) => {
            let kr = encryption::keyring();
            let blob = kr
                .encrypt(secret.as_bytes(), &aad(workspace_id))
                .map_err(|e| CredentialError::Crypto(e.to_string()))?;
            Some((blob, kr.current_version() as i16))
        }
        None => None,
    };
    let domains: Vec<Option<String>> = input.allowed_domains.iter().cloned().map(Some).collect();
    let row = conn.transaction(|conn| {
        let another_app = get(conn)?
            .is_some_and(|p| p.issuer_url != input.issuer_url || p.client_id != input.client_id);
        diesel::insert_into(p::table)
            .values((
                p::kind.eq(input.kind),
                p::display_name.eq(input.display_name),
                p::issuer_url.eq(input.issuer_url),
                p::client_id.eq(input.client_id),
                p::allowed_domains.eq(&domains),
                p::enabled.eq(input.enabled),
            ))
            .on_conflict(p::workspace_id)
            .do_update()
            .set((
                p::kind.eq(input.kind),
                p::display_name.eq(input.display_name),
                p::issuer_url.eq(input.issuer_url),
                p::client_id.eq(input.client_id),
                p::allowed_domains.eq(&domains),
                p::enabled.eq(input.enabled),
                p::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;
        if let Some((blob, kek_id)) = &sealed {
            diesel::update(p::table)
                .set((
                    p::encrypted_client_secret.eq(Some(blob)),
                    p::encrypted_kek_id.eq(Some(kek_id)),
                ))
                .execute(conn)?;
        } else if another_app {
            diesel::update(p::table)
                .set((
                    p::encrypted_client_secret.eq(None::<Vec<u8>>),
                    p::encrypted_kek_id.eq(None::<i16>),
                ))
                .execute(conn)?;
        }
        p::table
            .select(WorkspaceIdentityProvider::as_select())
            .first(conn)
    })?;
    Ok(row)
}

// sync-audit-only: workspace configuration, recorded by the audit trigger (secret redacted)
/// Turn the Microsoft Teams tab on or off. `None` when no provider is set up.
pub fn set_teams_enabled(
    conn: &mut DbConnection,
    enabled: bool,
) -> QueryResult<Option<WorkspaceIdentityProvider>> {
    diesel::update(p::table)
        .set((
            p::teams_enabled.eq(enabled),
            p::updated_at.eq(diesel::dsl::now),
        ))
        .returning(WorkspaceIdentityProvider::as_returning())
        .get_result(conn)
        .optional()
}

// sync-audit-only: workspace configuration, recorded by the audit trigger (secret redacted)
/// Remove the workspace's provider (requesters go back to email sign-in only).
pub fn delete(conn: &mut DbConnection) -> QueryResult<usize> {
    diesel::delete(p::table).execute(conn)
}

/// The decrypted client secret, or `Ok(None)` when none is stored. Verifies the
/// sidecar `kek_id` against the blob header and the workspace-bound AAD.
pub fn client_secret(row: &WorkspaceIdentityProvider) -> Result<Option<String>, CredentialError> {
    let Some(blob) = &row.encrypted_client_secret else {
        return Ok(None);
    };
    let sidecar = row.encrypted_kek_id.ok_or_else(|| {
        CredentialError::Crypto("client secret present but kek_id sidecar is null".into())
    })?;
    let blob_kek = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    if blob_kek as i16 != sidecar {
        return Err(CredentialError::Crypto(
            "requester SSO secret sidecar kek_id disagrees with blob".into(),
        ));
    }
    let bytes = encryption::keyring()
        .decrypt(blob, &aad(row.workspace_id))
        .map_err(|e| CredentialError::Crypto(e.to_string()))?;
    String::from_utf8(bytes.to_vec())
        .map(Some)
        .map_err(|_| CredentialError::Crypto("client secret is not valid UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    fn input(secret: Option<&str>) -> ProviderInput<'_> {
        ProviderInput {
            kind: "entra",
            display_name: "Microsoft",
            issuer_url: "https://login.microsoftonline.com/acme/v2.0",
            client_id: "client",
            client_secret: secret,
            allowed_domains: &[],
            enabled: true,
        }
    }

    #[test]
    fn the_client_secret_is_sealed_and_kept_when_not_resent() {
        let mut conn = setup_test_connection();
        let saved = save(&mut conn, 1, &input(Some("s3cret"))).unwrap();
        assert_ne!(
            saved.encrypted_client_secret.as_deref(),
            Some(b"s3cret".as_slice())
        );
        assert_eq!(client_secret(&saved).unwrap().as_deref(), Some("s3cret"));

        // Saving again without a secret keeps the stored one.
        let resaved = save(&mut conn, 1, &input(None)).unwrap();
        assert_eq!(client_secret(&resaved).unwrap().as_deref(), Some("s3cret"));

        // Another app: the old app's secret isn't carried over.
        let other = save(
            &mut conn,
            1,
            &ProviderInput {
                client_id: "another-client",
                ..input(None)
            },
        )
        .unwrap();
        assert_eq!(client_secret(&other).unwrap(), None);
        save(&mut conn, 1, &input(Some("s3cret"))).unwrap();
        assert_eq!(get(&mut conn).unwrap().map(|p| p.id), Some(saved.id));

        let teams = set_teams_enabled(&mut conn, true).unwrap().unwrap();
        assert!(teams.teams_enabled);
        assert_eq!(
            teams.teams_app_id, saved.teams_app_id,
            "the Teams app id is stable"
        );
        assert!(!saved.teams_enabled, "off until an admin turns it on");

        assert_eq!(delete(&mut conn).unwrap(), 1);
        assert!(get(&mut conn).unwrap().is_none());
    }
}
