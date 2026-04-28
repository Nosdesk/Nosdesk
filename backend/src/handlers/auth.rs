use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use bcrypt::verify;
use serde_json::json;
use uuid::Uuid;
use tracing::{debug, info, warn, error};


use crate::db::DbConnection;
use crate::handlers::helpers;
use crate::models::{
    LoginRequest, PasswordChangeRequest,
    UserRegistration, UserResponse
};
use crate::repository;
use crate::utils::{self, ValidationError, parse_uuid};
use crate::utils::auth::{hash_password, validate_password};
use crate::utils::mfa;
use crate::utils::rate_limit::{RateLimiter, get_redis_url};

// Import JWT utilities
use crate::utils::jwt::{JwtUtils, helpers as jwt_helpers};

/// Helper function to get password hash from user_auth_identities for local auth
fn get_local_password_hash(user_uuid: &Uuid, conn: &mut DbConnection) -> Result<String, String> {
    use diesel::prelude::*;
    use crate::schema::user_auth_identities;

    let password_hash: Option<String> = user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(user_uuid))
        .filter(user_auth_identities::provider_type.eq("local"))
        .select(user_auth_identities::password_hash)
        .first::<Option<String>>(conn)
        .optional()
        .map_err(|e| format!("Database error: {e}"))?
        .flatten();

    password_hash.ok_or_else(|| "No local password found for this user".to_string())
}

/// Helper function to update password hash in user_auth_identities for local auth
fn update_local_password_hash(user_uuid: &Uuid, new_password_hash: &str, conn: &mut DbConnection) -> Result<(), diesel::result::Error> {
    use diesel::prelude::*;
    use crate::schema::user_auth_identities;

    diesel::update(
        user_auth_identities::table
            .filter(user_auth_identities::user_uuid.eq(user_uuid))
            .filter(user_auth_identities::provider_type.eq("local"))
    )
    .set(user_auth_identities::password_hash.eq(Some(new_password_hash)))
    .execute(conn)?;

    Ok(())
}



/// Convert ValidationError to HTTP response
impl From<ValidationError> for HttpResponse {
    fn from(error: ValidationError) -> Self {
        match error {
            ValidationError::InvalidUuid(_) => HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": error.to_string()
            })),
            ValidationError::InvalidRole(_) => HttpResponse::BadRequest().json(json!({
                "status": "error", 
                "message": error.to_string()
            })),
            ValidationError::ValidationFailed(msg) => HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": msg
            })),
        }
    }
}

// JWT token creation functions moved to jwt utils module



/// Helper function to log password change security event
async fn log_password_change_event(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::security_events::{record_security_event, SecurityEventInput};

    record_security_event(
        conn,
        SecurityEventInput {
            user_uuid: *user_uuid,
            event_type: "password_changed",
            severity: "info",
            details: Some(json!({
                "action": "password_change",
                "success": true
            })),
            // Handler doesn't have the request in scope; this call site
            // pre-dates the shared helper and keeps its previous "no IP"
            // behavior. If we want IP here later, wire the request in.
            request: None,
            session_id: None,
        },
    )?;

    Ok(())
}

/// Parse a device name from a user-agent string.
fn parse_device_name(ua: &str) -> &'static str {
    if ua.contains("iPhone") { "iPhone" }
    else if ua.contains("iPad") { "iPad" }
    else if ua.contains("Android") { "Android Device" }
    else if ua.contains("Macintosh") || ua.contains("Mac OS") { "Mac" }
    else if ua.contains("Windows") { "Windows PC" }
    else if ua.contains("Linux") { "Linux" }
    else { "Unknown Device" }
}

/// Create a session record from an HTTP request. Returns the ActiveSession with its DB-generated session_id.
pub fn create_session_record(
    user_uuid: &Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<crate::models::ActiveSession, diesel::result::Error> {
    let ip_address = request.peer_addr()
        .and_then(|addr| addr.ip().to_string().parse().ok());

    let user_agent = request.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let device_name = user_agent.as_deref().map(|ua| parse_device_name(ua).to_string());

    let new_session = crate::models::NewActiveSession {
        user_uuid: *user_uuid,
        device_name,
        ip_address,
        user_agent,
        location: None,
        expires_at: chrono::Utc::now().naive_utc() + chrono::Duration::days(7),
        is_current: true,
    };

    crate::repository::active_sessions::create_session(conn, new_session)
}

/// Create a session + token pair, returning an HttpResponse with auth cookies set.
/// Shared by all login flows that use `create_login_response`.
pub(crate) fn complete_login(
    user: crate::models::User,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> HttpResponse {
    let user_uuid = user.uuid;

    let session = match create_session_record(&user_uuid, request, conn) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create session for user {}: {}", user_uuid, e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create authentication session"
            }));
        }
    };
    let family_id = Uuid::new_v4();

    match jwt_helpers::create_login_response(user, &session.session_id, &family_id, conn) {
        Ok((response, tokens)) => build_auth_cookie_response(response, &tokens),
        Err(error_response) => error_response,
    }
}

/// Create a session + MFA token pair, returning an HttpResponse with auth cookies set.
/// Shared by MFA and recovery login flows.
fn complete_mfa_login(
    user: crate::models::User,
    backup_code_used: bool,
    requires_regeneration: bool,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> HttpResponse {
    let user_uuid = user.uuid;

    let session = match create_session_record(&user_uuid, request, conn) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create session for user {}: {}", user_uuid, e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create authentication session"
            }));
        }
    };
    let family_id = Uuid::new_v4();

    match jwt_helpers::create_mfa_login_response(
        user, backup_code_used, requires_regeneration,
        &session.session_id, &family_id, conn,
    ) {
        Ok((response, tokens)) => build_auth_cookie_response(response, &tokens),
        Err(error_response) => error_response,
    }
}

/// Attach auth cookies to an HTTP response.
pub(crate) fn build_auth_cookie_response(
    body: impl serde::Serialize,
    tokens: &jwt_helpers::LoginTokens,
) -> HttpResponse {
    HttpResponse::Ok()
        .cookie(crate::utils::cookies::create_access_token_cookie(&tokens.access_token))
        .cookie(crate::utils::cookies::create_refresh_token_cookie(&tokens.refresh_token))
        .cookie(crate::utils::cookies::create_csrf_token_cookie(&tokens.csrf_token))
        .json(body)
}

// Account lockout configuration (IP rate limiting handled by middleware)
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECONDS: u64 = 900; // 15 minutes

