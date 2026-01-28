use anyhow::{anyhow, Result};
use base32;
use bcrypt::{verify as bcrypt_verify, hash as bcrypt_hash, DEFAULT_COST};
use qrcode::{QrCode, render::svg};
use base64::{Engine as _, engine::general_purpose};
use totp_rs::{Algorithm as TotpAlgorithm, TOTP, Secret};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng, RngCore};
use zeroize::ZeroizeOnDrop;
use uuid::Uuid;

use crate::models::{User, UserRole};
use crate::db::DbConnection;
use crate::repository;
use super::encryption;

/// Parse a boolean environment variable in a robust, user-friendly way
/// Accepts: true/false, 1/0, yes/no, on/off (case-insensitive)
fn parse_env_bool(var_name: &str, default_value: bool) -> bool {
    match std::env::var(var_name) {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => default_value,
            }
        }
        Err(_) => default_value,
    }
}

/// Secure wrapper for sensitive strings that zeros memory on drop
#[derive(ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(s)
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// MFA verification result
#[derive(Debug, Clone)]
pub struct MfaVerificationResult {
    pub is_valid: bool,
    pub backup_code_used: Option<String>,
    pub requires_backup_code_regeneration: bool,
}

/// Encrypt MFA secret using AES-256-GCM
/// Delegates to the encryption module for the actual cryptographic operations.
pub fn encrypt_mfa_secret(secret: &str) -> Result<String> {
    encryption::encrypt(secret)
}

/// Decrypt MFA secret using AES-256-GCM
/// Delegates to the encryption module for the actual cryptographic operations.
pub fn decrypt_mfa_secret(encrypted_hex: &str) -> Result<SecretString> {
    let decrypted = encryption::decrypt(encrypted_hex)?;
    Ok(SecretString::new(decrypted))
}

/// Generate a cryptographically secure random string for TOTP secret
/// Uses 160 bits of entropy (recommended minimum for TOTP secrets)
pub fn generate_totp_secret() -> SecretString {
    let mut secret_bytes = [0u8; 20]; // 20 bytes = 160 bits of entropy
    rand::thread_rng().fill_bytes(&mut secret_bytes);
    let secret = base32::encode(base32::Alphabet::RFC4648 { padding: true }, &secret_bytes);
    SecretString::new(secret)
}

/// Generate backup codes for MFA recovery - async version for performance
pub async fn generate_backup_codes_async() -> (Vec<String>, Vec<String>) {
    use tokio::task;
    
    let mut plaintext_codes = Vec::new();
    let mut hash_futures = Vec::new();
    
    // Generate all codes first
    for _ in 0..8 {
        let code: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();
        
        let code_clone = code.clone();
        plaintext_codes.push(code);
        
        // Create async hash task to avoid blocking
        let hash_future = task::spawn_blocking(move || {
            bcrypt_hash(&code_clone, DEFAULT_COST)
                .expect("Failed to hash backup code")
        });
        hash_futures.push(hash_future);
    }
    
    // Wait for all hashing to complete
    let mut hashed_codes = Vec::new();
    for future in hash_futures {
        let hash = future.await.expect("Hash task failed");
        hashed_codes.push(hash);
    }
    
    (plaintext_codes, hashed_codes)
}

/// QR code generation result containing both SVG and matrix data
pub struct QrCodeResult {
    /// Base64-encoded SVG data URL
    pub svg_data_url: String,
    /// Matrix data for frontend animated rendering
    pub matrix: crate::models::QrMatrix,
}

/// Generate QR code as SVG string and return matrix data for animated rendering
pub fn generate_qr_code(secret: &str, user_email: &str, service_name: &str) -> Result<QrCodeResult> {
    // Create TOTP URL for authenticator apps
    let totp_url = format!(
        "otpauth://totp/{service_name}:{user_email}?secret={secret}&issuer={service_name}"
    );

    let code = QrCode::new(&totp_url)
        .map_err(|e| anyhow!("Failed to generate QR code: {}", e))?;

    // Extract matrix data for frontend (row-major order)
    let size = code.width();
    let data: Vec<bool> = code.to_colors()
        .iter()
        .map(|c| *c == qrcode::Color::Dark)
        .collect();

    tracing::info!("QR code generated: size={}x{}, total_modules={}, data_len={}",
        size, size, size * size, data.len());

    let matrix = crate::models::QrMatrix { size, data };

    let svg = code
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .build();

    // Convert SVG to base64 data URL for frontend
    let base64_svg = general_purpose::STANDARD.encode(svg);
    let svg_data_url = format!("data:image/svg+xml;base64,{base64_svg}");

    Ok(QrCodeResult { svg_data_url, matrix })
}

