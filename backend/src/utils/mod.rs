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