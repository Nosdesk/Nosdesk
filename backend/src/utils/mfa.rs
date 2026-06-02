use anyhow::{anyhow, Result};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base32;
use base64::{engine::general_purpose, Engine as _};
use bcrypt::verify as bcrypt_verify;
use qrcode::{render::svg, QrCode};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng, RngCore};
use totp_rs::{Algorithm as TotpAlgorithm, Secret, TOTP};
use uuid::Uuid;
use zeroize::ZeroizeOnDrop;

use super::encryption;
use crate::db::DbConnection;
use crate::models::{User, UserRole};
use crate::repository;

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

/// AAD purpose tag for MFA TOTP secrets. Combined with the user UUID so
/// a cross-user ciphertext swap (attacker with SQL UPDATE) fails the
/// AEAD tag check. The constant tail also blocks a swap into a different
/// purpose using the same user_id (e.g. password reset blob).
const MFA_AAD_TAG: &[u8] = b".nosdesk.mfa.totp.v1";

/// AAD = `user_uuid.as_bytes() ‖ MFA_AAD_TAG`. Pinning ciphertext to a
/// specific row identity is RFC 5116 §1.2 "bind context" plus the OWASP
/// Crypto Storage Cheat Sheet recommendation. 16 + 20 = 36 bytes.
fn mfa_aad(user_uuid: &Uuid) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16 + MFA_AAD_TAG.len());
    buf.extend_from_slice(user_uuid.as_bytes());
    buf.extend_from_slice(MFA_AAD_TAG);
    buf
}

/// Encrypt an MFA TOTP secret. Returns `(blob, kek_id)` for storage in
/// `users.mfa_secret` and `users.mfa_secret_kek_id`. The kek_id is also
/// embedded in the blob; the sidecar exists for the rewrap query path.
pub fn encrypt_mfa_secret(secret: &str, user_uuid: &Uuid) -> Result<(Vec<u8>, i16)> {
    let kr = encryption::keyring();
    let aad = mfa_aad(user_uuid);
    let blob = kr
        .encrypt(secret.as_bytes(), &aad)
        .map_err(|e| anyhow!("MFA secret encrypt failed: {e}"))?;
    Ok((blob, kr.current_version() as i16))
}

/// Decrypt an MFA TOTP secret stored in `users.mfa_secret`. The caller
/// must pass the user UUID and the sidecar `kek_id`; we verify the
/// sidecar matches the blob's encoded kek_id (sidecar is a mirror, not
/// authoritative) and reject the row on mismatch.
pub fn decrypt_mfa_secret(
    blob: &[u8],
    sidecar_kek_id: i16,
    user_uuid: &Uuid,
) -> Result<SecretString> {
    let blob_kek_id = encryption::Keyring::read_kek_id(blob)
        .map_err(|e| anyhow!("MFA secret has malformed frame header: {e}"))?;
    if blob_kek_id as i16 != sidecar_kek_id {
        return Err(anyhow!(
            "MFA secret sidecar kek_id ({sidecar_kek_id}) disagrees with blob ({blob_kek_id}); refusing to decrypt"
        ));
    }
    let aad = mfa_aad(user_uuid);
    let plaintext = encryption::keyring()
        .decrypt(blob, &aad)
        .map_err(|e| anyhow!("MFA secret decrypt failed: {e}"))?;
    let s = String::from_utf8(plaintext.to_vec())
        .map_err(|_| anyhow!("MFA secret is not valid UTF-8"))?;
    Ok(SecretString::new(s))
}

/// Generate a cryptographically secure random string for TOTP secret
/// Uses 160 bits of entropy (recommended minimum for TOTP secrets)
pub fn generate_totp_secret() -> SecretString {
    let mut secret_bytes = [0u8; 20]; // 20 bytes = 160 bits of entropy
    rand::thread_rng().fill_bytes(&mut secret_bytes);
    let secret = base32::encode(base32::Alphabet::RFC4648 { padding: true }, &secret_bytes);
    SecretString::new(secret)
}

