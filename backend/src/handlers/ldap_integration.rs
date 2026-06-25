//! Admin endpoints for the per-workspace LDAP configuration.
//!
//! Reads are workspace-admin (the config is operational, and the bind password
//! never leaves the process — the model serde(skip)s the encrypted columns and
//! the response carries only a `has_bind_password` flag). Writes validate the
//! config shape, upsert the settings, then set/clear the bind password
//! separately so editing settings never disturbs a stored secret.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::{errors, helpers};
use crate::models::UpsertWorkspaceLdapSettings;
use crate::repository::workspace_ldap_settings as repo;
use crate::services::ldap::auth::{self as ldap_auth, LdapAuthError};
use crate::services::ldap::connector::LdapConnectError;

/// PUT body: the editable settings plus the out-of-band bind-password controls.
#[derive(Debug, Deserialize)]
pub struct LdapSettingsRequest {
    #[serde(flatten)]
    pub settings: UpsertWorkspaceLdapSettings,
    /// New plaintext bind password to encrypt + store. `None` leaves any stored
    /// password unchanged.
    pub bind_password: Option<String>,
    /// Explicitly remove the stored bind password.
    #[serde(default)]
    pub clear_bind_password: bool,
}

/// GET /ldap/settings — the current workspace's LDAP config (admin).
pub async fn get_ldap_settings(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can view LDAP settings");
    }
    match tc.run(|conn| repo::get(conn)) {
        Ok(Some(row)) => {
            let has_bind_password = row.encrypted_bind_password.is_some();
            HttpResponse::Ok().json(json!({
                "settings": row,
                "has_bind_password": has_bind_password,
            }))
        }
        Ok(None) => HttpResponse::Ok().json(json!({
            "settings": null,
            "has_bind_password": false,
        })),
        Err(e) => {
            error!(error = %e, "get ldap settings failed");
            errors::internal("Failed to load LDAP settings")
        }
    }
}

/// GET /ldap/presets — provider presets for pick-from-catalog (admin).
pub async fn get_ldap_presets(auth: AuthContext) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can view LDAP presets");
    }
    HttpResponse::Ok().json(provider_presets())
}

/// GET /ldap/sync-history — recent sync runs + cursor state for this workspace
/// (admin). Backs the "last run / history" panel in the admin UI.
pub async fn get_ldap_sync_history(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can view LDAP sync history");
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::forbidden("A resolved workspace is required");
    };
    let result = tc.run(|conn| {
        let runs = crate::repository::sync_history::list_recent_for_workspace(
            conn,
            workspace_id,
            &["ldap_users", "ldap_reconcile"],
            10,
        )?;
        let state = repo::get_sync_state(conn)?;
        Ok::<_, diesel::result::Error>((runs, state))
    });
    match result {
        Ok((runs, state)) => HttpResponse::Ok().json(json!({
            "runs": runs,
            "cursor": {
                "incremental_active": state.as_ref().map(|s| s.cookie.is_some()).unwrap_or(false),
                "last_full_reconcile_at": state.and_then(|s| s.last_full_reconcile_at),
            },
        })),
        Err(e) => {
            error!(error = %e, "get ldap sync history failed");
            errors::internal("Failed to load LDAP sync history")
        }
    }
}

/// PUT /ldap/settings — validate, upsert, then apply the bind-password controls.
pub async fn set_ldap_settings(
    mut tc: TenantConn,
    body: web::Json<LdapSettingsRequest>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can edit LDAP settings");
    }
    let LdapSettingsRequest {
        settings,
        bind_password,
        clear_bind_password,
    } = body.into_inner();

    if let Err(msg) = validate_settings(&settings) {
        return errors::unprocessable_entity(msg);
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::forbidden("A resolved workspace is required");
    };

    let result = tc.run(|conn| {
        repo::upsert(conn, settings)?;
        // Apply the bind-password controls after the row exists. `clear` wins if
        // both are sent.
        if clear_bind_password {
            repo::clear_bind_password(conn)?;
        } else if let Some(pw) = bind_password.as_deref() {
            if !pw.is_empty() {
                // CredentialError -> a generic Diesel error so tc.run's
                // QueryResult signature holds; the message is logged below.
                repo::set_bind_password(conn, workspace_id, pw).map_err(|e| {
                    error!(error = %e, "set ldap bind password failed");
                    diesel::result::Error::RollbackTransaction
                })?;
            }
        }
        repo::get(conn)
    });

    match result {
        Ok(Some(row)) => {
            let has_bind_password = row.encrypted_bind_password.is_some();
            HttpResponse::Ok().json(json!({
                "settings": row,
                "has_bind_password": has_bind_password,
            }))
        }
        Ok(None) => errors::internal("LDAP settings vanished after save"),
        Err(e) => {
            error!(error = %e, "set ldap settings failed");
            errors::internal("Failed to save LDAP settings")
        }
    }
}

