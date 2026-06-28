//! CORS origin allowlist (M5 product-side handoff Task 6).
//!
//! actix-cors's `.allowed_origin(...)` is exact-string only, which
//! fails every tenant subdomain preflight on a hosted deployment.
//! This module rolls a small allowlist with two pass-through modes:
//!
//!   * **Exact match** against `FRONTEND_URL` + any
//!     `ADDITIONAL_CORS_ORIGINS` — used for the dashboard apex and
//!     for operator-added third-party origins.
//!   * **Anchored regex** on `<slug>.<NOSDESK_TENANT_DOMAIN>(:port)?`
//!     — so every legitimate tenant subdomain passes without
//!     per-tenant config.
//!
//! The handoff doc's CORS-bypass classic is `.ends_with(suffix)`
//! matching, which lets `https://evil.com.nosdesk.app.attacker.com`
//! through. The regex here is `$`-anchored at the end of the host
//! (plus optional port), so a suffix-prefixed attacker host can't
//! slip in.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use tracing::warn;

/// Origins the official native (Tauri) app's webview presents, by platform:
/// macOS / iOS / Linux use `tauri://localhost`; Windows / Android use
/// `http://tauri.localhost` (or `https://tauri.localhost` with `useHttpsScheme`).
/// These are fixed Tauri platform constants, not deployment values.
///
/// Trusted by default so the app's SSE stream and collab WebSocket work, both
/// run in the webview (unlike REST, which uses the native HTTP client and never
/// touches this allowlist). Safe to trust: a web page cannot forge these origins
/// (the browser sets `Origin` to the real page origin), and every SSE / WS
/// connection is still token / bearer authenticated, so the origin check is
/// defense-in-depth, not the auth gate. Operators who don't want it set
/// `NOSDESK_ALLOW_NATIVE_APP=false`.
pub const NATIVE_APP_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

/// Whether the native-app origins are trusted. Default `true`; disabled only by
/// `NOSDESK_ALLOW_NATIVE_APP` set to an explicit falsey value
/// (`false` / `0` / `no` / `off`). Unset or anything else stays enabled.
pub fn native_app_allowed_from_env() -> bool {
    match std::env::var("NOSDESK_ALLOW_NATIVE_APP") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "no" | "off"
        ),
        Err(_) => true,
    }
}

#[derive(Debug, Clone)]
pub struct CorsAllowlist {
    /// Scheme+host[:port] of every exact-match allowed origin
    /// (canonical form `https://app.nosdesk.com` or
    /// `http://localhost:3000`).
    exact: HashSet<String>,
    /// Anchored regex matching any `<slug>.<tenant_domain>` origin.
    /// `None` when no `NOSDESK_TENANT_DOMAIN` is set (the
    /// self-hosted default — rely on FRONTEND_URL alone).
    tenant_re: Option<Regex>,
}

