use rand::Rng;

/// Generate a cryptographically secure CSRF token (32 bytes = 64 hex chars)
pub fn generate_csrf_token() -> String {
    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(token_bytes)
}

/// Validate a CSRF token by comparing it to the expected value
pub fn validate_csrf_token(provided: &str, expected: &str) -> bool {
    // Use constant-time comparison to prevent timing attacks
    use constant_time_eq::constant_time_eq;
    constant_time_eq(provided.as_bytes(), expected.as_bytes())
}
