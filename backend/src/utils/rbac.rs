//! Role-Based Access Control (RBAC) utilities
//!
//! This module provides centralised role checking functions and response helpers
//! for implementing consistent authorization across all API handlers.

use actix_web::{HttpMessage, HttpRequest, HttpResponse};
use serde_json::json;

use crate::models::{Claims, PlatformRole};
use crate::utils::scopes::{Action, Domain, ScopeSet};

// The scope taxonomy and the `ScopeSet` matcher live in
// `utils::scopes`. Re-exported here so existing `rbac::` call sites
// (e.g. mint-time validation in handlers/api_tokens.rs) keep resolving.
pub use crate::utils::scopes::{is_valid_token_scope, VALID_TOKEN_SCOPES};

/// Whether the principal's *platform role* may read the audit
/// surface: `platform_admin` (full operator) or `audit_reviewer` (the
/// read-only audit role). Plain users may not.
pub fn role_can_read_audit(claims: &Claims) -> bool {
    PlatformRole::from_db(&claims.platform_role).can_read_audit()
}

/// Gate for the unified audit endpoints. Authorisation is the
/// intersection of role and scope: the role must permit audit reads
/// (admin / audit_reviewer) AND the credential must carry the
/// `audit:read` scope (or the `full` superscope a session always has).
/// This lets a SIEM pull use an `audit:read`-only token bound to an
/// AuditReviewer service account without holding broader privileges,
/// while a `full` admin session works unchanged.
pub fn require_audit_read(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    let claims = require_auth(req)?;

    let scope_ok = ScopeSet::parse(&claims.scope).grants(Domain::Audit, Action::Read);
    if !(role_can_read_audit(&claims) && scope_ok) {
        return Err(HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "This action requires the audit:read scope and an admin or audit-reviewer role"
        })));
    }

    Ok(claims)
}

/// Extract claims from request and check if user is authenticated
/// Returns Ok(Claims) if authenticated, Err(HttpResponse) with 401 if not
pub fn require_auth(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    req.extensions().get::<Claims>().cloned().ok_or_else(|| {
        HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        }))
    })
}

// =====================================================================
// Phase 4 W2: split-role gating
// =====================================================================

/// True when the principal holds the platform-admin role
/// ([`Claims::platform_role`] == `platform_admin`). The home for the
/// privilege previously expressed as the global `users.role = admin`.
pub fn is_platform_admin(claims: &Claims) -> bool {
    claims.platform_role == "platform_admin"
}

/// Gate for endpoints that require platform-admin privilege:
/// workspace lifecycle (W1 admin handlers), hosted billing,
/// cross-tenant operator tools, and instance-wide settings. For
/// workspace-scoped admin/agent gating use
/// [`require_workspace_role`] instead.
pub fn require_platform_admin(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    let claims = require_auth(req)?;

    if !is_platform_admin(&claims) {
        return Err(HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "This action requires platform-admin privileges"
        })));
    }

    Ok(claims)
}

