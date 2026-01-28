use actix_web::{HttpResponse, Error as ActixError};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation, errors::ErrorKind};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
// Removed unused import: use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{Claims, User};
use crate::repository;
use crate::utils::{parse_uuid, uuid_to_string, role_to_string};

// Lazy static for JWT secret - initialized once
lazy_static::lazy_static! {
    pub static ref JWT_SECRET: String = 
        std::env::var("JWT_SECRET").expect("JWT_SECRET environment variable must be set");
}

/// JWT token creation and validation utilities
pub struct JwtUtils;

impl JwtUtils {
    /// Create a JWT token for a user with full scope
    pub fn create_token(user: &User) -> Result<String, JwtError> {
        Self::create_scoped_token(user, "full", 24 * 60 * 60)
    }

    /// Create a limited-scope JWT token for MFA recovery (15 minute expiry)
    pub fn create_mfa_recovery_token(user: &User) -> Result<String, JwtError> {
        Self::create_scoped_token(user, "mfa_recovery", 15 * 60)
    }

    /// Create a JWT token with specified scope and expiry
    fn create_scoped_token(user: &User, scope: &str, expiry_seconds: usize) -> Result<String, JwtError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtError::SystemTime)?
            .as_secs() as usize;

        let claims = Claims {
            sub: uuid_to_string(&user.uuid),
            name: user.name.clone(),
            email: String::new(), // Email removed from User struct
            role: role_to_string(&user.role),
            scope: scope.to_string(),
            exp: now + expiry_seconds,
            iat: now,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
        )
        .map_err(JwtError::EncodingError)
    }

    /// Create a short-lived SSE token (1 hour expiry)
    /// These tokens are specifically for Server-Sent Events and have reduced scope
    pub fn create_sse_token(user_id: &str, role: &str) -> Result<String, JwtError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtError::SystemTime)?
            .as_secs() as usize;

        // SSE tokens are short-lived (1 hour) and have minimal claims for security
        let claims = Claims {
            sub: user_id.to_string(),
            name: "SSE_TOKEN".to_string(), // Mark this as an SSE token
            email: String::new(), // No email needed for SSE
            role: role.to_string(),
            scope: "sse".to_string(), // SSE-specific scope
            exp: now + 3600, // 1 hour from now (in seconds)
            iat: now,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
        )
        .map_err(JwtError::EncodingError)
    }

    /// Validate a JWT token and return claims
    pub fn validate_token(token: &str) -> Result<Claims, JwtError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true; // Ensure token hasn't expired
        validation.validate_nbf = true; // Ensure token is not used before valid time
        validation.leeway = 30; // Allow 30 seconds of clock skew

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
            &validation,
        )?;

        Ok(token_data.claims)
    }

    /// Validate token and ensure user exists in database
    pub async fn validate_token_with_user_check(
        token: &str,
        conn: &mut DbConnection
    ) -> Result<(Claims, User), JwtError> {
        let claims = Self::validate_token(token)?;

        // Parse UUID from claims
        let user_uuid = parse_uuid(&claims.sub)
            .map_err(|_| JwtError::InvalidUserUuid)?;

        // Get user from database to ensure they still exist and are active
        let user = repository::get_user_by_uuid(&user_uuid, conn)
            .map_err(|_| JwtError::UserNotFound)?;

        // Verify role hasn't changed since token was issued
        let current_role = role_to_string(&user.role);
        if claims.role != current_role {
            return Err(JwtError::RoleMismatch {
                token_role: claims.role,
                current_role,
            });
        }

        // Skip session validation for SSE tokens (they're short-lived and not stored in active_sessions)
        // SSE tokens are identified by having "SSE_TOKEN" in the name field
        let is_sse_token = claims.name == "SSE_TOKEN";

        if !is_sse_token {
            // Check if session exists in database (session revocation check)
            // Hash the JWT token with SHA-256 to match stored session tokens
            use ring::digest;
            let hash = digest::digest(&digest::SHA256, token.as_bytes());
            let token_hash = hex::encode(hash.as_ref());

            // Verify session exists and hasn't been revoked
            match crate::repository::active_sessions::get_session_by_token(conn, &token_hash) {
                Ok(session) => {
                    // Verify session belongs to the user from the token
                    if session.user_uuid != user_uuid {
                        tracing::warn!("Session token mismatch: session belongs to {}, token claims {}",
                            session.user_uuid, user_uuid);
                        return Err(JwtError::SessionRevoked);
                    }
                    // Session exists and is valid
                },
                Err(_) => {
                    // Session doesn't exist or has been revoked
                    tracing::debug!("Session not found or revoked for token hash: {}", &token_hash[..8]);
                    return Err(JwtError::SessionRevoked);
                }
            }
        } else {
            // SSE tokens are short-lived (1 hour) and rely on expiry, not session revocation
            tracing::debug!("Validating SSE token for user {}", user_uuid);
        }

        Ok((claims, user))
    }

    /// Authenticate request using token string (for cookie-based auth)
    pub async fn authenticate_with_token(
        token: &str,
        conn: &mut DbConnection,
    ) -> Result<(Claims, User), ActixError> {
        match Self::validate_token_with_user_check(token, conn).await {
            Ok((claims, user)) => Ok((claims, user)),
            Err(jwt_error) => Err(jwt_error.into()),
        }
    }

    /// Extract claims from request extensions (set by cookie_auth_middleware)
    /// This is a DRY helper to avoid repeating the same pattern in every handler
    ///
    /// # Arguments
    /// * `req` - The HTTP request with extensions populated by middleware
    ///
    /// # Returns
    /// * `Ok(Claims)` - Successfully extracted claims
    /// * `Err(ActixError)` - No claims found (not authenticated)
    pub fn extract_claims(req: &actix_web::HttpRequest) -> Result<Claims, ActixError> {
        use actix_web::HttpMessage;

        req.extensions()
            .get::<Claims>()
            .cloned()
            .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))
    }

    /// Generate a cryptographically secure refresh token (32 bytes = 64 hex chars)
    pub fn generate_refresh_token() -> String {
        use rand::Rng;
        let token_bytes: [u8; 32] = rand::thread_rng().gen();
        hex::encode(token_bytes)
    }

    /// Hash a refresh token using SHA-256 for storage
    pub fn hash_refresh_token(token: &str) -> String {
        use ring::digest;
        let hash = digest::digest(&digest::SHA256, token.as_bytes());
        hex::encode(hash.as_ref())
    }
}