// Authentication handlers
pub async fn login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<LoginRequest>,
    request: HttpRequest,
) -> impl Responder {
    let redis_url = get_redis_url();
    let lockout_key = RateLimiter::login_attempt_key(&login_data.email);

    // Check if account is locked before any validation
    let is_production = std::env::var("ENVIRONMENT")
        .map(|v| v.to_lowercase() == "production")
        .unwrap_or(false);

    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %login_data.email, remaining_seconds, "Login attempt on locked account");
            return HttpResponse::TooManyRequests().json(json!({
                "status": "error",
                "message": format!("Account temporarily locked. Try again in {} minutes.", (remaining_seconds / 60) + 1),
                "retry_after": remaining_seconds
            }));
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            error!(error = %e, "Redis error checking account lockout");
            if is_production {
                // Fail closed in production - deny login if we can't verify lockout status
                return HttpResponse::ServiceUnavailable().json(json!({
                    "status": "error",
                    "message": "Authentication service temporarily unavailable. Please try again."
                }));
            }
            // Fail open in development for convenience
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email
    let user = match repository::get_user_by_email(&login_data.email, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(error = ?e, "Error finding user by email");
            // Record failed attempt even for non-existent users (prevents enumeration)
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Get password hash from user_auth_identities for local authentication
    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            warn!(user_uuid = %user.uuid, "No local password found for user");
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Verify password
    let password_matches = match verify(&login_data.password, &password_hash) {
        Ok(matches) => matches,
        Err(_) => false,
    };

    if !password_matches {
        // Record failed attempt
        match RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await {
            Ok(attempts) => {
                let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                if remaining == 0 {
                    warn!(email = %login_data.email, "Account locked after {} failed attempts", MAX_LOGIN_ATTEMPTS);
                    return HttpResponse::TooManyRequests().json(json!({
                        "status": "error",
                        "message": format!("Account locked after too many failed attempts. Try again in {} minutes.", LOCKOUT_DURATION_SECONDS / 60),
                        "retry_after": LOCKOUT_DURATION_SECONDS
                    }));
                }
                debug!(email = %login_data.email, attempts, remaining, "Failed login attempt");
            }
            Err(e) => warn!(error = %e, "Failed to record login attempt"),
        }
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
    }

    // Clear failed attempts on successful password verification
    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful auth");
    }

    // Check if user has TOTP MFA enabled - if so, require TOTP verification
    if mfa::user_has_mfa_enabled(&user) {
        let response = jwt_helpers::create_mfa_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    // Check if user has passkeys registered - require passkey verification
    if mfa::user_has_passkeys(&user) {
        let response = jwt_helpers::create_passkey_mfa_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    // Check MFA policy enforcement (for users without MFA enabled)
    if let Err(_policy_error) = mfa::validate_mfa_policy(&user).await {
        // Instead of blocking, offer MFA setup for users who need it
        let response = jwt_helpers::create_mfa_setup_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    complete_login(user, &request, &mut conn)
}

/// MFA Login - Verify MFA token and complete login
pub async fn mfa_login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<crate::models::MfaLoginRequest>,
    request: actix_web::HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email (same as regular login)
    let user = match repository::get_user_by_email(&login_data.email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Get password hash from user_auth_identities for local authentication
    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    let password_matches = match verify(&login_data.password, &password_hash) {
        Ok(matches) => matches,
        Err(_) => false,
    };

    if !password_matches {
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
    }

    // Check that user actually has MFA enabled
    if !mfa::user_has_mfa_enabled(&user) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA is not enabled for this account"
        }));
    }

    // Check rate limiting for MFA attempts
    if !mfa::check_mfa_rate_limit(&user.uuid).await {
        return HttpResponse::TooManyRequests().json(json!({
            "status": "error",
            "message": "Too many MFA attempts. Please try again later."
        }));
    }

    // Verify MFA token (TOTP or backup code)
    let mfa_result = match mfa::verify_mfa_token(&user.uuid, &login_data.mfa_token, &mut conn).await {
        Ok(result) => result,
        Err(e) => {
            // Log failed MFA attempt for security monitoring
            mfa::log_mfa_attempt(&user.uuid, false, "login", &request).await;
            
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": format!("MFA verification failed: {}", e)
            }));
        }
    };

    if !mfa_result.is_valid {
        // Log failed MFA attempt
        mfa::log_mfa_attempt(&user.uuid, false, "login", &request).await;
        
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid MFA token"
        }));
    }

    // Log successful MFA attempt
    mfa::log_mfa_attempt(&user.uuid, true, "login", &request).await;

    complete_mfa_login(
        user,
        mfa_result.backup_code_used.is_some(),
        mfa_result.requires_backup_code_regeneration,
        &request,
        &mut conn,
    )
}

/// Recovery code login - for users with passkey MFA who can't use their passkey
/// Accepts email + password + recovery_code, verifies backup code directly
pub async fn recovery_login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<crate::models::RecoveryLoginRequest>,
    request: actix_web::HttpRequest,
) -> impl Responder {
    let redis_url = get_redis_url();
    let lockout_key = RateLimiter::login_attempt_key(&login_data.email);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %login_data.email, remaining_seconds, "Recovery login attempt on locked account");
            return HttpResponse::TooManyRequests().json(json!({
                "status": "error",
                "message": format!("Account temporarily locked. Try again in {} minutes.", (remaining_seconds / 60) + 1),
                "retry_after": remaining_seconds
            }));
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            error!(error = %e, "Redis error checking account lockout for recovery login");
            let is_production = std::env::var("ENVIRONMENT")
                .map(|v| v.to_lowercase() == "production")
                .unwrap_or(false);
            if is_production {
                return HttpResponse::ServiceUnavailable().json(json!({
                    "status": "error",
                    "message": "Authentication service temporarily unavailable. Please try again."
                }));
            }
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email
    let user = match repository::get_user_by_email(&login_data.email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Verify password
    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    let password_matches = match verify(&login_data.password, &password_hash) {
        Ok(matches) => matches,
        Err(_) => false,
    };

    if !password_matches {
        let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
    }

    // Clear failed attempts on successful password verification
    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful recovery auth");
    }

    // Verify the user actually has passkeys or MFA (this endpoint is for recovery)
    if !mfa::user_has_passkeys(&user) && !mfa::user_has_mfa_enabled(&user) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "No MFA configured for this account"
        }));
    }

    // Check rate limiting for MFA/recovery attempts
    if !mfa::check_mfa_rate_limit(&user.uuid).await {
        return HttpResponse::TooManyRequests().json(json!({
            "status": "error",
            "message": "Too many recovery attempts. Please try again later."
        }));
    }

    // Verify recovery code directly (bypasses TOTP check)
    let result = match mfa::verify_backup_code(&user.uuid, &login_data.recovery_code.trim(), &mut conn).await {
        Ok(result) => result,
        Err(_) => {
            mfa::log_mfa_attempt(&user.uuid, false, "recovery_login", &request).await;
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid recovery code"
            }));
        }
    };

    if !result.is_valid {
        mfa::log_mfa_attempt(&user.uuid, false, "recovery_login", &request).await;
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid recovery code"
        }));
    }

    // Log successful recovery
    mfa::log_mfa_attempt(&user.uuid, true, "recovery_login", &request).await;

    info!(user_uuid = %user.uuid, "Recovery code login successful");

    complete_mfa_login(
        user,
        result.backup_code_used.is_some(),
        result.requires_backup_code_regeneration,
        &request,
        &mut conn,
    )
}

