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
use crate::models::WorkspaceEmailSettings;
use crate::repository::channels::CredentialError;
use crate::repository::workspace_email_settings as ws_settings;
use crate::sync::session::{run_in_workspace, BackgroundRunError};
use crate::utils::email::{EmailConfig, EmailService, SmtpSecurity};

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
        let mut usable: HashMap<i32, WorkspaceEmailSettings> =
            ws_settings::get_for_workspaces(conn, workspace_ids)?
                .into_iter()
                .filter(is_usable)
                .map(|r| (r.workspace_id, r))
                .collect();

        let mut out = HashMap::with_capacity(workspace_ids.len());
        for &ws in workspace_ids {
            if out.contains_key(&ws) {
                continue;
            }
            let svc = match usable.remove(&ws) {
                Some(r) => match build_workspace_service(&r) {
                    Ok(svc) => svc,
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
        match row {
            Some(ref r) if is_usable(r) => build_workspace_service(r),
            _ => self.fallback.clone().ok_or(ResolveError::NotConfigured),
        }
    }
}

/// Build the `EmailService` for a usable workspace row: decrypt the password
/// (empty when none is stored, for an auth-less relay) and assemble the
/// config. Shared by the single and batch resolution paths.
fn build_workspace_service(r: &WorkspaceEmailSettings) -> Result<Arc<EmailService>, ResolveError> {
    let password = ws_settings::decrypt_password(r)
        .map_err(ResolveError::Credential)?
        .unwrap_or_default();
    Ok(Arc::new(EmailService::new(build_email_config(r, password))))
}

/// A workspace identity is usable only when enabled and pointing at a host.
/// An enabled-but-hostless row (mid-setup) falls back rather than building a
/// transport that can't connect.
fn is_usable(r: &WorkspaceEmailSettings) -> bool {
    r.enabled && !r.smtp_host.trim().is_empty()
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
}