/// Custom error types for JWT operations
#[derive(Debug)]
pub enum JwtError {
    EncodingError(jsonwebtoken::errors::Error),
    SystemTime,
    InvalidUserUuid,
    UserNotFound,
    RoleMismatch { token_role: String, current_role: String },
    MissingToken,
    InsufficientPermissions { required: String, actual: String },
    InsufficientScope { required: String, actual: String },
    SessionRevoked,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EncodingError(e) => write!(f, "JWT encoding error: {e}"),
            Self::SystemTime => write!(f, "System time error"),
            Self::InvalidUserUuid => write!(f, "Invalid user UUID in token"),
            Self::UserNotFound => write!(f, "User not found or inactive"),
            Self::RoleMismatch { token_role, current_role } => {
                write!(f, "Role mismatch - token has '{token_role}', current role is '{current_role}'")
            }
            Self::MissingToken => write!(f, "Missing authentication token"),
            Self::InsufficientPermissions { required, actual } => {
                write!(f, "Insufficient permissions - required: {required}, actual: {actual}")
            }
            Self::InsufficientScope { required, actual } => {
                write!(f, "Insufficient token scope - required: {required}, actual: {actual}")
            }
            Self::SessionRevoked => write!(f, "Session has been revoked"),
        }
    }
}

impl std::error::Error for JwtError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EncodingError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<jsonwebtoken::errors::Error> for JwtError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        Self::EncodingError(err)
    }
}

/// Convert JWT errors to appropriate HTTP responses
impl From<JwtError> for ActixError {
    fn from(error: JwtError) -> Self {
        match error {
            JwtError::EncodingError(ref jwt_err) => {
                match jwt_err.kind() {
                    ErrorKind::ExpiredSignature => {
                        actix_web::error::ErrorUnauthorized("Token has expired")
                    },
                    ErrorKind::InvalidToken => {
                        actix_web::error::ErrorUnauthorized("Invalid token format")
                    },
                    _ => actix_web::error::ErrorUnauthorized("Invalid token"),
                }
            },
            JwtError::InvalidUserUuid | JwtError::UserNotFound => {
                actix_web::error::ErrorUnauthorized("Invalid user credentials")
            },
            JwtError::RoleMismatch { .. } => {
                actix_web::error::ErrorUnauthorized("Token role mismatch - please log in again")
            },
            JwtError::MissingToken => {
                actix_web::error::ErrorUnauthorized("Missing authentication token")
            },
            JwtError::InsufficientPermissions { .. } => {
                actix_web::error::ErrorForbidden("Insufficient permissions")
            },
            JwtError::InsufficientScope { .. } => {
                actix_web::error::ErrorForbidden("This action requires a full session - please log in again")
            },
            JwtError::SessionRevoked => {
                actix_web::error::ErrorUnauthorized("Session has been revoked - please log in again")
            },
            JwtError::SystemTime => {
                actix_web::error::ErrorInternalServerError("Server time error")
            },
        }
    }
}

