//! Per-workspace outbound email resolver.
//!
//! Outbound mail used to flow through a single instance-global
//! `EmailService` built from env. This resolver replaces that single
//! reference: given a workspace, it returns the `EmailService` to send
//! with, built from the workspace's own `workspace_email_settings` row.
//! When the workspace has no identity of its own, the fallback depends on
//! the deployment mode: on self-host the env-configured service is the
//! operator's own identity, so it applies directly (single-tenant self-host
//! is unchanged); on hosted, the workspace instead gets the MANAGED default
//! identity `support@<slug>.<NOSDESK_TENANT_DOMAIN>` — sent through the
//! platform relay but From the tenant's own subdomain, so tenant mail never
//! leaves on the shared platform domain.
//!
//! It is a **stateless builder**, deliberately not a cache. Every send path
//! already reads its DB inputs in a pinned phase and releases the
//! connection before the network send, so the resolver reads settings on
//! the connection the caller already holds ([`resolve_on_conn`]) rather than
//! checking out a second one. Building an `EmailService` is a decrypt plus a
//! `relay()` builder (no socket), so there is nothing worth caching across
//! sends, and no cross-instance staleness to manage.
//!
//! [`resolve_on_conn`]: OutboundEmailResolver::resolve_on_conn

use std::collections::HashMap;
use std::sync::Arc;

use diesel::QueryResult;

use crate::db::{DbConnection, Pool};
use crate::models::{
    workspace_email_sending_mode, workspace_email_verification_status, WorkspaceEmailSettings,
};
use crate::repository::channels::CredentialError;
use crate::repository::workspace_email_settings as ws_settings;
use crate::repository::workspaces;
use crate::sync::session::{run_in_workspace, BackgroundRunError};
use crate::utils::email::{DkimAlgorithm, DkimSigner, EmailConfig, EmailService, SmtpSecurity};

/// True when `recipient` is on the (global) suppression list — a prior hard
/// bounce or complaint. The queue worker checks this before every send; this is
/// the same guard for the DIRECT (non-queued) send paths (auto-ack, technician
/// replies), so none of them ships mail to a known-bad address and erodes the
/// shared relay's reputation. Fails OPEN: a lookup error attempts the send
/// rather than silently dropping it, matching the worker.
pub fn recipient_is_suppressed(pool: &Pool, recipient: &str) -> bool {
    crate::sync::session::background_run(pool, "background:direct_send_suppress_check", |conn| {
        crate::repository::email_suppressions::is_suppressed(conn, recipient)
    })
    .unwrap_or(false)
}

/// Resolves the outbound `EmailService` for a workspace.
pub struct OutboundEmailResolver {
    pool: Pool,
    /// The env-configured service, used when a workspace has no identity of
    /// its own. `None` when no env SMTP is configured at all.
    fallback: Option<Arc<EmailService>>,
    /// Whether WORKSPACE mail may fall back to the env identity. True only on
    /// self-host, where the env relay IS the single operator's own identity; on
    /// hosted the env identity is the SHARED PLATFORM identity, so tenant mail
    /// must never fall back to it (it would leave on the platform domain).
    /// Hosted workspaces without their own identity resolve to the managed
    /// default identity instead (see `managed_domain`).
    workspace_fallback_allowed: bool,
    /// Hosted only: the tenant base domain (`NOSDESK_TENANT_DOMAIN`) the
    /// managed default identity lives under. When set, a workspace with no
    /// usable identity of its own sends as
    /// `support@<slug>.<managed_domain>` through the platform relay (Easy
    /// DKIM signs `d=<managed_domain>` at the relay, DMARC-aligned relaxed).
    /// `None` on self-host, and on hosted instances without a tenant domain
    /// (where the old defer/fallback behaviour is preserved). Captured once
    /// at construction so resolution never re-reads env mid-drain.
    managed_domain: Option<String>,
}

#[derive(Debug)]
pub enum ResolveError {
    /// The workspace has no usable identity and there is no env fallback.
    NotConfigured,
    /// The stored SMTP password could not be decrypted.
    Credential(CredentialError),
    /// Reading the settings row failed.
    Db(diesel::result::Error),
    /// Acquiring a pooled connection / pinning the workspace failed.
    Background(BackgroundRunError),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "no outbound email identity configured"),
            Self::Credential(e) => write!(f, "outbound SMTP password: {e}"),
            Self::Db(e) => write!(f, "reading workspace email settings: {e}"),
            Self::Background(e) => write!(f, "resolving workspace email settings: {e}"),
        }
    }
}
impl std::error::Error for ResolveError {}