/// Logout endpoint - revokes session from DB and clears cookies
pub async fn logout(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    use crate::utils::cookies::{delete_access_token_cookie, delete_refresh_token_cookie, delete_csrf_token_cookie};

    // Best-effort session revocation — CASCADE handles linked refresh_tokens
    if let (Ok(claims), Ok(mut conn)) = (JwtUtils::extract_claims(&req), helpers::db_conn(&db_pool)) {
        if let Some(sid) = claims.session_uuid() {
            match crate::repository::active_sessions::revoke_session_by_uuid(&mut conn, &sid) {
                Ok(n) => tracing::info!("Logout: revoked {n} session(s) for sid {sid}"),
                Err(e) => tracing::warn!("Logout: failed to revoke session {sid}: {e}"),
            }
        }
    }

    HttpResponse::Ok()
        .cookie(delete_access_token_cookie())
        .cookie(delete_refresh_token_cookie())
        .cookie(delete_csrf_token_cookie())
        .json(json!({
            "success": true,
            "message": "Logged out successfully"
        }))
}

pub async fn register(
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<std::sync::Arc<crate::services::search::SearchService>>,
    user_data: web::Json<UserRegistration>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Comprehensive input validation using our validation utilities
    let mut validation_errors = Vec::new();

    // Validate name
    let trimmed_name = user_data.name.trim();
    if trimmed_name.is_empty() {
        validation_errors.push("name: Name is required".to_string());
    } else if trimmed_name.len() > 255 {
        validation_errors.push("name: Name must be less than 255 characters".to_string());
    }

    // Validate email
    let trimmed_email = user_data.email.trim();
    if trimmed_email.is_empty() {
        validation_errors.push("email: Email is required".to_string());
    } else if trimmed_email.len() > 255 {
        validation_errors.push("email: Email must be less than 255 characters".to_string());
    } else if !trimmed_email.contains('@') || !trimmed_email.contains('.') {
        validation_errors.push("email: Invalid email format".to_string());
    }

    // Validate password using centralized validation
    let password_validation = validate_password(&user_data.password);
    if !password_validation.valid {
        for error in password_validation.errors {
            validation_errors.push(format!("password: {error}"));
        }
    }

    // Validate role
    let trimmed_role = user_data.role.trim().to_lowercase();
    if !["admin", "technician", "user"].contains(&trimmed_role.as_str()) {
        validation_errors.push("role: Invalid role. Must be 'admin', 'technician', or 'user'".to_string());
    }

    // Validate optional fields
    if let Some(ref pronouns) = user_data.pronouns {
        if pronouns.len() > 50 {
            validation_errors.push("pronouns: Pronouns must be less than 50 characters".to_string());
        }
    }

    if let Some(ref avatar_url) = user_data.avatar_url {
        if avatar_url.len() > 500 {
            validation_errors.push("avatar_url: URL must be less than 500 characters".to_string());
        }
    }

    if let Some(ref banner_url) = user_data.banner_url {
        if banner_url.len() > 500 {
            validation_errors.push("banner_url: URL must be less than 500 characters".to_string());
        }
    }

    if let Some(ref avatar_thumb) = user_data.avatar_thumb {
        if avatar_thumb.len() > 500 {
            validation_errors.push("avatar_thumb: URL must be less than 500 characters".to_string());
        }
    }

    // If there are validation errors, return them
    if !validation_errors.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Validation failed",
            "errors": validation_errors
        }));
    }

    // Check if user with this email already exists
    if repository::get_user_by_email(&user_data.email, &mut conn).is_ok() {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "User with this email already exists"
        }));
    }

    // Hash the password
    let password_hash = match hash_password(&user_data.password) {
        Ok(hash) => hash,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error hashing password"
        })),
    };

    // Generate a UUID if not provided
    let user_uuid = Uuid::now_v7();

    // Parse role from string to enum
    let user_role = match utils::parse_role(&user_data.role) {
        Ok(role) => role,
        Err(e) => return e.into(),
    };

    // Create new user using builder pattern with normalized data
    let (normalized_name, normalized_email) = utils::normalization::normalize_user_data(&user_data.name, &user_data.email);
    let (new_user, email) = utils::NewUserBuilder::new(normalized_name, normalized_email.clone(), user_role)
        .with_uuid(user_uuid)
        .with_pronouns(utils::normalization::normalize_optional_string(user_data.pronouns.as_deref()))
        .with_avatar(
            utils::normalization::normalize_optional_string(user_data.avatar_url.as_deref()),
            utils::normalization::normalize_optional_string(user_data.avatar_thumb.as_deref())
        )
        .with_banner(utils::normalization::normalize_optional_string(user_data.banner_url.as_deref()))
        .build_with_email();

    // Save user to database with email (atomically creates both user and email entry)
    match repository::user_helpers::create_user_with_email(new_user, email, true, Some("manual".to_string()), &mut conn, Some(search_service.get_ref())) {
        Ok((created_user, _email_entry)) => {
            // Create local auth identity with password hash
            use diesel::prelude::*;
            use crate::schema::user_auth_identities;

            #[derive(diesel::Insertable)]
            #[diesel(table_name = user_auth_identities)]
            struct NewLocalAuthIdentity {
                user_uuid: Uuid,
                provider_type: String,
                external_id: String,
                email: Option<String>,
                password_hash: Option<String>,
            }

            let auth_identity = NewLocalAuthIdentity {
                user_uuid: created_user.uuid,
                provider_type: "local".to_string(),
                external_id: normalized_email.clone(),
                email: Some(normalized_email.clone()),
                password_hash: Some(password_hash),
            };

            if let Err(e) = diesel::insert_into(user_auth_identities::table)
                .values(&auth_identity)
                .execute(&mut conn) {
                error!(error = ?e, "Error creating auth identity");
                // Rollback by deleting the user
                let _ = repository::users::delete_user(&created_user.uuid, &mut conn, Some(search_service.get_ref()));
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error creating user authentication"
                }));
            }

            info!(user_name = %created_user.name, user_uuid = %created_user.uuid, "New user registered successfully");
            let response = repository::user_helpers::get_user_with_primary_email(created_user, &mut conn);
            HttpResponse::Created().json(response)
        },
        Err(e) => {
            error!(error = ?e, "Error creating user");
            
            // Provide more specific error messages for common issues
            let error_message = if format!("{e:?}").contains("duplicate") || format!("{e:?}").contains("unique") {
                "Email address already exists in the system"
            } else {
                "Error creating user"
            };
            
            HttpResponse::InternalServerError().json(json!({
            "status": "error",
                "message": error_message
            }))
        },
    }
}

