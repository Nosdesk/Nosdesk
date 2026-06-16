//! Transport-agnostic SSRF egress policy.
//!
//! The routability rules and the operator allowlist live here, free of
//! any HTTP/reqwest dependency, so every outbound connector that dials a
//! caller-influenced destination shares one policy:
//!
//! * [`safe_http`](super::safe_http) wires the predicate into a
//!   `reqwest` resolver for HTTP(S) call sites (webhooks, plugin proxy,
//!   registry, image proxy).
//! * Raw-TCP connectors (IMAP, SMTP) call [`resolve_and_validate`] to
//!   turn an admin-supplied `host:port` into a vetted set of
//!   `SocketAddr`s, then connect to one of *those* addresses. Connecting
//!   to the validated address (not re-resolving the hostname) is what
//!   closes the TOCTOU / DNS-rebinding window for those transports, the
//!   same property the reqwest resolver gives the HTTP path.
//!
//! Operator allowlist: `NOSDESK_OUTBOUND_ALLOWED_HOSTS`, a
//! comma-separated list of exact hostnames (case-insensitive), skips the
//! routability filter for those names only. Self-hosters use it to reach
//! a same-VPC relay or collector. Default empty. IP-literal hosts must be
//! listed by their literal form.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EgressError {
    #[error("{host}: dns lookup failed: {message}")]
    DnsLookup { host: String, message: String },
    #[error("{host}: resolves to no addresses")]
    NoAddresses { host: String },
    #[error("{host}: destination resolves to a non-routable address ({ip})")]
    NonRoutable { host: String, ip: IpAddr },
    #[error("destination IP {0} is non-routable")]
    NonRoutableIp(IpAddr),
}

/// Reject an IP that falls in a non-routable range (loopback, private,
/// link-local / cloud metadata, CGNAT, and the other reserved blocks).
/// The synchronous half of the policy: callers that already hold a
/// concrete IP (an IP-literal host, or a pre-resolved address) gate on
/// this directly.
pub fn validate_resolved_ip(ip: &IpAddr) -> Result<(), EgressError> {
    if ip_is_routable(ip) {
        Ok(())
    } else {
        Err(EgressError::NonRoutableIp(*ip))
    }
}

/// Resolve `host:port` and return the vetted addresses, or fail the whole
/// resolution if any address is non-routable. The async half of the
/// policy, for raw-TCP connectors.
///
/// Failing on the *first* non-routable address (rather than dropping it
/// and trying the rest) is intentional: a partial-drop mode masks a
/// misconfigured or hostile DNS record and leaves operators chasing a
/// "sometimes-it-works" symptom. An allowlisted host skips the filter
/// entirely but is still resolved, so the caller connects to a real
/// address. This also covers IP-literal hosts uniformly: `lookup_host`
/// echoes a literal straight back, and the routability check then applies
/// to it with no separate literal guard needed (unlike the reqwest path,
/// where hyper-util bypasses the resolver for literals).
pub async fn resolve_and_validate(host: &str, port: u16) -> Result<Vec<SocketAddr>, EgressError> {
    let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| EgressError::DnsLookup {
            host: host.to_string(),
            message: e.to_string(),
        })?
        .collect();

    if resolved.is_empty() {
        return Err(EgressError::NoAddresses {
            host: host.to_string(),
        });
    }

    if !is_host_allowlisted(host) {
        for addr in &resolved {
            let ip = addr.ip();
            if !ip_is_routable(&ip) {
                return Err(EgressError::NonRoutable {
                    host: host.to_string(),
                    ip,
                });
            }
        }
    }

    Ok(resolved)
}

/// True when `host` is named in `NOSDESK_OUTBOUND_ALLOWED_HOSTS`. Shared
/// by the reqwest resolver and the raw-TCP path so one allowlist governs
/// every transport.
pub fn is_host_allowlisted(host: &str) -> bool {
    let raw = match std::env::var("NOSDESK_OUTBOUND_ALLOWED_HOSTS") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let host_lower = host.to_ascii_lowercase();
    raw.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .any(|allowed| allowed == host_lower)
}

// =============================================================================
// Routability helpers
// =============================================================================
//
// These mirror the `IpAddr::is_global()` semantics that are still
// unstable on stable Rust, with the v4 ranges enumerated explicitly so
// the behaviour is auditable.

pub fn ip_is_routable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_routable(v4),
        IpAddr::V6(v6) => ipv6_is_routable(v6),
    }
}

pub fn ipv4_is_routable(ip: &Ipv4Addr) -> bool {
    if ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_documentation()
    {
        return false;
    }
    let octets = ip.octets();
    // Carrier-grade NAT (RFC 6598): 100.64.0.0/10.
    if octets[0] == 100 && (octets[1] & 0xc0) == 64 {
        return false;
    }
    // Reserved: 240.0.0.0/4 (class E, future-use).
    if octets[0] >= 240 {
        return false;
    }
    // "This network": 0.0.0.0/8.
    if octets[0] == 0 {
        return false;
    }
    // IETF-protocol assignments: 192.0.0.0/24.
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
        return false;
    }
    // Benchmarking: 198.18.0.0/15.
    if octets[0] == 198 && (octets[1] == 18 || octets[1] == 19) {
        return false;
    }
    true
}

