//! Canonical host and origin for a tenant workspace.
//!
//! A hosted workspace lives at `<slug>.<NOSDESK_TENANT_DOMAIN>` or a verified
//! `custom_domain`. This is the single source of truth for the workspace's
//! host/origin, used to build tenant-facing URLs (password-reset / invite /
//! notification links) and the per-workspace WebAuthn RP ID. Generation is
//! workspace-derived so it works off the response path (the email queue has no
//! `HttpRequest`) and always targets the canonical host even when the
//! triggering request arrived on a different one.
//!
//! Validating an *incoming* request origin (CORS, collab WS) is the dual
//! concern and lives in `utils::cors_allowlist` (the tenant-subdomain regex).
//! Don't reimplement that here.

use crate::models::Workspace;

/// The configured hosted tenant base domain (`NOSDESK_TENANT_DOMAIN`), e.g.
/// `nosdesk.dev`. `None` in self-hosted single-tenant deployments (or when set
/// blank).
pub fn tenant_domain() -> Option<String> {
    non_empty(std::env::var("NOSDESK_TENANT_DOMAIN").ok())
}

/// Localpart of the managed default email identity every hosted workspace
/// gets: `support@<slug>.<tenant_domain>`. One constant shared by the
/// outbound resolver, reply routing, the inbound recipient parser, and the
/// admin config surface so the address can never drift between them.
pub const MANAGED_LOCALPART: &str = "support";

/// The managed default address for a workspace: `support@<slug>.<domain>`.
/// The single composition point for the address (see [`MANAGED_LOCALPART`]).
pub fn managed_email_address(slug: &str, tenant_domain: &str) -> String {
    format!("{MANAGED_LOCALPART}@{slug}.{tenant_domain}")
}

/// A From display name safe to hand to `lettre`'s `Mailbox` parser. Strips
/// control characters (header injection) and the quoting-sensitive
/// `<`/`>`/`"`; collapses surrounding whitespace. Returns `None` when nothing
/// displayable survives, so the caller can fall back (e.g. to the slug) —
/// a workspace named `"<\r\n>"` must degrade, not fail every send.
pub fn sanitise_from_display_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | '"'))
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

/// Pure host builder: a non-empty `custom_domain` wins, else
/// `<slug>.<tenant_domain>` when a non-empty `tenant_domain` is given, else
/// `None`. Returns the bare host (no scheme). This host is also the workspace's
/// WebAuthn RP ID.
pub fn canonical_host_for(
    slug: &str,
    custom_domain: Option<&str>,
    tenant_domain: Option<&str>,
) -> Option<String> {
    if let Some(domain) = custom_domain.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(domain.to_string());
    }
    let tenant_domain = tenant_domain.map(str::trim).filter(|s| !s.is_empty())?;
    Some(format!("{slug}.{tenant_domain}"))
}

/// A workspace's canonical host (no scheme), reading the configured tenant
/// domain. `None` in self-hosted mode or when unconfigured.
fn workspace_host(workspace: &Workspace) -> Option<String> {
    canonical_host_for(
        &workspace.slug,
        workspace.custom_domain.as_deref(),
        tenant_domain().as_deref(),
    )
}

/// A workspace's canonical origin (`https://<host>`).
pub fn workspace_origin(workspace: &Workspace) -> Option<String> {
    workspace_host(workspace).map(|host| format!("https://{host}"))
}

/// `https://<host>` for a canonical host. Small helper so callers that already
/// hold a host (e.g. a `WorkspaceContext`) don't reinvent the scheme join.
pub fn origin_from_host(host: Option<String>) -> Option<String> {
    host.map(|host| format!("https://{host}"))
}

/// Base URL for links emailed to a workspace's members (resets, invites,
/// notifications): the workspace's canonical origin, else `FRONTEND_URL`.
/// `None` only when neither is set (self-host without `FRONTEND_URL`); the
/// caller supplies a last resort (typically the request host).
pub fn email_link_base(canonical_origin: Option<String>) -> Option<String> {
    canonical_origin.or_else(|| non_empty(std::env::var("FRONTEND_URL").ok()))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{canonical_host_for, managed_email_address, sanitise_from_display_name};

    #[test]
    fn managed_address_composition() {
        assert_eq!(
            managed_email_address("acme", "nosdesk.app"),
            "support@acme.nosdesk.app"
        );
    }

    #[test]
    fn display_name_sanitiser_strips_injection_chars() {
        assert_eq!(
            sanitise_from_display_name("Acme <Support>\r\nBcc: x").as_deref(),
            Some("Acme SupportBcc: x")
        );
        assert_eq!(
            sanitise_from_display_name("Acme, Inc.").as_deref(),
            Some("Acme, Inc.")
        );
        assert_eq!(sanitise_from_display_name("\"<\r\n>\""), None);
        assert_eq!(sanitise_from_display_name("   "), None);
    }

    #[test]
    fn custom_domain_wins_over_slug() {
        assert_eq!(
            canonical_host_for("acme", Some("help.acme.com"), Some("nosdesk.dev")),
            Some("help.acme.com".to_string())
        );
    }

    #[test]
    fn slug_plus_tenant_domain_when_no_custom_domain() {
        assert_eq!(
            canonical_host_for("acme", None, Some("nosdesk.dev")),
            Some("acme.nosdesk.dev".to_string())
        );
    }

    #[test]
    fn none_without_custom_domain_or_tenant_domain() {
        assert_eq!(canonical_host_for("acme", None, None), None);
    }

    #[test]
    fn blank_values_are_treated_as_unset() {
        // Blank custom domain falls through to the tenant-domain form.
        assert_eq!(
            canonical_host_for("acme", Some("  "), Some("nosdesk.dev")),
            Some("acme.nosdesk.dev".to_string())
        );
        // Blank tenant domain with no custom domain yields None.
        assert_eq!(canonical_host_for("acme", None, Some("")), None);
    }
}