/// POST /ldap/test-connection — connect + service-bind only, reporting the
/// outcome so an admin can validate the config (admin).
pub async fn test_ldap_connection(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can test the LDAP connection");
    }
    let row = match tc.run(|conn| repo::get(conn)) {
        Ok(Some(r)) => r,
        Ok(None) => return errors::unprocessable_entity("Configure LDAP settings before testing"),
        Err(e) => {
            error!(error = %e, "load ldap settings for test failed");
            return errors::internal("Failed to load LDAP settings");
        }
    };
    let bind_password = match repo::decrypt_bind_password(&row) {
        Ok(pw) => pw.unwrap_or_default(),
        Err(e) => {
            error!(error = %e, "decrypt ldap bind password failed");
            return errors::internal("Failed to read the stored bind password");
        }
    };

    match ldap_auth::test_connection(&row, &bind_password).await {
        Ok(()) => HttpResponse::Ok().json(json!({ "ok": true })),
        Err(e) => {
            // The egress rejection is the common self-host footgun (the DC sits on
            // RFC1918, which the policy rejects unless allowlisted), so spell out
            // the fix rather than leaving a bare "rejected" message.
            let mut message = e.to_string();
            if matches!(e, LdapAuthError::Connect(LdapConnectError::Egress(_))) {
                message.push_str(
                    ". If this is an on-prem directory, add the host to \
                     NOSDESK_OUTBOUND_ALLOWED_HOSTS.",
                );
            }
            HttpResponse::Ok().json(json!({ "ok": false, "error": message }))
        }
    }
}

/// GET /ldap/discover-groups — browse directory groups under the configured
/// group base DN, for the role-mapping picker (admin). Uses the SAVED config so
/// it reflects what a sync would actually see; read-only, no provisioning.
pub async fn discover_ldap_groups(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can browse LDAP groups");
    }
    let row = match tc.run(|conn| repo::get(conn)) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return errors::unprocessable_entity("Configure LDAP settings before browsing groups")
        }
        Err(e) => {
            error!(error = %e, "load ldap settings for discovery failed");
            return errors::internal("Failed to load LDAP settings");
        }
    };
    let has_base = row
        .group_config
        .get("group_base_dn")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_base {
        return HttpResponse::Ok()
            .json(json!({ "ok": false, "error": "Set a group base DN first.", "groups": [] }));
    }
    let bind_password = match repo::decrypt_bind_password(&row) {
        Ok(pw) => pw.unwrap_or_default(),
        Err(e) => {
            error!(error = %e, "decrypt ldap bind password failed");
            return errors::internal("Failed to read the stored bind password");
        }
    };
    match crate::services::ldap::groups::discover_groups(&row, &bind_password, 500).await {
        Ok(groups) => HttpResponse::Ok().json(json!({ "ok": true, "groups": groups })),
        Err(e) => {
            let mut message = e.to_string();
            if matches!(
                e,
                crate::services::ldap::sync::SyncError::Auth(LdapAuthError::Connect(
                    LdapConnectError::Egress(_)
                ))
            ) {
                message.push_str(
                    ". If this is an on-prem directory, add the host to \
                     NOSDESK_OUTBOUND_ALLOWED_HOSTS.",
                );
            }
            HttpResponse::Ok().json(json!({ "ok": false, "error": message, "groups": [] }))
        }
    }
}

/// POST /ldap/sync — run a full LDAP user sync for the request's workspace,
/// recording it in sync_history, and return the run stats (admin).
///
/// Synchronous for v1: the admin triggers it and waits for the result. It runs
/// on a request-pinned `nosdesk_app` connection (no elevation) -- the provisioner
/// already writes under exactly this context in the login path (the membership
/// write self-bypasses), and apply_directory_contact's RLS writes pass under the
/// workspace pin. Backgrounding + DirSync incremental are later P3 chunks.
pub async fn run_ldap_sync(
    db_pool: web::Data<crate::db::Pool>,
    request: HttpRequest,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can run an LDAP sync");
    }
    let Some(workspace_id) = helpers::request_workspace_id(&request) else {
        return errors::forbidden("A resolved workspace is required");
    };
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    helpers::pin_request_workspace(&request, &mut conn);

    let settings = match repo::get_for_workspace(&mut conn, workspace_id) {
        Ok(Some(s)) if s.enabled => s,
        Ok(_) => return errors::unprocessable_entity("Enable LDAP before running a sync"),
        Err(e) => {
            error!(error = %e, "load ldap settings for sync failed");
            return errors::internal("Failed to load LDAP settings");
        }
    };
    let bind_password = repo::decrypt_bind_password(&settings)
        .ok()
        .flatten()
        .unwrap_or_default();

    match crate::services::ldap::sync::run_recorded_sync(
        &mut conn,
        &settings,
        workspace_id,
        &bind_password,
        "ldap_users",
    )
    .await
    {
        Ok(rec) => HttpResponse::Ok().json(json!({
            "session_id": rec.history_id,
            "stats": rec.stats,
        })),
        Err(e) => {
            error!(error = %e, workspace_id, "ldap sync failed");
            errors::internal("LDAP sync failed; see server logs")
        }
    }
}