/// Generate backup codes for MFA recovery — async version for
/// performance.
///
/// Returns `(plaintext, hashed)` where `plaintext` is wrapped in
/// [`zeroize::Zeroizing`] so its backing allocations get wiped on
/// drop. Per the F2C deferred audit work and the convergent
/// Rust-crypto position (research-backed, see
/// `docs/auth-convergence.md`), this is **shallow** protection:
/// `Zeroize` here wipes the *source* `Vec<String>` when the
/// handler returns or unwinds. The clones that `serde_json::json!`
/// makes into a `Value` tree, and the actix-web response body
/// buffer, are NOT covered — they're freed unwiped before this
/// wrapper's `Drop` runs. We accept that: the response body is
/// ~64 bytes alive for microseconds, and the AES KEK itself is in
/// the env vars for the whole process lifetime, so any attacker
/// who can read freed heap pages already wins. The realistic
/// benefit of the wrap is panic-safety between codes generation
/// and response send.
///
/// Output shape: **10 codes × 10 mixed-case alphanumeric chars**,
/// hashed with **argon2id** (OWASP minimum profile, m=19 MiB t=2
/// p=1). Matches Stripe / GitHub / Google defaults at ~59 bits
/// of entropy per code; argon2id replaces the previous bcrypt
/// hash for consistency with the password module (memory-hardness
/// adds margin against GPU/ASIC parallel attack even though the
/// per-code entropy is the dominant security property).
///
/// Verify-path forward compatibility: `verify_backup_code` detects
/// the hash format prefix (`$argon2id$` vs `$2b$`) so codes
/// generated before this migration keep working until consumed.
/// Recovery codes are single-use; no data migration needed.
pub async fn generate_backup_codes_async() -> (zeroize::Zeroizing<Vec<String>>, Vec<String>) {
    use tokio::task;

    let mut plaintext_codes: Vec<String> = Vec::new();
    let mut hash_futures = Vec::new();

    // Generate all codes first. 10 codes × 10 chars mixed-case
    // alphanumeric → ~59 bits/code. The `Alphanumeric` distribution
    // already produces mixed-case [a-zA-Z0-9]; the legacy
    // implementation force-uppercased to 36-char alphabet (~41
    // bits) which we now drop for the full ~62-char alphabet.
    for _ in 0..10 {
        let code: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();

        let code_clone = code.clone();
        plaintext_codes.push(code);

        // argon2id hash off the request thread. spawn_blocking
        // because argon2 is intentionally slow (~100ms with the
        // OWASP minimum profile); awaiting it on the runtime
        // thread would block other tasks. Default `Argon2::default()`
        // uses Argon2id with OWASP-spec parameters.
        let hash_future = task::spawn_blocking(move || -> String {
            let salt = SaltString::generate(&mut rand::rngs::OsRng);
            Argon2::default()
                .hash_password(code_clone.as_bytes(), &salt)
                .expect("Failed to hash recovery code with argon2id")
                .to_string()
        });
        hash_futures.push(hash_future);
    }

    // Wait for all hashing to complete in parallel
    let mut hashed_codes = Vec::new();
    for future in hash_futures {
        let hash = future.await.expect("Hash task failed");
        hashed_codes.push(hash);
    }

    (zeroize::Zeroizing::new(plaintext_codes), hashed_codes)
}

