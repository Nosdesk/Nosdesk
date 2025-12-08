use bcrypt::{hash, DEFAULT_COST};
use crate::utils::{ValidationError, ValidationResult};

/// Hash a password securely using bcrypt
pub fn hash_password(password: &str) -> ValidationResult<String> {
    hash(password, DEFAULT_COST)
        .map_err(|_| ValidationError::ValidationFailed("Failed to hash password".to_string()))
} 