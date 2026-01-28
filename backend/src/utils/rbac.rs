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
}