impl OutboundEmailResolver {
    pub fn new(pool: Pool, fallback: Option<Arc<EmailService>>) -> Self {
        // Self-host: the env identity is the operator's own, so WORKSPACE mail
        // may fall back to it. Hosted: it's shared platform infra, never —
        // identity-less workspaces get the managed default instead.
        let workspace_fallback_allowed = crate::middleware::DeploymentMode::current()
            == crate::middleware::DeploymentMode::SelfHosted;
        let managed_domain = if workspace_fallback_allowed {
            None
        } else {
            crate::utils::tenant_origin::tenant_domain()
        };
        Self {
            pool,
            fallback,
            workspace_fallback_allowed,
            managed_domain,
        }
    }

    /// Test constructor that sets the workspace-fallback policy and managed
    /// domain explicitly, since `DeploymentMode::current()` is process-cached
    /// and env reads race across tests.
    #[cfg(test)]
    fn with_policy(
        pool: Pool,
        fallback: Option<Arc<EmailService>>,
        workspace_fallback_allowed: bool,
        managed_domain: Option<String>,
    ) -> Self {
        Self {
            pool,
            fallback,
            workspace_fallback_allowed,
            managed_domain,
        }
    }

    /// The env identity, but only when it's safe to send WORKSPACE mail from it
    /// (self-host). On hosted this is always `None`: tenant mail must never fall
    /// back to the shared platform identity. Used by [`resolve_batch`](Self::resolve_batch).
    fn workspace_safe_fallback(&self) -> Option<Arc<EmailService>> {
        if self.workspace_fallback_allowed {
            self.fallback.clone()
        } else {
            None
        }
    }

    /// Resolve on a connection the caller already holds. The caller is
    /// expected to have scoped that connection to `workspace_id` (or to hold
    /// a bypass connection); the read filters by `workspace_id` either way
    /// (`workspaces` is a global table, readable on both). No extra
    /// connection is checked out.
    pub fn resolve_on_conn(
        &self,
        conn: &mut DbConnection,
        workspace_id: i32,
    ) -> Result<Arc<EmailService>, ResolveError> {
        let row = ws_settings::get_for_workspace(conn, workspace_id).map_err(ResolveError::Db)?;
        let identity = if self.managed_applicable() {
            workspaces::identity_for_ids(conn, &[workspace_id])
                .map_err(ResolveError::Db)?
                .pop()
        } else {
            None
        };
        self.from_row(row, identity)
    }

    /// Resolve using the resolver's own pooled, RLS-pinned connection, for
    /// call sites with no connection in scope (the IMAP adapter at
    /// construction, the admin test-send endpoint).
    pub fn resolve_owned(&self, workspace_id: i32) -> Result<Arc<EmailService>, ResolveError> {
        let managed = self.managed_applicable();
        let (row, identity) =
            run_in_workspace(&self.pool, "email-resolver", workspace_id, |conn| {
                let row = ws_settings::get_for_workspace(conn, workspace_id)?;
                let identity = if managed {
                    workspaces::identity_for_ids(conn, &[workspace_id])?.pop()
                } else {
                    None
                };
                Ok((row, identity))
            })
            .map_err(ResolveError::Background)?;
        self.from_row(row, identity)
    }

    /// Whether the managed default identity can be built at all: hosted with
    /// a tenant domain, and a platform relay to send through.
    fn managed_applicable(&self) -> bool {
        self.managed_domain.is_some() && self.fallback.is_some()
    }

