//! LDAP user sync: a full directory scan that provisions/refreshes users,
//! mirroring the Microsoft Graph sync. Each directory entry maps to the same
//! transport-neutral sinks the Graph path uses: the scoped identity provisioner
//! (`provider_type="ldap"`) and `apply_directory_contact(source="ldap")`.
//!
//! Sync uses AD DirSync incremental sync (the cursor cookie lives in
//! workspace_ldap_sync_state); the first run sends an empty cookie and gets a
//! full snapshot. A full RFC-2696 paged scan (so AD's MaxPageSize doesn't
//! truncate) is the directory-agnostic fallback when DirSync isn't honored. See
//! `sync_users` for the flow. The scheduler (nightly reconcile) is a fast-follow.
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
use ldap3::controls::Control;
use ldap3::{Ldap, Scope, SearchEntry};
use serde_json::Value;
use tracing::{info, warn};

use crate::db::DbConnection;
use crate::models::{DirectoryAddress, DirectoryContact, WorkspaceLdapSettings};
use crate::repository::workspace_ldap_settings;
use crate::services::ldap::attrs::{all_values, attr_name, first_bin_value, first_value};
use crate::services::ldap::auth::{ensure_bind_creds, LdapAuthError};
use crate::services::ldap::{connector, dirsync};
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
    #[error("the directory did not return a DirSync response control")]
    DirSyncNotHonored,
    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),
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
    /// AD `objectSid` + `primaryGroupID`, for primary-group resolution.
    object_sid: String,
    primary_group_id: String,
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
        object_sid: attr_name(map, "object_sid", "objectSid"),
        primary_group_id: attr_name(map, "primary_group_id", "primaryGroupID"),
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
            &self.object_sid,
            &self.primary_group_id,
        ]
    }
}

/// A directory entry mapped to the identity + contact shapes the sinks consume.
pub struct MappedUser {
    pub external_id: String,
    /// The entry's DN, persisted so group membership (AD lists members by DN)
    /// can resolve back to this user.
    pub dn: String,
    /// Hex of this user's PRIMARY group SID (computed from objectSid +
    /// primaryGroupID), so the group sync can add the primary-group membership
    /// AD omits from the `member` list. `None` for non-AD directories.
    pub primary_group_sid: Option<String>,
    pub email: String,
    pub display_name: Option<String>,
    pub contact: DirectoryContact,
}

