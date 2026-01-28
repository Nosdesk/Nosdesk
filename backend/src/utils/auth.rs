use bcrypt::{hash, DEFAULT_COST};
use crate::utils::{ValidationError, ValidationResult};

/// Hash a password securely using bcrypt
pub fn hash_password(password: &str) -> ValidationResult<String> {
    hash(password, DEFAULT_COST)
        .map_err(|_| ValidationError::ValidationFailed("Failed to hash password".to_string()))
}

/// Password validation result
#[derive(Debug)]
pub struct PasswordValidation {
    pub valid: bool,
    pub errors: Vec<String>,
}

/// Validate password against length and optional complexity requirements
/// Complexity is enabled via REQUIRE_PASSWORD_COMPLEXITY=true
pub fn validate_password(password: &str) -> PasswordValidation {
    let mut errors = Vec::new();

    // Length requirements (always enforced)
    if password.len() < 8 {
        errors.push("Password must be at least 8 characters".to_string());
    }
    if password.len() > 128 {
        errors.push("Password must be less than 128 characters".to_string());
    }

    // Optional complexity requirements
    let require_complexity = std::env::var("REQUIRE_PASSWORD_COMPLEXITY")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    if require_complexity {
        if !password.chars().any(|c| c.is_uppercase()) {
            errors.push("Password must contain at least one uppercase letter".to_string());
        }
        if !password.chars().any(|c| c.is_lowercase()) {
            errors.push("Password must contain at least one lowercase letter".to_string());
        }
        if !password.chars().any(|c| c.is_numeric()) {
            errors.push("Password must contain at least one number".to_string());
        }
        if !password.chars().any(|c| !c.is_alphanumeric()) {
            errors.push("Password must contain at least one special character".to_string());
        }
    }

    PasswordValidation {
        valid: errors.is_empty(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_password_passes() {
        let result = validate_password("abcd1234");
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn too_short_password_fails() {
        let result = validate_password("1234567");
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("at least 8")));
    }

    #[test]
    fn exactly_8_chars_passes() {
        let result = validate_password("12345678");
        assert!(result.valid);
    }

    #[test]
    fn exactly_128_chars_passes() {
        let long = "a".repeat(128);
        let result = validate_password(&long);
        assert!(result.valid);
    }

    #[test]
    fn over_128_chars_fails() {
        let long = "a".repeat(129);
        let result = validate_password(&long);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("less than 128")));
    }

    #[test]
    fn empty_password_fails() {
        let result = validate_password("");
        assert!(!result.valid);
    }

    #[test]
    fn hash_password_returns_bcrypt_hash() {
        let hash = hash_password("test1234").unwrap();
        assert!(hash.starts_with("$2b$") || hash.starts_with("$2a$"));
    }
}