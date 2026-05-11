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

/// Extract claims from request and check if user is authenticated
/// Returns Ok(Claims) if authenticated, Err(HttpResponse) with 401 if not
pub fn require_auth(req: &HttpRequest) -> Result<Claims, HttpResponse> {
    req.extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| {
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
        let err = require_technician_or_admin(&req)
            .expect_err("user role should be forbidden");
        assert_eq!(err.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[test]
    fn require_technician_or_admin_allows_technician() {
        let req = req_with_claims(Some(create_test_claims("technician")));
        let claims = require_technician_or_admin(&req)
            .expect("technician should be allowed");
        assert_eq!(claims.role, "technician");
    }

    #[test]
    fn require_technician_or_admin_allows_admin() {
        let req = req_with_claims(Some(create_test_claims("admin")));
        let claims = require_technician_or_admin(&req).expect("admin should be allowed");
        assert_eq!(claims.role, "admin");
    }
}