/// Convert JWT errors to HTTP responses (for direct use in handlers)
impl From<JwtError> for HttpResponse {
    fn from(error: JwtError) -> Self {
        match error {
            JwtError::EncodingError(ref jwt_err) => {
                let message = match jwt_err.kind() {
                    ErrorKind::ExpiredSignature => "Token has expired",
                    ErrorKind::InvalidToken => "Invalid token format",
                    _ => "Invalid token",
                };
                HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": message
                }))
            },
            JwtError::InvalidUserUuid | JwtError::UserNotFound => {
                HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Invalid user credentials"
                }))
            },
            JwtError::RoleMismatch { .. } => {
                HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Token role mismatch - please log in again"
                }))
            },
            JwtError::MissingToken => {
                HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Missing authentication token"
                }))
            },
            JwtError::InsufficientPermissions { required, actual } => {
                HttpResponse::Forbidden().json(json!({
                    "status": "error",
                    "message": format!("Insufficient permissions - required: {}, actual: {}", required, actual)
                }))
            },
            JwtError::InsufficientScope { required, actual } => {
                HttpResponse::Forbidden().json(json!({
                    "status": "error",
                    "message": format!("This action requires a full session - please log in again (required: {}, actual: {})", required, actual)
                }))
            },
            JwtError::SessionRevoked => {
                HttpResponse::Unauthorized().json(json!({
                    "status": "error",
                    "message": "Session has been revoked - please log in again"
                }))
            },
            JwtError::SystemTime => {
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Server time error"
                }))
            },
        }
    }
}

/// Helper functions for common JWT operations
pub mod helpers {
    use super::*;

    /// Struct containing login tokens for cookie setting
    pub struct LoginTokens {
        pub access_token: String,
        pub refresh_token: String,
        pub csrf_token: String,
    }

