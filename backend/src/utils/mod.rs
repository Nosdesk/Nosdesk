pub mod analytics_cache;
pub mod auth;
pub mod bootstrap_token;
pub mod client_ip;
pub mod content;
pub mod cookies;
pub mod cors_allowlist;
pub mod csrf;
pub mod egress;
pub mod email;
pub mod email_branding;
pub mod encryption;
pub mod error_response;
pub mod file_validation;
pub mod geoip;
pub mod i18n;
pub mod image;
pub mod jwt;
pub mod locale;
pub mod login_timing;
pub mod markdown_export;
pub mod mfa;
pub mod pdf;
pub mod rate_limit;
pub mod rbac;
pub mod redis_yjs_cache;
pub mod reserved_slugs;
pub mod reset_tokens;
pub mod safe_http;
pub mod scopes;
pub mod security_events;
pub mod slug;
pub mod storage;
pub mod template_variables;
pub mod tenant_origin;
pub mod tracing_redact;
pub mod user;
pub mod utf8_trunc;
pub mod verp;
pub mod webauthn;
pub mod workspace_slug;

use crate::models::{PlatformRole, WorkspaceRole};
use uuid::Uuid;

/// Custom error types for better error handling
#[derive(Debug)]
pub enum ValidationError {
    InvalidUuid(String),
    InvalidRole(String),
    ValidationFailed(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUuid(s) => write!(f, "Invalid UUID format: {s}"),
            Self::InvalidRole(s) => write!(
                f,
                "Invalid role: {s}. Must be 'admin', 'technician', or 'user'"
            ),
            Self::ValidationFailed(s) => write!(f, "Validation failed: {s}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Result type alias for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Parse UUID from string with proper error handling
pub fn parse_uuid(uuid_str: &str) -> ValidationResult<Uuid> {
    Uuid::parse_str(uuid_str).map_err(|_| ValidationError::InvalidUuid(uuid_str.to_string()))
}

/// Convert UUID to string safely
pub fn uuid_to_string(uuid: &Uuid) -> String {
    uuid.to_string()
}

/// Parse a create-user / import "role" request string onto the W2
/// split `(platform_role, workspace_role)`. The legacy request
/// vocabulary is preserved for callers/clients:
///
/// - `admin`          → workspace admin (platform `user`). A created
///   admin manages their workspace; instance-wide platform-admin is
///   reserved for the bootstrap operator and isn't granted here.
/// - `technician`     → workspace agent (platform `user`).
/// - `user`           → workspace member (platform `user`).
/// - `audit_reviewer` → platform audit reviewer + workspace member.
pub fn parse_roles(role_str: &str) -> ValidationResult<(PlatformRole, WorkspaceRole)> {
    match role_str.trim().to_lowercase().as_str() {
        "admin" => Ok((PlatformRole::User, WorkspaceRole::Admin)),
        "technician" => Ok((PlatformRole::User, WorkspaceRole::Agent)),
        "user" => Ok((PlatformRole::User, WorkspaceRole::Member)),
        "audit_reviewer" => Ok((PlatformRole::AuditReviewer, WorkspaceRole::Member)),
        _ => Err(ValidationError::InvalidRole(role_str.to_string())),
    }
}

/// Normalize and trim string input
pub fn normalize_string(input: &str) -> String {
    input.trim().to_string()
}

/// Normalize email (trim + lowercase)
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub use image::*;
pub use user::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uuid_valid() {
        let uuid = parse_uuid("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(uuid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn parse_uuid_invalid() {
        assert!(parse_uuid("not-a-uuid").is_err());
    }

    #[test]
    fn parse_roles_valid() {
        assert_eq!(
            parse_roles("admin").unwrap(),
            (PlatformRole::User, WorkspaceRole::Admin)
        );
        assert_eq!(
            parse_roles("TECHNICIAN").unwrap(),
            (PlatformRole::User, WorkspaceRole::Agent)
        );
        assert_eq!(
            parse_roles("  User  ").unwrap(),
            (PlatformRole::User, WorkspaceRole::Member)
        );
        assert_eq!(
            parse_roles("audit_reviewer").unwrap(),
            (PlatformRole::AuditReviewer, WorkspaceRole::Member)
        );
    }

    #[test]
    fn parse_role_invalid() {
        assert!(parse_roles("superadmin").is_err());
    }

    #[test]
    fn normalize_string_trims() {
        assert_eq!(normalize_string("  hello  "), "hello");
    }

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email("  Alice@Example.COM  "),
            "alice@example.com"
        );
    }
}
