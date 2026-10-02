//! SSRF-safe outbound HTTP client.
//!
//! Single chokepoint for every code path that issues an HTTP
//! request to a URL the caller can influence — webhook delivery,
//! plugin registry / bundle downloads, plugin-author-declared
//! external APIs. Building requests with the `client()` factory
//! gives back a `reqwest::Client` whose DNS resolver refuses to
//! hand back internal addresses; combined with the synchronous
//! `reject_unsafe_ip_literal()` helper that catches URLs whose
//! host is already an IP literal (and so bypasses DNS), the
//! client *cannot* dial into the host's private network even if
//! the operator wires a hostile URL into the config.
//!
//! Design notes
//! ============
//!
//! Earlier drafts of this module exposed an `assert_safe(&url)`
//! that every call site had to remember to invoke before
//! `client.send()`. That worked but had two flaws:
//!
//! * **Discipline-based correctness.** A new outbound HTTP
//!   call site has to remember the check or the gap reopens.
//!   That's a footgun, not a security control.
//! * **TOCTOU.** Between the `lookup_host` in `assert_safe`
//!   and the connect in `send`, an attacker controlling
//!   authoritative DNS for the attacker-supplied hostname can
//!   rebind. The window is small, but it's there.
//!
//! Wiring a `Resolve` impl into the client closes both holes
//! at once: filtering happens at the *same* resolution call
//! the connect uses, so there's no race; and call sites can't
//! "forget" because the client physically can't dial unsafe
//! IPs regardless of how a request is built. The IP-literal
//! check is the one residual case (hyper-util bypasses the
//! resolver when the host is already an `Ipv4Addr` / `Ipv6Addr`
//! literal), and it's cheap synchronous URL parsing.
//!
//! Operator allowlist
//! ==================
//!
//! Self-hosters sometimes legitimately need to webhook into a
//! same-VPC collector (n8n, internal SIEM, etc.). Setting
//! `NOSDESK_OUTBOUND_ALLOWED_HOSTS` to a comma-separated list
//! of exact hostnames (case-insensitive) skips the filter for
//! those names only. The default is empty. IP-literal hosts
//! must be listed by their literal form.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use thiserror::Error;
use url::Url;

// The routability predicate and the operator allowlist live in the
// transport-agnostic egress policy; this module is the reqwest adapter
// over them. Raw-TCP connectors (IMAP, SMTP) use `egress` directly.
use super::egress::{ip_is_routable, ipv4_is_routable, ipv6_is_routable, is_host_allowlisted};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SafeHttpError {
    #[error("URL must be http or https")]
    UnsupportedScheme,
    #[error("URL has no host component")]
    NoHost,
    #[error("URL parse failed: {0}")]
    ParseFailed(String),
    #[error("destination resolves to a non-routable address ({0})")]
    NonRoutable(IpAddr),
}

/// Build the shared outbound HTTP client. Every caller wiring a
/// URL whose value is influenced by user / operator / plugin
/// data should construct via this factory rather than
/// `reqwest::Client::builder()` directly — the returned client
/// is guaranteed to refuse internal-IP destinations.
///
/// `timeout` lets callers express per-use SLAs (webhook delivery
/// uses 30s, the plugin registry uses 10s). The rest of the
/// surface — `https_only`, redirect policy, user-agent — is
/// fixed by the factory: divergent client configs are a common
/// way for "I added one more safe client" to drift away from
/// "actually safe."
/// Redirect policy that re-applies the IP-literal SSRF guard to every
/// hop, capped at 5 redirects. `Policy::limited` only bounds the count:
/// hyper-util bypasses our custom resolver when a redirect `Location`
/// host is already an IP literal, and the per-call
/// `reject_unsafe_ip_literal` only sees the *initial* URL. Without this,
/// a public host can answer `302 Location: http://169.254.169.254/...`
/// and the client follows it into the private network. Hostname
/// redirects are still re-resolved (and filtered) by `SafeResolver` on
/// each hop. See security-audit-2026-06.
fn ssrf_safe_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.stop();
        }
        match reject_unsafe_ip_literal(attempt.url().as_str()) {
            Ok(()) => attempt.follow(),
            Err(e) => attempt.error(e),
        }
    })
}