impl CorsAllowlist {
    /// Build the allowlist from the operator's CORS env config.
    /// `exact_origins` is the iterator over raw `FRONTEND_URL` +
    /// `ADDITIONAL_CORS_ORIGINS` strings; entries that fail URL
    /// parse log a warning and get skipped. `tenant_domain` is the
    /// shared suffix for hosted-mode subdomains (e.g.
    /// `nosdesk.app`); leave it `None` for self-hosted.
    pub fn new<'a>(
        exact_origins: impl IntoIterator<Item = &'a str>,
        tenant_domain: Option<&str>,
        allow_native_app: bool,
    ) -> Self {
        let mut exact = HashSet::new();
        // The native app's webview origins are fixed platform constants, so they
        // go straight in as literals (no URL parse, which is unreliable for the
        // `tauri://` custom scheme). Both the HTTP CORS layer and the collab WS
        // origin guard read this set, so this is the only wiring needed.
        if allow_native_app {
            for origin in NATIVE_APP_ORIGINS {
                exact.insert(origin.to_string());
            }
        }
        for raw in exact_origins {
            match url::Url::parse(raw) {
                Ok(u) => {
                    if u.host_str().is_some() {
                        let port = match u.port() {
                            Some(p) => format!(":{p}"),
                            None => String::new(),
                        };
                        exact.insert(format!(
                            "{}://{}{}",
                            u.scheme(),
                            u.host_str().unwrap(),
                            port
                        ));
                    } else {
                        warn!(origin = %raw, "CORS origin has no host; ignoring");
                    }
                }
                Err(e) => {
                    warn!(origin = %raw, error = ?e, "CORS origin failed URL parse; ignoring");
                }
            }
        }

        let tenant_re = tenant_domain.map(|suffix| {
            let escaped = regex::escape(suffix);
            // Slug: lowercase alphanumeric, hyphens allowed but not
            // leading/trailing. Matches the DB CHECK on workspaces.slug.
            // Optional port after the suffix.
            let pattern =
                format!(r"^https?://[a-z0-9]([a-z0-9-]{{0,62}}[a-z0-9])?\.{escaped}(:[0-9]+)?$");
            Regex::new(&pattern).expect("compile tenant subdomain regex")
        });

        Self { exact, tenant_re }
    }

    /// Number of exact-match origins (for the boot-time info log).
    pub fn exact_count(&self) -> usize {
        self.exact.len()
    }

    /// Returns true if the origin should be reflected back to the
    /// browser by the CORS preflight handler. Origin strings come
    /// straight from the browser's `Origin:` header.
    pub fn allows(&self, origin: &str) -> bool {
        if self.exact.contains(origin) {
            return true;
        }
        if let Some(re) = self.tenant_re.as_ref() {
            if re.is_match(origin) {
                return true;
            }
        }
        false
    }

    /// Build the allowlist from the process environment: `FRONTEND_URL`
    /// (the canonical origin), comma-separated `ADDITIONAL_CORS_ORIGINS`,
    /// and the optional `NOSDESK_TENANT_DOMAIN` subdomain suffix. This is
    /// the lazy fallback for [`global`] when `main` hasn't installed one
    /// (tests, CLI tooling); `main` builds the authoritative instance
    /// (which also enforces the production FRONTEND_URL requirement) and
    /// installs it via [`set_global`].
    pub fn from_env() -> Self {
        let frontend_url =
            std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let additional: Vec<String> = std::env::var("ADDITIONAL_CORS_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let tenant = std::env::var("NOSDESK_TENANT_DOMAIN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self::new(
            std::iter::once(frontend_url.as_str()).chain(additional.iter().map(String::as_str)),
            tenant.as_deref(),
            native_app_allowed_from_env(),
        )
    }
}

// Process-wide CORS allowlist. CORS config is constant for the process
// lifetime (env-derived), so it lives in a global rather than per-App
// `web::Data` — both the HTTP CORS layer and the collab WebSocket origin
// guard read it via `global()`, with no app_data to wire (or forget) in
// every test. Mirrors how the rest of the backend holds env config:
// JWT_SECRET (utils::jwt), the MFA keyring (utils::encryption), OIDC
// clients (oidc), etc.
static GLOBAL: OnceLock<CorsAllowlist> = OnceLock::new();

/// Install the authoritative allowlist. Called once from `main` at
/// startup; a no-op if one is already set.
pub fn set_global(allowlist: CorsAllowlist) {
    let _ = GLOBAL.set(allowlist);
}

