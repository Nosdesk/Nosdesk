//! Typed primitives for the plugin system.
//!
//! "Parse, don't validate." Each newtype here has a single
//! constructor that enforces the invariant on the way in. Past
//! that boundary the rest of the codebase (validator, proxy,
//! install pipeline) consumes typed values, so the parse logic
//! exists in exactly one place. This is the architectural fix
//! for the permission/host/URL bypass classes the review found.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

// =============================================================================
// Host
// =============================================================================

/// A DNS hostname that has passed strict syntactic validation.
/// Lowercase, LDH labels, no port, no userinfo, no wildcards, no
/// IP literal, no path. Construct via [`Host::parse`].
///
/// `Ord` is implemented (via the inner lowercase string) so
/// `BTreeMap<Host, _>` produces deterministic iteration order,
/// which matters when re-serialising the manifest for canonical
/// digest computation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Host(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    Empty,
    /// Contains `:` (port), `@` (userinfo), `/` (path), or whitespace.
    InvalidCharacter,
    /// Bare `.` boundary or empty label between dots.
    InvalidLabelStructure,
    /// Label is longer than 63 octets, or whole hostname over 253.
    TooLong,
    /// Non-LDH character in a label.
    InvalidLabelCharacter,
    /// Label starts or ends with `-`.
    HyphenBoundary,
    /// Looks like an IPv4 / IPv6 literal (we want named hosts only).
    IpLiteral,
    /// Has a `*` (use HostPattern for wildcard hosts).
    Wildcard,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "host is empty"),
            Self::InvalidCharacter => {
                write!(
                    f,
                    "host contains an invalid character (port, userinfo, path, or whitespace)"
                )
            }
            Self::InvalidLabelStructure => {
                write!(f, "host has an empty label or leading/trailing dot")
            }
            Self::TooLong => write!(f, "host or label exceeds the DNS length limit"),
            Self::InvalidLabelCharacter => {
                write!(
                    f,
                    "host label contains a non-LDH character (only a-z, 0-9, hyphen allowed)"
                )
            }
            Self::HyphenBoundary => write!(f, "host label starts or ends with a hyphen"),
            Self::IpLiteral => write!(f, "host looks like an IP literal; named hosts only"),
            Self::Wildcard => write!(f, "host contains a wildcard; use HostPattern for those"),
        }
    }
}

impl std::error::Error for HostError {}

