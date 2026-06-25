//! LDAP user sync: a full directory scan that provisions/refreshes users,
//! mirroring the Microsoft Graph sync. Each directory entry maps to the same
//! transport-neutral sinks the Graph path uses: the scoped identity provisioner
//! (`provider_type="ldap"`) and `apply_directory_contact(source="ldap")`.
//!
//! v1 is a FULL scan (paged via RFC 2696 so AD's MaxPageSize doesn't truncate).
//! Incremental DirSync + the run model (sync_history, scheduling) land in later
//! P3 chunks; this chunk is the scan + mapping core.
//!
//! DEPROVISIONING is a deliberate v1 no-op: this provisions/refreshes only and
//! never disables users who vanished from the directory (plan open decision #5).
//! A terminated AD account fails its bind so it can't log in regardless.
//!
//! INTERLOCK for the future deprovision pass (review finding): each entry here
//! commits independently with no outer transaction, and `sync_users` returns
//! `Ok` ONLY on a clean stream EOF (a mid-scan error short-circuits via `?`), so
//! the caller's `completed` status is a genuine scan-complete signal. A
//! deprovision pass that disables "users not seen this scan" MUST gate on that
//! signal AND apply a mass-deletion circuit-breaker, or a scan that died after N
//! of M users would mass-disable the unseen M-N real users.

use ldap3::adapters::{Adapter, EntriesOnly, PagedResults};
use ldap3::{Scope, SearchEntry};
use serde_json::Value;
use tracing::warn;

