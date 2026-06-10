//! API token scope enforcement.
//!
//! This module owns the route policy that maps an incoming request to
//! the scope it requires, plus (step 3) the middleware that enforces it
//! against a narrowed token's `ScopeSet`.
//!
//! Design (see docs/plans/api-token-scopes-plan.md):
//!   * `full` credentials (every cookie session and every un-narrowed
//!     token) short-circuit in the middleware and never reach this
//!     policy. Only deliberately-narrowed API tokens are constrained.
//!   * The policy returns a `ScopeRequirement`. Anything not explicitly
//!     mapped defaults to `Full`, which a narrowed token can never
//!     satisfy, so a new or cross-cutting route fail-closes rather than
//!     silently widening a narrowed token's reach.
//!   * Scope is a second, orthogonal layer: the handler's existing role
//!     gate still runs. A request must pass both.

use actix_web::http::Method;

use crate::utils::scopes::{Action, Domain};

/// What a request requires of a narrowed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRequirement {
    /// Any authenticated credential, including the most narrowly-scoped
    /// token. For identity / self routes that expose only the caller's
    /// own context (who am I), which is not a resource domain.
    Any,
    /// The token's `ScopeSet` must grant `(domain, action)`.
    Capability(Domain, Action),
    /// Only a `full` credential may call this route. The default for
    /// any path not explicitly mapped (sync data-plane, file serving,
    /// image proxy, and anything new), so unmapped routes fail-closed
    /// for narrowed tokens.
    Full,
}

/// Resolve the scope a request requires from its method and path.
///
/// `path` is the concrete request path (e.g. `/api/tickets/5`), matched
/// on the first segment after `/api/` with a few longest-prefix
/// overrides. Returns `Full` for anything unmapped.
pub fn required_scope(method: &Method, path: &str) -> ScopeRequirement {
    let rest = match path.strip_prefix("/api/") {
        Some(r) => r,
        // Not under /api (or already stripped): be safe.
        None => return ScopeRequirement::Full,
    };

    // --- longest-prefix overrides (checked before the segment map) ---

    // The audit log lives under /api/admin/audit* but is its own domain:
    // an `audit:read` SIEM token must reach it WITHOUT the broad `admin`
    // scope. Must come before the generic `admin -> Admin` rule below.
    if rest.starts_with("admin/audit") {
        return ScopeRequirement::Capability(Domain::Audit, Action::Read);
    }
    // Full search reindex is a platform-admin operation; keep narrowed
    // non-admin tokens out (the role gate also restricts it).
    if rest.starts_with("search/rebuild") {
        return ScopeRequirement::Capability(Domain::Admin, Action::Write);
    }

    let action = action_for(method, rest);
    let segment = rest.split(['/', '?']).next().unwrap_or("");

    // Identity / self routes: reading your own context needs only
    // authentication (any token), while editing your own profile is a
    // users-domain write so a read-only token can't do it.
    if segment == "me" {
        return match action {
            Action::Read => ScopeRequirement::Any,
            Action::Write => ScopeRequirement::Capability(Domain::Users, Action::Write),
        };
    }

    // (read_domain, write_domain). They differ only for "broadly-read,
    // admin-managed" metadata: everything that files a ticket reads
    // categories / workflow states, but only admins manage them, so the
    // write requires the `admin` scope. Splitting read from write keeps
    // the scope honest on its own rather than leaning on the role gate
    // to compensate (defence in depth = two real layers).
    let (read_domain, write_domain) = match segment {
        // Admin / config surface (also role-gated; the scope keeps a
        // data-scoped token like tickets:write from reaching it).
        "admin" | "rules" | "rule-applications" | "plugins" | "sla" | "integrations"
        | "msgraph" | "channels" | "webhooks" | "feature-flags" | "email" | "backup"
        | "scheduler" | "branding" | "import" => (Domain::Admin, Domain::Admin),

        // Read by ticket automations, managed only by admins.
        "categories" | "workflow-states" => (Domain::Tickets, Domain::Admin),

        // Ticket workspace (saved-views are per-user, so their writes
        // stay in Tickets; canned-response management lives under
        // /admin/ and is matched as Admin above).
        "tickets" | "comments" | "tags" | "saved-views" | "canned-responses" => {
            (Domain::Tickets, Domain::Tickets)
        }

        // Asset inventory (assets/{id}/audit is the ASSET audit trail,
        // handled here, not the security log).
        "assets" | "devices" => (Domain::Assets, Domain::Assets),

        "documentation" | "docs" | "knowledge-gaps" => (Domain::Docs, Domain::Docs),

        "projects" | "cycles" => (Domain::Projects, Domain::Projects),

        "users" | "groups" => (Domain::Users, Domain::Users),

        "notifications" => (Domain::Notifications, Domain::Notifications),

        "dashboard" | "analytics" | "search" => (Domain::Analytics, Domain::Analytics),

        "audit" => (Domain::Audit, Domain::Audit),

        // sync (whole-workspace data plane), files, image-proxy, events,
        // uploads, and anything unrecognised: require full (fail-closed).
        _ => return ScopeRequirement::Full,
    };

    let domain = match action {
        Action::Read => read_domain,
        Action::Write => write_domain,
    };
    ScopeRequirement::Capability(domain, action)
}