pub fn ipv6_is_routable(ip: &Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_multicast() || ip.is_unspecified() {
        return false;
    }
    let segments = ip.segments();
    // Unique local (fc00::/7).
    if (segments[0] & 0xfe00) == 0xfc00 {
        return false;
    }
    // Link local (fe80::/10).
    if (segments[0] & 0xffc0) == 0xfe80 {
        return false;
    }
    // IPv4-mapped (::ffff:0:0/96) — recurse on the embedded v4 so an
    // IPv6-cloaked v4 loopback can't slip past.
    if segments[0] == 0
        && segments[1] == 0
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0xffff
    {
        let mapped = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            (segments[6] & 0xff) as u8,
            (segments[7] >> 8) as u8,
            (segments[7] & 0xff) as u8,
        );
        return ipv4_is_routable(&mapped);
    }
    // Documentation (2001:db8::/32).
    if segments[0] == 0x2001 && segments[1] == 0xdb8 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_loopback_v4() {
        assert!(!ipv4_is_routable(&Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn rejects_rfc1918() {
        assert!(!ipv4_is_routable(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!ipv4_is_routable(&Ipv4Addr::new(172, 16, 0, 1)));
        assert!(!ipv4_is_routable(&Ipv4Addr::new(172, 31, 255, 255)));
        assert!(!ipv4_is_routable(&Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn rejects_link_local_and_metadata() {
        // 169.254.169.254 is the AWS / Azure / GCP metadata IP and the
        // canonical SSRF target.
        assert!(!ipv4_is_routable(&Ipv4Addr::new(169, 254, 169, 254)));
        assert!(!ipv4_is_routable(&Ipv4Addr::new(169, 254, 0, 1)));
    }

    #[test]
    fn rejects_cgnat_and_zero() {
        assert!(!ipv4_is_routable(&Ipv4Addr::new(100, 64, 0, 1)));
        assert!(!ipv4_is_routable(&Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!ipv4_is_routable(&Ipv4Addr::new(0, 0, 0, 0)));
    }

    #[test]
    fn accepts_public_v4() {
        assert!(ipv4_is_routable(&Ipv4Addr::new(8, 8, 8, 8)));
        assert!(ipv4_is_routable(&Ipv4Addr::new(1, 1, 1, 1)));
        assert!(ipv4_is_routable(&Ipv4Addr::new(140, 82, 114, 4)));
    }

    #[test]
    fn rejects_v6_loopback_and_local() {
        assert!(!ipv6_is_routable(&Ipv6Addr::LOCALHOST));
        assert!(!ipv6_is_routable(&"fe80::1".parse::<Ipv6Addr>().unwrap()));
        assert!(!ipv6_is_routable(&"fc00::1".parse::<Ipv6Addr>().unwrap()));
        assert!(!ipv6_is_routable(&"fd12::1".parse::<Ipv6Addr>().unwrap()));
    }

    #[test]
    fn rejects_v6_mapped_v4_loopback() {
        let mapped: Ipv6Addr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(!ipv6_is_routable(&mapped));
    }

    #[test]
    fn accepts_public_v6() {
        let pub_v6: Ipv6Addr = "2606:4700:4700::1111".parse().unwrap();
        assert!(ipv6_is_routable(&pub_v6));
    }

    #[test]
    fn validate_resolved_ip_rejects_loopback() {
        let err = validate_resolved_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))).unwrap_err();
        assert!(matches!(err, EgressError::NonRoutableIp(_)));
    }

    #[test]
    fn validate_resolved_ip_passes_public() {
        validate_resolved_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))).unwrap();
    }

    #[tokio::test]
    async fn resolve_and_validate_rejects_loopback_literal() {
        // An IP literal is echoed back by lookup_host and must be caught
        // by the routability check, with no separate literal guard.
        let err = resolve_and_validate("127.0.0.1", 993).await.unwrap_err();
        assert!(matches!(err, EgressError::NonRoutable { .. }));
    }

    #[tokio::test]
    async fn resolve_and_validate_rejects_metadata_literal() {
        let err = resolve_and_validate("169.254.169.254", 80)
            .await
            .unwrap_err();
        assert!(matches!(err, EgressError::NonRoutable { .. }));
    }

    // The allowlist reads a process-global env var, so the reject-vs-allow
    // assertions for the same host (localhost) are serialized into one
    // test under a lock. Run separately they race the var against each
    // other under cargo's parallel harness.
    #[tokio::test]
    async fn resolve_and_validate_localhost_reject_then_allowlist() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        // No allowlist: localhost (loopback) is rejected.
        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");
        let err = resolve_and_validate("localhost", 993).await.unwrap_err();
        assert!(matches!(err, EgressError::NonRoutable { .. }));

        // Allowlisted: the same host resolves with the filter skipped.
        std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "localhost");
        let res = resolve_and_validate("localhost", 993).await;
        std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS");
        let addrs = res.expect("allowlisted host should resolve without a routability rejection");
        assert!(!addrs.is_empty());
        assert!(addrs.iter().all(|a| a.port() == 993));
    }
}