pub async fn change_password(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    password_data: web::Json<PasswordChangeRequest>,
) -> impl Responder {
    // Validate new password first
    if password_data.new_password.len() < 8 {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "New password validation failed: Password must be at least 8 characters long"
        }));
    } else if password_data.new_password.len() > 128 {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "New password validation failed: Password must be less than 128 characters"
        }));
    }

    // Get database connection
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => {
            // Get current password hash from user_auth_identities
            let current_password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
                Ok(hash) => hash,
                Err(_) => {
                    warn!(user_uuid = %user.uuid, "No local password found for user");
                    return HttpResponse::BadRequest().json(json!({
                        "status": "error",
                        "message": "No local password found for this user"
                    }));
                }
            };

            // Verify current password
            let password_matches = match verify(&password_data.current_password, &current_password_hash) {
                Ok(matches) => matches,
                Err(_) => {
                    error!("Error verifying current password during password change");
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error verifying password"
                    }));
                }
            };

            if !password_matches {
                return HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Current password is incorrect"
                }));
            }

            // Check if new password is the same as current password
            if verify(&password_data.new_password, &current_password_hash).unwrap_or(false) {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "New password must be different from current password"
                }));
            }

            // Hash the new password
            let new_password_hash = match hash_password(&password_data.new_password) {
                Ok(hash) => hash,
                Err(_) => return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error hashing new password"
                })),
            };

            // Update the user's password hash in user_auth_identities and password_changed_at timestamp in users
            use diesel::prelude::*;
            let now = chrono::Utc::now().naive_utc();

            // Update password hash in user_auth_identities
            if let Err(e) = update_local_password_hash(&user.uuid, &new_password_hash, &mut conn) {
                error!(error = ?e, "Error updating password hash");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error updating password"
                }));
            }

            // Update password_changed_at timestamp in users table
            match diesel::update(crate::schema::users::table.find(&user.uuid))
                .set(crate::schema::users::password_changed_at.eq(now))
                .execute(&mut conn) {
                Ok(_) => {
                    info!(user_name = %user.name, "Password changed successfully");

                    // Log security event for password change
                    if let Err(e) = log_password_change_event(&user.uuid, &mut conn).await {
                        tracing::warn!("Failed to log password change event: {}", e);
                        // Don't fail the password change if logging fails
                    }

                    // Revoke all other sessions for security (defense in depth)
                    if let Some(claims) = req.extensions().get::<crate::models::Claims>() {
                        if let Some(sid) = claims.session_uuid() {
                            match crate::repository::active_sessions::revoke_other_sessions_by_uuid(
                                &mut conn, &user.uuid, &sid,
                            ) {
                                Ok(n) if n > 0 => info!(revoked_count = n, user_name = %user.name, "Revoked other sessions after password change"),
                                Ok(_) => {},
                                Err(e) => tracing::warn!("Failed to revoke other sessions after password change for user {}: {e}", user.uuid),
                            }
                        }
                    }

                    HttpResponse::Ok().json(json!({
                        "status": "success",
                        "message": "Password changed successfully"
                    }))
                },
                Err(e) => {
                    error!(error = ?e, "Error updating password");
                    HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error updating password"
                    }))
                }
            }
        },
        Err(_) => HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    }
}

// Handler to get current authenticated user
pub async fn get_current_user(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID"
        })),
    };

    // Get user from database using claims
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    HttpResponse::Ok().json(UserResponse::from(user))
}

/// Check if system requires initial setup
pub async fn check_setup_status(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    // Log access for audit purposes
    let client_ip = req.peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    
    debug!("Setup status check from IP: {}", client_ip);
    
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::count_users(&mut conn) {
        Ok(user_count) => {
            // Check if Microsoft auth is configured via environment variables
            let microsoft_auth_enabled = std::env::var("MICROSOFT_CLIENT_ID").is_ok()
                && std::env::var("MICROSOFT_TENANT_ID").is_ok()
                && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok();

            // Check if OIDC is configured
            let oidc_enabled = crate::config_utils::is_oidc_enabled();
            let oidc_display_name = if oidc_enabled {
                Some(crate::oidc::get_display_name_cached())
            } else {
                None
            };

            let response = crate::models::OnboardingStatus {
                requires_setup: user_count == 0,
                user_count,
                microsoft_auth_enabled,
                oidc_enabled,
                oidc_display_name,
            };

            // Security headers now applied globally by SecurityHeaders middleware
            HttpResponse::Ok().json(response)
        },
        Err(e) => {
            error!("Error counting users: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to check setup status"
            }))
        }
    }
}