impl Host {
    /// Parse and normalise. Lowercases, then enforces:
    /// - non-empty
    /// - no `:`, `@`, `/`, whitespace, `*`
    /// - dot-separated LDH labels per RFC 1035
    /// - each label 1-63 chars, no leading/trailing hyphen
    /// - whole host <= 253 chars
    /// - not an IP literal (named hosts only)
    pub fn parse(s: &str) -> Result<Self, HostError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(HostError::Empty);
        }
        if s.contains('*') {
            return Err(HostError::Wildcard);
        }
        if s.contains(':')
            || s.contains('@')
            || s.contains('/')
            || s.chars().any(char::is_whitespace)
        {
            return Err(HostError::InvalidCharacter);
        }
        if s.len() > 253 {
            return Err(HostError::TooLong);
        }
        if s.starts_with('.') || s.ends_with('.') {
            return Err(HostError::InvalidLabelStructure);
        }

        let lc = s.to_ascii_lowercase();

        // Reject pure IPv4 dotted-quad. IPv6 needs `:` which we already caught.
        if lc.split('.').all(|part| part.parse::<u8>().is_ok()) && lc.contains('.') {
            return Err(HostError::IpLiteral);
        }

        for label in lc.split('.') {
            if label.is_empty() {
                return Err(HostError::InvalidLabelStructure);
            }
            if label.len() > 63 {
                return Err(HostError::TooLong);
            }
            if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err(HostError::InvalidLabelCharacter);
            }
            if label.starts_with('-') || label.ends_with('-') {
                return Err(HostError::HyphenBoundary);
            }
        }

        Ok(Host(lc))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Host {
    type Err = HostError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for Host {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Host {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Host::parse(&s).map_err(serde::de::Error::custom)
    }
}

// =============================================================================
// HostPattern
// =============================================================================

/// A host pattern used in `network:<pattern>` permissions. Either
/// an exact [`Host`] or a single-level wildcard (`*.example.com`)
/// that matches any direct subdomain plus the apex itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostPattern {
    Exact(Host),
    /// `*.example.com`: matches `example.com`, `api.example.com`,
    /// but NOT `deep.api.example.com`. Single-level by design.
    Wildcard(Host),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPatternError {
    Empty,
    /// Wildcard not in `*.<host>` form.
    BadWildcardShape,
    Host(HostError),
}

impl fmt::Display for HostPatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "host pattern is empty"),
            Self::BadWildcardShape => {
                write!(f, "wildcard host pattern must be of the form `*.<host>` with a single leading wildcard label")
            }
            Self::Host(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HostPatternError {}

impl HostPattern {
    pub fn parse(s: &str) -> Result<Self, HostPatternError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(HostPatternError::Empty);
        }
        if let Some(rest) = s.strip_prefix("*.") {
            if rest.is_empty() || rest.contains('*') {
                return Err(HostPatternError::BadWildcardShape);
            }
            let host = Host::parse(rest).map_err(HostPatternError::Host)?;
            Ok(HostPattern::Wildcard(host))
        } else if s.contains('*') {
            Err(HostPatternError::BadWildcardShape)
        } else {
            let host = Host::parse(s).map_err(HostPatternError::Host)?;
            Ok(HostPattern::Exact(host))
        }
    }

    /// Does this pattern match the given host? Single-level
    /// wildcard semantics: `*.example.com` matches `example.com`
    /// and `<one_label>.example.com`, but not deeper subdomains.
    /// (Tightening this rule later is backwards-compatible; loosening
    /// would not be.)
    pub fn matches(&self, host: &Host) -> bool {
        match self {
            HostPattern::Exact(h) => h == host,
            HostPattern::Wildcard(apex) => {
                if host == apex {
                    return true;
                }
                let host_str = host.as_str();
                let apex_str = apex.as_str();
                if !host_str.ends_with(apex_str) {
                    return false;
                }
                // Must have exactly one extra label.
                let prefix = &host_str[..host_str.len() - apex_str.len()];
                prefix.ends_with('.')
                    && prefix.len() > 1
                    && !prefix[..prefix.len() - 1].contains('.')
            }
        }
    }

    /// Does this pattern fully cover the other? Used for the
    /// "every auth host is allowed by some network: permission"
    /// cross-check: an exact `auth.api.github.com` is covered by
    /// either `network:api.github.com` (exact) or
    /// `network:*.github.com` (wildcard).
    pub fn covers(&self, other: &HostPattern) -> bool {
        match (self, other) {
            (HostPattern::Exact(a), HostPattern::Exact(b)) => a == b,
            (HostPattern::Wildcard(a), HostPattern::Exact(b)) => {
                HostPattern::Wildcard(a.clone()).matches(b)
            }
            // Wildcard in `auth` is forbidden at parse time, so the
            // remaining cases are only reached if a future schema
            // change permits it. Conservative: only equal patterns
            // cover each other.
            (HostPattern::Wildcard(a), HostPattern::Wildcard(b)) => a == b,
            (HostPattern::Exact(_), HostPattern::Wildcard(_)) => false,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            HostPattern::Exact(h) => h.as_str().to_string(),
            HostPattern::Wildcard(h) => format!("*.{}", h.as_str()),
        }
    }
}

impl fmt::Display for HostPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_string())
    }
}

// =============================================================================
// WebUrl
// =============================================================================

/// An https URL. Anywhere a URL is rendered as a link in
/// untrusted UI, this is the type that should be passed in.
/// Construct via [`WebUrl::parse`].
#[derive(Debug, Clone)]
pub struct WebUrl(url::Url);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebUrlError {
    /// Not a parseable URL at all.
    Invalid,
    /// Scheme is not https.
    NotHttps,
    /// URL has no host (e.g. `https:///path`).
    NoHost,
}

impl fmt::Display for WebUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => write!(f, "URL is not parseable"),
            Self::NotHttps => write!(f, "URL must use the https scheme"),
            Self::NoHost => write!(f, "URL must have a host"),
        }
    }
}

impl std::error::Error for WebUrlError {}

