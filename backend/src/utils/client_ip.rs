//! Trusted-proxy-aware client IP extraction.
//!
//! One canonical source of truth for "what IP is this request
//! coming from." Every handler / middleware that keys rate-limit
//! state, security-event logs, or session fingerprints on a
//! client address should read it through this module — the
//! direct `peer_addr()` / `connection_info().realip_remote_addr()`
//! shapes were a footgun:
//!
//! * Raw `peer_addr()` ignored `X-Forwarded-For` entirely. Behind
//!   a reverse proxy (the standard deployment shape) every
//!   request looks like it came from the proxy, so per-IP rate
//!   limits effectively rate-limited the proxy, not the client.
//! * Actix's `connection_info().realip_remote_addr()` trusts
//!   `X-Forwarded-For` / `X-Real-IP` / `Forwarded` unconditionally
//!   with no proxy gate. A direct request from a public network
//!   could set `X-Forwarded-For: 1.2.3.4` and bypass per-IP
//!   limits keyed on the spoofed header. Same shape as the
//!   classic AWS / Cloudflare misconfiguration.
//!
//! Algorithm
//! =========
//!
//! 1. Let `peer` = the TCP peer address. This is the address
//!    that actually opened the socket; it cannot be spoofed
//!    over the wire.
//! 2. If `peer` is not in `TRUSTED_PROXIES`, return `peer`.
//!    Any `X-Forwarded-For` on the request is attacker-controlled
//!    and ignored.
//! 3. Otherwise, walk the `X-Forwarded-For` header from right to
//!    left. The rightmost entries are appended by the trusted
//!    proxies; skip them. The first entry that is *not* in
//!    `TRUSTED_PROXIES` is the real client. If every entry is a
//!    trusted proxy (unusual but legal), fall back to `peer`.
//!
//! The right-to-left walk is critical. A left-to-right "first
//! entry wins" is the well-known XFF SSRF pattern: an attacker
//! adds `X-Forwarded-For: 1.2.3.4` on their own request, the
//! proxy appends the attacker's real IP, and the application
//! sees `1.2.3.4` first. Walking from the right and stopping at
//! the first untrusted entry forces the attacker to forge an
//! XFF entry *after* the trusted-proxy hops, which they can't.
//!
//! Configuration
//! =============
//!
//! `TRUSTED_PROXIES` is a comma-separated list of CIDR ranges
//! (e.g. `10.0.0.0/8,172.16.0.0/12`). Empty / unset means
//! `X-Forwarded-For` is never trusted — the only safe default
//! for an internet-exposed backend with no proxy in front.

use std::net::{IpAddr, SocketAddr};

use ipnetwork::IpNetwork;

const TRUSTED_PROXIES_ENV: &str = "TRUSTED_PROXIES";
const XFF_HEADER: &str = "X-Forwarded-For";

/// Resolve the client IP given the TCP peer address and an
/// optional `X-Forwarded-For` header value. Pure: takes parts,
/// returns IP; the request-shape adapters below feed into it.
///
/// Returns `None` only when `peer_addr` is `None` (i.e. the
/// connection has no socket address, which shouldn't happen for
/// real TCP traffic but can in tests / mock harnesses).
pub fn resolve(peer_addr: Option<SocketAddr>, xff_header: Option<&str>) -> Option<IpAddr> {
    let peer = peer_addr?.ip();
    let trusted = trusted_proxies();
    if trusted.is_empty() || !ip_in_any(&peer, &trusted) {
        // No proxy gate or the peer isn't one of our proxies —
        // the XFF header is attacker-controlled. Don't read it.
        return Some(peer);
    }
    // Peer is a trusted proxy. The XFF header it appended is
    // legitimate metadata; walk it from the right.
    if let Some(xff) = xff_header {
        for raw in xff.split(',').rev() {
            let candidate = raw.trim();
            if candidate.is_empty() {
                continue;
            }
            let Ok(ip) = candidate.parse::<IpAddr>() else {
                continue;
            };
            if !ip_in_any(&ip, &trusted) {
                return Some(ip);
            }
        }
    }
    // Either no XFF, malformed, or every entry is itself a
    // trusted proxy. Falling back to peer is the conservative
    // answer — we know it's a trusted proxy, but at least we're
    // not handing back an attacker-controlled string.
    Some(peer)
}

/// Resolve the client IP from an `actix_web::HttpRequest`.
pub fn from_http_request(req: &actix_web::HttpRequest) -> Option<IpAddr> {
    let xff = req
        .headers()
        .get(XFF_HEADER)
        .and_then(|h| h.to_str().ok());
    resolve(req.peer_addr(), xff)
}

/// Resolve the client IP from an `actix_web::dev::ServiceRequest`
/// (the shape used inside middleware before the request reaches
/// a handler).
pub fn from_service_request(req: &actix_web::dev::ServiceRequest) -> Option<IpAddr> {
    let xff = req
        .headers()
        .get(XFF_HEADER)
        .and_then(|h| h.to_str().ok());
    resolve(req.peer_addr(), xff)
}

fn trusted_proxies() -> Vec<IpNetwork> {
    let raw = match std::env::var(TRUSTED_PROXIES_ENV) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<IpNetwork>().ok())
        .collect()
}