// ---- Validation ------------------------------------------------------------

// Cleartext "plain" is intentionally not accepted: a plain bind ships the
// service + user passwords unencrypted (RFC 4513 §5.1.3). Only LDAPS and
// StartTLS (which upgrades before the bind) are allowed.
const TLS_MODES: [&str; 2] = ["ldaps", "starttls"];
const AUTH_MODES: [&str; 2] = ["simple_bind", "mtls"];

/// Validate the editable settings before they are persisted. The DB also CHECKs
/// tls_mode/auth_mode, but validating here returns a clean 422 with a reason
/// instead of a 500 from the constraint, and covers app-level rules the DB
/// can't (the RFC 4515 filter template, the enabled-requires-host rule).
fn validate_settings(s: &UpsertWorkspaceLdapSettings) -> Result<(), String> {
    if !TLS_MODES.contains(&s.tls_mode.as_str()) {
        return Err(format!(
            "tls_mode must be one of {}, got '{}'",
            TLS_MODES.join(", "),
            s.tls_mode
        ));
    }
    if !AUTH_MODES.contains(&s.auth_mode.as_str()) {
        return Err(format!(
            "auth_mode must be one of {}, got '{}'",
            AUTH_MODES.join(", "),
            s.auth_mode
        ));
    }
    if !(1..=65535).contains(&s.port) {
        return Err(format!("port must be 1-65535, got {}", s.port));
    }
    if s.page_size <= 0 {
        return Err("page_size must be positive".into());
    }
    if s.connect_timeout_secs <= 0 {
        return Err("connect_timeout_secs must be positive".into());
    }
    // The user filter is a template the connector substitutes the (escaped)
    // login into; without the placeholder it would match the same entry for
    // every login, so reject it.
    if !s.user_filter.contains("{username}") {
        return Err("user_filter must contain the {username} placeholder".into());
    }
    // A config that's turned on must have somewhere to connect.
    if s.enabled {
        if s.host.trim().is_empty() {
            return Err("host is required when LDAP is enabled".into());
        }
        if s.user_base_dn.trim().is_empty() {
            return Err("user_base_dn is required when LDAP is enabled".into());
        }
    }
    validate_group_role_config(&s.group_config)?;
    Ok(())
}

/// Guard the group->role mapping config against privilege-escalation footguns.
/// The `default_role` is applied to EVERY directory user who matches no mapped
/// group, so a mistyped `"admin"` there would blanket-elevate the whole
/// directory on the next sync. Keep the default least-privilege, and never let
/// the directory mint owners.
fn validate_group_role_config(gc: &serde_json::Value) -> Result<(), String> {
    if let Some(default_role) = gc.get("default_role").and_then(|v| v.as_str()) {
        if default_role != "member" {
            return Err(format!(
                "default_role must be 'member' (the unmatched-user fallback is least-privilege); \
                 map an explicit group to grant a higher role, got '{default_role}'"
            ));
        }
    }
    if let Some(mappings) = gc.get("role_mappings").and_then(|v| v.as_array()) {
        for m in mappings {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if !matches!(role, "member" | "agent" | "admin") {
                return Err(format!(
                    "role_mappings role must be one of member/agent/admin (the directory cannot \
                     mint owners), got '{role}'"
                ));
            }
        }
    }
    Ok(())
}

// ---- Provider presets ------------------------------------------------------

