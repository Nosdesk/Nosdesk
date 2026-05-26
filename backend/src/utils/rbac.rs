//! Role-Based Access Control (RBAC) utilities
//!
//! This module provides centralised role checking functions and response helpers
//! for implementing consistent authorization across all API handlers.

use actix_web::{HttpMessage, HttpRequest, HttpResponse};
use serde_json::json;

use crate::models::Claims;

/// Check if user has technician or admin role
pub fn is_technician_or_admin(claims: &Claims) -> bool {
    claims.role == "admin" || claims.role == "technician"
}

/// Check if user has admin role
pub fn is_admin(claims: &Claims) -> bool {
    claims.role == "admin"
}

/// API-token scope strings the server recognises. `full` is the
/// implicit superscope every session token carries; the rest are
/// narrowing scopes a token can be minted with. Adding a scope here
/// is what makes it accepted at token-creation time.
pub const VALID_TOKEN_SCOPES: &[&str] = &["full", "audit:read"];

/// True if `scope` is a recognised token scope (used to validate
/// requested scopes when minting a token).
pub fn is_valid_token_scope(scope: &str) -> bool {
    VALID_TOKEN_SCOPES.contains(&scope)
}

/// Whether the principal's scope set grants `wanted`. The scope claim
/// is a space-separated set (OAuth convention) for API tokens, or the
/// literal `full` superscope for interactive sessions. `full` grants
/// everything; otherwise an exact member match is required.
pub fn has_scope(claims: &Claims, wanted: &str) -> bool {
    claims.scope == "full" || claims.scope.split_whitespace().any(|s| s == wanted)
}

/// Whether the principal's *role* may read the audit surface.
/// Admin (full operator) or AuditReviewer (the read-only audit role
/// from D4). Technicians and users may not.
pub fn role_can_read_audit(claims: &Claims) -> bool {
    claims.role == "admin" || claims.role == "audit_reviewer"
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

    if !(role_can_read_audit(&claims) && has_scope(&claims, "audit:read")) {
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

/// Extract claims and verify technician or admin role
/// Returns Ok(Claims) if authorized, Err(HttpResponse) with 401/403 if not
pub fn require_technician_or_admin(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    let claims = require_auth(req)?;

    if !is_technician_or_admin(&claims) {
        return Err(HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "This action requires technician or administrator privileges"
        })));
    }

    Ok(claims)
}

/// Extract claims and verify admin role
/// Returns Ok(Claims) if authorized, Err(HttpResponse) with 401/403 if not
pub fn require_admin(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    let claims = require_auth(req)?;

    if !is_admin(&claims) {
        return Err(HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "This action requires administrator privileges"
        })));
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_claims(role: &str) -> Claims {
        Claims {
            sub: "test-uuid".to_string(),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            role: role.to_string(),
            scope: "full".to_string(),
            sid: None,
            exp: 0,
            iat: 0,
        }
    }

    #[test]
    fn test_is_admin() {
        assert!(is_admin(&create_test_claims("admin")));
        assert!(!is_admin(&create_test_claims("technician")));
        assert!(!is_admin(&create_test_claims("user")));
    }

    #[test]
    fn test_is_technician_or_admin() {
        assert!(is_technician_or_admin(&create_test_claims("admin")));
        assert!(is_technician_or_admin(&create_test_claims("technician")));
        assert!(!is_technician_or_admin(&create_test_claims("user")));
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
        assert_eq!(claims.role, "user");
    }

    #[test]
    fn require_admin_rejects_user_role_with_403() {
        let req = req_with_claims(Some(create_test_claims("user")));
        let err = require_admin(&req).expect_err("user role should be forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_admin_rejects_technician_with_403() {
        let req = req_with_claims(Some(create_test_claims("technician")));
        let err = require_admin(&req).expect_err("technician should be forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_admin_allows_admin() {
        let req = req_with_claims(Some(create_test_claims("admin")));
        let claims = require_admin(&req).expect("admin should be allowed");
        assert_eq!(claims.role, "admin");
    }

    #[test]
    fn require_admin_returns_401_without_claims() {
        // Catches the "added a route, forgot the cookie middleware" regression:
        // require_admin must distinguish unauthenticated from wrong-role rather
        // than collapsing both to 403. A 401 vs 403 distinction matters because
        // the frontend redirects to login on 401 but shows an error toast on 403.
        let req = req_with_claims(None);
        let err = require_admin(&req).expect_err("no claims should error");
        assert_eq!(err.status(), actix_web::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn require_technician_or_admin_rejects_user_with_403() {
        let req = req_with_claims(Some(create_test_claims("user")));
        let err = require_technician_or_admin(&req).expect_err("user role should be forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_technician_or_admin_allows_technician() {
        let req = req_with_claims(Some(create_test_claims("technician")));
        let claims = require_technician_or_admin(&req).expect("technician should be allowed");
        assert_eq!(claims.role, "technician");
    }

    #[test]
    fn require_technician_or_admin_allows_admin() {
        let req = req_with_claims(Some(create_test_claims("admin")));
        let claims = require_technician_or_admin(&req).expect("admin should be allowed");
        assert_eq!(claims.role, "admin");
    }

    /// Build claims with an explicit role + scope for audit-gate tests.
    fn claims_role_scope(role: &str, scope: &str) -> Claims {
        Claims {
            scope: scope.to_string(),
            ..create_test_claims(role)
        }
    }

    #[test]
    fn has_scope_treats_full_as_superscope() {
        assert!(has_scope(&claims_role_scope("admin", "full"), "audit:read"));
        assert!(has_scope(
            &claims_role_scope("admin", "audit:read other"),
            "audit:read"
        ));
        assert!(!has_scope(
            &claims_role_scope("admin", "other"),
            "audit:read"
        ));
    }

    #[test]
    fn valid_token_scopes_allowlist() {
        assert!(is_valid_token_scope("full"));
        assert!(is_valid_token_scope("audit:read"));
        assert!(!is_valid_token_scope("audit:write"));
        assert!(!is_valid_token_scope("admin"));
    }

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
