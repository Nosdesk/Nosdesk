use actix_web::{web, HttpResponse, Responder, HttpRequest};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use bcrypt::verify;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use tracing::{error, debug};


use crate::db::DbConnection;
use crate::models::{
    Claims, LoginRequest, PasswordChangeRequest,
    UserRegistration, UserResponse, UserRole
};
use crate::repository;
use crate::utils::{self, ValidationError, parse_uuid};
use crate::utils::auth::hash_password;
use crate::utils::mfa;

// Import JWT utilities
use crate::utils::jwt::{JwtUtils, helpers as jwt_helpers};

// Admin password reset request
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct AdminPasswordResetRequest {
    pub user_id: i32,
    pub new_password: String,
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



/// Helper function to parse UUID from string (like JWT claims)
fn parse_uuid_from_string(uuid_str: &str) -> Result<Uuid, String> {
    Uuid::parse_str(uuid_str).map_err(|_| "Invalid UUID format".to_string())
}

/// Helper function to convert UserRole enum to string for JWT
fn user_role_to_string(role: &UserRole) -> String {
    match role {
        UserRole::Admin => "admin".to_string(),
        UserRole::Technician => "technician".to_string(),
        UserRole::User => "user".to_string(),
    }
}

/// Helper function to parse string to UserRole enum
fn parse_user_role(role_str: &str) -> Result<UserRole, String> {
    match role_str.to_lowercase().as_str() {
        "admin" => Ok(UserRole::Admin),
        "technician" => Ok(UserRole::Technician),
        "user" => Ok(UserRole::User),
        _ => Err("Invalid role".to_string()),
    }
}

/// Helper function to log password change security event
async fn log_password_change_event(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use diesel::prelude::*;
    use crate::schema::security_events;

    #[derive(diesel::Insertable)]
    #[diesel(table_name = security_events)]
    struct NewSecurityEvent {
        user_uuid: Uuid,
        event_type: String,
        ip_address: Option<String>,
        user_agent: Option<String>,
        location: Option<String>,
        details: Option<serde_json::Value>,
        severity: String,
        created_at: chrono::NaiveDateTime,
        session_id: Option<i32>,
    }

    let new_event = NewSecurityEvent {
        user_uuid: *user_uuid,
        event_type: "password_changed".to_string(),
        ip_address: None,
        user_agent: None,
        location: None,
        details: Some(json!({
            "action": "password_change",
            "success": true
        })),
        severity: "info".to_string(),
        created_at: chrono::Utc::now().naive_utc(),
        session_id: None,
    };

    diesel::insert_into(security_events::table)
        .values(&new_event)
        .execute(conn)?;

    Ok(())
}

/// Helper function to create a session record after successful login
async fn create_session_record(
    user_uuid: &Uuid,
    token: &str,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    // Hash the JWT token with SHA-256 for storage
    use ring::digest;
    let hash = digest::digest(&digest::SHA256, token.as_bytes());
    let token_hash = hex::encode(hash.as_ref());

    // Extract IP address from request
    let ip_address = request.peer_addr()
        .map(|addr| addr.ip().to_string());

    // Extract user agent from request headers
    let user_agent = request.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Parse device name from user agent (simple parsing)
    let device_name = user_agent.as_ref().and_then(|ua| {
        // Simple device name extraction - can be enhanced later
        if ua.contains("iPhone") {
            Some("iPhone".to_string())
        } else if ua.contains("iPad") {
            Some("iPad".to_string())
        } else if ua.contains("Android") {
            Some("Android Device".to_string())
        } else if ua.contains("Macintosh") || ua.contains("Mac OS") {
            Some("Mac".to_string())
        } else if ua.contains("Windows") {
            Some("Windows PC".to_string())
        } else if ua.contains("Linux") {
            Some("Linux".to_string())
        } else {
            Some("Unknown Device".to_string())
        }
    });

    // Set expiration to 24 hours from now (matching JWT expiration)
    let expires_at = chrono::Utc::now().naive_utc() + chrono::Duration::hours(24);

    // Create new session record
    let new_session = crate::models::NewActiveSession {
        session_token: token_hash,
        user_uuid: *user_uuid,
        device_name,
        ip_address,
        user_agent,
        location: None, // Could be derived from IP in the future
        expires_at,
        is_current: true,
    };

    // Insert session into database
    match crate::repository::active_sessions::create_session(conn, new_session) {
        Ok(session) => {
            tracing::info!("Session created for user {}: session_id={}", user_uuid, session.id);
            Ok(())
        },
        Err(e) => {
            tracing::error!("Failed to create session for user {}: {}", user_uuid, e);
            Err(Box::new(e))
        }
    }
}

// Authentication handlers
pub async fn login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<LoginRequest>,
    request: HttpRequest,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Find user by email
    let user = match repository::get_user_by_email(&login_data.email, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            eprintln!("Error finding user by email: {:?}", e);
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };



    // Check if user has a password hash for local authentication
    let password_hash = match String::from_utf8(user.password_hash.clone()) {
        Ok(hash) => {
            if hash.is_empty() {
                eprintln!("No password hash found for user: {}", user.id);
                return HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Invalid email or password"
                }));
            }
            hash
        }
        Err(_) => {
            eprintln!("Invalid password hash format for user: {}", user.id);
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
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
    }

    // Check if user has MFA enabled - if so, require MFA verification
    if mfa::user_has_mfa_enabled(&user) {
        let response = jwt_helpers::create_mfa_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    // Check MFA policy enforcement (for users without MFA enabled)
    if let Err(_policy_error) = mfa::validate_mfa_policy(&user).await {
        // Instead of blocking, offer MFA setup for users who need it
        let response = jwt_helpers::create_mfa_setup_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    // Store user UUID before moving user into create_login_response
    let user_uuid = user.uuid;

    // Create standard login response (no MFA required)
    match jwt_helpers::create_login_response(user) {
        Ok(response) => {
            // Create session record after successful login
            if let Some(ref token) = response.token {
                if let Err(e) = create_session_record(&user_uuid, token, &request, &mut conn).await {
                    tracing::warn!("Failed to create session record for user {}: {}", user_uuid, e);
                    // Don't fail the login if session creation fails
                }
            }
            HttpResponse::Ok().json(response)
        },
        Err(error_response) => error_response,
    }
}

/// MFA Login - Verify MFA token and complete login
pub async fn mfa_login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<crate::models::MfaLoginRequest>,
    request: actix_web::HttpRequest,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
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

    // Verify password first
    let password_hash = match String::from_utf8(user.password_hash.clone()) {
        Ok(hash) => {
            if hash.is_empty() {
                return HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Invalid email or password"
                }));
            }
            hash
        }
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

    // Store user UUID before moving user into create_mfa_login_response
    let user_uuid = user.uuid;

    // Create successful MFA login response
    match jwt_helpers::create_mfa_login_response(
        user,
        mfa_result.backup_code_used.is_some(),
        mfa_result.requires_backup_code_regeneration,
    ) {
        Ok(response) => {
            // Create session record after successful MFA login
            if let Some(ref token) = response.token {
                if let Err(e) = create_session_record(&user_uuid, token, &request, &mut conn).await {
                    tracing::warn!("Failed to create session record for user {}: {}", user_uuid, e);
                    // Don't fail the login if session creation fails
                }
            }
            HttpResponse::Ok().json(response)
        },
        Err(error_response) => error_response,
    }
}