pub async fn setup_initial_admin(
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<std::sync::Arc<crate::services::search::SearchService>>,
    admin_data: web::Json<crate::models::AdminSetupRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Security check: Only allow setup if no users exist
    match repository::count_users(&mut conn) {
        Ok(user_count) => {
            if user_count > 0 {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Setup has already been completed. Users already exist in the system."
                }));
            }
        },
        Err(e) => {
            error!(error = ?e, "Error counting users");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to verify setup status"
            }));
        }
    }

    // Comprehensive input validation using our validation utilities
    let mut validation_errors = Vec::new();

    // Validate name
    let trimmed_name = admin_data.name.trim();
    if trimmed_name.is_empty() {
        validation_errors.push("name: Name is required".to_string());
    } else if trimmed_name.len() > 255 {
        validation_errors.push("name: Name must be less than 255 characters".to_string());
    }

    // Validate email
    let trimmed_email = admin_data.email.trim();
    if trimmed_email.is_empty() {
        validation_errors.push("email: Email is required".to_string());
    } else if trimmed_email.len() > 255 {
        validation_errors.push("email: Email must be less than 255 characters".to_string());
    } else if !trimmed_email.contains('@') || !trimmed_email.contains('.') {
        validation_errors.push("email: Invalid email format".to_string());
    }

    // Validate password
    if admin_data.password.len() < 8 {
        validation_errors.push("password: Password must be at least 8 characters long".to_string());
    } else if admin_data.password.len() > 128 {
        validation_errors.push("password: Password must be less than 128 characters".to_string());
    }

    // If there are validation errors, return them
    if !validation_errors.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Validation failed",
            "errors": validation_errors
        }));
    }

    // Hash the password
    let password_hash = match hash_password(&admin_data.password) {
        Ok(hash) => hash,
        Err(e) => {
            error!(error = ?e, "Error hashing password");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error processing password"
            }));
        }
    };

    // Generate UUID for the admin user
    let _user_uuid = Uuid::now_v7();

    // Create the admin user using convenience function with email
    let (normalized_name, normalized_email) = utils::normalization::normalize_user_data(&admin_data.name, &admin_data.email);
    let (new_user, email) = utils::NewUserBuilder::admin_user(
        normalized_name,
        normalized_email.clone()
    ).build_with_email();

    match repository::user_helpers::create_user_with_email(new_user, email, true, Some("manual".to_string()), &mut conn, Some(search_service.get_ref())) {
        Ok((created_user, _email_entry)) => {
            // Create local auth identity with password hash
            use diesel::prelude::*;
            use crate::schema::user_auth_identities;

            #[derive(diesel::Insertable)]
            #[diesel(table_name = user_auth_identities)]
            struct NewLocalAuthIdentity {
                user_uuid: Uuid,
                provider_type: String,
                external_id: String,
                email: Option<String>,
                password_hash: Option<String>,
            }

            let auth_identity = NewLocalAuthIdentity {
                user_uuid: created_user.uuid,
                provider_type: "local".to_string(),
                external_id: normalized_email.clone(),
                email: Some(normalized_email.clone()),
                password_hash: Some(password_hash),
            };

            if let Err(e) = diesel::insert_into(user_auth_identities::table)
                .values(&auth_identity)
                .execute(&mut conn) {
                error!(error = ?e, "Error creating auth identity");
                // Rollback by deleting the user
                let _ = repository::users::delete_user(&created_user.uuid, &mut conn, Some(search_service.get_ref()));
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error creating user authentication"
                }));
            }

            info!(user_name = %created_user.name, "Initial admin user created successfully");

            let response = crate::models::AdminSetupResponse {
                success: true,
                message: "Initial admin user created successfully".to_string(),
                user: Some(repository::user_helpers::get_user_with_primary_email(created_user, &mut conn)),
            };
            HttpResponse::Created().json(response)
        },
        Err(e) => {
            error!(error = ?e, "Error creating admin user");
            
            // Provide more specific error messages for common issues
            let error_message = if format!("{e:?}").contains("duplicate") || format!("{e:?}").contains("unique") {
                "Email address already exists in the system"
            } else {
                "Error creating admin user"
            };
            
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": error_message
            }))
        }
    }
}

// === MFA (Multi-Factor Authentication) Handlers ===

/// MFA Setup - Generate secret and QR code
pub async fn mfa_setup(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID"
        })),
    };

    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Check if MFA_ENCRYPTION_KEY is set - use the util function
    if mfa::encrypt_mfa_secret("test").is_err() {
        tracing::error!("MFA setup failed: encryption key not configured");
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "MFA not properly configured on server"
        }));
    }

    // Generate new TOTP secret (backup codes will be generated after successful verification)
    let secret = mfa::generate_totp_secret();

    // Get user's primary email for QR code
    let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
        .unwrap_or_else(|| format!("user-{}", user.uuid));

    // Generate QR code
    let qr_result = match mfa::generate_qr_code(secret.as_str(), &user_email, "Nosdesk") {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to generate QR code: {}", e);
            return HttpResponse::InternalServerError().json(json!({
            "status": "error",
                "message": "Failed to generate QR code"
            }));
        }
    };

    tracing::info!("MFA setup initiated for user: {}", user.uuid);

    let response = crate::models::MfaSetupResponse {
        secret: secret.as_str().to_string(),
        qr_code: qr_result.svg_data_url,
        // Do not generate or return backup codes until after verification completes
        backup_codes: vec![],
        qr_matrix: Some(qr_result.matrix),
    };

    HttpResponse::Ok().json(response)
}

/// MFA Verify Setup - Verify the TOTP token during setup
pub async fn mfa_verify_setup(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaVerifySetupRequest>,
) -> impl Responder {
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Verify the TOTP token (timing-safe verification)
    if !mfa::verify_totp_token(&request.secret, &request.token) {
        tracing::warn!("MFA setup verification failed for user: {}", claims.sub);
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid verification code"
        }));
    }

    tracing::info!("MFA setup verification successful for user: {}", claims.sub);

    // Return success - backup codes will be generated after enabling
    let response = crate::models::MfaVerifySetupResponse {
        success: true,
        backup_codes: vec![],
    };

    HttpResponse::Ok().json(response)
}

/// MFA Enable - Enable MFA for the user
pub async fn mfa_enable(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaEnableRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    tracing::info!("Enabling MFA for user: {}", user_uuid);

    // Validate inputs securely
    let mfa_secret = match &request.secret {
        Some(secret) if !secret.is_empty() => secret,
        _ => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA secret is required"
        })),
    };

    // Generate backup codes on the server after successful verification

    // Final TOTP verification before enabling
    if !mfa::verify_totp_token(mfa_secret, &request.token) {
        tracing::warn!("MFA enable verification failed for user: {}", user_uuid);
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid verification code"
        }));
    }

    // Encrypt the MFA secret before storage
    let encrypted_secret = match mfa::encrypt_mfa_secret(mfa_secret) {
        Ok(encrypted) => encrypted,
        Err(e) => {
            tracing::error!("Failed to encrypt MFA secret: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to secure MFA data"
            }));
        }
    };

    // Generate backup codes now that verification succeeded
    let (backup_codes_plaintext, backup_codes_hashed) = mfa::generate_backup_codes_async().await;

    // Use pre-hashed backup codes from setup phase
    // Note: backup_codes from frontend are plaintext from setup, hashed here for storage
    // This maintains security while keeping the API simple
    
    let backup_codes_json = match serde_json::to_value(&backup_codes_hashed) {
        Ok(json) => json,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to serialize backup codes"
        })),
    };

    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(true),
        mfa_secret: Some(encrypted_secret),
        mfa_backup_codes: Some(backup_codes_json),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match repository::update_user_mfa(&user_uuid, mfa_update, &mut conn) {
        Ok(_) => {
            tracing::info!("MFA enabled successfully for user: {}", user_uuid);
            // Return plaintext backup codes so the client can display them once
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "MFA enabled successfully",
                "backup_codes": backup_codes_plaintext
            }))
        },
        Err(e) => {
            tracing::error!("Failed to enable MFA in database: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to enable MFA"
            }))
        }
    }
}