/// Read for GET/HEAD, write for mutations, with an override for the
/// handful of endpoints that mutate nothing despite using POST (batch
/// fetch, search query, sync snapshot) so a read-only token can use
/// them. `rest` is the path with the `/api/` prefix already stripped.
fn action_for(method: &Method, rest: &str) -> Action {
    if matches!(*method, Method::GET | Method::HEAD) {
        return Action::Read;
    }
    if *method == Method::POST {
        let first = rest.split('/').next().unwrap_or("");
        let last = rest
            .split(['/', '?'])
            .filter(|s| !s.is_empty())
            .next_back()
            .unwrap_or("");
        if last == "batch" || last == "search" || first == "search" || first == "sync" {
            return Action::Read;
        }
    }
    Action::Write
}

#[cfg(test)]
mod tests {
    use super::*;
    use ScopeRequirement::Capability;

    fn req(method: Method, path: &str) -> ScopeRequirement {
        required_scope(&method, path)
    }

    #[test]
    fn ticket_routes_map_to_tickets_with_method_action() {
        assert_eq!(
            req(Method::GET, "/api/tickets/5"),
            Capability(Domain::Tickets, Action::Read)
        );
        assert_eq!(
            req(Method::POST, "/api/tickets"),
            Capability(Domain::Tickets, Action::Write)
        );
        assert_eq!(
            req(Method::DELETE, "/api/tickets/5"),
            Capability(Domain::Tickets, Action::Write)
        );
        // ticket metadata read by ticket automations
        assert_eq!(
            req(Method::GET, "/api/categories"),
            Capability(Domain::Tickets, Action::Read)
        );
        assert_eq!(
            req(Method::GET, "/api/workflow-states"),
            Capability(Domain::Tickets, Action::Read)
        );
        assert_eq!(
            req(Method::POST, "/api/comments/9"),
            Capability(Domain::Tickets, Action::Write)
        );
    }

    #[test]
    fn domain_segment_mapping() {
        assert_eq!(
            req(Method::GET, "/api/assets/1"),
            Capability(Domain::Assets, Action::Read)
        );
        assert_eq!(
            req(Method::GET, "/api/devices"),
            Capability(Domain::Assets, Action::Read)
        );
        // asset audit trail is Assets, NOT the security audit log
        assert_eq!(
            req(Method::GET, "/api/assets/1/audit"),
            Capability(Domain::Assets, Action::Read)
        );
        assert_eq!(
            req(Method::PUT, "/api/documentation/3"),
            Capability(Domain::Docs, Action::Write)
        );
        assert_eq!(
            req(Method::GET, "/api/knowledge-gaps"),
            Capability(Domain::Docs, Action::Read)
        );
        assert_eq!(
            req(Method::POST, "/api/projects"),
            Capability(Domain::Projects, Action::Write)
        );
        assert_eq!(
            req(Method::GET, "/api/cycles/2"),
            Capability(Domain::Projects, Action::Read)
        );
        assert_eq!(
            req(Method::PATCH, "/api/users/abc"),
            Capability(Domain::Users, Action::Write)
        );
        assert_eq!(
            req(Method::GET, "/api/groups"),
            Capability(Domain::Users, Action::Read)
        );
        assert_eq!(
            req(Method::GET, "/api/notifications"),
            Capability(Domain::Notifications, Action::Read)
        );
        assert_eq!(
            req(Method::GET, "/api/dashboard/kpi"),
            Capability(Domain::Analytics, Action::Read)
        );
    }