/// Verify TOTP token with timing-attack protection and clock drift tolerance
/// Uses SHA1 algorithm for maximum compatibility with authenticator apps
pub fn verify_totp_token(secret: &str, token: &str) -> bool {
    let secret_bytes = match Secret::Encoded(secret.to_string()).to_bytes() {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    
    let totp = match TOTP::new(
        TotpAlgorithm::SHA1,  // SHA1 for compatibility with most authenticator apps
        6,                    // 6-digit codes (industry standard)
        1,                    // 1 step = 30 seconds
        30,                   // 30-second window (industry standard)
        secret_bytes,
    ) {
        Ok(totp) => totp,
        Err(_) => return false,
    };
    
    // TOTP verification with ±30 second tolerance for clock drift
    totp.check_current(token).unwrap_or(false) ||
    totp.check(token, chrono::Utc::now().timestamp() as u64 - 30) ||
    totp.check(token, chrono::Utc::now().timestamp() as u64 + 30)
}

/// Verify backup code and mark it as used
pub async fn verify_backup_code(
    user_uuid: &Uuid,
    provided_code: &str,
    conn: &mut DbConnection,
) -> Result<MfaVerificationResult> {
    let user = repository::get_user_by_uuid(user_uuid, conn)
        .map_err(|_| anyhow!("User not found"))?;

    // Get backup codes
    let backup_codes = user.mfa_backup_codes
        .as_ref()
        .and_then(|codes| codes.as_array())
        .ok_or_else(|| anyhow!("No backup codes found"))?;

    let mut remaining_codes = Vec::new();
    let mut code_found = false;
    let mut used_code = None;

    // Check each backup code
    for code_value in backup_codes {
        if let Some(hashed_code) = code_value.as_str() {
            if !code_found && bcrypt_verify(provided_code, hashed_code).unwrap_or(false) {
                code_found = true;
                used_code = Some(provided_code.to_string());
                // Don't add the used code to remaining_codes
            } else {
                remaining_codes.push(code_value.clone());
            }
        }
    }

    if !code_found {
        return Ok(MfaVerificationResult {
            is_valid: false,
            backup_code_used: None,
            requires_backup_code_regeneration: false,
        });
    }

    // Check if running low on backup codes before moving remaining_codes
    let requires_regeneration = remaining_codes.len() <= 2;

    // Update backup codes in database (remove the used one)
    let updated_codes = serde_json::Value::Array(remaining_codes);
    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: None,
        mfa_secret: None,
        mfa_backup_codes: Some(updated_codes),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    repository::update_user_mfa(user_uuid, mfa_update, conn)
        .map_err(|_| anyhow!("Failed to update backup codes"))?;

    Ok(MfaVerificationResult {
        is_valid: true,
        backup_code_used: used_code,
        requires_backup_code_regeneration: requires_regeneration,
    })
}

/// Comprehensive MFA verification (TOTP or backup code)
pub async fn verify_mfa_token(
    user_uuid: &Uuid,
    token: &str,
    conn: &mut DbConnection,
) -> Result<MfaVerificationResult> {
    let user = repository::get_user_by_uuid(user_uuid, conn)
        .map_err(|_| anyhow!("User not found"))?;

    if !user.mfa_enabled {
        return Err(anyhow!("MFA is not enabled for this user"));
    }

    // First try TOTP verification
    if let Some(ref encrypted_secret) = user.mfa_secret {
        let secret = decrypt_mfa_secret(encrypted_secret)?;
        if verify_totp_token(secret.as_str(), token) {
            // TOTP replay prevention: check if this code was already used
            if check_totp_replay(user_uuid, token).await {
                tracing::warn!(user_uuid = %user_uuid, "TOTP replay attack detected");
                return Err(anyhow!("This code has already been used. Please wait for a new code."));
            }
            // Mark code as used
            mark_totp_used(user_uuid, token).await;
            return Ok(MfaVerificationResult {
                is_valid: true,
                backup_code_used: None,
                requires_backup_code_regeneration: false,
            });
        }
    }

    // If TOTP fails, try backup code verification
    verify_backup_code(user_uuid, token, conn).await
}

/// Check if TOTP code was already used (replay prevention)
/// Returns true if replay detected, false if code is fresh
/// Fails closed in production (assumes replay) for security
async fn check_totp_replay(user_uuid: &Uuid, token: &str) -> bool {
    use redis::AsyncCommands;
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let is_production = std::env::var("ENVIRONMENT")
        .map(|v| v.to_lowercase() == "production")
        .unwrap_or(false);

    let redis_url = crate::utils::rate_limit::get_redis_url();
    let client = match redis::Client::open(redis_url.as_str()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for TOTP replay check");
            return is_production; // Fail closed in production
        }
    };

    let mut con = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis async connection failed for TOTP replay check");
            return is_production; // Fail closed in production
        }
    };

    // Hash the token to avoid storing it directly
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    let token_hash = hasher.finish();
    let key = format!("totp_used:{user_uuid}:{token_hash}");

    let exists: bool = con.exists(&key).await.unwrap_or(false);
    exists
}

