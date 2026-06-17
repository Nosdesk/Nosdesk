//! Per-workspace outbound email resolver.
//!
//! Outbound mail used to flow through a single instance-global
//! `EmailService` built from env. This resolver replaces that single
//! reference: given a workspace, it returns the `EmailService` to send
//! with, built from the workspace's own `workspace_email_settings` row, and
//! falls back to the env-configured service when the workspace has no
//! identity (so single-tenant self-host is unchanged).
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
        Self { pool, fallback }
    }

    /// Resolve on a connection the caller already holds. The caller is
    /// expected to have scoped that connection to `workspace_id` (or to hold
    /// a bypass connection); the read filters by `workspace_id` either way.
    /// No extra connection is checked out.
    pub fn resolve_on_conn(
        &self,
        conn: &mut DbConnection,
        workspace_id: i32,
    ) -> Result<Arc<EmailService>, ResolveError> {
        let row = ws_settings::get_for_workspace(conn, workspace_id).map_err(ResolveError::Db)?;
        self.from_row(row)
    }

    /// Resolve using the resolver's own pooled, RLS-pinned connection, for
    /// call sites with no connection in scope (the IMAP adapter at
    /// construction, the admin test-send endpoint).
    pub fn resolve_owned(&self, workspace_id: i32) -> Result<Arc<EmailService>, ResolveError> {
        let row = run_in_workspace(&self.pool, "email-resolver", workspace_id, |conn| {
            ws_settings::get_for_workspace(conn, workspace_id)
        })
        .map_err(ResolveError::Background)?;
        self.from_row(row)
    }

    /// Resolve the sending service for each of `workspace_ids` in one read,
    /// for the queue worker's drain. A workspace with a usable identity maps
    /// to its own service; one without maps to the fallback (when present).
    /// A workspace whose stored password won't decrypt is omitted, so its
    /// mail defers rather than sending from the wrong identity. Workspaces
    /// with neither a usable identity nor a fallback are omitted too, so the
    /// caller reads a missing entry as "unconfigured".
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

        let mut out = HashMap::with_capacity(workspace_ids.len());
        for &ws in workspace_ids {
            if out.contains_key(&ws) {
                continue;
            }
            let svc = match by_ws.get(&ws) {
                Some(r) => match self.build_for_row(r) {
                    Ok(Some(svc)) => svc,
                    Ok(None) => match &self.fallback {
                        Some(f) => f.clone(),
                        None => continue,
                    },
                    Err(e) => {
                        tracing::warn!(
                            workspace_id = ws,
                            error = %e,
                            "skipping workspace email identity; its mail will defer"
                        );
                        continue;
                    }
                },
                None => match &self.fallback {
                    Some(f) => f.clone(),
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
    /// fallback, or `NotConfigured` when there is none.
    fn from_row(
        &self,
        row: Option<WorkspaceEmailSettings>,
    ) -> Result<Arc<EmailService>, ResolveError> {
        if let Some(ref r) = row {
            if let Some(svc) = self.build_for_row(r)? {
                return Ok(svc);
            }
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

    fn resolver_with_fallback() -> OutboundEmailResolver {
        OutboundEmailResolver::new(setup_test_pool(), Some(fallback_service()))
    }

    fn resolver_no_fallback() -> OutboundEmailResolver {
        OutboundEmailResolver::new(setup_test_pool(), None)
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
    fn resolve_batch_maps_workspace_and_fallback() {
        let mut conn = setup_test_connection();
        repo::upsert(&mut conn, enabled_fields()).unwrap();

        // RLS scopes the test connection to workspace 1, so 424242 reads as
        // "no row" and takes the fallback, exactly as an unconfigured
        // workspace would under the worker's bypass read.
        let map = resolver_with_fallback()
            .resolve_batch(&mut conn, &[1, 424242])
            .unwrap();
        assert_eq!(
            map.get(&1).unwrap().config().from_email,
            "support@acme.test"
        );
        assert_eq!(
            map.get(&424242).unwrap().config().from_email,
            "platform@fallback.test"
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