/// Map one entry. Returns `None` (skip) when it lacks the immutable external_id
/// or an email — both are required to provision a user cleanly.
fn map_entry(entry: &SearchEntry, a: &AttrNames) -> Option<MappedUser> {
    let external_id = first_value(entry, &a.external_id)?;
    let email = first_value(entry, &a.email)?;

    // Primary group: objectSid (binary) + primaryGroupID (the group RID) ->
    // the primary group's SID, which the group sync matches against group SIDs.
    let primary_group_sid = first_bin_value(entry, &a.object_sid)
        .zip(first_value(entry, &a.primary_group_id).and_then(|s| s.parse::<u32>().ok()))
        .and_then(|(sid, rid)| crate::services::ldap::sid::primary_group_sid(&sid, rid))
        .map(|sid| crate::services::ldap::sid::to_hex(&sid));

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
        dn: entry.dn.clone(),
        primary_group_sid,
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

/// Connect + service-bind, shared by the user + group syncs.
pub(crate) async fn connect_and_bind(
    settings: &WorkspaceLdapSettings,
    bind_password: &str,
) -> Result<Ldap, SyncError> {
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
    Ok(svc)
}

/// Run a user sync for `workspace_id`. `conn` MUST be pinned to that workspace
/// (the provisioning + contact writes are RLS/audit-scoped). Returns the run
/// stats; per-entry errors are counted, not fatal.
///
/// Uses AD DirSync: the first run (no stored cookie) sends an EMPTY cookie and
/// AD returns ALL matching objects + a fresh cookie; later runs send the stored
/// cookie and get only the changes since. The cookie is persisted only on a
/// clean completion. If the directory rejects DirSync (the service account
/// lacks the Replicating-Directory-Changes right, or it isn't AD), it falls back
/// to a full RFC-2696 paged scan, which works against any directory; the
/// idempotent sinks make the re-scan safe.
pub async fn sync_users(
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    workspace_id: i32,
    bind_password: &str,
) -> Result<SyncStats, SyncError> {
    let attrs = resolve_attrs(&settings.attribute_map);
    let filter = build_sync_filter(&settings.user_filter);

    let mut svc = connect_and_bind(settings, bind_password).await?;

    // DirSync must search at the directory NC root (MS-ADTS 3.1.1.3.4.1.3): a
    // subtree base returns insufficientAccessRights. Read defaultNamingContext
    // from RootDSE; dirsync_pass filters entries back to user_base_dn client-
    // side. Falls back to user_base_dn if RootDSE can't be read (non-AD).
    let dirsync_base = read_default_naming_context(&mut svc)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| settings.user_base_dn.clone());

    let prior_cookie = workspace_ldap_settings::get_sync_state(conn)?
        .and_then(|s| s.cookie)
        .unwrap_or_default();
    let full_run = prior_cookie.is_empty();

    match dirsync_pass(
        &mut svc,
        conn,
        settings,
        &dirsync_base,
        &attrs,
        &filter,
        workspace_id,
        prior_cookie,
    )
    .await
    {
        Ok(stats) => {
            if full_run {
                let _ = workspace_ldap_settings::mark_full_reconcile(conn, workspace_id);
            }
            Ok(stats)
        }
        // The directory can't do DirSync: drop the unusable cursor and fall back
        // to a full paged scan, which works against any directory.
        Err(e) if dirsync_unsupported(&e) => {
            warn!(error = %e, "ldap DirSync not honored; falling back to a full paged scan");
            let _ = workspace_ldap_settings::set_cookie(conn, workspace_id, "dirsync", None);
            full_scan_pass(&mut svc, conn, settings, &attrs, &filter, workspace_id).await
        }
        // Transient (timeout / dropped stream / DB / server limit): keep the
        // last-known-good cursor and surface the failure as a failed run, so one
        // blip doesn't cost two full scans + lose incremental sync.
        Err(e) => {
            warn!(error = %e, "ldap DirSync sync failed; cursor preserved");
            Err(e)
        }
    }
}

/// Backstop against a non-conforming DC that never clears MoreResults; the
/// non-advancing-cookie check below is the primary guard, this is the ceiling.
const MAX_DIRSYNC_ROUNDS: usize = 100_000;

/// Read defaultNamingContext (the domain NC root) from RootDSE. `None` if the
/// directory doesn't expose it (non-AD), so the caller falls back to user_base_dn.
async fn read_default_naming_context(svc: &mut Ldap) -> Result<Option<String>, SyncError> {
    let (entries, _res) = svc
        .with_timeout(connector::OP_TIMEOUT)
        .search(
            "",
            Scope::Base,
            "(objectClass=*)",
            vec!["defaultNamingContext"],
        )
        .await?
        .success()?;
    Ok(entries
        .into_iter()
        .next()
        .and_then(|e| first_value(&SearchEntry::construct(e), "defaultNamingContext")))
}

/// True when an error means the directory doesn't support the (critical) DirSync
/// control, so we should fall back to a full scan rather than fail the run:
///   * our own DirSyncNotHonored signal (rc==0 but no response control),
///   * `unavailableCriticalExtension` (rc 12) — the directory has no DirSync
///     (every non-AD server, e.g. OpenLDAP, rejects the critical control this way),
///   * `insufficientAccessRights` (rc 50) — an AD service account without the
///     Replicating-Directory-Changes right.
fn dirsync_unsupported(e: &SyncError) -> bool {
    match e {
        SyncError::DirSyncNotHonored => true,
        SyncError::Ldap(ldap3::LdapError::LdapResult { result }) => {
            result.rc == 12 || result.rc == 50
        }
        _ => false,
    }
}

