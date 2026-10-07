//! Who a person vouched for by the workspace's own identity provider is.
//! Shared by requester SSO (portal sign-in) and the Microsoft Teams tab, so a
//! person reaches the same account whichever way they come in.
//!
//! A sign-in carries one or more keys the provider keeps stable for the person
//! (most durable first). In order:
//! 1. an account already linked to any of those keys in this workspace;
//! 2. otherwise the email the provider asserts, only at a domain the admin
//!    listed and never when the provider marks it unverified: it links the
//!    account with that address, or creates a requester;
//! 3. otherwise a refusal.
//!
//! The keys not yet linked are then linked to the account, so the next sign-in
//! finds it by key, whatever the email says by then. Keying on the provider's
//! ids, not the email, is what stops a changed or unverified email from
//! reaching someone else's account.
//!
//! Microsoft Entra keys: `entra:{tenant id}` + object id (the same for every
//! app in the tenant, so the portal and the Teams tab agree), and for requester
//! SSO also the issuer + `sub` (per app).

use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::repository::user_auth_identities as identities;

/// One stable id for the person: `provider_type` + `external_id` in
/// `user_auth_identities`, scoped to the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityKey {
    pub provider_type: String,
    pub external_id: String,
}

/// A person's Entra tenant and object id.
pub fn entra_key(tenant_id: &str, object_id: &str) -> IdentityKey {
    IdentityKey {
        provider_type: format!("entra:{}", tenant_id.to_lowercase()),
        external_id: object_id.to_lowercase(),
    }
}

/// What the provider says about the person.
pub struct Assertion<'a> {
    /// Most durable first.
    pub keys: &'a [IdentityKey],
    pub email: Option<&'a str>,
    /// `Some(false)` when the provider says the address is unverified.
    pub email_verified: Option<bool>,
    pub name: Option<&'a str>,
    /// Recorded on new links (`requester_sso`, `teams`).
    pub via: &'a str,
}

/// Why a verified sign-in still can't become a session.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// No email at a listed domain (or the provider marked it unverified).
    Domain,
}

// sync-audit-only: links identities, provisions the requester and seat through their own audited repository writes
/// The person's account in `workspace_id`, found, linked or created as the
/// module doc describes, with a member seat and their primary email marked
/// verified (the provider vouches for it). Run workspace-pinned.
pub fn resolve(
    conn: &mut DbConnection,
    workspace_id: i32,
    allowed_domains: &[String],
    assertion: &Assertion<'_>,
) -> Result<Result<Uuid, Refused>, diesel::result::Error> {
    let mut linked = Vec::with_capacity(assertion.keys.len());
    for key in assertion.keys {
        linked.push(identities::find_user_by_scoped_identity(
            workspace_id,
            &key.provider_type,
            &key.external_id,
            conn,
        )?);
    }

    let email = assertion
        .email
        .map(|e| e.trim().to_lowercase())
        .filter(|e| e.contains('@'));
    let user = match linked.iter().flatten().next() {
        Some(user) => *user,
        None => {
            let Some(email) = email.as_deref() else {
                return Ok(Err(Refused::Domain));
            };
            let domain = email.rsplit('@').next().unwrap_or_default();
            if assertion.email_verified == Some(false)
                || !allowed_domains.iter().any(|d| d == domain)
            {
                return Ok(Err(Refused::Domain));
            }
            let name = assertion
                .name
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .or_else(|| crate::utils::name_from_email(email))
                .unwrap_or_else(|| email.to_string());
            crate::repository::user_helpers::find_or_provision_requester(email, &name, conn, None)?
                .uuid
        }
    };

    // Link the keys nobody holds yet (a key held by someone else stays theirs).
    for (key, holder) in assertion.keys.iter().zip(&linked) {
        if holder.is_none() {
            identities::create_identity(
                crate::models::NewUserAuthIdentity {
                    user_uuid: user,
                    provider_type: key.provider_type.clone(),
                    external_id: key.external_id.clone(),
                    email: email.clone(),
                    metadata: Some(json!({ "via": assertion.via })),
                    password_hash: None,
                    workspace_id: Some(workspace_id),
                },
                conn,
            )?;
        }
    }

    // Already a member: a no-op (ON CONFLICT DO NOTHING).
    crate::repository::workspaces::add_membership(
        conn,
        workspace_id,
        user,
        "member",
        crate::repository::workspaces::SeatWriteAuthority::Product,
    )?;
    crate::repository::user_emails::mark_primary_verified(conn, &user)?;
    Ok(Ok(user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    fn key(p: &str, id: &str) -> IdentityKey {
        IdentityKey {
            provider_type: p.into(),
            external_id: id.into(),
        }
    }

    fn assert_as<'a>(keys: &'a [IdentityKey], email: Option<&'a str>) -> Assertion<'a> {
        Assertion {
            keys,
            email,
            email_verified: None,
            name: Some("Sam"),
            via: "test",
        }
    }

    #[test]
    fn the_portal_and_the_teams_tab_reach_the_same_person() {
        let mut conn = setup_test_connection();
        let domains = ["ri-test.example".to_string()];
        let entra = entra_key("11111111-2222-3333-4444-555555555555", "AAAA-oid");
        // Requester SSO: the Entra key plus the per-app issuer + sub.
        let sso = [
            entra.clone(),
            key("https://login.microsoftonline.com/t/v2.0", "sub-1"),
        ];
        let person = resolve(
            &mut conn,
            1,
            &domains,
            &assert_as(&sso, Some("sam@ri-test.example")),
        )
        .unwrap()
        .unwrap();
        assert!(crate::middleware::cookie_auth::is_workspace_member(
            &mut conn, 1, person
        ));

        // The Teams tab: only the Entra key, and an email that has since changed.
        let teams = [entra];
        assert_eq!(
            resolve(
                &mut conn,
                1,
                &domains,
                &assert_as(&teams, Some("sam.new@elsewhere.example"))
            )
            .unwrap(),
            Ok(person),
            "the key wins over the email",
        );
    }

    #[test]
    fn a_new_key_links_by_email_and_emails_are_trusted_only_at_listed_domains() {
        let mut conn = setup_test_connection();
        let domains = ["ri-link.example".to_string()];
        let first = [key("issuer", "old-sub")];
        let person = resolve(
            &mut conn,
            1,
            &domains,
            &assert_as(&first, Some("kim@ri-link.example")),
        )
        .unwrap()
        .unwrap();
        // An existing SSO user opens the Teams tab for the first time.
        let teams = [entra_key("t", "kim-oid")];
        assert_eq!(
            resolve(
                &mut conn,
                1,
                &domains,
                &assert_as(&teams, Some("Kim@RI-link.example"))
            )
            .unwrap(),
            Ok(person)
        );
        assert_eq!(
            resolve(&mut conn, 1, &domains, &assert_as(&teams, None)).unwrap(),
            Ok(person),
            "now linked by key"
        );

        let stranger = [entra_key("t", "eve-oid")];
        assert_eq!(
            resolve(
                &mut conn,
                1,
                &domains,
                &assert_as(&stranger, Some("eve@elsewhere.example"))
            )
            .unwrap(),
            Err(Refused::Domain)
        );
        let unverified = Assertion {
            email_verified: Some(false),
            ..assert_as(&stranger, Some("eve@ri-link.example"))
        };
        assert_eq!(
            resolve(&mut conn, 1, &domains, &unverified).unwrap(),
            Err(Refused::Domain)
        );
        assert_eq!(
            resolve(&mut conn, 1, &domains, &assert_as(&stranger, None)).unwrap(),
            Err(Refused::Domain)
        );
    }
}