/// Mark TOTP code as used (replay prevention)
async fn mark_totp_used(user_uuid: &Uuid, token: &str) {
    use redis::AsyncCommands;
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    let redis_url = crate::utils::rate_limit::get_redis_url();
    let client = match redis::Client::open(redis_url.as_str()) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut con = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    let token_hash = hasher.finish();
    let key = format!("totp_used:{user_uuid}:{token_hash}");

    // Set with 90-second TTL (TOTP validity window + clock drift tolerance)
    let _: Result<(), _> = con.set_ex(&key, "1", 90).await;
}

/// Check if MFA should be required for a user based on OWASP recommendations
pub fn should_require_mfa(user_role: &UserRole) -> bool {
    match user_role {
        // Allow deployments to optionally disable admin MFA requirement (useful for local/dev)
        // Default remains secure (required) unless explicitly disabled via env
        // Env var: REQUIRE_ADMIN_MFA=true|false (accepts 1/0, yes/no, on/off)
        UserRole::Admin => parse_env_bool("REQUIRE_ADMIN_MFA", true),
        UserRole::Technician => true, // High privilege users
        UserRole::User => false,      // Could be made configurable via env var
    }
}

/// Check if user has MFA enabled and enforce policy
pub async fn validate_mfa_policy(user: &User) -> Result<()> {
    if should_require_mfa(&user.role) && !user.mfa_enabled {
        return Err(anyhow!(
            "MFA is required for {} users. Please enable MFA on your account.", 
            match user.role {
                UserRole::Admin => "administrator",
                UserRole::Technician => "technician", 
                UserRole::User => "user",
            }
        ));
    }
    Ok(())
}

/// Check if user has MFA enabled
pub fn user_has_mfa_enabled(user: &User) -> bool {
    user.mfa_enabled && user.mfa_secret.is_some()
}

/// Log security events for MFA attempts
pub async fn log_mfa_attempt(
    user_uuid: &Uuid,
    success: bool,
    attempt_type: &str,
    request: &actix_web::HttpRequest,
) {
    let user_agent = request.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown");
    
    let ip_address = request.peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    
    if success {
        tracing::info!(
            "Successful MFA {} for user {} from IP {} using {}",
            attempt_type, user_uuid, ip_address, user_agent
        );
    } else {
        tracing::warn!(
            "Failed MFA {} for user {} from IP {} using {}",
            attempt_type, user_uuid, ip_address, user_agent
        );
    }
}