/// Verify a recovery-code plaintext against a stored hash,
/// dispatching to argon2id or bcrypt based on the hash format
/// prefix. New codes (post-migration) are argon2id; pre-migration
/// codes are bcrypt and continue to verify correctly until
/// consumed.
///
/// Returns false on hash-parse errors so an unexpected/corrupt
/// hash string doesn't return Err (which would break the
/// constant-time verify loop's "check every code" guarantee).
fn verify_recovery_code_hash(plaintext: &str, stored_hash: &str) -> bool {
    if stored_hash.starts_with("$argon2") {
        match PasswordHash::new(stored_hash) {
            Ok(parsed) => Argon2::default()
                .verify_password(plaintext.as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        }
    } else {
        // Legacy bcrypt hash (`$2a$` / `$2b$` / `$2y$` prefixes).
        bcrypt_verify(plaintext, stored_hash).unwrap_or(false)
    }
}

/// QR code generation result containing both SVG and matrix data
pub struct QrCodeResult {
    /// Base64-encoded SVG data URL
    pub svg_data_url: String,
    /// Matrix data for frontend animated rendering
    pub matrix: crate::models::QrMatrix,
}

/// Generate QR code as SVG string and return matrix data for animated rendering
pub fn generate_qr_code(
    secret: &str,
    user_email: &str,
    service_name: &str,
) -> Result<QrCodeResult> {
    // Create TOTP URL for authenticator apps
    let totp_url =
        format!("otpauth://totp/{service_name}:{user_email}?secret={secret}&issuer={service_name}");

    let code = QrCode::new(&totp_url).map_err(|e| anyhow!("Failed to generate QR code: {}", e))?;

    // Extract matrix data for frontend (row-major order)
    let size = code.width();
    let data: Vec<bool> = code
        .to_colors()
        .iter()
        .map(|c| *c == qrcode::Color::Dark)
        .collect();

    tracing::info!(
        "QR code generated: size={}x{}, total_modules={}, data_len={}",
        size,
        size,
        size * size,
        data.len()
    );

    let matrix = crate::models::QrMatrix { size, data };

    let svg = code.render::<svg::Color>().min_dimensions(200, 200).build();

    // Convert SVG to base64 data URL for frontend
    let base64_svg = general_purpose::STANDARD.encode(svg);
    let svg_data_url = format!("data:image/svg+xml;base64,{base64_svg}");

    Ok(QrCodeResult {
        svg_data_url,
        matrix,
    })
}

/// Verify TOTP token with timing-attack protection and clock drift tolerance
/// Uses SHA1 algorithm for maximum compatibility with authenticator apps
pub fn verify_totp_token(secret: &str, token: &str) -> bool {
    let secret_bytes = match Secret::Encoded(secret.to_string()).to_bytes() {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    let totp = match TOTP::new(
        TotpAlgorithm::SHA1, // SHA1 for compatibility with most authenticator apps
        6,                   // 6-digit codes (industry standard)
        1,                   // skew = 1 step: accept the previous and next code
        30,                  // 30-second step
        secret_bytes,
    ) {
        Ok(totp) => totp,
        Err(_) => return false,
    };

    // `check_current` already honours the skew above, so it accepts the
    // current code plus one step either side (±30s) for clock drift.
    // The previous code added explicit ±30s offset checks on top of
    // that, widening the acceptance window to ±90s; rely on the
    // library's built-in window instead (NIST SP 800-63B favours a
    // tight validation window).
    totp.check_current(token).unwrap_or(false)
}

/// Verify a recovery code against the user's unused-codes set,
/// consuming the matched code atomically.
///
/// Constant-time semantics (closes F2C.3 M4): every unused code
/// is bcrypt-verified before consuming the matched id, so the
/// per-call latency is independent of which code matched (or
/// whether any did). bcrypt itself is intentionally slow
/// (~50-100ms per verify); N ≤ 10 unused codes per user makes
/// the worst-case latency ≤ 1s, acceptable for the rare
/// recovery-code path.
///
/// The pre-decoupling implementation early-returned on the first
/// match (latency leaked the matched code's position) and used
/// an app-side read-modify-write of the JSONB array on `users`
/// (concurrent consumers raced for the row lock, second saw a
/// stale array and could lose a consumption). Both fixed by:
///   - Looping over every unused code regardless of an earlier
///     match → constant work.
///   - Single-statement consumption via
///     `repository::user_recovery_codes::consume_by_id` →
///     atomic row-level lock; concurrent consumers see one
///     succeed, the rest see "already consumed" (which is the
///     correct outcome).
pub async fn verify_backup_code(
    user_uuid: &Uuid,
    provided_code: &str,
    conn: &mut DbConnection,
) -> Result<MfaVerificationResult> {
    let unused = repository::user_recovery_codes::list_unused(conn, user_uuid)
        .map_err(|_| anyhow!("Failed to load recovery codes"))?;

    // Verify against EVERY unused code before consuming, so
    // latency is independent of which one matched. Even when an
    // early entry matches, keep verifying the rest so the loop
    // does a fixed amount of work per call. (`std::hint::black_box`
    // would be ideal here but it's not stabilised on the verify
    // path; keeping the loop structure honest is the next-best.)
    let mut matched_id: Option<i64> = None;
    for code in &unused {
        // Format-aware verify (argon2id for new codes, bcrypt for
        // pre-migration codes). Mixing formats in one verify loop
        // makes per-call latency depend on the format mix, but
        // that only leaks "how many pre-vs-post-migration codes
        // this user has" which has no attack value.
        if verify_recovery_code_hash(provided_code, &code.code_hash) {
            // Pin the FIRST match — multiple matches would mean
            // the same plaintext was registered twice, which our
            // generator doesn't produce, but defensively we still
            // only consume one.
            if matched_id.is_none() {
                matched_id = Some(code.id);
            }
        }
    }

    let matched = match matched_id {
        Some(id) => id,
        None => {
            return Ok(MfaVerificationResult {
                is_valid: false,
                backup_code_used: None,
                requires_backup_code_regeneration: false,
            });
        }
    };

    // Single-statement atomic consumption. Returns false when
    // someone else already consumed this id between our verify
    // loop and now — treat that as "not valid for this attempt"
    // (the concurrent winner already authenticated).
    let consumed = repository::user_recovery_codes::consume_by_id(conn, matched)
        .map_err(|_| anyhow!("Failed to consume recovery code"))?;
    if !consumed {
        return Ok(MfaVerificationResult {
            is_valid: false,
            backup_code_used: None,
            requires_backup_code_regeneration: false,
        });
    }

    // Suggest regeneration when the consumed code dropped the
    // unused count to ≤ 2. We had `unused.len()` codes; one was
    // just consumed; if `unused.len() - 1 <= 2` then regenerate.
    let requires_regeneration = unused.len() <= 3;

    Ok(MfaVerificationResult {
        is_valid: true,
        backup_code_used: Some(provided_code.to_string()),
        requires_backup_code_regeneration: requires_regeneration,
    })
}

/// Comprehensive MFA verification (TOTP or backup code)
pub async fn verify_mfa_token(
    user_uuid: &Uuid,
    token: &str,
    conn: &mut DbConnection,
) -> Result<MfaVerificationResult> {
    // Active-only — a soft-deleted user must not complete MFA
    // verification even if they hold a pending-MFA cookie from
    // a successful password step. F2C.2 H4.
    let user = repository::users::find_active_by_uuid(user_uuid, conn)
        .map_err(|_| anyhow!("User not found"))?;

    if !user.mfa_enabled {
        return Err(anyhow!("MFA is not enabled for this user"));
    }

    // First try TOTP verification
    if let Some(ref encrypted_secret) = user.mfa_secret {
        let kek_id = user
            .mfa_secret_kek_id
            .ok_or_else(|| anyhow!("MFA secret stored without sidecar kek_id; row is corrupt"))?;
        let secret = decrypt_mfa_secret(encrypted_secret, kek_id, &user.uuid)?;
        if verify_totp_token(secret.as_str(), token) {
            // TOTP replay prevention: check if this code was already used
            if check_totp_replay(user_uuid, token).await {
                tracing::warn!(user_uuid = %user_uuid, "TOTP replay attack detected");
                return Err(anyhow!(
                    "This code has already been used. Please wait for a new code."
                ));
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

/// Build the Redis key used for TOTP replay tracking.
///
/// The token is hashed with SHA-256 rather than stored verbatim,
/// so a snapshot of Redis doesn't expose recently-used codes. Plain
/// SHA-256 (not HMAC) is sufficient here because:
///
/// * The replay cache only needs determinism + collision-resistance,
///   not unforgeability — the secret-keeping job is done by the TOTP
///   secret itself, which never reaches this layer.
/// * The user uuid is already in plaintext in the key, so a brute-
///   force precomputation over the 10^6 possible 6-digit tokens
///   buys the attacker only the codes they already had to know to
///   construct the Redis key in the first place.
///
/// The earlier implementation used `std::collections::hash_map::
/// DefaultHasher`, whose output is documented as unstable across
/// Rust releases. A toolchain bump silently invalidated the entire
/// replay cache, opening a small but real window. SHA-256 is
/// stable, fast enough for one hash per login, and pulls in no new
/// dependencies (`ring` is already used for AES-256-GCM elsewhere
/// in this module's neighbourhood).
fn totp_replay_key(user_uuid: &Uuid, token: &str) -> String {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    ctx.update(user_uuid.as_bytes());
    ctx.update(b":");
    ctx.update(token.as_bytes());
    let digest = ctx.finish();
    format!("totp_used:{user_uuid}:{}", hex::encode(digest.as_ref()))
}

/// Check if TOTP code was already used (replay prevention)
/// Returns true if replay detected, false if code is fresh.
///
/// Fails CLOSED by default (a Redis outage is treated as "replay
/// detected", denying the attempt) because most self-hosted installs
/// never set `ENVIRONMENT=production`. Operators who want the old
/// fail-open behaviour for local development set
/// `NOSDESK_MFA_REPLAY_DEV_FAIL_OPEN=1` explicitly; it is documented as
/// dev-only.
async fn check_totp_replay(user_uuid: &Uuid, token: &str) -> bool {
    use redis::AsyncCommands;

    // `true` means "treat Redis failures as replay" (deny). Default on;
    // only the explicit dev flag turns it off.
    let fail_closed = std::env::var("NOSDESK_MFA_REPLAY_DEV_FAIL_OPEN")
        .map(|v| v.trim() != "1")
        .unwrap_or(true);

    let redis_url = crate::utils::rate_limit::get_redis_url();
    let client = match redis::Client::open(redis_url.as_str()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for TOTP replay check");
            return fail_closed;
        }
    };

    let mut con = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis async connection failed for TOTP replay check");
            return fail_closed;
        }
    };

    let key = totp_replay_key(user_uuid, token);
    con.exists(&key).await.unwrap_or(fail_closed)
}

/// Mark TOTP code as used (replay prevention)
async fn mark_totp_used(user_uuid: &Uuid, token: &str) {
    use redis::AsyncCommands;

    let redis_url = crate::utils::rate_limit::get_redis_url();
    let client = match redis::Client::open(redis_url.as_str()) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut con = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(_) => return,
    };

    let key = totp_replay_key(user_uuid, token);
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
        UserRole::Technician => true,    // High privilege users
        UserRole::AuditReviewer => true, // Reads sensitive audit data
        UserRole::User => false,         // Could be made configurable via env var
    }
}

/// Check if user has MFA enabled and enforce policy
/// Considers both TOTP and passkeys as valid MFA methods.
///
/// Returns an error not just on policy violation but also on DB
/// errors from the passkey-count lookup — the security gate must
/// not silently downgrade to "no passkeys → no MFA → policy
/// failed" when we actually don't know. The caller can
/// distinguish via `anyhow` `chain()` if needed; in practice the
/// caller surfaces both as a 5xx and tells the user to retry.
pub async fn validate_mfa_policy(user: &User, conn: &mut crate::db::DbConnection) -> Result<()> {
    let role = crate::repository::user_helpers::legacy_role_for_user(
        conn,
        user.uuid,
        &user.platform_role,
    );
    if !should_require_mfa(&role) || user.mfa_enabled {
        return Ok(());
    }
    if user_has_passkeys(conn, &user.uuid)? {
        return Ok(());
    }
    Err(anyhow!(
        "MFA is required for {} users. Please enable MFA on your account.",
        match role {
            UserRole::Admin => "administrator",
            UserRole::Technician => "technician",
            UserRole::AuditReviewer => "audit reviewer",
            UserRole::User => "user",
        }
    ))
}

/// Check if user has TOTP MFA enabled
pub fn user_has_mfa_enabled(user: &User) -> bool {
    user.mfa_enabled && user.mfa_secret.is_some()
}

/// Check if user has any passkeys registered.
///
/// Returns `Result<bool>` rather than `bool` so a DB error
/// during the count query propagates up to the caller instead of
/// silently collapsing to "no passkeys" and downgrading the
/// security gate. The pre-fix `unwrap_or(false)` here is the
/// exact pattern the nosdesk-com F2C.1 C1 audit finding flagged
/// as CRITICAL: a passkey-only user could be granted a
/// password-only session if the count query transiently failed
/// (connection pool exhausted, replica lag, network blip). The
/// rule (Bowyer / "fail closed on security gates"): a gate that
/// can't read its own state must deny, not downgrade.
///
/// Callers that previously used `if user_has_passkeys(...) {
/// return passkey_required }` now use `?` to propagate the DB
/// error as an internal-error response.
pub fn user_has_passkeys(conn: &mut crate::db::DbConnection, user_uuid: &Uuid) -> Result<bool> {
    crate::repository::passkey_credentials::count_for_user(conn, user_uuid)
        .map(|n| n > 0)
        .map_err(|e| anyhow!("Failed to check passkey registration: {:?}", e))
}

/// Log security events for MFA attempts
pub async fn log_mfa_attempt(
    user_uuid: &Uuid,
    success: bool,
    attempt_type: &str,
    request: &actix_web::HttpRequest,
) {
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown");

    let ip_address = crate::utils::client_ip::from_http_request(request)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    if success {
        tracing::info!(
            "Successful MFA {} for user {} from IP {} using {}",
            attempt_type,
            user_uuid,
            ip_address,
            user_agent
        );
    } else {
        tracing::warn!(
            "Failed MFA {} for user {} from IP {} using {}",
            attempt_type,
            user_uuid,
            ip_address,
            user_agent
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
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            created_at: now,
            updated_at: now,
            password_changed_at: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: "user".to_string(),
            feature_flag_overrides: serde_json::json!({}),
            deleted_at: None,
        };

        assert!(!user_has_mfa_enabled(&base_user));

        let mut enabled_no_secret = base_user.clone();
        enabled_no_secret.mfa_enabled = true;
        assert!(!user_has_mfa_enabled(&enabled_no_secret));

        let mut has_secret_not_enabled = base_user.clone();
        has_secret_not_enabled.mfa_secret = Some(b"opaque-blob".to_vec());
        has_secret_not_enabled.mfa_secret_kek_id = Some(1);
        assert!(!user_has_mfa_enabled(&has_secret_not_enabled));

        let mut both = base_user.clone();
        both.mfa_enabled = true;
        both.mfa_secret = Some(b"opaque-blob".to_vec());
        both.mfa_secret_kek_id = Some(1);
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
    fn totp_replay_key_is_deterministic() {
        let user = Uuid::parse_str("0190a1b2-c3d4-7e80-9abc-def012345678").unwrap();
        let a = totp_replay_key(&user, "123456");
        let b = totp_replay_key(&user, "123456");
        assert_eq!(a, b);
        assert!(a.starts_with(&format!("totp_used:{user}:")));
        // 64-hex-character SHA-256 digest after the second colon.
        let hex = a.rsplit(':').next().unwrap();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn totp_replay_key_differs_per_token() {
        let user = Uuid::parse_str("0190a1b2-c3d4-7e80-9abc-def012345678").unwrap();
        let a = totp_replay_key(&user, "123456");
        let b = totp_replay_key(&user, "654321");
        assert_ne!(a, b);
    }

    #[test]
    fn totp_replay_key_differs_per_user() {
        let user_a = Uuid::parse_str("0190a1b2-c3d4-7e80-9abc-def012345678").unwrap();
        let user_b = Uuid::parse_str("0190a1b2-c3d4-7e80-9abc-def0123456ff").unwrap();
        assert_ne!(
            totp_replay_key(&user_a, "123456"),
            totp_replay_key(&user_b, "123456"),
        );
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

    // ---- Recovery code hash format dispatch -----------------------
    //
    // Migration property: verify_recovery_code_hash must accept both
    // argon2id (new) and bcrypt (pre-migration) hashes so existing
    // user codes keep working until consumed.

    #[test]
    fn verify_dispatches_argon2id_hash() {
        // Generate an argon2id hash inline and verify against it.
        let plaintext = "TEST1234ab";
        let salt = SaltString::generate(&mut rand::rngs::OsRng);
        let hash = Argon2::default()
            .hash_password(plaintext.as_bytes(), &salt)
            .unwrap()
            .to_string();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_recovery_code_hash(plaintext, &hash));
        assert!(!verify_recovery_code_hash("wrong-code", &hash));
    }

    #[test]
    fn verify_dispatches_legacy_bcrypt_hash() {
        // A bcrypt hash with `$2b$` prefix; verified via the legacy
        // path. Use a low cost to keep the test fast.
        let plaintext = "LEGACY12";
        let hash = bcrypt::hash(plaintext, 4).expect("bcrypt hash failed");
        assert!(hash.starts_with("$2b$"));
        assert!(verify_recovery_code_hash(plaintext, &hash));
        assert!(!verify_recovery_code_hash("wrong-code", &hash));
    }

    #[test]
    fn verify_rejects_corrupt_hash_string_without_panic() {
        // A malformed hash string must not panic the verify loop —
        // returning false keeps the constant-time guarantee.
        assert!(!verify_recovery_code_hash("anything", "not-a-real-hash"));
        assert!(!verify_recovery_code_hash(
            "anything",
            "$argon2id$malformed"
        ));
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
                tracing::warn!(
                    "Denying MFA attempt due to rate limit check failure (fail-closed mode)"
                );
                false
            } else {
                tracing::warn!("Allowing MFA attempt due to rate limit check failure (fail-open mode in non-production)");
                true
            }
        }
    }
}