    /// Create a successful login response with tokens (caller sets cookies)
    pub fn create_login_response(user: User, conn: &mut DbConnection) -> Result<(crate::models::LoginResponse, LoginTokens), HttpResponse> {
        let token = JwtUtils::create_token(&user)
            .map_err(|_| HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error generating token"
            })))?;

        // Generate refresh token
        let refresh_token = JwtUtils::generate_refresh_token();
        let refresh_token_hash = JwtUtils::hash_refresh_token(&refresh_token);

        // Store refresh token (7 days expiration)
        let refresh_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
        let new_refresh_token = crate::models::NewRefreshToken {
            token_hash: refresh_token_hash,
            user_uuid: user.uuid,
            expires_at: refresh_expires,
        };

        if let Err(e) = crate::repository::refresh_tokens::create_refresh_token(conn, new_refresh_token) {
            tracing::error!("Failed to store refresh token: {}", e);
            return Err(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create refresh token"
            })));
        }

        // Generate CSRF token
        let csrf_token = crate::utils::csrf::generate_csrf_token();

        let response = crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            user_uuid: Some(user.uuid.to_string()),
            csrf_token: Some(csrf_token.clone()),
            user: Some(user.into()),
            message: Some("Login successful".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
        };

        let tokens = LoginTokens {
            access_token: token,
            refresh_token,
            csrf_token,
        };

        Ok((response, tokens))
    }

    /// Create a response indicating MFA is required
    pub fn create_mfa_required_response(user_uuid: uuid::Uuid) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: Some(true),
            mfa_setup_required: Some(false),
            user_uuid: Some(user_uuid.to_string()),
            csrf_token: None,
            user: None,
            message: Some("Multi-factor authentication required".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
        }
    }

    /// Create a response indicating MFA setup is required
    pub fn create_mfa_setup_required_response(user_uuid: uuid::Uuid) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: Some(false),
            mfa_setup_required: Some(true),
            user_uuid: Some(user_uuid.to_string()),
            csrf_token: None,
            user: None,
            message: Some("Multi-factor authentication setup required for your account type".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
        }
    }



    /// Create a successful MFA login response with tokens (caller sets cookies)
    pub fn create_mfa_login_response(
        user: User,
        backup_code_used: bool,
        requires_regeneration: bool,
        conn: &mut DbConnection,
    ) -> Result<(crate::models::LoginResponse, LoginTokens), HttpResponse> {
        let token = JwtUtils::create_token(&user)
            .map_err(|_| HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error generating token"
            })))?;

        // Generate refresh token
        let refresh_token = JwtUtils::generate_refresh_token();
        let refresh_token_hash = JwtUtils::hash_refresh_token(&refresh_token);

        // Store refresh token (7 days expiration)
        let refresh_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
        let new_refresh_token = crate::models::NewRefreshToken {
            token_hash: refresh_token_hash,
            user_uuid: user.uuid,
            expires_at: refresh_expires,
        };

        if let Err(e) = crate::repository::refresh_tokens::create_refresh_token(conn, new_refresh_token) {
            tracing::error!("Failed to store refresh token: {}", e);
            return Err(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create refresh token"
            })));
        }

        // Generate CSRF token
        let csrf_token = crate::utils::csrf::generate_csrf_token();

        let mut message = "Login successful".to_string();
        if backup_code_used {
            message = if requires_regeneration {
                "Login successful using backup code. You have 2 or fewer backup codes remaining - please regenerate them soon.".to_string()
            } else {
                "Login successful using backup code".to_string()
            };
        }

        let response = crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            user_uuid: Some(user.uuid.to_string()),
            csrf_token: Some(csrf_token.clone()),
            user: Some(user.into()),
            message: Some(message),
            mfa_backup_code_used: Some(backup_code_used),
            requires_backup_code_regeneration: Some(requires_regeneration),
            backup_codes: None,
        };

        let tokens = LoginTokens {
            access_token: token,
            refresh_token,
            csrf_token,
        };

        Ok((response, tokens))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_refresh_token_deterministic() {
        let token = "test-token-value";
        let hash1 = JwtUtils::hash_refresh_token(token);
        let hash2 = JwtUtils::hash_refresh_token(token);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_refresh_token_differs() {
        let hash1 = JwtUtils::hash_refresh_token("token-a");
        let hash2 = JwtUtils::hash_refresh_token("token-b");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn generate_refresh_token_length() {
        let token = JwtUtils::generate_refresh_token();
        assert_eq!(token.len(), 64);
    }

    #[test]
    fn generate_refresh_token_unique() {
        let t1 = JwtUtils::generate_refresh_token();
        let t2 = JwtUtils::generate_refresh_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn create_token_and_validate_roundtrip() {
        unsafe { std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only"); }

        // Force lazy_static initialization by accessing JWT_SECRET
        let _ = &*JWT_SECRET;

        let user = crate::models::User {
            uuid: uuid::Uuid::new_v4(),
            name: "Test User".to_string(),
            role: crate::models::UserRole::Admin,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            passkey_credentials: None,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            password_changed_at: None,
        };

        let token = JwtUtils::create_token(&user).expect("Failed to create token");
        let claims = JwtUtils::validate_token(&token).expect("Failed to validate token");

        assert_eq!(claims.sub, user.uuid.to_string());
        assert_eq!(claims.name, "Test User");
        assert_eq!(claims.scope, "full");
    }

    #[test]
    fn create_sse_token_has_sse_scope() {
        unsafe { std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only"); }
        let _ = &*JWT_SECRET;

        let user_id = uuid::Uuid::new_v4().to_string();
        let token = JwtUtils::create_sse_token(&user_id, "admin").expect("Failed to create SSE token");
        let claims = JwtUtils::validate_token(&token).expect("Failed to validate SSE token");
        assert_eq!(claims.scope, "sse");
        assert_eq!(claims.sub, user_id);
    }

    #[test]
    fn mfa_required_response() {
        let uuid = uuid::Uuid::new_v4();
        let resp = helpers::create_mfa_required_response(uuid);
        assert!(!resp.success);
        assert_eq!(resp.mfa_required, Some(true));
        assert_eq!(resp.mfa_setup_required, Some(false));
        assert_eq!(resp.user_uuid, Some(uuid.to_string()));
    }

    #[test]
    fn mfa_setup_required_response() {
        let uuid = uuid::Uuid::new_v4();
        let resp = helpers::create_mfa_setup_required_response(uuid);
        assert!(!resp.success);
        assert_eq!(resp.mfa_required, Some(false));
        assert_eq!(resp.mfa_setup_required, Some(true));
        assert_eq!(resp.user_uuid, Some(uuid.to_string()));
    }
}