/// DirSync incremental: search at `base` (the NC root, required by the control),
/// keeping only entries under the configured user_base_dn, looping the cookie
/// until MoreResults is clear. The cookie is persisted ONLY on clean completion.
async fn dirsync_pass(
    svc: &mut Ldap,
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    base: &str,
    attrs: &AttrNames,
    filter: &str,
    workspace_id: i32,
    mut cookie: Vec<u8>,
) -> Result<SyncStats, SyncError> {
    // The NC-root search returns changed users across the domain; keep only
    // those under user_base_dn (DN suffix match, DNs are case-insensitive).
    let subtree = settings.user_base_dn.to_lowercase();
    let mut stats = SyncStats::default();
    let mut rounds = 0usize;
    loop {
        rounds += 1;
        if rounds > MAX_DIRSYNC_ROUNDS {
            warn!(
                rounds,
                "ldap DirSync exceeded the round cap; treating as not honored"
            );
            return Err(SyncError::DirSyncNotHonored);
        }
        let mut search = svc
            .with_timeout(connector::OP_TIMEOUT)
            .with_controls(vec![dirsync::request_control(&cookie)])
            .streaming_search_with(
                vec![Box::new(EntriesOnly::new()) as Box<dyn Adapter<_, _>>],
                base,
                Scope::Subtree,
                filter,
                attrs.request_list(),
            )
            .await?;
        while let Some(re) = search.next().await? {
            let entry = SearchEntry::construct(re);
            if entry.dn.to_lowercase().ends_with(&subtree) {
                provision_entry(conn, attrs, workspace_id, entry, &mut stats);
            }
        }
        let res = search.finish().await;
        // insufficientAccessRights (50) = no DirSync right / wrong base -> fall
        // back to a full scan. Any OTHER non-success rc (a server limit, a
        // transient condition) propagates as SyncError::Ldap and must NOT wipe
        // the cursor. rc==0 with no response control = the server ignored the
        // control = genuinely not honored.
        if res.rc == 50 {
            return Err(SyncError::DirSyncNotHonored);
        }
        let res = res.success()?;
        let resp = extract_dirsync_response(&res.ctrls).ok_or(SyncError::DirSyncNotHonored)?;
        let advanced = resp.cookie != cookie;
        cookie = resp.cookie;
        if !resp.more_results {
            break;
        }
        if !advanced {
            warn!("ldap DirSync: MoreResults set but the cookie did not advance; stopping");
            break;
        }
    }
    workspace_ldap_settings::set_cookie(conn, workspace_id, "dirsync", Some(&cookie))?;
    Ok(stats)
}

/// Full RFC-2696 paged scan (the directory-agnostic fallback).
async fn full_scan_pass(
    svc: &mut Ldap,
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    attrs: &AttrNames,
    filter: &str,
    workspace_id: i32,
) -> Result<SyncStats, SyncError> {
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
            filter,
            attrs.request_list(),
        )
        .await?;
    let mut stats = SyncStats::default();
    while let Some(re) = stream.next().await? {
        provision_entry(
            conn,
            attrs,
            workspace_id,
            SearchEntry::construct(re),
            &mut stats,
        );
    }
    let _ = stream.finish().await;
    let _ = workspace_ldap_settings::mark_full_reconcile(conn, workspace_id);
    Ok(stats)
}