/// MFA Disable - Disable MFA for the user (requires full scope)
pub async fn mfa_disable(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaDisableRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Verify scope - must be "full" (MFA disable requires being fully authenticated)
    if claims.scope != "full" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "This action requires a full session"
        }));
    }

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    // Get user
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Always verify password for MFA disable (even with recovery token)
    // This prevents account takeover if recovery email is compromised
    let password_hash_str = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error reading password hash"
        })),
    };

    if !verify(&request.password, &password_hash_str).unwrap_or(false) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid password"
        }));
    }

    // Disable MFA
    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(false),
        mfa_secret: None, // Clear the secret
        mfa_backup_codes: Some(serde_json::Value::Null), // Clear backup codes
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match repository::update_user_mfa(&user_uuid, mfa_update, &mut conn) {
        Ok(_) => {
            tracing::info!("MFA disabled for user: {} (scope: {})", user_uuid, claims.scope);
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "MFA disabled successfully"
            }))
        },
        Err(e) => {
            error!(error = ?e, "Error disabling MFA");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to disable MFA"
            }))
        }
    }
}

/// MFA Regenerate Backup Codes - Generate new backup codes
pub async fn mfa_regenerate_backup_codes(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaRegenerateBackupCodesRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    // Get user to verify password
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Verify password
    let password_hash_str = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error reading password hash"
        })),
    };

    if !verify(&request.password, &password_hash_str).unwrap_or(false) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid password"
        }));
    }

    // Generate new backup codes
    let (backup_codes_plaintext, backup_codes_hashed) = mfa::generate_backup_codes_async().await;
    let backup_codes_json = serde_json::to_value(&backup_codes_hashed).unwrap();

    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: None, // Don't change MFA enabled status
        mfa_secret: None,  // Don't change secret
        mfa_backup_codes: Some(backup_codes_json),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match repository::update_user_mfa(&user_uuid, mfa_update, &mut conn) {
        Ok(_) => {
            let response = crate::models::MfaRegenerateBackupCodesResponse {
                backup_codes: backup_codes_plaintext,
            };
            HttpResponse::Ok().json(response)
        },
        Err(e) => {
            error!(error = ?e, "Error regenerating backup codes");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to regenerate backup codes"
            }))
        }
    }
}

/// MFA Status - Get current MFA status for the user
pub async fn mfa_status(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    // Get user MFA status
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Check if user has backup codes
    let has_backup_codes = user.mfa_backup_codes
        .as_ref()
        .and_then(|codes| codes.as_array())
        .map(|array| !array.is_empty())
        .unwrap_or(false);

    let response = crate::models::MfaStatusResponse {
        enabled: user.mfa_enabled,
        has_backup_codes,
    };

    HttpResponse::Ok().json(response)
}



/// MFA Setup for Login (Unauthenticated) - For users who need MFA to login but haven't set it up yet
pub async fn mfa_setup_login(
    db_pool: web::Data<crate::db::Pool>,
    request: web::Json<crate::models::MfaSetupLoginRequest>,
) -> impl Responder {
    let redis_url = get_redis_url();
    let email_lower = request.email.to_lowercase();
    let lockout_key = RateLimiter::login_attempt_key(&email_lower);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %email_lower, remaining_seconds, "MFA setup attempt on locked account");
            return HttpResponse::TooManyRequests().json(json!({
                "status": "error",
                "message": format!("Account temporarily locked. Try again in {} seconds.", remaining_seconds)
            }));
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            warn!("Failed to check account lockout: {:?}", e);
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email and verify password (same as login flow)
    let user = match repository::get_user_by_email(&email_lower, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            // Record failed attempt even for non-existent users (prevents enumeration)
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Get password hash from user_auth_identities for local authentication
    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    let password_matches = match verify(&request.password, &password_hash) {
        Ok(matches) => matches,
        Err(_) => false,
    };

    if !password_matches {
        // Record failed attempt
        match RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await {
            Ok(attempts) => {
                let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                if remaining == 0 {
                    warn!(email = %email_lower, "Account locked after failed MFA setup attempts");
                }
            }
            Err(e) => warn!("Failed to record failed attempt: {:?}", e),
        }
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
    }

    // Clear failed attempts on successful password verification
    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful MFA setup auth");
    }

    // Verify that user actually needs MFA setup (security check)
    if mfa::user_has_mfa_enabled(&user) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA is already enabled for this account"
        }));
    }

    // Verify that MFA is required for this user
    if let Ok(_) = mfa::validate_mfa_policy(&user).await {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA is not required for this account"
        }));
    }

    // Check if MFA_ENCRYPTION_KEY is set
    if mfa::encrypt_mfa_secret("test").is_err() {
        tracing::error!("MFA setup failed: encryption key not configured");
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "MFA not properly configured on server"
        }));
    }

    // Generate new TOTP secret (backup codes will be generated after verification)
    let secret = mfa::generate_totp_secret();

    // Get user's primary email for QR code
    let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
        .unwrap_or_else(|| format!("user-{}", user.uuid));

    // Generate QR code
    let qr_result = match mfa::generate_qr_code(secret.as_str(), &user_email, "Nosdesk") {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to generate QR code: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to generate QR code"
            }));
        }
    };

    tracing::info!("MFA setup initiated for user during login: {}", user.uuid);

    let response = crate::models::MfaSetupResponse {
        secret: secret.as_str().to_string(),
        qr_code: qr_result.svg_data_url,
        backup_codes: vec![],
        qr_matrix: Some(qr_result.matrix),
    };

    HttpResponse::Ok().json(response)
}

