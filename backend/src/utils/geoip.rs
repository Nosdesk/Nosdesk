//! Offline IP geolocation for the active-sessions display.
//!
//! Resolves a client IP to a coarse `"City, Country"` (or just
//! `"Country"`) label from a local MaxMind-format database (`.mmdb`).
//! The lookup is fully offline: it never leaves the process, so no
//! client IP is sent to any third party, which is the whole reason to
//! prefer a local database over a geolocation API for a self-hosted
//! product.
//!
//! Nosdesk ships no database. The operator points `GEOIP_DB_PATH` at a
//! `.mmdb` file, for example the free CC BY 4.0 DB-IP Lite database, or
//! a licensed MaxMind / DB-IP commercial database for better accuracy
//! without an attribution requirement. When the variable is unset or the
//! file can't be read, geolocation is disabled and sessions simply store
//! no location (the row falls back to device + last-active). Because the
//! reader takes any MaxMind-format file, swapping a Country database for
//! a City one is a file change with no code change.

use std::net::IpAddr;
use std::sync::OnceLock;

use maxminddb::{geoip2, Reader};

const GEOIP_DB_PATH_ENV: &str = "GEOIP_DB_PATH";

/// Process-wide resolver. `None` means geolocation is disabled (no path
/// configured, or the database failed to open).
static RESOLVER: OnceLock<Option<GeoIpResolver>> = OnceLock::new();

struct GeoIpResolver {
    reader: Reader<Vec<u8>>,
}

impl GeoIpResolver {
    fn resolve(&self, ip: IpAddr) -> Option<String> {
        // Private / reserved ranges never carry a public location, so
        // short-circuit before touching the database.
        if is_reserved(ip) {
            return None;
        }
        // A miss decodes to `Ok(None)` rather than erroring; either way
        // we degrade to no location.
        let record = self
            .reader
            .lookup(ip)
            .ok()?
            .decode::<geoip2::City>()
            .ok()??;
        let city = record.city.names.english;
        let country = record.country.names.english;
        match (city, country) {
            (Some(c), Some(co)) => Some(format!("{c}, {co}")),
            (None, Some(co)) => Some(co.to_string()),
            // City-only with no country is not useful on its own.
            _ => None,
        }
    }
}

/// Open the GeoIP database once at startup from `GEOIP_DB_PATH`.
/// Idempotent; only the first call has any effect. Logs once whether
/// geolocation ended up enabled so operators can see it in the boot log.
pub fn init_from_env() {
    RESOLVER.get_or_init(load_from_env);
}

fn load_from_env() -> Option<GeoIpResolver> {
    match std::env::var(GEOIP_DB_PATH_ENV) {
        Ok(path) if !path.trim().is_empty() => match Reader::open_readfile(&path) {
            Ok(reader) => {
                tracing::info!(path = %path, "GeoIP session location enabled");
                Some(GeoIpResolver { reader })
            }
            Err(e) => {
                tracing::warn!(
                    path = %path,
                    error = %e,
                    "GeoIP disabled: GEOIP_DB_PATH set but the database could not be opened"
                );
                None
            }
        },
        _ => {
            tracing::info!("GeoIP session location disabled (GEOIP_DB_PATH not set)");
            None
        }
    }
}

/// Resolve a client IP to a coarse `"City, Country"` label. Returns
/// `None` when geolocation is disabled, the IP is private/reserved, or
/// the address isn't present in the database. Cheap and lock-free; the
/// database reader is opened once and shared.
pub fn lookup(ip: IpAddr) -> Option<String> {
    RESOLVER
        .get()
        .and_then(Option::as_ref)
        .and_then(|resolver| resolver.resolve(ip))
}

/// Loopback, private (RFC 1918), link-local, CGNAT (100.64.0.0/10),
/// unspecified, broadcast, and IPv6 link-local / unique-local addresses
/// never resolve to a public location.
fn is_reserved(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // CGNAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // unique-local fc00::/7
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reserved(s: &str) -> bool {
        is_reserved(s.parse::<IpAddr>().unwrap())
    }

    #[test]
    fn is_reserved_classifies_ranges() {
        // Reserved / private ranges never carry a public location.
        for ip in [
            "127.0.0.1",       // loopback
            "10.0.0.1",        // RFC1918
            "172.16.0.1",      // RFC1918
            "192.168.1.1",     // RFC1918
            "169.254.1.1",     // link-local
            "100.64.0.1",      // CGNAT
            "0.0.0.0",         // unspecified
            "255.255.255.255", // broadcast
            "::1",             // v6 loopback
            "fe80::1",         // v6 link-local
            "fc00::1",         // v6 unique-local
        ] {
            assert!(reserved(ip), "{ip} should be reserved");
        }
        // Public addresses are not reserved.
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(!reserved(ip), "{ip} should not be reserved");
        }
    }

    /// End-to-end check against a real MaxMind-format database. Ignored by
    /// default; supply one and run:
    ///
    /// ```text
    /// GEOIP_TEST_DB=/path/to/dbip-city-lite.mmdb \
    ///   cargo test --lib geoip -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "requires a real .mmdb at GEOIP_TEST_DB"]
    fn resolves_known_ips() {
        let path = std::env::var("GEOIP_TEST_DB").expect("set GEOIP_TEST_DB to a .mmdb file path");
        let resolver = GeoIpResolver {
            reader: Reader::open_readfile(&path).expect("open test database"),
        };

        // A well-known public IP resolves to a non-empty label.
        let google = resolver.resolve("8.8.8.8".parse().unwrap());
        eprintln!("8.8.8.8 -> {google:?}");
        assert!(google.is_some(), "8.8.8.8 should resolve to a location");

        let cloudflare = resolver.resolve("1.1.1.1".parse().unwrap());
        eprintln!("1.1.1.1 -> {cloudflare:?}");
        assert!(cloudflare.is_some(), "1.1.1.1 should resolve to a location");

        // Private / loopback addresses never resolve.
        assert_eq!(resolver.resolve("192.168.1.1".parse().unwrap()), None);
        assert_eq!(resolver.resolve("127.0.0.1".parse().unwrap()), None);
    }
}