use crate::db::DbConnection;
use crate::models::{DirectoryAddress, DirectoryContact, WorkspaceLdapSettings};
use crate::services::ldap::attrs::{all_values, attr_name, first_value};
use crate::services::ldap::auth::{ensure_bind_creds, LdapAuthError};
use crate::services::ldap::connector;
use crate::services::oauth_provisioning::{find_or_create_projected_user, ProjectedUserInput};

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct SyncStats {
    /// Directory entries returned by the search.
    pub seen: usize,
    /// Entries skipped (missing external_id or email — can't be provisioned).
    pub skipped: usize,
    /// Users provisioned or refreshed.
    pub synced: usize,
    /// Per-entry failures (provisioning or contact write); the scan continues.
    pub errors: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("LDAP auth/connect: {0}")]
    Auth(#[from] LdapAuthError),
    #[error("LDAP error: {0}")]
    Ldap(#[from] ldap3::LdapError),
}

/// The LDAP attribute names to read, resolved once from the workspace's
/// `attribute_map` with AD-flavoured defaults (overridable per provider).
struct AttrNames {
    external_id: String,
    email: String,
    display_name: String,
    title: String,
    organization: String,
    department: String,
    office: String,
    phone: String,
    mobile: String,
    street: String,
    city: String,
    region: String,
    postal: String,
    country: String,
}

fn resolve_attrs(map: &Value) -> AttrNames {
    AttrNames {
        external_id: attr_name(map, "external_id", "objectGUID"),
        email: attr_name(map, "email", "mail"),
        display_name: attr_name(map, "display_name", "displayName"),
        title: attr_name(map, "title", "title"),
        organization: attr_name(map, "organization", "company"),
        department: attr_name(map, "department", "department"),
        office: attr_name(map, "office_location", "physicalDeliveryOfficeName"),
        phone: attr_name(map, "phone", "telephoneNumber"),
        mobile: attr_name(map, "mobile", "mobile"),
        street: attr_name(map, "street", "streetAddress"),
        city: attr_name(map, "city", "l"),
        region: attr_name(map, "region", "st"),
        postal: attr_name(map, "postal_code", "postalCode"),
        country: attr_name(map, "country", "co"),
    }
}

impl AttrNames {
    /// The attribute set to request in the search.
    fn request_list(&self) -> Vec<&str> {
        vec![
            &self.external_id,
            &self.email,
            &self.display_name,
            &self.title,
            &self.organization,
            &self.department,
            &self.office,
            &self.phone,
            &self.mobile,
            &self.street,
            &self.city,
            &self.region,
            &self.postal,
            &self.country,
        ]
    }
}

/// A directory entry mapped to the identity + contact shapes the sinks consume.
pub struct MappedUser {
    pub external_id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub contact: DirectoryContact,
}

/// Map one entry. Returns `None` (skip) when it lacks the immutable external_id
/// or an email — both are required to provision a user cleanly.
fn map_entry(entry: &SearchEntry, a: &AttrNames) -> Option<MappedUser> {
    let external_id = first_value(entry, &a.external_id)?;
    let email = first_value(entry, &a.email)?;

    let mut phones: Vec<(String, String)> = Vec::new();
    for v in all_values(entry, &a.phone) {
        phones.push((v, "work".to_string()));
    }
    for v in all_values(entry, &a.mobile) {
        phones.push((v, "mobile".to_string()));
    }

    let street = first_value(entry, &a.street);
    let city = first_value(entry, &a.city);
    let region = first_value(entry, &a.region);
    let postal_code = first_value(entry, &a.postal);
    let country = first_value(entry, &a.country);
    let address = (street.is_some()
        || city.is_some()
        || region.is_some()
        || postal_code.is_some()
        || country.is_some())
    .then_some(DirectoryAddress {
        street,
        city,
        region,
        postal_code,
        country,
    });

    Some(MappedUser {
        external_id,
        email,
        display_name: first_value(entry, &a.display_name),
        contact: DirectoryContact {
            job_title: first_value(entry, &a.title),
            organization: first_value(entry, &a.organization),
            department: first_value(entry, &a.department),
            office_location: first_value(entry, &a.office),
            phones,
            address,
        },
    })
}

/// Turn the auth filter template into a list-all filter by replacing the
/// `{username}` placeholder with the presence wildcard `*` (NOT escaped — it's
/// an intentional presence match), so `(sAMAccountName={username})` becomes the
/// all-users `(sAMAccountName=*)`.
fn build_sync_filter(template: &str) -> String {
    template.replace("{username}", "*")
}

/// Run a full user sync for `workspace_id`. `conn` MUST be pinned to that
/// workspace (the provisioning + contact writes are RLS/audit-scoped). Returns
/// the run stats; per-entry errors are counted, not fatal.
pub async fn sync_users(
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    workspace_id: i32,
    bind_password: &str,
) -> Result<SyncStats, SyncError> {
    let attrs = resolve_attrs(&settings.attribute_map);
    let filter = build_sync_filter(&settings.user_filter);

    ensure_bind_creds(settings, bind_password).map_err(SyncError::Auth)?;
    let mut svc = connector::connect(settings)
        .await
        .map_err(LdapAuthError::Connect)?;
    if svc
        .with_timeout(connector::OP_TIMEOUT)
        .simple_bind(&settings.bind_dn, bind_password)
        .await?
        .rc
        != 0
    {
        return Err(SyncError::Auth(LdapAuthError::ServiceBind));
    }

    let page = settings.page_size.max(1) as i32;
    let adapters: Vec<Box<dyn Adapter<_, _>>> = vec![
        Box::new(EntriesOnly::new()),
        Box::new(PagedResults::new(page)),
    ];
    let mut stream = svc
        .with_timeout(connector::OP_TIMEOUT)
        .streaming_search_with(
            adapters,
            &settings.user_base_dn,
            Scope::Subtree,
            &filter,
            attrs.request_list(),
        )
        .await?;

    let mut stats = SyncStats::default();
    while let Some(re) = stream.next().await? {
        stats.seen += 1;
        let entry = SearchEntry::construct(re);
        let mapped = match map_entry(&entry, &attrs) {
            Some(m) => m,
            None => {
                stats.skipped += 1;
                continue;
            }
        };

        let input = ProjectedUserInput {
            iss: "ldap".to_string(),
            sub: mapped.external_id,
            identity_workspace_id: Some(workspace_id),
            email: mapped.email,
            email_verified: true,
            name: mapped.display_name,
            role: "member".to_string(),
            workspace_id,
            password_hash: None,
            metadata: None,
        };
        match find_or_create_projected_user(conn, input) {
            Ok(outcome) => {
                let uuid = outcome.into_user().uuid;
                if let Err(e) = crate::repository::user_contact::apply_directory_contact(
                    conn,
                    uuid,
                    "ldap",
                    &mapped.contact,
                    None,
                ) {
                    warn!(error = %e, "ldap sync: apply contact failed");
                    stats.errors += 1;
                } else {
                    stats.synced += 1;
                }
            }
            Err(e) => {
                warn!(error = %e, "ldap sync: provision failed");
                stats.errors += 1;
            }
        }
    }
    let _ = stream.finish().await;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn entry(attrs: &[(&str, &str)], bin: &[(&str, &[u8])]) -> SearchEntry {
        SearchEntry {
            dn: "cn=test,dc=acme,dc=test".to_string(),
            attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), vec![v.to_string()]))
                .collect(),
            bin_attrs: bin
                .iter()
                .map(|(k, v)| (k.to_string(), vec![v.to_vec()]))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn build_sync_filter_lists_all_users() {
        assert_eq!(
            build_sync_filter("(&(objectClass=user)(sAMAccountName={username}))"),
            "(&(objectClass=user)(sAMAccountName=*))"
        );
    }

    #[test]
    fn maps_an_ad_entry_with_binary_guid() {
        let e = entry(
            &[
                ("mail", "alice@acme.test"),
                ("displayName", "Alice Smith"),
                ("title", "Engineer"),
                ("company", "Acme"),
                ("department", "R&D"),
                ("physicalDeliveryOfficeName", "B12"),
                ("telephoneNumber", "111"),
                ("mobile", "222"),
                ("streetAddress", "1 St"),
                ("l", "Town"),
            ],
            &[("objectGUID", &[0x00, 0x0f, 0xff])],
        );
        let mapped = map_entry(&e, &resolve_attrs(&json!({}))).unwrap();
        assert_eq!(mapped.external_id, "000fff");
        assert_eq!(mapped.email, "alice@acme.test");
        assert_eq!(mapped.display_name.as_deref(), Some("Alice Smith"));
        assert_eq!(mapped.contact.job_title.as_deref(), Some("Engineer"));
        assert_eq!(mapped.contact.organization.as_deref(), Some("Acme"));
        assert_eq!(mapped.contact.office_location.as_deref(), Some("B12"));
        assert_eq!(mapped.contact.phones.len(), 2);
        assert!(mapped
            .contact
            .phones
            .contains(&("111".into(), "work".into())));
        assert!(mapped
            .contact
            .phones
            .contains(&("222".into(), "mobile".into())));
        let addr = mapped.contact.address.unwrap();
        assert_eq!(addr.street.as_deref(), Some("1 St"));
        assert_eq!(addr.city.as_deref(), Some("Town"));
        assert!(addr.country.is_none());
    }

    #[test]
    fn skips_an_entry_without_external_id_or_email() {
        // No external_id.
        let e = entry(&[("mail", "x@y.test")], &[]);
        assert!(map_entry(&e, &resolve_attrs(&json!({}))).is_none());
        // No email.
        let e = entry(&[], &[("objectGUID", &[1, 2, 3])]);
        assert!(map_entry(&e, &resolve_attrs(&json!({}))).is_none());
    }

    #[test]
    fn honors_attribute_map_overrides() {
        let map = json!({ "email": "userPrincipalName", "organization": "o" });
        let e = entry(
            &[("userPrincipalName", "u@acme.test"), ("o", "AcmeOrg")],
            &[("objectGUID", &[9])],
        );
        let mapped = map_entry(&e, &resolve_attrs(&map)).unwrap();
        assert_eq!(mapped.email, "u@acme.test");
        assert_eq!(mapped.contact.organization.as_deref(), Some("AcmeOrg"));
    }
}