/// Map + provision one directory entry, updating the run stats. Per-entry
/// failures are counted, never fatal to the run.
fn provision_entry(
    conn: &mut DbConnection,
    attrs: &AttrNames,
    workspace_id: i32,
    entry: SearchEntry,
    stats: &mut SyncStats,
) {
    stats.seen += 1;
    let mapped = match map_entry(&entry, attrs) {
        Some(m) => m,
        None => {
            stats.skipped += 1;
            return;
        }
    };
    let external_id = mapped.external_id.clone();
    let dn = mapped.dn.clone();
    let primary_group_sid = mapped.primary_group_sid.clone();
    let input = ProjectedUserInput {
        iss: "ldap".to_string(),
        sub: mapped.external_id,
        identity_workspace_id: Some(workspace_id),
        email: mapped.email,
        email_verified: true,
        name: mapped.display_name,
        // LDAP doesn't carry our global handle.
        username: None,
        // Directory sync carries neither our avatar nor the verified-set.
        avatar_url: None,
        verified_email_set: None,
        role: "member".to_string(),
        workspace_id,
        password_hash: None,
        metadata: None,
    };
    match find_or_create_projected_user(conn, input) {
        Ok(outcome) => {
            let uuid = outcome.into_user().uuid;
            // Refresh the DN (in the identity's metadata) so the group sync can
            // resolve membership back to this user even after an OU move.
            if let Err(e) = crate::repository::user_auth_identities::set_ldap_identity_meta(
                conn,
                workspace_id,
                &external_id,
                &dn,
                primary_group_sid.as_deref(),
            ) {
                // A stale DN silently drops the user from explicit-member groups
                // until the next clean sync; surface it rather than swallow it.
                warn!(error = %e, "ldap sync: DN/SID metadata refresh failed");
                stats.errors += 1;
            }
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

/// Find + decode the DirSync response control among a result's controls.
fn extract_dirsync_response(ctrls: &[Control]) -> Option<dirsync::DirSyncResponse> {
    ctrls.iter().find_map(|c| {
        if c.1.ctype == dirsync::DIRSYNC_OID {
            c.1.val
                .as_deref()
                .and_then(|v| dirsync::decode_response_value(v).ok())
        } else {
            None
        }
    })
}

/// A sync run recorded in `sync_history`.
pub struct RecordedSync {
    pub history_id: i32,
    pub stats: SyncStats,
}

/// Run a sync bracketed by a `sync_history` row (running -> completed /
/// completed_with_errors / failed). The single home for the run-recording, used
/// by BOTH the admin trigger and the nightly reconcile. `conn` MUST be pinned to
/// `workspace_id` (the writes are RLS/audit-scoped). On a sync failure the row is
/// recorded as `failed` and the error returned.
pub async fn run_recorded_sync(
    conn: &mut DbConnection,
    settings: &WorkspaceLdapSettings,
    workspace_id: i32,
    bind_password: &str,
    sync_type: &str,
) -> Result<RecordedSync, SyncError> {
    use crate::models::{NewSyncHistory, SyncHistoryUpdate};
    use crate::repository::sync_history;

    let history = sync_history::create_sync_history(
        conn,
        NewSyncHistory {
            sync_type: sync_type.to_string(),
            status: "running".to_string(),
            started_at: chrono::Utc::now().naive_utc(),
            completed_at: None,
            error_message: None,
            records_processed: None,
            records_created: None,
            records_updated: None,
            records_failed: None,
            tenant_id: None,
            is_delta: false,
        },
    )?;

    let result = sync_users(conn, settings, workspace_id, bind_password).await;
    let completed_at = Some(Some(chrono::Utc::now().naive_utc()));

    match result {
        Ok(stats) => {
            // A green "completed" must not hide per-entry failures; skips
            // (entries legitimately lacking email/external_id) are benign.
            let status = if stats.errors > 0 {
                "completed_with_errors"
            } else {
                "completed"
            };
            sync_history::update_sync_history(
                conn,
                history.id,
                SyncHistoryUpdate {
                    status: Some(status.to_string()),
                    completed_at,
                    error_message: None,
                    records_processed: Some(stats.seen as i32),
                    records_created: None,
                    records_updated: Some(stats.synced as i32),
                    records_failed: Some(stats.errors as i32),
                },
            )?;
            // Group sync runs AFTER users so the DN->uuid map is complete.
            // Best-effort: a group failure doesn't fail the user sync run.
            if crate::services::ldap::groups::is_configured(settings) {
                match crate::services::ldap::groups::sync_groups(
                    conn,
                    settings,
                    workspace_id,
                    bind_password,
                )
                .await
                {
                    Ok(g) => {
                        info!(
                            groups = g.groups_synced,
                            members = g.members_resolved,
                            unresolved = g.members_unresolved,
                            "ldap group sync completed"
                        );
                        // Role mapping runs ONLY on a COMPLETE group sync, so a
                        // failed/partial sync can't apply authoritative roles off
                        // stale memberships. No-op unless role_mappings are set.
                        match crate::services::ldap::role_mapping::apply_role_mappings(
                            conn,
                            settings,
                            workspace_id,
                        ) {
                            Ok(r) if r.changed > 0 || r.errors > 0 => {
                                info!(
                                    changed = r.changed,
                                    errors = r.errors,
                                    "ldap role mapping applied"
                                )
                            }
                            Ok(_) => {}
                            Err(e) => warn!(error = %e, "ldap role mapping failed"),
                        }
                    }
                    Err(e) => warn!(error = %e, "ldap group sync failed; skipping role mapping"),
                }
            }
            Ok(RecordedSync {
                history_id: history.id,
                stats,
            })
        }
        Err(e) => {
            let _ = sync_history::update_sync_history(
                conn,
                history.id,
                SyncHistoryUpdate {
                    status: Some("failed".to_string()),
                    completed_at,
                    error_message: Some(e.to_string()),
                    records_processed: None,
                    records_created: None,
                    records_updated: None,
                    records_failed: None,
                },
            );
            Err(e)
        }
    }
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