    /// Resolve the sending service for each of `workspace_ids` in one read,
    /// for the queue worker's drain. This is the WORKSPACE-identity path
    /// (the worker resolves PLATFORM rows via [`platform`](Self::platform)).
    ///
    /// A workspace with its own usable identity (a verified sending domain or
    /// an smtp_relay) maps to that service. A workspace WITHOUT one gets, on
    /// HOSTED, the MANAGED default identity `support@<slug>.<tenant_domain>`
    /// — it does NOT fall back to the SHARED platform identity. Tenant mail
    /// carries tenant-controlled content (workspace name, customer names,
    /// ticket text); sending it from the platform domain would lend the
    /// platform's reputation to phishing and risk its deliverability, so it
    /// leaves on the tenant's own subdomain instead. A hosted workspace is
    /// omitted (its mail defers) only when the managed identity can't be
    /// built either: no tenant domain configured, no platform relay, or the
    /// workspace is archived. On SELF-HOST the env identity is the single
    /// operator's own, so an unconfigured workspace falls back to it
    /// (matching the direct-send path) rather than stranding the operator's
    /// notification mail. (A row whose stored password won't decrypt is
    /// omitted on both modes, deferring rather than mis-sending.)
    pub fn resolve_batch(
        &self,
        conn: &mut DbConnection,
        workspace_ids: &[i32],
    ) -> QueryResult<HashMap<i32, Arc<EmailService>>> {
        let by_ws: HashMap<i32, WorkspaceEmailSettings> =
            ws_settings::get_for_workspaces(conn, workspace_ids)?
                .into_iter()
                .map(|r| (r.workspace_id, r))
                .collect();
        // One indexed read covers every workspace that may need the managed
        // identity; `workspaces` is global, so this works on the worker's
        // bypass connection.
        let managed_identities: HashMap<i32, (String, String)> = if self.managed_applicable() {
            workspaces::identity_for_ids(conn, workspace_ids)?
                .into_iter()
                .map(|(id, slug, name)| (id, (slug, name)))
                .collect()
        } else {
            HashMap::new()
        };

        let mut out = HashMap::with_capacity(workspace_ids.len());
        for &ws in workspace_ids {
            if out.contains_key(&ws) {
                continue;
            }
            // No usable workspace identity (no row, disabled, fallback mode,
            // hostless relay, unverified domain): managed identity on hosted,
            // env identity on self-host, defer when neither exists.
            let unconfigured = |ws: i32| {
                managed_identities
                    .get(&ws)
                    .and_then(|(slug, name)| self.build_managed_service(slug, name))
                    .or_else(|| self.workspace_safe_fallback())
            };
            let svc = match by_ws.get(&ws) {
                Some(r) => match self.build_for_row(r) {
                    Ok(Some(svc)) => svc,
                    Ok(None) => match unconfigured(ws) {
                        Some(svc) => svc,
                        None => continue,
                    },
                    Err(e) => {
                        // A configured-but-broken identity (e.g. a key that won't
                        // decrypt): defer rather than mis-send from a different
                        // identity than the admin configured, on both modes.
                        tracing::warn!(
                            workspace_id = ws,
                            error = %e,
                            "skipping workspace email identity; its mail will defer"
                        );
                        continue;
                    }
                },
                None => match unconfigured(ws) {
                    Some(svc) => svc,
                    None => continue,
                },
            };
            out.insert(ws, svc);
        }
        Ok(out)
    }

    /// The env fallback identity, for platform/auth mail (password reset,
    /// invitation) that must not send from a tenant's relay.
    pub fn platform(&self) -> Option<Arc<EmailService>> {
        self.fallback.clone()
    }

    /// Whether an env fallback identity exists. The comment-relay enqueue
    /// gate uses this (conn-free, so it doesn't amplify the hot comment-
    /// create path): a channel that produces comments already requires the
    /// instance identity to poll inbound, so the fallback's presence is the
    /// baseline "outbound is possible" signal. Per-workspace identities
    /// override the From at send time; they don't change whether a row
    /// should be queued, and the worker defers a row it can't resolve.
    pub fn has_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    /// A usable workspace row builds a workspace service; otherwise the
    /// managed default identity (hosted), else the env fallback (self-host,
    /// or hosted without a tenant domain — the pre-managed behaviour), else
    /// `NotConfigured`. Once the managed identity is applicable, hosted
    /// direct sends stop leaking the platform From: a workspace whose
    /// identity can't be built (archived, missing) errs rather than sending
    /// tenant content from the shared platform domain.
    fn from_row(
        &self,
        row: Option<WorkspaceEmailSettings>,
        identity: Option<(i32, String, String)>,
    ) -> Result<Arc<EmailService>, ResolveError> {
        if let Some(ref r) = row {
            if let Some(svc) = self.build_for_row(r)? {
                return Ok(svc);
            }
        }
        if self.managed_applicable() {
            return identity
                .and_then(|(_, slug, name)| self.build_managed_service(&slug, &name))
                .ok_or(ResolveError::NotConfigured);
        }
        self.fallback.clone().ok_or(ResolveError::NotConfigured)
    }