/// The process CORS allowlist. Lazily built from the environment if `main`
/// has not installed one (tests, tooling), so request handlers never
/// depend on app_data being registered.
pub fn global() -> &'static CorsAllowlist {
    GLOBAL.get_or_init(CorsAllowlist::from_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(tenant: Option<&str>) -> CorsAllowlist {
        // Native-app origins off here so the existing assertions stay focused on
        // exact/tenant matching; the native-app behaviour has its own tests.
        CorsAllowlist::new(
            ["https://app.nosdesk.com", "http://localhost:3000"],
            tenant,
            false,
        )
    }

    #[test]
    fn exact_match_passes() {
        let allow = build(Some("nosdesk.app"));
        assert!(allow.allows("https://app.nosdesk.com"));
        assert!(allow.allows("http://localhost:3000"));
    }

    #[test]
    fn exact_match_wrong_scheme_blocked() {
        let allow = build(Some("nosdesk.app"));
        assert!(!allow.allows("http://app.nosdesk.com"));
        assert!(!allow.allows("https://localhost:3000"));
    }

    #[test]
    fn exact_match_wrong_port_blocked() {
        let allow = build(Some("nosdesk.app"));
        assert!(!allow.allows("http://localhost:5173"));
    }

    #[test]
    fn tenant_subdomain_passes() {
        let allow = build(Some("nosdesk.app"));
        assert!(allow.allows("https://acme.nosdesk.app"));
        assert!(allow.allows("https://acme-co.nosdesk.app"));
        assert!(allow.allows("https://acme.nosdesk.app:443"));
        assert!(allow.allows("http://acme.nosdesk.app")); // either scheme
    }

    #[test]
    fn substring_bypass_attempts_blocked() {
        let allow = build(Some("nosdesk.app"));
        // Classic suffix-match bypass: attacker host has tenant
        // suffix as a substring but with more after it.
        assert!(!allow.allows("https://acme.nosdesk.app.attacker.com"));
        assert!(!allow.allows("https://evil.com/nosdesk.app"));
        // Multi-label subdomain — our regex only allows ONE label
        // before the tenant suffix.
        assert!(!allow.allows("https://a.b.nosdesk.app"));
    }

    #[test]
    fn malformed_slug_blocked() {
        let allow = build(Some("nosdesk.app"));
        // Hyphen at the start or end of the slug.
        assert!(!allow.allows("https://-acme.nosdesk.app"));
        assert!(!allow.allows("https://acme-.nosdesk.app"));
        // Uppercase. Browsers should send lowercase but if they
        // don't, we reject.
        assert!(!allow.allows("https://Acme.nosdesk.app"));
    }

    #[test]
    fn additional_origin_accepted_alongside_canonical() {
        // Multi-origin self-host: FRONTEND_URL is a hostname and
        // ADDITIONAL_CORS_ORIGINS adds a bare LAN IP (the school /
        // self-signed-cert case). Both must pass, and the collab
        // WebSocket guard relies on this same allowlist, so this also
        // pins that a non-canonical-but-allowed origin can open the
        // socket while an unrelated origin can't.
        let allow = CorsAllowlist::new(
            ["https://helpdesk.school.internal", "https://10.0.5.20:8443"],
            None,
            false,
        );
        assert!(allow.allows("https://helpdesk.school.internal"));
        assert!(allow.allows("https://10.0.5.20:8443"));
        // Wrong port and an unrelated origin stay blocked.
        assert!(!allow.allows("https://10.0.5.20"));
        assert!(!allow.allows("https://evil.example.com"));
    }

    #[test]
    fn no_tenant_domain_means_subdomain_blocked() {
        let allow = build(None);
        assert!(allow.allows("https://app.nosdesk.com")); // exact still works
        assert!(!allow.allows("https://acme.nosdesk.app"));
    }

    #[test]
    fn tenant_domain_with_regex_metachars_is_escaped() {
        // If an operator accidentally sets the tenant domain with a
        // regex meta-character, the escape pass means it still only
        // matches the literal.
        let allow = CorsAllowlist::new(["https://app.nosdesk.com"], Some("foo|nosdesk.app"), false);
        // The literal suffix is `foo|nosdesk.app`; only origins
        // ending in exactly that string pass. The pipe shouldn't
        // act as a regex alternation.
        assert!(!allow.allows("https://acme.foo"));
        assert!(!allow.allows("https://acme.nosdesk.app"));
        assert!(allow.allows("https://acme.foo|nosdesk.app"));
    }

    #[test]
    fn native_app_origins_allowed_when_enabled() {
        // The collab WS guard and the HTTP CORS layer share this allowlist, so
        // this pins that the native app's webview origins (all platforms) pass
        // when enabled, alongside the normal exact origins.
        let allow = CorsAllowlist::new(["https://app.nosdesk.com"], Some("nosdesk.app"), true);
        assert!(allow.allows("tauri://localhost")); // macOS / iOS / Linux
        assert!(allow.allows("http://tauri.localhost")); // Windows / Android
        assert!(allow.allows("https://tauri.localhost")); // useHttpsScheme
                                                          // Normal origins still work.
        assert!(allow.allows("https://app.nosdesk.com"));
        assert!(allow.allows("https://acme.nosdesk.app"));
    }

    #[test]
    fn native_app_origins_blocked_when_disabled() {
        let allow = CorsAllowlist::new(["https://app.nosdesk.com"], Some("nosdesk.app"), false);
        assert!(!allow.allows("tauri://localhost"));
        assert!(!allow.allows("http://tauri.localhost"));
        assert!(!allow.allows("https://tauri.localhost"));
        // The real origins are unaffected by the toggle.
        assert!(allow.allows("https://app.nosdesk.com"));
    }

    #[test]
    fn native_app_lookalike_origin_not_trusted() {
        // Enabling the native app must not trust an attacker host that merely
        // contains the literal (exact-match only, no substring/suffix matching).
        let allow = CorsAllowlist::new(["https://app.nosdesk.com"], None, true);
        assert!(!allow.allows("tauri://localhost.attacker.com"));
        assert!(!allow.allows("https://tauri.localhost.evil.com"));
        assert!(!allow.allows("https://nottauri.localhost"));
    }
}