/// MFA Enable for Login (Unauthenticated) - Complete MFA setup and login
pub async fn mfa_enable_login(
    db_pool: web::Data<crate::db::Pool>,
    request: web::Json<crate::models::MfaEnableLoginRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let redis_url = get_redis_url();
    let email_lower = request.email.to_lowercase();
    let lockout_key = RateLimiter::login_attempt_key(&email_lower);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %email_lower, remaining_seconds, "MFA enable attempt on locked account");
            return HttpResponse::TooManyRequests().json(json!({
                "status": "error",
                "message": format!("Account temporarily locked. Try again in {} seconds.", remaining_seconds)
            }));
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            warn!("Failed to check account lockout: {:?}", e);
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email and verify password again (security)
    let user = match repository::get_user_by_email(&email_lower, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Get password hash from user_auth_identities for local authentication
    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await;
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    let password_matches = match verify(&request.password, &password_hash) {
        Ok(matches) => matches,
        Err(_) => false,
    };

    if !password_matches {
        match RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS).await {
            Ok(attempts) => {
                let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                if remaining == 0 {
                    warn!(email = %email_lower, "Account locked after failed MFA enable attempts");
                }
            }
            Err(e) => warn!("Failed to record failed attempt: {:?}", e),
        }
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
    }

    // Clear failed attempts on successful password verification
    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful MFA enable auth");
    }

    // Security checks - same as setup
    if mfa::user_has_mfa_enabled(&user) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA is already enabled for this account"
        }));
    }

    if let Ok(_) = mfa::validate_mfa_policy(&user).await {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA is not required for this account"
        }));
    }

    // Validate inputs securely
    let mfa_secret = match &request.secret {
        Some(secret) if !secret.is_empty() => secret,
        _ => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "MFA secret is required"
        })),
    };

    // Backup codes are generated after verification, not required in request

    // Final TOTP verification before enabling
    if !mfa::verify_totp_token(mfa_secret, &request.token) {
        tracing::warn!("MFA enable verification failed for user during login: {}", user.uuid);
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid verification code"
        }));
    }

    // Encrypt the MFA secret before storage
    let encrypted_secret = match mfa::encrypt_mfa_secret(mfa_secret) {
        Ok(encrypted) => encrypted,
        Err(e) => {
            tracing::error!("Failed to encrypt MFA secret: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to secure MFA data"
            }));
        }
    };

    // Generate backup codes now that verification succeeded
    let (backup_codes_plaintext, backup_codes_hashed) = mfa::generate_backup_codes_async().await;

    let backup_codes_json = match serde_json::to_value(&backup_codes_hashed) {
        Ok(json) => json,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to serialize backup codes"
        })),
    };

    // Enable MFA in database
    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(true),
        mfa_secret: Some(encrypted_secret),
        mfa_backup_codes: Some(backup_codes_json),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Store user UUID before moving
    let user_uuid = user.uuid;

    match repository::update_user_mfa(&user_uuid, mfa_update, &mut conn) {
        Ok(_) => {
            tracing::info!("MFA enabled successfully for user during login: {}", user_uuid);

            // Create session + tokens (same as login, but attach backup codes)
            let session = match create_session_record(&user_uuid, &http_request, &mut conn) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to create session for MFA enable login {}: {}", user_uuid, e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to create authentication session"
                    }));
                }
            };
            let family_id = Uuid::new_v4();

            match jwt_helpers::create_login_response(user, &session.session_id, &family_id, &mut conn) {
                Ok((mut response, tokens)) => {
                    response.backup_codes = Some(backup_codes_plaintext);
                    build_auth_cookie_response(response, &tokens)
                },
                Err(error_response) => error_response,
            }
        },
        Err(e) => {
            tracing::error!("Failed to enable MFA in database: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to enable MFA"
            }))
        }
    }
}

// === SESSION MANAGEMENT HANDLERS ===

/// Get all active sessions for the current user
pub async fn get_user_sessions(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    let current_sid = claims.session_uuid();

    // Get all sessions for the user
    let sessions = match crate::repository::active_sessions::get_user_sessions(&mut conn, &user_uuid) {
        Ok(sessions) => sessions,
        Err(e) => {
            tracing::error!("Failed to get user sessions: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to retrieve sessions"
            }));
        }
    };

    // Convert sessions to response format
    let session_responses: Vec<serde_json::Value> = sessions.into_iter().map(|session| {
        let is_current = current_sid.map_or(false, |sid| session.session_id == sid);
        json!({
            "id": session.id,
            "session_id": session.session_id.to_string(),
            "device_name": session.device_name,
            "ip_address": session.ip_address.map(|ip| ip.to_string()),
            "user_agent": session.user_agent,
            "location": session.location,
            "created_at": session.created_at,
            "last_active": session.last_active,
            "expires_at": session.expires_at,
            "is_current": is_current
        })
    }).collect();

    HttpResponse::Ok().json(json!({
        "status": "success",
        "sessions": session_responses
    }))
}

/// Revoke a specific session
pub async fn revoke_session(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<i32>,
) -> impl Responder {
    let session_id = path.into_inner();

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    // Verify the session belongs to this user before revoking
    match crate::repository::active_sessions::get_session_by_id(&mut conn, session_id) {
        Ok(session) if session.user_uuid == user_uuid => {
            // Session belongs to user, proceed with revocation
        },
        Ok(_) => {
            return HttpResponse::Forbidden().json(json!({
                "status": "error",
                "message": "You can only revoke your own sessions"
            }));
        },
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "Session not found"
            }));
        }
    }

    // Revoke the session
    match crate::repository::active_sessions::revoke_session(&mut conn, session_id) {
        Ok(count) if count > 0 => {
            tracing::info!("Session {} revoked for user {}", session_id, user_uuid);
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Session revoked successfully"
            }))
        },
        Ok(_) => HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "Session not found"
        })),
        Err(e) => {
            tracing::error!("Failed to revoke session: {}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to revoke session"
            }))
        }
    }
}

/// Revoke all other sessions (keep current session active)
pub async fn revoke_all_other_sessions(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };

    // Revoke all other sessions (if we can't identify current session, revoke everything)
    let revoke_result = match claims.session_uuid() {
        Some(sid) => crate::repository::active_sessions::revoke_other_sessions_by_uuid(
            &mut conn, &user_uuid, &sid
        ),
        None => crate::repository::active_sessions::revoke_other_sessions(
            &mut conn, &user_uuid, None
        ),
    };

    match revoke_result {
        Ok(revoked_count) => {
            tracing::info!("Revoked {} other session(s) for user {}", revoked_count, user_uuid);
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": format!("Successfully revoked {} session(s)", revoked_count),
                "revoked_count": revoked_count
            }))
        },
        Err(e) => {
            tracing::error!("Failed to revoke other sessions: {}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to revoke sessions"
            }))
        }
    }
}

// === TOKEN REFRESH HANDLER ===