    /// Build the sending service for a row by its `sending_mode`, or `None`
    /// when the row isn't in a usable state (disabled, fallback mode, an
    /// enabled-but-hostless smtp_relay, or an unverified verified_domain), in
    /// which case the caller uses the env fallback. An `Err` means a usable
    /// mode whose build failed (e.g. a key won't decrypt); callers defer such
    /// a row rather than sending it from the wrong identity.
    fn build_for_row(
        &self,
        r: &WorkspaceEmailSettings,
    ) -> Result<Option<Arc<EmailService>>, ResolveError> {
        if !r.enabled {
            return Ok(None);
        }
        match r.sending_mode.as_str() {
            workspace_email_sending_mode::SMTP_RELAY => {
                if r.smtp_host.trim().is_empty() {
                    return Ok(None);
                }
                let password = ws_settings::decrypt_password(r)
                    .map_err(ResolveError::Credential)?
                    .unwrap_or_default();
                // The host is tenant-supplied: build an untrusted-relay service
                // that SSRF-validates the host and connects to a validated
                // address on every send.
                Ok(Some(Arc::new(EmailService::new_untrusted_relay(
                    build_email_config(r, password),
                ))))
            }
            workspace_email_sending_mode::VERIFIED_DOMAIN => {
                if r.verification_status != workspace_email_verification_status::VERIFIED {
                    return Ok(None);
                }
                self.build_verified_domain_service(r).map(Some)
            }
            // `fallback` or any unexpected value.
            _ => Ok(None),
        }
    }

    /// The managed default identity: send through the **instance relay** with
    /// From `support@<slug>.<tenant_domain>` and the workspace name as the
    /// display name. No product-side DKIM signer — the relay (SES) Easy-DKIM
    /// signs `d=<tenant_domain>`, which DMARC-aligns (relaxed) with the
    /// subdomain From. `None` when the managed identity isn't applicable
    /// (self-host, no tenant domain, or no relay).
    fn build_managed_service(&self, slug: &str, name: &str) -> Option<Arc<EmailService>> {
        let domain = self.managed_domain.as_deref()?;
        let relay = self.fallback.as_ref()?;
        let mut config = relay.config().clone();
        config.from_name = crate::utils::tenant_origin::sanitise_from_display_name(name)
            .unwrap_or_else(|| slug.to_string());
        config.from_email = crate::utils::tenant_origin::managed_email_address(slug, domain);
        Some(Arc::new(EmailService::smtp_with_dkim(config, None)))
    }

    /// Verified-domain mode: send through the **instance relay** (the env
    /// transport's SMTP settings) with the workspace's `From` and a DKIM signer
    /// for its domain, so DMARC passes on DKIM alignment. Requires the env
    /// relay to exist (there's nothing else to send through).
    fn build_verified_domain_service(
        &self,
        r: &WorkspaceEmailSettings,
    ) -> Result<Arc<EmailService>, ResolveError> {
        let relay = self.fallback.as_ref().ok_or(ResolveError::NotConfigured)?;
        let mut config = relay.config().clone();
        config.from_name = r.from_name.clone();
        config.from_email = r.from_email.clone();

        let pem = ws_settings::decrypt_dkim_key(r)
            .map_err(ResolveError::Credential)?
            .ok_or(ResolveError::NotConfigured)?;
        let signer = DkimSigner {
            selector: r
                .dkim_selector
                .clone()
                .unwrap_or_else(|| "nosdesk".to_string()),
            domain: r.sending_domain.clone().unwrap_or_default(),
            private_key: pem,
            algorithm: parse_dkim_algorithm(r.dkim_algorithm.as_deref()),
        };
        Ok(Arc::new(EmailService::smtp_with_dkim(config, Some(signer))))
    }
}