impl WebUrl {
    pub fn parse(s: &str) -> Result<Self, WebUrlError> {
        let u = url::Url::parse(s.trim()).map_err(|_| WebUrlError::Invalid)?;
        if u.scheme() != "https" {
            return Err(WebUrlError::NotHttps);
        }
        if u.host().is_none() {
            return Err(WebUrlError::NoHost);
        }
        Ok(WebUrl(u))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_inner(self) -> url::Url {
        self.0
    }
}

impl fmt::Display for WebUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl Serialize for WebUrl {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for WebUrl {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        WebUrl::parse(&s).map_err(serde::de::Error::custom)
    }
}

// =============================================================================
// Permission
// =============================================================================

/// A capability granted by a plugin manifest. Parsed from the
/// wire form once at manifest load time; everything past the
/// validator consumes this enum, never raw strings.
///
/// The serde impls preserve the wire format on the way out, so
/// JSON manifests round-trip identically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Permission {
    TicketRead,
    TicketWrite,
    TicketComment,
    TicketDelete,
    AssetRead,
    AssetWrite,
    UserRead,
    StoragePlugin,
    CollectionRead,
    CollectionWrite,
    Network(HostPattern),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionError {
    /// Bare resource form (e.g. just `tickets:read`) that doesn't
    /// match any known capability.
    Unknown(String),
    /// `network:<pattern>` was malformed.
    NetworkPattern(HostPatternError),
    Empty,
}

impl fmt::Display for PermissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(s) => write!(f, "unknown permission {s:?}"),
            Self::NetworkPattern(e) => write!(f, "invalid network permission: {e}"),
            Self::Empty => write!(f, "permission string is empty"),
        }
    }
}

impl std::error::Error for PermissionError {}

impl Permission {
    pub fn parse(s: &str) -> Result<Self, PermissionError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(PermissionError::Empty);
        }
        if let Some(host_str) = s.strip_prefix("network:") {
            let pattern = HostPattern::parse(host_str).map_err(PermissionError::NetworkPattern)?;
            return Ok(Permission::Network(pattern));
        }
        match s {
            "ticket:read" => Ok(Permission::TicketRead),
            "ticket:write" => Ok(Permission::TicketWrite),
            "ticket:comment" => Ok(Permission::TicketComment),
            "ticket:delete" => Ok(Permission::TicketDelete),
            "asset:read" => Ok(Permission::AssetRead),
            "asset:write" => Ok(Permission::AssetWrite),
            "user:read" => Ok(Permission::UserRead),
            "storage:plugin" => Ok(Permission::StoragePlugin),
            "collection:read" => Ok(Permission::CollectionRead),
            "collection:write" => Ok(Permission::CollectionWrite),
            other => Err(PermissionError::Unknown(other.to_string())),
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Permission::TicketRead => "ticket:read".to_string(),
            Permission::TicketWrite => "ticket:write".to_string(),
            Permission::TicketComment => "ticket:comment".to_string(),
            Permission::TicketDelete => "ticket:delete".to_string(),
            Permission::AssetRead => "asset:read".to_string(),
            Permission::AssetWrite => "asset:write".to_string(),
            Permission::UserRead => "user:read".to_string(),
            Permission::StoragePlugin => "storage:plugin".to_string(),
            Permission::CollectionRead => "collection:read".to_string(),
            Permission::CollectionWrite => "collection:write".to_string(),
            Permission::Network(p) => format!("network:{}", p.as_string()),
        }
    }

    pub fn network_pattern(&self) -> Option<&HostPattern> {
        match self {
            Permission::Network(p) => Some(p),
            _ => None,
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_string())
    }
}

impl Serialize for Permission {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.as_string())
    }
}

impl<'de> Deserialize<'de> for Permission {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Permission::parse(&s).map_err(serde::de::Error::custom)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Host -----

    #[test]
    fn host_accepts_simple_dns() {
        assert!(Host::parse("api.github.com").is_ok());
        assert!(Host::parse("example.com").is_ok());
        assert!(Host::parse("a.b.c.d.e").is_ok());
        assert!(Host::parse("foo-bar.example.com").is_ok());
    }

    #[test]
    fn host_lowercases() {
        let h = Host::parse("API.GitHub.COM").unwrap();
        assert_eq!(h.as_str(), "api.github.com");
    }

    #[test]
    fn host_rejects_port() {
        assert_eq!(
            Host::parse("foo.com:8080"),
            Err(HostError::InvalidCharacter)
        );
    }

    #[test]
    fn host_rejects_userinfo() {
        assert_eq!(
            Host::parse("user@foo.com"),
            Err(HostError::InvalidCharacter)
        );
        assert_eq!(
            Host::parse("user:pass@foo.com"),
            Err(HostError::InvalidCharacter)
        );
    }