    #[test]
    fn me_is_any_for_reads_and_users_write_for_self_edit() {
        // Identity reads need only authentication, so even a narrowly
        // scoped token can ask who it is.
        assert_eq!(req(Method::GET, "/api/me"), ScopeRequirement::Any);
        assert_eq!(
            req(Method::GET, "/api/me/workspaces"),
            ScopeRequirement::Any
        );
        // Editing your own profile is a users-domain write.
        assert_eq!(
            req(Method::PATCH, "/api/me"),
            Capability(Domain::Users, Action::Write)
        );
    }

    #[test]
    fn admin_managed_metadata_reads_as_tickets_writes_as_admin() {
        // Read by everything that files a ticket...
        assert_eq!(
            req(Method::GET, "/api/categories"),
            Capability(Domain::Tickets, Action::Read)
        );
        assert_eq!(
            req(Method::GET, "/api/workflow-states"),
            Capability(Domain::Tickets, Action::Read)
        );
        // ...but managed only by admins, so a tickets:write token can't
        // touch them; the write needs the admin scope.
        assert_eq!(
            req(Method::POST, "/api/categories"),
            Capability(Domain::Admin, Action::Write)
        );
        assert_eq!(
            req(Method::PUT, "/api/workflow-states/3"),
            Capability(Domain::Admin, Action::Write)
        );
    }

    #[test]
    fn admin_surface_maps_to_admin() {
        for p in [
            "/api/admin/email/config",
            "/api/admin/backup/export",
            "/api/admin/channels",
            "/api/webhooks",
            "/api/sla/workspace-summary",
            "/api/plugins",
            "/api/rules",
            "/api/integrations/graph",
            "/api/feature-flags/x",
        ] {
            assert_eq!(
                required_scope(&Method::GET, p),
                Capability(Domain::Admin, Action::Read),
                "{p} should be Admin read"
            );
        }
        assert_eq!(
            req(Method::POST, "/api/admin/channels"),
            Capability(Domain::Admin, Action::Write)
        );
    }

    #[test]
    fn audit_log_is_its_own_domain_even_under_admin() {
        // The security audit log: an audit:read SIEM token must reach it
        // without the broad admin scope.
        assert_eq!(
            req(Method::GET, "/api/admin/audit-log"),
            Capability(Domain::Audit, Action::Read)
        );
        assert_eq!(
            req(Method::GET, "/api/admin/audit/export"),
            Capability(Domain::Audit, Action::Read)
        );
    }

    #[test]
    fn search_query_is_analytics_read_but_rebuild_is_admin_write() {
        assert_eq!(
            req(Method::GET, "/api/search?q=x"),
            Capability(Domain::Analytics, Action::Read)
        );
        // POST search mutates nothing
        assert_eq!(
            req(Method::POST, "/api/search"),
            Capability(Domain::Analytics, Action::Read)
        );
        assert_eq!(
            req(Method::POST, "/api/search/rebuild"),
            Capability(Domain::Admin, Action::Write)
        );
    }

    #[test]
    fn read_via_post_overrides() {
        // batch fetch and search mutate nothing
        assert_eq!(
            req(Method::POST, "/api/users/batch"),
            Capability(Domain::Users, Action::Read)
        );
    }

    #[test]
    fn unmapped_and_cross_cutting_routes_require_full() {
        for p in [
            "/api/sync/bootstrap",
            "/api/sync/delta",
            "/api/files/tickets/5/x.png",
            "/api/image-proxy/sig/enc",
            "/api/events/stream",
            "/api/widgets-not-a-real-domain",
        ] {
            assert_eq!(
                required_scope(&Method::POST, p),
                ScopeRequirement::Full,
                "{p} should require full"
            );
        }
    }

    #[test]
    fn non_api_path_requires_full() {
        assert_eq!(req(Method::GET, "/health"), ScopeRequirement::Full);
    }
}