fn ip_in_any(ip: &IpAddr, networks: &[IpNetwork]) -> bool {
    networks.iter().any(|n| n.contains(*ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests run in the same process, and we mutate the
    /// `TRUSTED_PROXIES` env var. Serialise them through a mutex
    /// so they don't read each other's writes mid-flight.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_trusted<F: FnOnce()>(value: &str, f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(TRUSTED_PROXIES_ENV, value);
        f();
        std::env::remove_var(TRUSTED_PROXIES_ENV);
    }

    fn with_unset<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(TRUSTED_PROXIES_ENV);
        f();
    }

    fn sock(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn unset_env_means_xff_is_ignored() {
        with_unset(|| {
            let r = resolve(Some(sock("203.0.113.1:80")), Some("1.2.3.4"));
            assert_eq!(r, Some(ip("203.0.113.1")));
        });
    }

    #[test]
    fn peer_not_in_trusted_means_xff_is_ignored() {
        with_trusted("10.0.0.0/8", || {
            // Peer is on a public network; an attacker could have
            // sent any XFF they wanted. Use peer.
            let r = resolve(Some(sock("203.0.113.1:80")), Some("1.2.3.4"));
            assert_eq!(r, Some(ip("203.0.113.1")));
        });
    }

    #[test]
    fn peer_in_trusted_walks_xff_right_to_left() {
        with_trusted("10.0.0.0/8", || {
            // Single-hop reverse proxy: backend sees the proxy at
            // 10.0.0.5; proxy added the client's real IP to XFF.
            let r = resolve(
                Some(sock("10.0.0.5:80")),
                Some("198.51.100.7"),
            );
            assert_eq!(r, Some(ip("198.51.100.7")));
        });
    }

    #[test]
    fn peer_in_trusted_skips_chained_trusted_proxies() {
        with_trusted("10.0.0.0/8", || {
            // Two-hop chain: client → edge proxy (10.0.0.6) → app
            // proxy (10.0.0.5) → backend. XFF carries the client
            // then the edge proxy; we walk from the right and
            // skip the trusted hop.
            let r = resolve(
                Some(sock("10.0.0.5:80")),
                Some("198.51.100.7, 10.0.0.6"),
            );
            assert_eq!(r, Some(ip("198.51.100.7")));
        });
    }

    #[test]
    fn attacker_cannot_spoof_via_prepended_xff() {
        with_trusted("10.0.0.0/8", || {
            // Attacker at 5.5.5.5 sends `X-Forwarded-For: 1.2.3.4`
            // attempting to forge a different source IP. The
            // edge proxy at 10.0.0.5 appends the attacker's real
            // IP, so the backend sees XFF "1.2.3.4, 5.5.5.5".
            // Walking from the right finds the attacker's real
            // IP first (5.5.5.5), not the forged value (1.2.3.4).
            let r = resolve(
                Some(sock("10.0.0.5:80")),
                Some("1.2.3.4, 5.5.5.5"),
            );
            assert_eq!(r, Some(ip("5.5.5.5")));
        });
    }

    #[test]
    fn empty_xff_falls_back_to_peer() {
        with_trusted("10.0.0.0/8", || {
            let r = resolve(Some(sock("10.0.0.5:80")), Some(""));
            assert_eq!(r, Some(ip("10.0.0.5")));
        });
    }

    #[test]
    fn missing_xff_falls_back_to_peer() {
        with_trusted("10.0.0.0/8", || {
            let r = resolve(Some(sock("10.0.0.5:80")), None);
            assert_eq!(r, Some(ip("10.0.0.5")));
        });
    }

    #[test]
    fn malformed_xff_entries_are_skipped() {
        with_trusted("10.0.0.0/8", || {
            let r = resolve(
                Some(sock("10.0.0.5:80")),
                Some("not-an-ip, 198.51.100.7"),
            );
            assert_eq!(r, Some(ip("198.51.100.7")));
        });
    }

    #[test]
    fn all_xff_entries_trusted_falls_back_to_peer() {
        with_trusted("10.0.0.0/8", || {
            // Pathological: every hop is inside trusted
            // infrastructure (e.g. an internal-only deployment).
            // No real client IP to report; fall back to peer.
            let r = resolve(
                Some(sock("10.0.0.5:80")),
                Some("10.0.0.6, 10.0.0.7"),
            );
            assert_eq!(r, Some(ip("10.0.0.5")));
        });
    }

    #[test]
    fn multiple_cidrs_in_env() {
        with_trusted("10.0.0.0/8, 172.16.0.0/12", || {
            let r = resolve(
                Some(sock("172.20.5.5:80")),
                Some("198.51.100.7"),
            );
            assert_eq!(r, Some(ip("198.51.100.7")));
        });
    }

    #[test]
    fn ipv6_peer_in_trusted_resolves_through_xff() {
        with_trusted("fd00::/8", || {
            let r = resolve(
                Some(sock("[fd00::1]:80")),
                Some("2001:db8::1"),
            );
            assert_eq!(r, Some(ip("2001:db8::1")));
        });
    }

    #[test]
    fn invalid_cidr_entries_in_env_are_ignored() {
        with_trusted("not-a-cidr, 10.0.0.0/8", || {
            // The bogus entry is dropped; the valid one still
            // governs the gate.
            let r = resolve(
                Some(sock("10.0.0.5:80")),
                Some("198.51.100.7"),
            );
            assert_eq!(r, Some(ip("198.51.100.7")));
        });
    }

    #[test]
    fn no_peer_addr_returns_none() {
        with_unset(|| {
            assert_eq!(resolve(None, Some("1.2.3.4")), None);
        });
    }
}