    #[test]
    fn host_rejects_path() {
        assert_eq!(Host::parse("foo.com/bar"), Err(HostError::InvalidCharacter));
        assert_eq!(Host::parse("/foo.com"), Err(HostError::InvalidCharacter));
    }

    #[test]
    fn host_rejects_whitespace() {
        assert_eq!(Host::parse("foo .com"), Err(HostError::InvalidCharacter));
        assert_eq!(Host::parse("foo\tcom"), Err(HostError::InvalidCharacter));
    }

    #[test]
    fn host_rejects_wildcard() {
        assert_eq!(Host::parse("*.foo.com"), Err(HostError::Wildcard));
        assert_eq!(Host::parse("foo.*"), Err(HostError::Wildcard));
    }

    #[test]
    fn host_rejects_ipv4_literal() {
        assert_eq!(Host::parse("127.0.0.1"), Err(HostError::IpLiteral));
        assert_eq!(Host::parse("8.8.8.8"), Err(HostError::IpLiteral));
    }

    #[test]
    fn host_rejects_empty_label() {
        assert_eq!(
            Host::parse("foo..com"),
            Err(HostError::InvalidLabelStructure)
        );
        assert_eq!(
            Host::parse(".foo.com"),
            Err(HostError::InvalidLabelStructure)
        );
        assert_eq!(
            Host::parse("foo.com."),
            Err(HostError::InvalidLabelStructure)
        );
    }

    #[test]
    fn host_rejects_hyphen_boundary() {
        assert_eq!(Host::parse("-foo.com"), Err(HostError::HyphenBoundary));
        assert_eq!(Host::parse("foo-.com"), Err(HostError::HyphenBoundary));
    }

    #[test]
    fn host_rejects_non_ldh() {
        assert_eq!(
            Host::parse("foo_bar.com"),
            Err(HostError::InvalidLabelCharacter)
        );
        assert_eq!(
            Host::parse("café.com"),
            Err(HostError::InvalidLabelCharacter)
        );
    }

    #[test]
    fn host_rejects_label_too_long() {
        let long_label = "a".repeat(64);
        let host = format!("{long_label}.com");
        assert_eq!(Host::parse(&host), Err(HostError::TooLong));
    }

    #[test]
    fn host_rejects_empty() {
        assert_eq!(Host::parse(""), Err(HostError::Empty));
        assert_eq!(Host::parse("   "), Err(HostError::Empty));
    }

    // ----- HostPattern -----

    #[test]
    fn host_pattern_exact_matches_only_self() {
        let p = HostPattern::parse("api.github.com").unwrap();
        let h = Host::parse("api.github.com").unwrap();
        assert!(p.matches(&h));
        let other = Host::parse("api.gitlab.com").unwrap();
        assert!(!p.matches(&other));
    }

    #[test]
    fn host_pattern_wildcard_matches_apex_and_one_label() {
        let p = HostPattern::parse("*.github.com").unwrap();
        assert!(p.matches(&Host::parse("github.com").unwrap()));
        assert!(p.matches(&Host::parse("api.github.com").unwrap()));
        assert!(p.matches(&Host::parse("gist.github.com").unwrap()));
    }

    #[test]
    fn host_pattern_wildcard_does_not_match_deeper() {
        let p = HostPattern::parse("*.github.com").unwrap();
        assert!(!p.matches(&Host::parse("v1.api.github.com").unwrap()));
    }

    #[test]
    fn host_pattern_wildcard_does_not_match_unrelated() {
        let p = HostPattern::parse("*.github.com").unwrap();
        assert!(!p.matches(&Host::parse("attacker.test").unwrap()));
        assert!(!p.matches(&Host::parse("notgithub.com").unwrap()));
        // Suffix match attack: "fakegithub.com" ends with "github.com"
        // but it's not a subdomain.
        assert!(!p.matches(&Host::parse("fakegithub.com").unwrap()));
    }

    #[test]
    fn host_pattern_rejects_bare_wildcard() {
        assert_eq!(
            HostPattern::parse("*"),
            Err(HostPatternError::BadWildcardShape)
        );
        assert_eq!(
            HostPattern::parse("*."),
            Err(HostPatternError::BadWildcardShape)
        );
    }

    #[test]
    fn host_pattern_rejects_inner_wildcard() {
        assert_eq!(
            HostPattern::parse("foo.*.com"),
            Err(HostPatternError::BadWildcardShape)
        );
        assert_eq!(
            HostPattern::parse("*foo.com"),
            Err(HostPatternError::BadWildcardShape)
        );
    }