/// Refresh access token using refresh token (with reuse detection + grace period)
pub async fn refresh_token(
    db_pool: web::Data<crate::db::Pool>,
    request: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // 1. Read refresh cookie → hash → lookup
    let refresh_cookie = match request.cookie(crate::utils::cookies::REFRESH_TOKEN_COOKIE) {
        Some(cookie) => cookie.value().to_string(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Refresh token not found"
            }));
        }
    };

    let token_hash = JwtUtils::hash_refresh_token(&refresh_cookie);

    let old_token = match crate::repository::refresh_tokens::get_refresh_token_by_hash(&mut conn, &token_hash) {
        Ok(token) => token,
        Err(_) => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid or expired refresh token"
            }));
        }
    };

    // 2. Check if revoked
    if old_token.revoked_at.is_some() {
        tracing::warn!("Revoked refresh token presented, family={}", old_token.family_id);
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Refresh token has been revoked"
        }));
    }

    // 3. Reuse detection
    if old_token.is_used {
        let now = chrono::Utc::now().naive_utc();
        let within_grace = old_token.grace_expires_at
            .map_or(false, |grace| grace > now);

        if !within_grace {
            // Token reuse outside grace period — potential theft!
            tracing::warn!("Refresh token reuse detected outside grace period! Revoking family={}", old_token.family_id);
            let _ = crate::repository::refresh_tokens::revoke_token_family(&mut conn, &old_token.family_id);
            if let Some(sid) = old_token.session_id {
                let _ = crate::repository::active_sessions::revoke_session_by_uuid(&mut conn, &sid);
            }
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Token reuse detected — session revoked for security"
            }));
        }
        // Within grace period — allow (concurrent tab scenario)
        tracing::debug!("Refresh token reuse within grace period, family={}", old_token.family_id);
    }

    // 4. Get user
    let user = match repository::get_user_by_uuid(&old_token.user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };

    // 5. Determine session_id (from token, or create new session for tokens without one)
    let session_id = match old_token.session_id {
        Some(sid) => sid,
        None => {
            // Token created before this migration — create a new session
            match create_session_record(&user.uuid, &request, &mut conn) {
                Ok(session) => session.session_id,
                Err(e) => {
                    tracing::error!("Failed to create session during refresh: {}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to create session"
                    }));
                }
            }
        }
    };

    // 6. Generate new access JWT with same sid
    let new_access_token = match JwtUtils::create_token(&user, &session_id) {
        Ok(token) => token,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create access token"
            }));
        }
    };

    // 7. Generate new refresh token
    let new_refresh_raw = JwtUtils::generate_refresh_token();
    let new_refresh_hash = JwtUtils::hash_refresh_token(&new_refresh_raw);

    // 8. Mark old token used (if not already)
    if !old_token.is_used {
        let grace_until = chrono::Utc::now().naive_utc() + chrono::Duration::seconds(5);
        if let Err(e) = crate::repository::refresh_tokens::mark_token_used(
            &mut conn, &token_hash, &new_refresh_hash, grace_until,
        ) {
            tracing::error!("Failed to mark old refresh token as used: {}", e);
        }
    }

    // 9. Create new refresh token with same family_id and session_id
    let new_refresh_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
    let new_refresh_record = crate::models::NewRefreshToken {
        token_hash: new_refresh_hash,
        user_uuid: user.uuid,
        expires_at: new_refresh_expires,
        session_id: Some(session_id),
        family_id: old_token.family_id,
    };

    if let Err(e) = crate::repository::refresh_tokens::create_refresh_token(&mut conn, new_refresh_record) {
        tracing::error!("Failed to store new refresh token: {}", e);
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to create refresh token"
        }));
    }

    // 10. Update session activity
    let new_session_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
    if let Err(e) = crate::repository::active_sessions::update_session_activity(
        &mut conn, &session_id, new_session_expires,
    ) {
        tracing::warn!("Failed to update session activity: {}", e);
    }

    // 11. Return new tokens
    let new_csrf_token = crate::utils::csrf::generate_csrf_token();

    let response = crate::models::RefreshTokenResponse {
        success: true,
        csrf_token: new_csrf_token.clone(),
    };

    HttpResponse::Ok()
        .cookie(crate::utils::cookies::create_access_token_cookie(&new_access_token))
        .cookie(crate::utils::cookies::create_refresh_token_cookie(&new_refresh_raw))
        .cookie(crate::utils::cookies::create_csrf_token_cookie(&new_csrf_token))
        .json(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App, http::StatusCode};
    use crate::test_helpers::{setup_test_pool, create_test_claims, TestFixtures};
    use crate::models::UserRole;

    /// Helper to create a test app with auth routes
    fn test_app(pool: crate::db::Pool) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/setup/status", web::get().to(check_setup_status))
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register))
            .route("/me", web::get().to(get_current_user))
    }

    // =========================================================================
    // PUBLIC ENDPOINT TESTS (no auth required)
    // =========================================================================

    #[actix_web::test]
    async fn check_setup_status_returns_ok() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        let req = test::TestRequest::get()
            .uri("/setup/status")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);

        // Verify response structure
        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("requires_setup").is_some());
        assert!(json.get("user_count").is_some());
        assert!(json.get("microsoft_auth_enabled").is_some());
        assert!(json.get("oidc_enabled").is_some());
    }

    #[actix_web::test]
    async fn login_with_invalid_credentials_fails() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        // Use a unique email per run to avoid triggering the Redis rate limiter
        // across repeated test invocations.
        let unique_email = format!("nonexistent_{}@example.com", uuid::Uuid::new_v4());

        let login_request = serde_json::json!({
            "email": unique_email,
            "password": "wrongpassword"
        });

        let req = test::TestRequest::post()
            .uri("/login")
            .set_json(&login_request)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("error"));
    }

    #[actix_web::test]
    async fn register_creates_user_when_allowed() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        // Generate unique email to avoid conflicts with existing test data
        let unique_email = format!("testuser_{}@example.com", uuid::Uuid::new_v4());

        let registration = serde_json::json!({
            "name": "Test User",
            "email": unique_email,
            "role": "user",
            "password": "SecurePassword123!"
        });

        let req = test::TestRequest::post()
            .uri("/register")
            .set_json(&registration)
            .to_request();
        let resp = test::call_service(&app, req).await;

        // Registration should succeed with 201 Created
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("uuid").is_some());
        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("Test User"));
    }

    // =========================================================================
    // PROTECTED ENDPOINT TESTS (auth required)
    // =========================================================================

    #[actix_web::test]
    async fn get_current_user_requires_auth() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        // Request without authentication
        let req = test::TestRequest::get()
            .uri("/me")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("error"));
        assert_eq!(json.get("message").and_then(|v| v.as_str()), Some("Authentication required"));
    }

    #[actix_web::test]
    async fn get_current_user_with_auth_succeeds() {
        let pool = setup_test_pool();
        let (user_uuid, claims) = {
            let mut conn = pool.get().unwrap();
            let user = TestFixtures::create_user(&mut conn, "authuser", UserRole::User);
            let claims = create_test_claims(&user);
            (user.uuid, claims)
        }; // conn dropped here

        let app = test::init_service(test_app(pool.clone())).await;

        let req = test::TestRequest::get()
            .uri("/me")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("uuid").and_then(|v| v.as_str()), Some(user_uuid.to_string().as_str()));
        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("authuser"));
    }
}