/// Get MFA rate limiting configuration from environment
/// Defaults: 5 attempts per 15 minutes (OWASP recommended for production)
/// Can be relaxed for development via environment variables
fn get_mfa_rate_limit_config() -> (u32, u64) {
    let max_attempts = std::env::var("MFA_MAX_ATTEMPTS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(5); // Default: 5 attempts

    let window_seconds = std::env::var("MFA_WINDOW_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(900); // Default: 900 seconds (15 minutes)

    (max_attempts, window_seconds)
}

/// MFA rate limiting check using Redis
/// Configurable via environment variables:
/// - MFA_MAX_ATTEMPTS: Maximum attempts (default: 5)
/// - MFA_WINDOW_SECONDS: Time window in seconds (default: 900 = 15 min)
///
/// Example for development: MFA_MAX_ATTEMPTS=50 MFA_WINDOW_SECONDS=60
///
/// # Arguments
/// * `user_uuid` - User's UUID to check rate limit for
///
/// # Returns
/// * `true` - Request is allowed (under limit)
/// * `false` - Rate limit exceeded (too many attempts)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_require_mfa_for_technician() {
        assert!(should_require_mfa(&UserRole::Technician));
    }

    #[test]
    fn should_not_require_mfa_for_regular_user() {
        assert!(!should_require_mfa(&UserRole::User));
    }

    #[test]
    fn user_has_mfa_enabled_requires_both_flag_and_secret() {
        let now = chrono::Utc::now().naive_utc();
        let base_user = User {
            uuid: Uuid::new_v4(),
            name: "test".into(),
            role: UserRole::User,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None,
            created_at: now,
            updated_at: now,
            password_changed_at: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
        };

        assert!(!user_has_mfa_enabled(&base_user));

        let mut enabled_no_secret = base_user.clone();
        enabled_no_secret.mfa_enabled = true;
        assert!(!user_has_mfa_enabled(&enabled_no_secret));

        let mut has_secret_not_enabled = base_user.clone();
        has_secret_not_enabled.mfa_secret = Some("secret".into());
        assert!(!user_has_mfa_enabled(&has_secret_not_enabled));

        let mut both = base_user.clone();
        both.mfa_enabled = true;
        both.mfa_secret = Some("secret".into());
        assert!(user_has_mfa_enabled(&both));
    }

    #[test]
    fn generate_totp_secret_is_valid_base32() {
        let secret = generate_totp_secret();
        let decoded = base32::decode(base32::Alphabet::RFC4648 { padding: true }, secret.as_str());
        assert!(decoded.is_some());
        assert_eq!(decoded.unwrap().len(), 20); // 160 bits
    }

    #[test]
    fn generate_qr_code_produces_svg_data_url() {
        let result = generate_qr_code("JBSWY3DPEHPK3PXP", "test@example.com", "Nosdesk");
        assert!(result.is_ok());
        let qr = result.unwrap();
        assert!(qr.svg_data_url.starts_with("data:image/svg+xml;base64,"));
        assert!(qr.matrix.size > 0);
        assert_eq!(qr.matrix.data.len(), qr.matrix.size * qr.matrix.size);
    }
}

pub async fn check_mfa_rate_limit(user_uuid: &Uuid) -> bool {
    use crate::utils::rate_limit::RateLimiter;

    // Get configuration from environment
    let (max_attempts, window_seconds) = get_mfa_rate_limit_config();

    // Get Redis URL from environment
    let redis_url = crate::utils::rate_limit::get_redis_url();

    // Generate rate limit key for this user
    let key = RateLimiter::mfa_attempt_key(user_uuid);

    // Check the rate limit
    match RateLimiter::check_rate_limit(&redis_url, &key, max_attempts, window_seconds).await {
        Ok(allowed) => {
            if !allowed {
                tracing::warn!(
                    "MFA rate limit exceeded for user {} ({} attempts in {} seconds)",
                    user_uuid,
                    max_attempts,
                    window_seconds
                );
            }
            allowed
        }
        Err(e) => {
            tracing::error!("MFA rate limit check failed for user {}: {}", user_uuid, e);
            // Fail-closed in production (security-critical), fail-open in development (convenience)
            let is_production = std::env::var("ENVIRONMENT")
                .map(|v| v.to_lowercase() == "production")
                .unwrap_or(false);
            if is_production {
                tracing::warn!("Denying MFA attempt due to rate limit check failure (fail-closed mode)");
                false
            } else {
                tracing::warn!("Allowing MFA attempt due to rate limit check failure (fail-open mode in non-production)");
                true
            }
        }
    }
} 