    #[test]
    fn host_pattern_rejects_double_wildcard() {
        assert_eq!(
            HostPattern::parse("*.*.com"),
            Err(HostPatternError::BadWildcardShape)
        );
    }

    #[test]
    fn host_pattern_covers_exact() {
        let exact = HostPattern::parse("api.github.com").unwrap();
        let same = HostPattern::parse("api.github.com").unwrap();
        let other = HostPattern::parse("api.gitlab.com").unwrap();
        let wildcard = HostPattern::parse("*.github.com").unwrap();
        assert!(exact.covers(&same));
        assert!(!exact.covers(&other));
        assert!(wildcard.covers(&same));
        assert!(!exact.covers(&wildcard));
    }

    // ----- WebUrl -----

    #[test]
    fn web_url_accepts_https() {
        assert!(WebUrl::parse("https://example.com").is_ok());
        assert!(WebUrl::parse("https://example.com/path?q=1").is_ok());
    }

    #[test]
    fn web_url_rejects_http() {
        assert!(matches!(
            WebUrl::parse("http://example.com"),
            Err(WebUrlError::NotHttps)
        ));
    }

    #[test]
    fn web_url_rejects_javascript_scheme() {
        assert!(matches!(
            WebUrl::parse("javascript:alert(1)"),
            Err(WebUrlError::NotHttps)
        ));
    }

    #[test]
    fn web_url_rejects_file_scheme() {
        assert!(matches!(
            WebUrl::parse("file:///etc/passwd"),
            Err(WebUrlError::NotHttps)
        ));
    }

    #[test]
    fn web_url_rejects_data_scheme() {
        assert!(matches!(
            WebUrl::parse("data:text/html,<script>"),
            Err(WebUrlError::NotHttps)
        ));
    }

    #[test]
    fn web_url_rejects_garbage() {
        assert!(matches!(
            WebUrl::parse("not a url"),
            Err(WebUrlError::Invalid)
        ));
        assert!(matches!(WebUrl::parse(""), Err(WebUrlError::Invalid)));
    }

    // ----- Permission -----

    #[test]
    fn permission_parses_known_capabilities() {
        assert_eq!(
            Permission::parse("ticket:read").unwrap(),
            Permission::TicketRead
        );
        assert_eq!(
            Permission::parse("storage:plugin").unwrap(),
            Permission::StoragePlugin
        );
        assert_eq!(
            Permission::parse("collection:write").unwrap(),
            Permission::CollectionWrite
        );
    }

    #[test]
    fn permission_parses_network_exact() {
        let p = Permission::parse("network:api.github.com").unwrap();
        match p {
            Permission::Network(HostPattern::Exact(h)) => assert_eq!(h.as_str(), "api.github.com"),
            _ => panic!("expected exact network pattern"),
        }
    }

    #[test]
    fn permission_parses_network_wildcard() {
        let p = Permission::parse("network:*.github.com").unwrap();
        match p {
            Permission::Network(HostPattern::Wildcard(h)) => assert_eq!(h.as_str(), "github.com"),
            _ => panic!("expected wildcard network pattern"),
        }
    }

    #[test]
    fn permission_rejects_unknown() {
        assert!(matches!(
            Permission::parse("tickets:read"),
            Err(PermissionError::Unknown(_))
        ));
        assert!(matches!(
            Permission::parse("storage"),
            Err(PermissionError::Unknown(_))
        ));
    }

    #[test]
    fn permission_rejects_malformed_network() {
        assert!(matches!(
            Permission::parse("network:127.0.0.1"),
            Err(PermissionError::NetworkPattern(_))
        ));
        assert!(matches!(
            Permission::parse("network:foo:8080"),
            Err(PermissionError::NetworkPattern(_))
        ));
        assert!(matches!(
            Permission::parse("network:user@foo.com"),
            Err(PermissionError::NetworkPattern(_))
        ));
        assert!(matches!(
            Permission::parse("network:*"),
            Err(PermissionError::NetworkPattern(_))
        ));
    }

    #[test]
    fn permission_round_trips_through_serde() {
        let inputs = [
            "ticket:read",
            "storage:plugin",
            "network:api.github.com",
            "network:*.github.com",
        ];
        for s in inputs {
            let p = Permission::parse(s).unwrap();
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{s}\""));
            let back: Permission = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }
}