pub async fn register(
    db_pool: web::Data<crate::db::Pool>,
    user_data: web::Json<UserRegistration>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
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

    // Validate password
    if user_data.password.len() < 8 {
        validation_errors.push("password: Password must be at least 8 characters long".to_string());
    } else if user_data.password.len() > 128 {
        validation_errors.push("password: Password must be less than 128 characters".to_string());
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
    if let Ok(_) = repository::get_user_by_email(&user_data.email, &mut conn) {
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
    let user_uuid = Uuid::new_v4();

    // Parse role from string to enum
    let user_role = match utils::parse_role(&user_data.role) {
        Ok(role) => role,
        Err(e) => return e.into(),
    };

    // Create new user using builder pattern with normalized data
    let (normalized_name, normalized_email) = utils::normalization::normalize_user_data(&user_data.name, &user_data.email);
    let new_user = utils::NewUserBuilder::new(normalized_name, normalized_email, user_role)
        .with_uuid(user_uuid)
        .with_password_hash(password_hash.as_bytes().to_vec())
        .with_pronouns(utils::normalization::normalize_optional_string(user_data.pronouns.as_ref()))
        .with_avatar(
            utils::normalization::normalize_optional_string(user_data.avatar_url.as_ref()),
            utils::normalization::normalize_optional_string(user_data.avatar_thumb.as_ref())
        )
        .with_banner(utils::normalization::normalize_optional_string(user_data.banner_url.as_ref()))
        .build();

    // Save user to database
    match repository::create_user(new_user, &mut conn) {
        Ok(created_user) => {
            println!("✅ New user registered successfully: {}", created_user.email);
            HttpResponse::Created().json(UserResponse::from(created_user))
        },
        Err(e) => {
            eprintln!("Error creating user: {:?}", e);
            
            // Provide more specific error messages for common issues
            let error_message = if format!("{:?}", e).contains("duplicate") || format!("{:?}", e).contains("unique") {
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

// Middleware for validating JWT tokens
#[allow(dead_code)]
pub async fn validate_token(auth: BearerAuth, db_pool: web::Data<crate::db::Pool>) -> Result<UserResponse, actix_web::Error> {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return Err(actix_web::error::ErrorInternalServerError("Database error")),
    };
    
    let (_claims, user) = JwtUtils::authenticate_request(&auth, &mut conn).await?;
    Ok(UserResponse::from(user))
}

pub async fn change_password(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
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
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate the token and get claims
    let (claims, _user) = match JwtUtils::authenticate_request(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_e) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
            // Check if user has a password hash for local authentication
            let current_password_hash = match String::from_utf8(user.password_hash.clone()) {
                Ok(hash) => {
                    if hash.is_empty() {
                        eprintln!("No password hash found for user: {}", user.id);
                        return HttpResponse::BadRequest().json(json!({
                            "status": "error",
                            "message": "No local password found for this user"
                        }));
                    }
                    hash
                }
                Err(_) => {
                    eprintln!("Invalid password hash format for user: {}", user.id);
                    return HttpResponse::BadRequest().json(json!({
                        "status": "error",
                        "message": "No password is set for this account"
                    }));
                }
            };

            // Verify current password
            let password_matches = match verify(&password_data.current_password, &current_password_hash) {
                Ok(matches) => matches,
                Err(_) => {
                    eprintln!("Error verifying current password during password change");
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

            // Update the user's password hash and password_changed_at timestamp
            use diesel::prelude::*;
            let now = chrono::Utc::now().naive_utc();

            match diesel::update(crate::schema::users::table.find(user.id))
                .set((
                    crate::schema::users::password_hash.eq(new_password_hash.as_bytes()),
                    crate::schema::users::password_changed_at.eq(now)
                ))
                .execute(&mut conn) {
                Ok(_) => {
                    println!("✅ Password changed successfully for user: {}", user.email);

                    // Log security event for password change
                    if let Err(e) = log_password_change_event(&user.uuid, &mut conn).await {
                        tracing::warn!("Failed to log password change event: {}", e);
                        // Don't fail the password change if logging fails
                    }

                    // Revoke all other sessions for security (defense in depth)
                    // Get current session token from the Bearer auth token
                    let current_session_token = auth.token();

                    // Hash the JWT token to match stored session tokens (SHA-256)
                    use ring::digest;
                    let hash = digest::digest(&digest::SHA256, current_session_token.as_bytes());
                    let token_hash = hex::encode(hash.as_ref());

                    // Look up current session ID to preserve it
                    let current_session_id = match crate::repository::active_sessions::get_session_by_token(
                        &mut conn,
                        &token_hash
                    ) {
                        Ok(session) => Some(session.id),
                        Err(e) => {
                            tracing::warn!("Could not find current session during password change for user {}: {}", user.uuid, e);
                            None
                        }
                    };

                    // Revoke all other sessions (keep current session active)
                    match crate::repository::active_sessions::revoke_other_sessions(
                        &mut conn,
                        &user.uuid,
                        current_session_id
                    ) {
                        Ok(revoked_count) => {
                            if revoked_count > 0 {
                                println!("🔒 Revoked {} other session(s) after password change for user: {}", revoked_count, user.email);
                            }
                        },
                        Err(e) => {
                            tracing::warn!("Failed to revoke other sessions after password change for user {}: {}", user.uuid, e);
                            // Don't fail the password change if session revocation fails
                        }
                    }

                    HttpResponse::Ok().json(json!({
                        "status": "success",
                        "message": "Password changed successfully"
                    }))
                },
                Err(e) => {
                    eprintln!("Error updating password: {:?}", e);
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

// Helper function to validate token internally - deprecated, use JwtUtils instead
pub async fn validate_token_internal(auth: &BearerAuth, conn: &mut DbConnection) -> Result<Claims, actix_web::Error> {
    let (claims, _user) = JwtUtils::authenticate_request(auth, conn).await?;
    Ok(claims)
}

#[allow(dead_code)]
pub async fn admin_reset_password(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    reset_data: web::Json<AdminPasswordResetRequest>,
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate the token and check admin permissions
    let (_claims, _user) = match jwt_helpers::require_admin(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_) => return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can reset passwords"
        })),
    };

    // Get the target user
    let user = match repository::get_user_by_id(reset_data.user_id, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Hash the new password
    let new_password_hash = match hash_password(&reset_data.new_password) {
        Ok(hash) => hash,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error hashing new password"
        })),
    };

    // Update the user's password hash directly
    use diesel::prelude::*;
    match diesel::update(crate::schema::users::table.find(user.id))
        .set(crate::schema::users::password_hash.eq(new_password_hash.as_bytes()))
        .execute(&mut conn) {
        Ok(_) => {
            println!("✅ Password reset successfully for user: {}", user.email);
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Password reset successfully"
            }))
        },
        Err(e) => {
            eprintln!("Error resetting password: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error updating password"
            }))
        }
    }
}