/// Per-provider defaults for pick-from-catalog. The admin picks one, then fills
/// the base DN + bind credentials. The config object is provider-agnostic; a
/// preset only prefills sensible defaults (port, tls, username attribute, filter
/// template, attribute map, group model).
fn provider_presets() -> serde_json::Value {
    json!([
        {
            "id": "active_directory",
            "label": "Active Directory",
            "defaults": {
                "port": 636, "tls_mode": "ldaps",
                "username_attribute": "sAMAccountName",
                "user_filter": "(&(objectCategory=person)(objectClass=user)(sAMAccountName={username}))",
                "attribute_map": { "email": "mail", "display_name": "displayName", "first_name": "givenName", "last_name": "sn", "external_id": "objectGUID" },
                "group_config": { "membership_mode": "memberOf", "member_attribute": "member", "object_class": "group" }
            }
        },
        {
            "id": "openldap",
            "label": "OpenLDAP",
            "defaults": {
                "port": 636, "tls_mode": "ldaps",
                "username_attribute": "uid",
                "user_filter": "(&(objectClass=inetOrgPerson)(uid={username}))",
                "attribute_map": { "email": "mail", "display_name": "cn", "first_name": "givenName", "last_name": "sn", "external_id": "entryUUID" },
                "group_config": { "membership_mode": "search_member", "member_attribute": "member", "object_class": "groupOfNames" }
            }
        },
        {
            "id": "freeipa",
            "label": "FreeIPA",
            "defaults": {
                "port": 636, "tls_mode": "ldaps",
                "username_attribute": "uid",
                "user_filter": "(&(objectClass=inetOrgPerson)(uid={username}))",
                "attribute_map": { "email": "mail", "display_name": "displayName", "first_name": "givenName", "last_name": "sn", "external_id": "ipaUniqueID" },
                "group_config": { "membership_mode": "search_member", "member_attribute": "member", "object_class": "groupOfNames" }
            }
        },
        {
            "id": "jumpcloud",
            "label": "JumpCloud",
            "defaults": {
                "port": 636, "tls_mode": "ldaps",
                "username_attribute": "uid",
                "user_filter": "(&(objectClass=inetOrgPerson)(uid={username}))",
                "attribute_map": { "email": "mail", "display_name": "cn", "first_name": "givenName", "last_name": "sn", "external_id": "entryUUID" },
                "group_config": { "membership_mode": "search_member", "member_attribute": "member", "object_class": "groupOfNames" }
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid() -> UpsertWorkspaceLdapSettings {
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
            bind_dn: "cn=svc,dc=acme,dc=test".into(),
            user_base_dn: "ou=people,dc=acme,dc=test".into(),
            username_attribute: "sAMAccountName".into(),
            user_filter: "(&(objectClass=user)(sAMAccountName={username}))".into(),
            page_size: 500,
            attribute_map: json!({}),
            group_config: json!({}),
            provisioning: json!({}),
        }
    }

    #[test]
    fn accepts_a_valid_config() {
        assert!(validate_settings(&valid()).is_ok());
    }

    #[test]
    fn rejects_bad_enum_values() {
        let mut s = valid();
        s.tls_mode = "ssl".into();
        assert!(validate_settings(&s).is_err());
        let mut s = valid();
        s.auth_mode = "kerberos".into();
        assert!(validate_settings(&s).is_err());
    }

    #[test]
    fn rejects_privileged_group_role_config() {
        // A privileged default_role would blanket-elevate the unmatched directory.
        let mut s = valid();
        s.group_config = serde_json::json!({ "default_role": "admin" });
        assert!(validate_settings(&s).is_err());
        let mut s = valid();
        s.group_config = serde_json::json!({ "default_role": "owner" });
        assert!(validate_settings(&s).is_err());
        // The directory can't mint owners via a mapping.
        let mut s = valid();
        s.group_config =
            serde_json::json!({ "role_mappings": [{ "group": "X", "role": "owner" }] });
        assert!(validate_settings(&s).is_err());
        // A least-privilege default + agent/admin mappings are fine.
        let mut s = valid();
        s.group_config = serde_json::json!({
            "default_role": "member",
            "role_mappings": [{ "group": "Admins", "role": "admin" }],
        });
        assert!(validate_settings(&s).is_ok());
    }

    #[test]
    fn rejects_a_filter_without_the_username_placeholder() {
        let mut s = valid();
        s.user_filter = "(objectClass=user)".into();
        assert!(validate_settings(&s).is_err());
    }

    #[test]
    fn rejects_out_of_range_port_and_sizes() {
        let mut s = valid();
        s.port = 0;
        assert!(validate_settings(&s).is_err());
        let mut s = valid();
        s.page_size = 0;
        assert!(validate_settings(&s).is_err());
    }

    #[test]
    fn enabled_requires_host_and_base_dn() {
        let mut s = valid();
        s.host = "  ".into();
        assert!(validate_settings(&s).is_err());
        // Disabled config may be incomplete.
        let mut s = valid();
        s.enabled = false;
        s.host = "".into();
        s.user_base_dn = "".into();
        assert!(validate_settings(&s).is_ok());
    }

    #[test]
    fn presets_are_well_formed() {
        let p = provider_presets();
        let arr = p.as_array().unwrap();
        assert!(arr.iter().any(|x| x["id"] == "active_directory"));
        // Every preset's filter template carries the placeholder.
        for preset in arr {
            let f = preset["defaults"]["user_filter"].as_str().unwrap();
            assert!(
                f.contains("{username}"),
                "preset {} filter missing placeholder",
                preset["id"]
            );
        }
    }
}
