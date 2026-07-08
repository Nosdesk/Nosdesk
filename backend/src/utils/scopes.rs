//! API token scope model.
//!
//! Scopes narrow an API token within its owner's role: enforcement is
//! always role AND scope, never scope alone (a scope cannot widen what
//! the owner's role permits). `full` is the superscope every cookie
//! session and every un-narrowed token carries.
//!
//! This module is the single matcher for both the audit handler gate
//! (`rbac::require_audit_read`) and the token-scope middleware, so the
//! two can't drift.

use std::collections::HashSet;

/// Coarse, use-case-driven resource domain a scope or route belongs to
/// (one bucket per automation use case, not one per table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    Tickets,
    Assets,
    Docs,
    Projects,
    Users,
    Analytics,
    Notifications,
    Audit,
    Admin,
}

impl Domain {
    /// Parse the domain half of a `domain:action` scope string (or the
    /// bare `admin` scope). Returns None for unknown domains.
    fn from_token(s: &str) -> Option<Self> {
        Some(match s {
            "tickets" => Self::Tickets,
            "assets" => Self::Assets,
            "docs" => Self::Docs,
            "projects" => Self::Projects,
            "users" => Self::Users,
            "analytics" => Self::Analytics,
            "notifications" => Self::Notifications,
            "audit" => Self::Audit,
            "admin" => Self::Admin,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Read,
    Write,
}

/// The full set of scope strings a token may be minted with. Mint-time
/// validation (`is_valid_token_scope`) rejects anything outside this
/// list so a typo can't create a token no route will ever honour.
pub const VALID_TOKEN_SCOPES: &[&str] = &[
    "full",
    "*:read",
    "tickets:read",
    "tickets:write",
    "assets:read",
    "assets:write",
    "docs:read",
    "docs:write",
    "projects:read",
    "projects:write",
    "users:read",
    "users:write",
    "analytics:read",
    "notifications:read",
    "notifications:write",
    "audit:read",
    "admin",
];

/// True if `scope` is a recognised, mintable scope string.
pub fn is_valid_token_scope(scope: &str) -> bool {
    VALID_TOKEN_SCOPES.contains(&scope)
}

/// A parsed capability set, built from the space-separated `scope`
/// string a credential carries. The canonical runtime matcher for
/// "may this credential do `action` on `domain`?".
#[derive(Debug, Default)]
pub struct ScopeSet {
    /// `full`: grants every (domain, action), including audit.
    full: bool,
    /// `*:read`: grants read on every domain EXCEPT audit. The audit
    /// log is the security trail, so reading it requires the explicit
    /// `audit:read` scope (matching the SIEM-service-account model);
    /// a read-everything dashboard token must not pick it up silently.
    star_read: bool,
    caps: HashSet<(Domain, Action)>,
}

impl ScopeSet {
    /// Parse a space-separated scope string (OAuth/RFC 6749 convention).
    /// Unknown tokens are ignored: mint-time `is_valid_token_scope` is
    /// the allowlist, while this runtime matcher stays lenient so a
    /// legacy or future scope value degrades to "grants nothing extra"
    /// rather than erroring mid-request.
    pub fn parse(scope: &str) -> Self {
        let mut set = ScopeSet::default();
        for tok in scope.split_whitespace() {
            match tok {
                "full" => set.full = true,
                "*:read" => set.star_read = true,
                "admin" => {
                    set.caps.insert((Domain::Admin, Action::Read));
                    set.caps.insert((Domain::Admin, Action::Write));
                }
                other => {
                    if let Some((domain, action)) = parse_domain_action(other) {
                        set.caps.insert((domain, action));
                    }
                }
            }
        }
        set
    }

    /// Whether this set grants `action` on `domain`. `write` implies
    /// `read` for the same domain.
    pub fn grants(&self, domain: Domain, action: Action) -> bool {
        if self.full {
            return true;
        }
        if self.star_read && action == Action::Read && domain != Domain::Audit {
            return true;
        }
        if self.caps.contains(&(domain, action)) {
            return true;
        }
        // write implies read for the same domain
        action == Action::Read && self.caps.contains(&(domain, Action::Write))
    }
}

/// Parse a `domain:action` scope token. `full`, `*:read`, and the bare
/// `admin` scope are handled by the caller; this returns None for them
/// and for any malformed or unknown value.
fn parse_domain_action(tok: &str) -> Option<(Domain, Action)> {
    let (domain, action) = tok.split_once(':')?;
    let domain = Domain::from_token(domain)?;
    let action = match action {
        "read" => Action::Read,
        "write" => Action::Write,
        _ => return None,
    };
    Some((domain, action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_token_scopes_allowlist() {
        for s in ["full", "*:read", "tickets:write", "audit:read", "admin"] {
            assert!(is_valid_token_scope(s), "{s} should be valid");
        }
        for s in [
            "audit:write",
            "tickets:delete",
            "admin:write",
            "everything",
            "",
        ] {
            assert!(!is_valid_token_scope(s), "{s} should be invalid");
        }
    }

    #[test]
    fn full_grants_everything_including_audit() {
        let s = ScopeSet::parse("full");
        assert!(s.grants(Domain::Tickets, Action::Write));
        assert!(s.grants(Domain::Admin, Action::Write));
        assert!(s.grants(Domain::Audit, Action::Read));
    }

    #[test]
    fn star_read_is_read_everything_except_audit() {
        let s = ScopeSet::parse("*:read");
        assert!(s.grants(Domain::Tickets, Action::Read));
        assert!(s.grants(Domain::Assets, Action::Read));
        assert!(s.grants(Domain::Admin, Action::Read));
        // not write
        assert!(!s.grants(Domain::Tickets, Action::Write));
        // and not the audit log
        assert!(!s.grants(Domain::Audit, Action::Read));
    }

    #[test]
    fn write_implies_read_same_domain_only() {
        let s = ScopeSet::parse("tickets:write");
        assert!(s.grants(Domain::Tickets, Action::Write));
        assert!(s.grants(Domain::Tickets, Action::Read));
        assert!(!s.grants(Domain::Assets, Action::Read));
        assert!(!s.grants(Domain::Assets, Action::Write));
    }

    #[test]
    fn read_does_not_imply_write() {
        let s = ScopeSet::parse("tickets:read");
        assert!(s.grants(Domain::Tickets, Action::Read));
        assert!(!s.grants(Domain::Tickets, Action::Write));
    }

    #[test]
    fn admin_grants_admin_read_and_write_only() {
        let s = ScopeSet::parse("admin");
        assert!(s.grants(Domain::Admin, Action::Read));
        assert!(s.grants(Domain::Admin, Action::Write));
        assert!(!s.grants(Domain::Tickets, Action::Write));
        // admin scope is not audit scope
        assert!(!s.grants(Domain::Audit, Action::Read));
    }

    #[test]
    fn audit_read_grants_only_audit_read() {
        let s = ScopeSet::parse("audit:read");
        assert!(s.grants(Domain::Audit, Action::Read));
        assert!(!s.grants(Domain::Tickets, Action::Read));
        assert!(!s.grants(Domain::Admin, Action::Read));
    }

    #[test]
    fn multiple_scopes_union() {
        let s = ScopeSet::parse("tickets:write assets:read");
        assert!(s.grants(Domain::Tickets, Action::Write));
        assert!(s.grants(Domain::Assets, Action::Read));
        assert!(!s.grants(Domain::Assets, Action::Write));
        assert!(!s.grants(Domain::Docs, Action::Read));
    }

    #[test]
    fn empty_and_unknown_grant_nothing() {
        assert!(!ScopeSet::parse("").grants(Domain::Tickets, Action::Read));
        let s = ScopeSet::parse("bogus tickets:sideways :read read:");
        assert!(!s.grants(Domain::Tickets, Action::Read));
    }
}