/// Gate for workspace-scoped endpoints. Looks up the caller's
/// `workspace_members.role` for the resolved [`WorkspaceContext`]
/// and compares to `min` per the role ordering (Owner > Admin >
/// Agent > Member).
///
/// Returns the membership row on success so the handler can read
/// the actual role without a second query. Returns 401 if there
/// are no claims, 403 if the user has no membership in the
/// resolved workspace or their role doesn't meet `min`, 500 on a
/// DB failure.
///
/// **Note:** this looks up the membership row via the request's
/// extracted [`WorkspaceContext`] + [`Pool`]; handlers must wrap
/// the relevant routes in a workspace-resolving middleware before
/// this can succeed. For W2 the resolver is the existing
/// `workspace_context` middleware (added in 3a); W3 will tighten
/// the contract by requiring the resolver to also stamp the
/// membership row up front to save the duplicate query.
pub fn require_workspace_role(
    req: &HttpRequest,
    min: crate::models::WorkspaceRole,
) -> Result<Claims, HttpResponse> {
    use actix_web::web;
    use diesel::result::Error as DieselError;
    use uuid::Uuid;

    let claims = require_auth(req)?;
    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => {
            return Err(HttpResponse::Unauthorized().json(json!({
                "error": "Unauthorized",
                "message": "Token subject is not a valid user identifier"
            })));
        }
    };

    let workspace_id = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .map(|wc| wc.workspace_id);
    let workspace_id = match workspace_id {
        Some(id) => id,
        None => {
            // Handler-level unit tests build a minimal App without
            // WorkspaceContextMiddleware (see TenantConn for the
            // mirror pattern). Fall back to the bootstrap workspace
            // so the gate can still be exercised against the
            // workspace_members row claims_for() seeds. Gated to
            // cfg(test) so a production route mis-wiring fails
            // fast with a 500 instead of silently degrading.
            #[cfg(test)]
            {
                1
            }
            #[cfg(not(test))]
            {
                return Err(HttpResponse::InternalServerError().json(json!({
                    "error": "Internal server error",
                    "message": "WorkspaceContext missing — route is mis-wired"
                })));
            }
        }
    };

    let pool = match req.app_data::<web::Data<crate::db::Pool>>() {
        Some(p) => p,
        None => {
            return Err(HttpResponse::InternalServerError().json(json!({
                "error": "Internal server error",
                "message": "Database pool not in app data"
            })));
        }
    };
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => {
            return Err(HttpResponse::InternalServerError().json(json!({
                "error": "Internal server error",
                "message": "Database connection failed"
            })));
        }
    };

    let membership =
        match crate::repository::workspaces::membership(&mut conn, workspace_id, user_uuid) {
            Ok(Some(m)) => m,
            Ok(None) => {
                return Err(HttpResponse::Forbidden().json(json!({
                    "error": "Forbidden",
                    "message": "You are not a member of this workspace"
                })));
            }
            Err(DieselError::NotFound) => {
                return Err(HttpResponse::Forbidden().json(json!({
                    "error": "Forbidden",
                    "message": "You are not a member of this workspace"
                })));
            }
            Err(_) => {
                return Err(HttpResponse::InternalServerError().json(json!({
                    "error": "Internal server error",
                    "message": "Workspace membership lookup failed"
                })));
            }
        };

    let actual = crate::models::WorkspaceRole::from_db(&membership.role);
    if !actual.meets(min) {
        return Err(HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": format!(
                "This action requires {} privileges or higher in this workspace",
                min.as_str()
            )
        })));
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_claims(role: &str) -> Claims {
        // Map the legacy test role token onto the platform role the
        // post-W2 claims carry (workspace-level roles aren't in the
        // token): admin -> platform_admin; audit_reviewer ->
        // audit_reviewer; everything else -> plain user.
        let platform_role = match role {
            "admin" => "platform_admin",
            "audit_reviewer" => "audit_reviewer",
            _ => "user",
        };
        Claims {
            sub: "test-uuid".to_string(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            platform_role: platform_role.to_string(),
            scope: "full".to_string(),
            sid: None,
            exp: 0,
            iat: 0,
        }
    }

    /// Build a TestRequest::default() with the given claims pre-inserted,
    /// converted to an HttpRequest so the require_* functions (which take
    /// HttpRequest, not ServiceRequest) can read from extensions.
    fn req_with_claims(claims: Option<Claims>) -> HttpRequest {
        let req = actix_web::test::TestRequest::default().to_http_request();
        if let Some(c) = claims {
            req.extensions_mut().insert(c);
        }
        req
    }

    #[test]
    fn require_auth_returns_401_without_claims() {
        let req = req_with_claims(None);
        let err = require_auth(&req).expect_err("no claims should error");
        assert_eq!(err.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn require_auth_returns_claims_when_present() {
        let req = req_with_claims(Some(create_test_claims("user")));
        let claims = require_auth(&req).expect("claims should be returned");
        assert_eq!(claims.platform_role, "user");
    }

    #[test]
    fn require_platform_admin_rejects_non_admin_with_403() {
        let req = req_with_claims(Some(create_test_claims("user")));
        let err = require_platform_admin(&req).expect_err("non-admin should be forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_platform_admin_allows_platform_admin() {
        let req = req_with_claims(Some(create_test_claims("admin")));
        let claims = require_platform_admin(&req).expect("platform admin should be allowed");
        assert_eq!(claims.platform_role, "platform_admin");
    }

    #[test]
    fn require_platform_admin_returns_401_without_claims() {
        // 401 vs 403 distinction matters: the frontend redirects to
        // login on 401 but shows an error toast on 403.
        let req = req_with_claims(None);
        let err = require_platform_admin(&req).expect_err("no claims should error");
        assert_eq!(err.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }

    /// Build claims with an explicit role + scope for audit-gate tests.
    fn claims_role_scope(role: &str, scope: &str) -> Claims {
        Claims {
            scope: scope.to_string(),
            ..create_test_claims(role)
        }
    }

    // Scope parsing / validity is covered in `utils::scopes`; here we
    // only exercise the audit gate's role-AND-scope intersection below.

    #[test]
    fn require_audit_read_allows_admin_session() {
        let req = req_with_claims(Some(claims_role_scope("admin", "full")));
        assert!(require_audit_read(&req).is_ok());
    }

    #[test]
    fn require_audit_read_allows_audit_reviewer_session() {
        let req = req_with_claims(Some(claims_role_scope("audit_reviewer", "full")));
        assert!(require_audit_read(&req).is_ok());
    }

    #[test]
    fn require_audit_read_allows_scoped_token_on_reviewer() {
        // SIEM pull: AuditReviewer service account + audit:read-only token.
        let req = req_with_claims(Some(claims_role_scope("audit_reviewer", "audit:read")));
        assert!(require_audit_read(&req).is_ok());
    }

    #[test]
    fn require_audit_read_rejects_technician() {
        let req = req_with_claims(Some(claims_role_scope("technician", "full")));
        let err = require_audit_read(&req).expect_err("technician forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_audit_read_rejects_admin_without_scope() {
        // An admin-owned token narrowed to some other scope must not
        // reach the audit surface: authorisation is role AND scope.
        let req = req_with_claims(Some(claims_role_scope("admin", "tickets:read")));
        let err = require_audit_read(&req).expect_err("missing scope forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_audit_read_returns_401_without_claims() {
        let req = req_with_claims(None);
        let err = require_audit_read(&req).expect_err("no claims should error");
        assert_eq!(err.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }
}