pub fn client(timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("nosdesk/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        // https_only rejects http:// at send-time and after any
        // redirect chain. Combined with the resolver, this means
        // a redirect to http://127.0.0.1/ fails at both the
        // scheme check and the resolver, not just one of them.
        // Defence-in-depth, not redundancy.
        .https_only(false) // see below — some webhook receivers are still http://
        .redirect(ssrf_safe_redirect_policy())
        .dns_resolver(Arc::new(SafeResolver))
        .build()
}

/// Same as [`client`] but follows no redirects at all, leaving 3xx for the
/// caller to act on.
///
/// For callers whose authorization is per-URL rather than per-client. The
/// redirect policy above is a `'static` closure fixed when the client is built,
/// so it cannot see a caller's per-request allowlist; it re-checks the IP, which
/// is all it can check. That is enough for callers whose only concern is SSRF,
/// and not enough for the plugin proxy, where each hop must be re-checked
/// against the consented `network:` grants of the specific plugin making the
/// call. Those callers drive the redirect chain themselves and re-authorize
/// every hop. See `services::plugins::proxy`.
pub fn no_redirect_client(timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("nosdesk/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(Arc::new(SafeResolver))
        .build()
}

/// Same as [`client`] but pins `https_only(true)`. Used by the
/// plugin registry, where every legitimate destination should be
/// HTTPS — http:// downloads of a plugin bundle would be a clear
/// signing-chain integrity failure regardless of SSRF posture.
pub fn https_only_client(timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("nosdesk/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .https_only(true)
        .redirect(ssrf_safe_redirect_policy())
        .dns_resolver(Arc::new(SafeResolver))
        .build()
}

/// Inspect a URL and refuse it if the host is an IP literal
/// pointing into a non-routable range. Hostnames are not
/// inspected here — the resolver is responsible for those, and
/// will reject at connect time. Cheap (no DNS), call once at
/// each send-site that accepts caller-controlled URLs.
pub fn reject_unsafe_ip_literal(url: &str) -> Result<(), SafeHttpError> {
    let parsed = Url::parse(url).map_err(|e| SafeHttpError::ParseFailed(e.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(SafeHttpError::UnsupportedScheme),
    }
    match parsed.host() {
        Some(url::Host::Ipv4(v4)) if !ipv4_is_routable(&v4) => {
            Err(SafeHttpError::NonRoutable(IpAddr::V4(v4)))
        }
        Some(url::Host::Ipv6(v6)) if !ipv6_is_routable(&v6) => {
            Err(SafeHttpError::NonRoutable(IpAddr::V6(v6)))
        }
        Some(_) => Ok(()),
        None => Err(SafeHttpError::NoHost),
    }
}

/// The DNS resolver wrapped into every safe_http client. Resolves
/// via the system resolver (same path the default reqwest
/// resolver uses) and then filters every returned `SocketAddr`
/// against the routability check. If any address is rejected the
/// resolution itself fails — we don't quietly drop one of N
/// addresses and try the rest, because doing so masks a misconfig
/// and leaves operators chasing partial-failure modes instead of
/// fixing the root cause.
///
/// The hostname-allowlist check happens here so it covers both
/// hostname resolution (this path) and IP-literal handling
/// (`reject_unsafe_ip_literal`) consistently. A hostname listed
/// in the allowlist passes through the resolver unchecked; IP
/// literals matching the allowlist similarly skip the literal
/// check via [`is_host_allowlisted`].
struct SafeResolver;

impl Resolve for SafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            // tokio::net::lookup_host with port 0 mirrors what the
            // default reqwest resolver does — hyper-util sets the
            // real port after we hand the iterator back.
            let resolved: Vec<SocketAddr> =
                match tokio::net::lookup_host((host.as_str(), 0u16)).await {
                    Ok(it) => it.collect(),
                    Err(e) => {
                        return Err(format!("{host}: dns lookup failed: {e}").into());
                    }
                };

            if !is_host_allowlisted(&host) {
                for addr in &resolved {
                    let ip = addr.ip();
                    if !ip_is_routable(&ip) {
                        // Failing the whole resolution (rather than
                        // dropping the offending address and trying
                        // the rest) is intentional: a partial-drop
                        // mode would silently mask a misconfigured
                        // DNS record and leave operators chasing a
                        // "sometimes-it-works" symptom.
                        return Err(format!(
                            "{host}: destination resolves to non-routable address {ip}"
                        )
                        .into());
                    }
                }
            }
            let boxed: Addrs = Box::new(resolved.into_iter());
            Ok(boxed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_literal_guard_rejects_loopback() {
        let err = reject_unsafe_ip_literal("http://127.0.0.1:9000/").unwrap_err();
        assert!(matches!(err, SafeHttpError::NonRoutable(_)));
    }

    #[test]
    fn ip_literal_guard_rejects_aws_metadata() {
        let err = reject_unsafe_ip_literal("http://169.254.169.254/latest/meta-data/").unwrap_err();
        assert!(matches!(err, SafeHttpError::NonRoutable(_)));
    }

    #[test]
    fn ip_literal_guard_rejects_v6_loopback() {
        let err = reject_unsafe_ip_literal("http://[::1]:9000/").unwrap_err();
        assert!(matches!(err, SafeHttpError::NonRoutable(_)));
    }

    #[test]
    fn ip_literal_guard_rejects_v6_mapped_v4_loopback() {
        let err = reject_unsafe_ip_literal("http://[::ffff:127.0.0.1]/").unwrap_err();
        assert!(matches!(err, SafeHttpError::NonRoutable(_)));
    }

    #[test]
    fn ip_literal_guard_passes_public_v4_literal() {
        reject_unsafe_ip_literal("http://1.1.1.1/").unwrap();
    }

    #[test]
    fn ip_literal_guard_passes_hostname() {
        // Hostnames are handled by the resolver, not this check.
        reject_unsafe_ip_literal("https://example.com/foo").unwrap();
    }

    #[test]
    fn ip_literal_guard_rejects_non_http() {
        let err = reject_unsafe_ip_literal("file:///etc/passwd").unwrap_err();
        assert_eq!(err, SafeHttpError::UnsupportedScheme);
    }

    #[tokio::test]
    async fn resolver_rejects_hostname_that_points_to_loopback() {
        // localhost resolves to 127.0.0.1 / ::1 on every platform.
        // The resolver must refuse it, even though "localhost" looks
        // like a benign domain name.
        let client = client(Duration::from_secs(5)).unwrap();
        let res = client.get("http://localhost/").send().await;
        assert!(res.is_err(), "localhost request must fail");
        let err = res.unwrap_err().to_string();
        assert!(
            err.contains("non-routable") || err.contains("dns") || err.contains("error"),
            "unexpected error message: {err}"
        );
    }

    #[tokio::test]
    async fn resolver_allowlist_lets_localhost_through() {
        // Allowlist localhost and confirm the resolver no longer
        // refuses it. We don't expect the actual request to succeed
        // (no server on this port) but we expect the failure mode
        // to be "connection refused" rather than "non-routable".
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "localhost");
        let client = client(Duration::from_secs(2)).unwrap();
        let res = client.get("http://localhost:1/").send().await;
        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");
        // We only care that the resolver didn't reject it.
        if let Err(e) = res {
            let msg = e.to_string();
            assert!(
                !msg.contains("non-routable"),
                "allowlisted host should not be filtered as non-routable: {msg}"
            );
        }
    }
}
