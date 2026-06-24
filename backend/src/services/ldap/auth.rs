//! LDAP search-then-bind authentication.
//!
//! The general flow that handles AD, OpenLDAP, and nested OUs uniformly:
//!   1. reject an empty/whitespace password (RFC 4513 §5.1.2: a zero-length
//!      password downgrades a simple bind to an anonymous bind that many servers
//!      return *success* for — a full auth bypass);
//!   2. bind as the read-only service account;
//!   3. subtree-search the user base with the RFC-4515-escaped login substituted
//!      into the filter template, requiring EXACTLY ONE entry;
//!   4. open a FRESH connection and simple-bind as the resolved DN with the
//!      user-supplied password — bind success is the authentication proof.
//!
//! Step 4 uses a fresh connection so the service account's bind isn't reused as
//! the user, and the user bind isn't left on a pooled/shared connection.

use ldap3::{LdapResult, Scope, SearchEntry};

use crate::models::WorkspaceLdapSettings;
use crate::services::ldap::attrs::{attr_name, first_value};
use crate::services::ldap::connector::{self, LdapConnectError};
use crate::services::ldap::escape::escape_filter_value;

#[derive(Debug, thiserror::Error)]
pub enum LdapAuthError {
    #[error("password must not be empty")]
    EmptyPassword,
    #[error("LDAP connect: {0}")]
    Connect(#[from] LdapConnectError),
    #[error("service account bind failed (check bind_dn / bind password)")]
    ServiceBind,
    #[error("user not found")]
    UserNotFound,
    #[error("the search matched {0} entries; expected exactly one")]
    AmbiguousUser(usize),
    #[error("the directory entry is missing its external-id attribute '{0}'")]
    MissingExternalId(String),
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("LDAP error: {0}")]
    Ldap(#[from] ldap3::LdapError),
}

/// The identity resolved from a successful LDAP authentication, ready to feed
/// the scoped identity provisioner (`provider_type = "ldap"`, the external_id
/// keyed within the workspace).
#[derive(Debug, Clone)]
pub struct LdapAuthnResult {
    /// The immutable directory id (entryUUID/objectGUID), used as external_id.
    pub external_id: String,
    pub dn: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

/// Substitute the (RFC-4515-escaped) login into the filter template. Public for
/// the injection test; never build a filter by raw concatenation.
pub fn build_user_filter(template: &str, username: &str) -> String {
    template.replace("{username}", &escape_filter_value(username))
}

fn is_success(res: &LdapResult) -> bool {
    res.rc == 0
}

/// Authenticate `username`/`user_password` against the workspace's directory.
/// `bind_password` is the decrypted service-account password. Returns the
/// resolved directory identity on success.
pub async fn authenticate(
    settings: &WorkspaceLdapSettings,
    bind_password: &str,
    username: &str,
    user_password: &str,
) -> Result<LdapAuthnResult, LdapAuthError> {
    // 1. Empty-password guard, BEFORE any network call.
    if user_password.trim().is_empty() {
        return Err(LdapAuthError::EmptyPassword);
    }

    let ext_id_attr = attr_name(&settings.attribute_map, "external_id", "entryUUID");
    let email_attr = attr_name(&settings.attribute_map, "email", "mail");
    let name_attr = attr_name(&settings.attribute_map, "display_name", "cn");

    // 2. Service bind.
    let mut svc = connector::connect(settings).await?;
    let svc_bind = svc.simple_bind(&settings.bind_dn, bind_password).await?;
    if !is_success(&svc_bind) {
        return Err(LdapAuthError::ServiceBind);
    }

    // 3. Escaped subtree search, requiring exactly one entry.
    let filter = build_user_filter(&settings.user_filter, username);
    let attrs = vec![ext_id_attr.clone(), email_attr.clone(), name_attr.clone()];
    let (entries, _res) = svc
        .search(&settings.user_base_dn, Scope::Subtree, &filter, attrs)
        .await?
        .success()?;
    let _ = svc.unbind().await;

    let entry = match entries.len() {
        1 => SearchEntry::construct(entries.into_iter().next().unwrap()),
        0 => return Err(LdapAuthError::UserNotFound),
        n => return Err(LdapAuthError::AmbiguousUser(n)),
    };

    let dn = entry.dn.clone();
    let external_id =
        first_value(&entry, &ext_id_attr).ok_or(LdapAuthError::MissingExternalId(ext_id_attr))?;
    let email = first_value(&entry, &email_attr);
    let display_name = first_value(&entry, &name_attr);

    // 4. Fresh connection, user bind = the authentication proof.
    let mut user_conn = connector::connect(settings).await?;
    let user_bind = user_conn.simple_bind(&dn, user_password).await?;
    let _ = user_conn.unbind().await;
    if !is_success(&user_bind) {
        return Err(LdapAuthError::InvalidCredentials);
    }

    Ok(LdapAuthnResult {
        external_id,
        dn,
        email,
        display_name,
    })
}

/// Validate a config by connecting and service-binding only (no user search).
/// Used by the admin test-connection endpoint to surface config problems.
pub async fn test_connection(
    settings: &WorkspaceLdapSettings,
    bind_password: &str,
) -> Result<(), LdapAuthError> {
    let mut svc = connector::connect(settings).await?;
    let res = svc.simple_bind(&settings.bind_dn, bind_password).await?;
    let _ = svc.unbind().await;
    if !is_success(&res) {
        return Err(LdapAuthError::ServiceBind);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_user_filter_escapes_injection() {
        // The classic LDAP filter injection must be neutralised, not break out.
        assert_eq!(
            build_user_filter("(&(objectClass=user)(uid={username}))", "*)(uid=*"),
            "(&(objectClass=user)(uid=\\2a\\29\\28uid=\\2a))"
        );
        // A normal login substitutes verbatim.
        assert_eq!(
            build_user_filter("(uid={username})", "alice"),
            "(uid=alice)"
        );
    }

    fn settings() -> WorkspaceLdapSettings {
        WorkspaceLdapSettings {
            workspace_id: 1,
            enabled: true,
            host: "127.0.0.1".into(),
            port: 636,
            tls_mode: "ldaps".into(),
            verify_certs: true,
            ca_cert_pem: None,
            follow_referrals: false,
            connect_timeout_secs: 3,
            auth_mode: "simple_bind".into(),
            bind_dn: "cn=svc,dc=acme,dc=test".into(),
            encrypted_bind_password: None,
            encrypted_kek_id: None,
            user_base_dn: "ou=people,dc=acme,dc=test".into(),
            username_attribute: "uid".into(),
            user_filter: "(uid={username})".into(),
            page_size: 500,
            attribute_map: json!({}),
            group_config: json!({}),
            provisioning: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    // The empty-password guard short-circuits before any network call, so this
    // needs no server. A full bind/search roundtrip is covered by the P3 Samba
    // integration harness.
    #[tokio::test]
    async fn rejects_an_empty_password_before_connecting() {
        let err = authenticate(&settings(), "svc-pw", "alice", "   ")
            .await
            .unwrap_err();
        assert!(matches!(err, LdapAuthError::EmptyPassword), "got {err:?}");
    }
}