// Handler to get current authenticated user
pub async fn get_current_user(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let (_claims, user) = match JwtUtils::authenticate_request(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to get database connection"
        }))
    };

    match repository::count_users(&mut conn) {
        Ok(user_count) => {
            let response = crate::models::OnboardingStatus {
                requires_setup: user_count == 0,
                user_count,
            };
            
            HttpResponse::Ok()
                .insert_header(("X-Content-Type-Options", "nosniff"))
                .insert_header(("X-Frame-Options", "DENY"))
                .insert_header(("X-XSS-Protection", "1; mode=block"))
                .json(response)
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
    admin_data: web::Json<crate::models::AdminSetupRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
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
            eprintln!("Error counting users: {:?}", e);
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
            eprintln!("Error hashing password: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error processing password"
            }));
        }
    };

    // Generate UUID for the admin user
    let _user_uuid = Uuid::new_v4();

    // Create the admin user using convenience function
    let (normalized_name, normalized_email) = utils::normalization::normalize_user_data(&admin_data.name, &admin_data.email);
    let new_user = utils::NewUserBuilder::admin_user(
        normalized_name,
        normalized_email,
        password_hash.as_bytes().to_vec()
    ).build();

    match repository::create_user(new_user, &mut conn) {
        Ok(created_user) => {
            println!("✅ Initial admin user created successfully: {}", created_user.email);
            
            let response = crate::models::AdminSetupResponse {
                success: true,
                message: "Initial admin user created successfully".to_string(),
                user: Some(UserResponse::from(created_user)),
            };
            HttpResponse::Created().json(response)
        },
        Err(e) => {
            eprintln!("Error creating admin user: {:?}", e);
            
            // Provide more specific error messages for common issues
            let error_message = if format!("{:?}", e).contains("duplicate") || format!("{:?}", e).contains("unique") {
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
    auth: BearerAuth,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let (_claims, user) = match JwtUtils::authenticate_request(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    
    // Generate QR code
    let qr_code = match mfa::generate_qr_code(secret.as_str(), &user.email, "Nosdesk") {
        Ok(qr) => qr,
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
        qr_code,
        // Do not generate or return backup codes until after verification completes
        backup_codes: vec![],
    };

    HttpResponse::Ok().json(response)
}

/// MFA Verify Setup - Verify the TOTP token during setup
pub async fn mfa_verify_setup(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    request: web::Json<crate::models::MfaVerifySetupRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    auth: BearerAuth,
    request: web::Json<crate::models::MfaEnableRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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

    // We'll generate backup codes on the server after successful verification

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

    // Use the pre-hashed backup codes from setup phase
    // Note: backup_codes from frontend are plaintext from setup, but we hash them here
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

/// MFA Disable - Disable MFA for the user
pub async fn mfa_disable(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    request: web::Json<crate::models::MfaDisableRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    let password_hash_str = match String::from_utf8(user.password_hash.clone()) {
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
        Ok(_) => HttpResponse::Ok().json(json!({
            "status": "success",
            "message": "MFA disabled successfully"
        })),
        Err(e) => {
            eprintln!("Error disabling MFA: {:?}", e);
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
    auth: BearerAuth,
    request: web::Json<crate::models::MfaRegenerateBackupCodesRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    let password_hash_str = match String::from_utf8(user.password_hash.clone()) {
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
            eprintln!("Error regenerating backup codes: {:?}", e);
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
    auth: BearerAuth,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Find user by email and verify password (same as login flow)
    let user = match repository::get_user_by_email(&request.email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Verify password
    let password_hash = match String::from_utf8(user.password_hash.clone()) {
        Ok(hash) => {
            if hash.is_empty() {
                return HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Invalid email or password"
                }));
            }
            hash
        }
        Err(_) => {
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
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
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
    
    // Generate QR code
    let qr_code = match mfa::generate_qr_code(secret.as_str(), &user.email, "Nosdesk") {
        Ok(qr) => qr,
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
        qr_code,
        backup_codes: vec![],
    };

    HttpResponse::Ok().json(response)
}

/// MFA Enable for Login (Unauthenticated) - Complete MFA setup and login
pub async fn mfa_enable_login(
    db_pool: web::Data<crate::db::Pool>,
    request: web::Json<crate::models::MfaEnableLoginRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Find user by email and verify password again (security)
    let user = match repository::get_user_by_email(&request.email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid email or password"
            }));
        }
    };

    // Verify password
    let password_hash = match String::from_utf8(user.password_hash.clone()) {
        Ok(hash) => {
            if hash.is_empty() {
                return HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Invalid email or password"
                }));
            }
            hash
        }
        Err(_) => {
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
        return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid email or password"
        }));
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

    match repository::update_user_mfa(&user.uuid, mfa_update, &mut conn) {
        Ok(_) => {
            tracing::info!("MFA enabled successfully for user during login: {}", user.uuid);
            
            // Generate JWT token and complete login
            match jwt_helpers::create_login_response(user) {
                Ok(mut response) => {
                    // Attach plaintext backup codes for one-time display
                    response.backup_codes = Some(backup_codes_plaintext);
                    HttpResponse::Ok().json(response)
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

/// Enhanced MFA reset with multiple verification steps (OWASP recommendations)
/// This function demonstrates a secure MFA reset procedure
#[allow(dead_code)]
async fn initiate_secure_mfa_reset(
    user_uuid: &uuid::Uuid, 
    password: &str,
    conn: &mut DbConnection
) -> Result<String, String> {
    // Step 1: Verify current password
    let user = repository::get_user_by_uuid(user_uuid, conn)
        .map_err(|_| "User not found")?;
    
    let password_hash_str = String::from_utf8(user.password_hash.clone())
        .map_err(|_| "Error reading password hash")?;
    
    if !bcrypt::verify(password, &password_hash_str).unwrap_or(false) {
        return Err("Invalid password".to_string());
    }
    
    // Step 2: Generate secure reset token
    use rand::Rng;
    let reset_token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    
    // Step 3: In production, you would:
    // - Store reset token in database with expiry (15 minutes)
    // - Send email with reset link containing token
    // - Require user to click email link AND re-enter password
    // - Log the reset attempt for security monitoring
    // - Consider requiring admin approval for high-privilege users
    
    tracing::warn!("MFA reset initiated for user: {} (secure procedures should be implemented)", user_uuid);

    Ok(reset_token)
}

// === SESSION MANAGEMENT HANDLERS ===

/// Get all active sessions for the current user
pub async fn get_user_sessions(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let (claims, _user) = match JwtUtils::authenticate_request(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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

    // Get current session token to mark it as current
    let current_session_token = auth.token();
    use ring::digest;
    let hash = digest::digest(&digest::SHA256, current_session_token.as_bytes());
    let token_hash = hex::encode(hash.as_ref());

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
        json!({
            "id": session.id,
            "device_name": session.device_name,
            "ip_address": session.ip_address,
            "user_agent": session.user_agent,
            "location": session.location,
            "created_at": session.created_at,
            "last_active": session.last_active,
            "expires_at": session.expires_at,
            "is_current": session.session_token == token_hash
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
    auth: BearerAuth,
    path: web::Path<i32>,
) -> impl Responder {
    let session_id = path.into_inner();

    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let (claims, _user) = match JwtUtils::authenticate_request(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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
    match crate::repository::active_sessions::get_session_by_token(&mut conn, &session_id.to_string()) {
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
            // Try to revoke anyway in case it's a valid session ID (not found by token lookup)
            // The revoke_session function will handle if it doesn't exist
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
    auth: BearerAuth,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let (claims, _user) = match JwtUtils::authenticate_request(&auth, &mut conn).await {
        Ok((claims, user)) => (claims, user),
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
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

    // Get current session token to preserve it
    let current_session_token = auth.token();
    use ring::digest;
    let hash = digest::digest(&digest::SHA256, current_session_token.as_bytes());
    let token_hash = hex::encode(hash.as_ref());

    // Look up current session ID
    let current_session_id = match crate::repository::active_sessions::get_session_by_token(
        &mut conn,
        &token_hash
    ) {
        Ok(session) => Some(session.id),
        Err(e) => {
            tracing::warn!("Could not find current session for user {}: {}", user_uuid, e);
            None
        }
    };

    // Revoke all other sessions
    match crate::repository::active_sessions::revoke_other_sessions(
        &mut conn,
        &user_uuid,
        current_session_id
    ) {
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