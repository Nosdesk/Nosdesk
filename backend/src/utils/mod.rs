pub mod auth;
pub mod user;
pub mod image;
pub mod jwt;
pub mod sse;
pub mod mfa;
pub mod storage;
pub mod email;
pub mod email_branding;
pub mod reset_tokens;
pub mod csrf;
pub mod cookies;
pub mod encryption;
pub mod file_validation;
pub mod rate_limit;
pub mod redis_yjs_cache;
pub mod rbac;
pub mod pdf;
pub mod webauthn;

use uuid::Uuid;
use crate::models::UserRole;

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
            Self::InvalidRole(s) => write!(f, "Invalid role: {s}. Must be 'admin', 'technician', or 'user'"),
            Self::ValidationFailed(s) => write!(f, "Validation failed: {s}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Result type alias for validation operations
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Parse UUID from string with proper error handling
pub fn parse_uuid(uuid_str: &str) -> ValidationResult<Uuid> {
    Uuid::parse_str(uuid_str)
        .map_err(|_| ValidationError::InvalidUuid(uuid_str.to_string()))
}

/// Convert UUID to string safely
pub fn uuid_to_string(uuid: &Uuid) -> String {
    uuid.to_string()
}

/// Convert UserRole enum to string for JWT and API responses
pub fn role_to_string(role: &UserRole) -> String {
    match role {
        UserRole::Admin => "admin".to_string(),
        UserRole::Technician => "technician".to_string(),
        UserRole::User => "user".to_string(),
    }
}

/// Parse string to UserRole enum
pub fn parse_role(role_str: &str) -> ValidationResult<UserRole> {
    match role_str.trim().to_lowercase().as_str() {
        "admin" => Ok(UserRole::Admin),
        "technician" => Ok(UserRole::Technician),
        "user" => Ok(UserRole::User),
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

pub use user::*;
pub use image::*;

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
    fn role_to_string_conversions() {
        assert_eq!(role_to_string(&UserRole::Admin), "admin");
        assert_eq!(role_to_string(&UserRole::Technician), "technician");
        assert_eq!(role_to_string(&UserRole::User), "user");
    }

    #[test]
    fn parse_role_valid() {
        assert_eq!(parse_role("admin").unwrap(), UserRole::Admin);
        assert_eq!(parse_role("TECHNICIAN").unwrap(), UserRole::Technician);
        assert_eq!(parse_role("  User  ").unwrap(), UserRole::User);
    }

    #[test]
    fn parse_role_invalid() {
        assert!(parse_role("superadmin").is_err());
    }

    #[test]
    fn normalize_string_trims() {
        assert_eq!(normalize_string("  hello  "), "hello");
    }

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(normalize_email("  Alice@Example.COM  "), "alice@example.com");
    }
}