/// Map the stored `dkim_algorithm` string onto the enum. `rsa` is the default
/// for any unexpected value (v1 only generates RSA keys).
fn parse_dkim_algorithm(s: Option<&str>) -> DkimAlgorithm {
    match s {
        Some("ed25519") => DkimAlgorithm::Ed25519,
        _ => DkimAlgorithm::Rsa,
    }
}

fn build_email_config(r: &WorkspaceEmailSettings, password: String) -> EmailConfig {
    EmailConfig {
        smtp_host: r.smtp_host.clone(),
        // The DB CHECK constrains smtp_port to 1..=65535, so the clamp is a
        // belt-and-braces narrowing to u16 rather than real saturation.
        smtp_port: r.smtp_port.clamp(1, u16::MAX as i32) as u16,
        smtp_username: r.smtp_username.clone(),
        smtp_password: password,
        from_name: r.from_name.clone(),
        from_email: r.from_email.clone(),
        enabled: true,
        security: parse_security(&r.smtp_security),
    }
}

/// Map the stored `smtp_security` string onto the transport enum. The DB
/// CHECK restricts the column to these three values; an unexpected value
/// degrades to STARTTLS, the safe default.
fn parse_security(s: &str) -> SmtpSecurity {
    match s {
        "tls" => SmtpSecurity::Tls,
        "plaintext" => SmtpSecurity::Plaintext,
        _ => SmtpSecurity::StartTls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UpsertWorkspaceEmailSettings;
    use crate::repository::workspace_email_settings as repo;
    use crate::test_helpers::{setup_test_connection, setup_test_pool};

    fn fallback_service() -> Arc<EmailService> {
        Arc::new(EmailService::new(EmailConfig {
            smtp_host: "smtp.platform.test".into(),
            smtp_port: 587,
            smtp_username: "platform".into(),
            smtp_password: "platform-pw".into(),
            from_name: "Platform".into(),
            from_email: "platform@fallback.test".into(),
            enabled: true,
            security: SmtpSecurity::StartTls,
        }))
    }

    // Self-host policy (WORKSPACE mail may fall back to the env identity), set
    // explicitly so tests don't depend on the process-cached DeploymentMode.
    fn resolver_with_fallback() -> OutboundEmailResolver {
        OutboundEmailResolver::with_policy(setup_test_pool(), Some(fallback_service()), true, None)
    }

    fn resolver_no_fallback() -> OutboundEmailResolver {
        OutboundEmailResolver::with_policy(setup_test_pool(), None, true, None)
    }

    // Hosted policy WITHOUT a tenant domain: WORKSPACE mail must never fall
    // back to the shared platform identity, and with no managed domain an
    // unconfigured workspace is omitted (defers) — the pre-managed behaviour.
    fn resolver_hosted_with_fallback() -> OutboundEmailResolver {
        OutboundEmailResolver::with_policy(setup_test_pool(), Some(fallback_service()), false, None)
    }

    // Hosted policy WITH a tenant domain: an unconfigured workspace gets the
    // managed default identity `support@<slug>.nosdesk.test`.
    fn resolver_hosted_managed() -> OutboundEmailResolver {
        OutboundEmailResolver::with_policy(
            setup_test_pool(),
            Some(fallback_service()),
            false,
            Some("nosdesk.test".into()),
        )
    }

    fn enabled_fields() -> UpsertWorkspaceEmailSettings {
        UpsertWorkspaceEmailSettings {
            enabled: true,
            from_name: "Acme Support".into(),
            from_email: "support@acme.test".into(),
            smtp_host: "smtp.acme.test".into(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            smtp_username: "acme".into(),
            sending_mode: "smtp_relay".into(),
        }
    }

    #[test]
    fn resolves_workspace_identity_when_enabled() {
        let mut conn = setup_test_connection();
        repo::upsert(&mut conn, enabled_fields()).unwrap();

        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        let cfg = svc.config();
        assert_eq!(cfg.from_email, "support@acme.test");
        assert_eq!(cfg.smtp_host, "smtp.acme.test");
        assert_eq!(cfg.smtp_port, 465);
        assert_eq!(cfg.security, SmtpSecurity::Tls);
    }

    #[test]
    fn decrypts_password_into_config() {
        let mut conn = setup_test_connection();
        repo::upsert(&mut conn, enabled_fields()).unwrap();
        repo::set_password(&mut conn, 1, "acme-smtp-pw").unwrap();

        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().smtp_password, "acme-smtp-pw");
    }

    #[test]
    fn falls_back_when_workspace_disabled() {
        let mut conn = setup_test_connection();
        let mut fields = enabled_fields();
        fields.enabled = false;
        repo::upsert(&mut conn, fields).unwrap();

        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().from_email, "platform@fallback.test");
    }

    #[test]
    fn falls_back_when_workspace_unconfigured() {
        let mut conn = setup_test_connection();
        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().from_email, "platform@fallback.test");
    }

    #[test]
    fn errors_when_no_identity_and_no_fallback() {
        let mut conn = setup_test_connection();
        // `unwrap_err` would require EmailService: Debug; match instead.
        let result = resolver_no_fallback().resolve_on_conn(&mut conn, 1);
        assert!(matches!(result, Err(ResolveError::NotConfigured)));
    }

    #[test]
    fn enabled_but_hostless_falls_back() {
        let mut conn = setup_test_connection();
        let mut fields = enabled_fields();
        fields.smtp_host = "".into();
        repo::upsert(&mut conn, fields).unwrap();

        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().from_email, "platform@fallback.test");
    }

    #[test]
    fn has_fallback_reflects_env_service() {
        assert!(resolver_with_fallback().has_fallback());
        assert!(!resolver_no_fallback().has_fallback());
    }

    #[test]
    fn parse_security_maps_known_values() {
        assert_eq!(parse_security("tls"), SmtpSecurity::Tls);
        assert_eq!(parse_security("starttls"), SmtpSecurity::StartTls);
        assert_eq!(parse_security("plaintext"), SmtpSecurity::Plaintext);
        assert_eq!(parse_security("nonsense"), SmtpSecurity::StartTls);
    }

    #[test]
    fn resolve_batch_maps_own_identity_and_omits_unconfigured_on_hosted() {
        let mut conn = setup_test_connection();
        repo::upsert(&mut conn, enabled_fields()).unwrap();

        // HOSTED: workspace 1 has its own identity; 424242 reads as "no row"
        // (unconfigured). Even WITH a platform fallback present, the
        // unconfigured workspace is OMITTED — tenant mail must not send from
        // the SHARED platform identity — so the worker defers it.
        let map = resolver_hosted_with_fallback()
            .resolve_batch(&mut conn, &[1, 424242])
            .unwrap();
        assert_eq!(
            map.get(&1).unwrap().config().from_email,
            "support@acme.test"
        );
        assert!(
            map.get(&424242).is_none(),
            "on hosted an unconfigured workspace must not fall back to the platform identity"
        );
    }

    #[test]
    fn resolve_batch_falls_back_for_unconfigured_on_self_host() {
        let mut conn = setup_test_connection();
        repo::upsert(&mut conn, enabled_fields()).unwrap();

        // SELF-HOST: the env identity is the operator's own, so an unconfigured
        // workspace falls back to it rather than stranding its notification mail.
        let map = resolver_with_fallback()
            .resolve_batch(&mut conn, &[1, 424242])
            .unwrap();
        assert_eq!(
            map.get(&1).unwrap().config().from_email,
            "support@acme.test"
        );
        assert_eq!(
            map.get(&424242).unwrap().config().from_email,
            "platform@fallback.test",
            "on self-host an unconfigured workspace falls back to the env identity"
        );
    }

    #[test]
    fn resolve_batch_omits_unconfigured_without_fallback() {
        let mut conn = setup_test_connection();
        let map = resolver_no_fallback()
            .resolve_batch(&mut conn, &[424242])
            .unwrap();
        assert!(map.get(&424242).is_none());
    }

    // The bootstrap workspace in the test DB is id=1, slug 'default', name
    // 'Workspace' — the managed identity assertions below build on it.

    #[test]
    fn hosted_managed_resolves_unconfigured_to_tenant_subdomain_identity() {
        let mut conn = setup_test_connection();
        // No settings row at all: on hosted with a tenant domain, the
        // workspace resolves to its managed identity instead of deferring.
        let svc = resolver_hosted_managed()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        let cfg = svc.config();
        assert_eq!(cfg.from_email, "support@default.nosdesk.test");
        assert_eq!(cfg.from_name, "Workspace");
        // ...through the platform relay's transport settings.
        assert_eq!(cfg.smtp_host, "smtp.platform.test");
    }

    #[test]
    fn hosted_managed_applies_to_disabled_and_fallback_mode_rows() {
        let mut conn = setup_test_connection();
        let mut fields = enabled_fields();
        fields.enabled = false;
        repo::upsert(&mut conn, fields).unwrap();

        // `enabled=false` means "not using my own SMTP identity", not "mail
        // kill-switch": managed still applies, matching self-host's fallback.
        let svc = resolver_hosted_managed()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().from_email, "support@default.nosdesk.test");
    }

    #[test]
    fn hosted_managed_own_identity_still_wins() {
        let mut conn = setup_test_connection();
        repo::upsert(&mut conn, enabled_fields()).unwrap();

        let svc = resolver_hosted_managed()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(
            svc.config().from_email,
            "support@acme.test",
            "a usable workspace identity beats the managed default"
        );
    }

    #[test]
    fn hosted_managed_batch_maps_unconfigured_and_omits_unknown_workspace() {
        let mut conn = setup_test_connection();
        let map = resolver_hosted_managed()
            .resolve_batch(&mut conn, &[1, 424242])
            .unwrap();
        assert_eq!(
            map.get(&1).unwrap().config().from_email,
            "support@default.nosdesk.test"
        );
        assert!(
            map.get(&424242).is_none(),
            "no workspaces row (archived/unknown) still defers — never the platform From"
        );
    }

    #[test]
    fn hosted_managed_errors_direct_send_for_unknown_workspace() {
        let mut conn = setup_test_connection();
        // Direct sends must not leak the platform identity either: with the
        // managed tier applicable, an unresolvable workspace errs instead of
        // falling back to the env From (the pre-managed asymmetry).
        let result = resolver_hosted_managed().resolve_on_conn(&mut conn, 424242);
        assert!(matches!(result, Err(ResolveError::NotConfigured)));
    }

    #[test]
    fn hosted_without_tenant_domain_direct_send_keeps_env_fallback() {
        let mut conn = setup_test_connection();
        // Hosted but no NOSDESK_TENANT_DOMAIN: preserve the old direct-send
        // fallback rather than newly erroring.
        let svc = resolver_hosted_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().from_email, "platform@fallback.test");
    }

    #[test]
    fn verified_domain_not_yet_verified_falls_back() {
        let mut conn = setup_test_connection();
        let mut fields = enabled_fields();
        fields.sending_mode = "verified_domain".into();
        repo::upsert(&mut conn, fields).unwrap();
        // No DKIM provisioned, status defaults to 'unverified': we must NOT
        // send from the tenant's unverified domain, so fall back.
        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        assert_eq!(svc.config().from_email, "platform@fallback.test");
    }

    // Generates one RSA-2048 keypair (CPU-bound), so the verified-domain build
    // path is asserted in a single test.
    #[test]
    fn verified_domain_sends_via_relay_with_workspace_from() {
        use diesel::prelude::*;
        let mut conn = setup_test_connection();

        let mut fields = enabled_fields();
        fields.sending_mode = "verified_domain".into();
        fields.from_email = "support@acme.test".into();
        repo::upsert(&mut conn, fields).unwrap();
        repo::provision_dkim(&mut conn, 1, "acme.test").unwrap();
        {
            // Mark verified directly; the verification repo fn lands next step.
            use crate::schema::workspace_email_settings::dsl as w;
            diesel::update(w::workspace_email_settings)
                .set(w::verification_status.eq("verified"))
                .execute(&mut conn)
                .unwrap();
        }

        let svc = resolver_with_fallback()
            .resolve_on_conn(&mut conn, 1)
            .unwrap();
        // The From is the workspace's verified address...
        assert_eq!(svc.config().from_email, "support@acme.test");
        // ...but the transport is the instance relay (the fallback's host), not
        // a per-workspace relay. DKIM signing rides on this transport (covered
        // by the email-module signing tests).
        assert_eq!(svc.config().smtp_host, "smtp.platform.test");
    }
}
