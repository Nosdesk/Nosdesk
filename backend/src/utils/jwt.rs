use actix_web::{Error as ActixError, HttpResponse};
use jsonwebtoken::{
    decode, encode, errors::ErrorKind, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
// Removed unused import: use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{Claims, User};
use crate::repository;
use crate::utils::{parse_uuid, uuid_to_string};

// Lazy static for JWT secret - initialized once
lazy_static::lazy_static! {
    pub static ref JWT_SECRET: String =
        std::env::var("JWT_SECRET").expect("JWT_SECRET environment variable must be set");
}

/// Lifetime of a sessionless connection token (SSE + collab WebSocket). Single
/// source of truth: it sets both the JWT `exp` (in `create_connection_token`)
/// and the `expires_in` the mint endpoints report to the client, so the client's
/// refresh-before-expiry cache can never disagree with the real expiry.
pub const CONNECTION_TOKEN_TTL_SECS: usize = 3600;

/// JWT token creation and validation utilities
pub struct JwtUtils;

impl JwtUtils {
    /// Create a JWT token for a user with full scope (15 min expiry).
    /// Mints a session token. The only role carried is the
    /// platform role (read straight off `user.platform_role`); the
    /// per-workspace role is resolved per-request from
    /// `workspace_members`, so the token stays workspace-independent.
    pub fn create_token(user: &User, session_id: &uuid::Uuid) -> Result<String, JwtError> {
        // Belt-and-suspenders: refuse to mint a token for a
        // soft-deleted user even if a caller is holding a stale
        // User reference. login_timing already filters these at
        // verify; OAuth callbacks and other token-issuing paths
        // route through here so the guard covers all surfaces.
        if user.deleted_at.is_some() {
            return Err(JwtError::UserNotFound);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtError::SystemTime)?
            .as_secs() as usize;

        let claims = Claims {
            sub: uuid_to_string(&user.uuid),
            name: user.name.clone(),
            email: String::new(),
            platform_role: user.platform_role.clone(),
            scope: "full".to_string(),
            sid: Some(session_id.to_string()),
            workspace_uuid: None,
            exp: now + 15 * 60, // 15 minutes
            iat: now,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
        )
        .map_err(JwtError::EncodingError)
    }

    /// Mint a short-lived (1h), sessionless "connection token" for a URL-only
    /// channel that can't send an `Authorization` header (EventSource SSE,
    /// WebSocket collab), so the token rides in the query string instead.
    /// `workspace_uuid` binds the selected workspace (Model C) so the channel
    /// authorizes against it without the selection header; `None` for
    /// Host-derived / self-hosted callers. `scope` distinguishes what a channel
    /// accepts (read-only `sse` vs write-capable `collab`); the per-channel
    /// handler enforces the scope it requires.
    fn create_connection_token(
        user_id: &str,
        platform_role: &str,
        workspace_uuid: Option<uuid::Uuid>,
        scope: &str,
        name: &str,
    ) -> Result<String, JwtError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtError::SystemTime)?
            .as_secs() as usize;

        let claims = Claims {
            sub: user_id.to_string(),
            name: name.to_string(),
            email: String::new(),
            platform_role: platform_role.to_string(),
            scope: scope.to_string(),
            sid: None,
            workspace_uuid,
            exp: now + CONNECTION_TOKEN_TTL_SECS,
            iat: now,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
        )
        .map_err(JwtError::EncodingError)
    }

    /// Read-only connection token for Server-Sent Events.
    pub fn create_sse_token(
        user_id: &str,
        platform_role: &str,
        workspace_uuid: Option<uuid::Uuid>,
    ) -> Result<String, JwtError> {
        Self::create_connection_token(user_id, platform_role, workspace_uuid, "sse", "SSE_TOKEN")
    }

    /// Write-capable connection token for the collaborative-editing WebSocket.
    /// Authorization is still per-document (workspace membership + visibility,
    /// enforced in the WS handler); this token only attests who + which workspace.
    pub fn create_collab_token(
        user_id: &str,
        platform_role: &str,
        workspace_uuid: Option<uuid::Uuid>,
    ) -> Result<String, JwtError> {
        Self::create_connection_token(
            user_id,
            platform_role,
            workspace_uuid,
            "collab",
            "COLLAB_TOKEN",
        )
    }

    /// Create a customer-portal session access token (15 minutes).
    ///
    /// Scope `portal` (refused on the agent surface) and the workspace UUID
    /// bound into the token so the portal gate can confirm the session belongs
    /// to the origin's workspace and reject a token replayed onto a different
    /// tenant's portal. Mirrors [`create_token`] otherwise.
    pub fn create_portal_token(
        user: &User,
        workspace_uuid: uuid::Uuid,
        session_id: &uuid::Uuid,
    ) -> Result<String, JwtError> {
        if user.deleted_at.is_some() {
            return Err(JwtError::UserNotFound);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtError::SystemTime)?
            .as_secs() as usize;

        let claims = Claims {
            sub: uuid_to_string(&user.uuid),
            name: user.name.clone(),
            email: String::new(),
            platform_role: user.platform_role.clone(),
            scope: crate::middleware::cookie_auth::PORTAL_SCOPE.to_string(),
            sid: Some(session_id.to_string()),
            workspace_uuid: Some(workspace_uuid),
            exp: now + 15 * 60, // 15 minutes
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
        conn: &mut DbConnection,
    ) -> Result<(Claims, User), JwtError> {
        let claims = Self::validate_token(token)?;

        // Parse UUID from claims
        let user_uuid = parse_uuid(&claims.sub).map_err(|_| JwtError::InvalidUserUuid)?;

        // Get user from database to ensure they still exist and are
        // active. Soft-deleted users keep their row (audit / history)
        // but are not allowed to act with cached tokens. Existing
        // sessions stop working immediately on soft-delete, which is
        // the property the admin "delete user" flow promises.
        // `find_active_by_uuid` returns NotFound for both
        // "row missing" and "row soft-deleted" — same outcome here.
        let user = repository::users::find_active_by_uuid(&user_uuid, conn)
            .map_err(|_| JwtError::UserNotFound)?;

        // Verify the platform role hasn't changed since the token was
        // issued: a demotion out of platform_admin (or audit_reviewer)
        // invalidates the token immediately instead of waiting for the
        // 15-minute expiry. Per-workspace role changes don't need a
        // token-level guard because AuthContext re-reads
        // workspace_members on every request, so a workspace demotion
        // takes effect on the next call regardless of the token.
        if claims.platform_role != user.platform_role {
            return Err(JwtError::RoleMismatch {
                token_role: claims.platform_role,
                current_role: user.platform_role.clone(),
            });
        }

        // Connection tokens (sse / collab streams) are short-lived and
        // sessionless (never written to active_sessions), so skip the session
        // lookup. The per-channel handler enforces the scope it accepts.
        let is_connection_token = matches!(claims.scope.as_str(), "sse" | "collab");

        if !is_connection_token {
            // Use sid claim to look up session by stable UUID
            let sid_str = claims.sid.as_deref().ok_or(JwtError::SessionRevoked)?;

            let session_uuid =
                uuid::Uuid::parse_str(sid_str).map_err(|_| JwtError::SessionRevoked)?;

            match crate::repository::active_sessions::get_session_by_session_id(conn, &session_uuid)
            {
                Ok(session) => {
                    if session.user_uuid != user_uuid {
                        tracing::warn!(
                            "Session UUID mismatch: session belongs to {}, token claims {}",
                            session.user_uuid,
                            user_uuid
                        );
                        return Err(JwtError::SessionRevoked);
                    }
                    if session.expires_at < chrono::Utc::now().naive_utc() {
                        tracing::debug!("Session expired for sid {}", sid_str);
                        return Err(JwtError::SessionRevoked);
                    }
                }
                Err(_) => {
                    tracing::debug!("Session not found or revoked for sid: {}", sid_str);
                    return Err(JwtError::SessionRevoked);
                }
            }
        } else {
            tracing::debug!(
                scope = %claims.scope,
                "Validating sessionless connection token for user {}",
                user_uuid
            );
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
    RoleMismatch {
        token_role: String,
        current_role: String,
    },
    MissingToken,
    InsufficientPermissions {
        required: String,
        actual: String,
    },
    InsufficientScope {
        required: String,
        actual: String,
    },
    SessionRevoked,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EncodingError(e) => write!(f, "JWT encoding error: {e}"),
            Self::SystemTime => write!(f, "System time error"),
            Self::InvalidUserUuid => write!(f, "Invalid user UUID in token"),
            Self::UserNotFound => write!(f, "User not found or inactive"),
            Self::RoleMismatch {
                token_role,
                current_role,
            } => {
                write!(
                    f,
                    "Role mismatch - token has '{token_role}', current role is '{current_role}'"
                )
            }
            Self::MissingToken => write!(f, "Missing authentication token"),
            Self::InsufficientPermissions { required, actual } => {
                write!(
                    f,
                    "Insufficient permissions - required: {required}, actual: {actual}"
                )
            }
            Self::InsufficientScope { required, actual } => {
                write!(
                    f,
                    "Insufficient token scope - required: {required}, actual: {actual}"
                )
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
            JwtError::EncodingError(ref jwt_err) => match jwt_err.kind() {
                ErrorKind::ExpiredSignature => {
                    actix_web::error::ErrorUnauthorized("Token has expired")
                }
                ErrorKind::InvalidToken => {
                    actix_web::error::ErrorUnauthorized("Invalid token format")
                }
                _ => actix_web::error::ErrorUnauthorized("Invalid token"),
            },
            JwtError::InvalidUserUuid | JwtError::UserNotFound => {
                actix_web::error::ErrorUnauthorized("Invalid user credentials")
            }
            JwtError::RoleMismatch { .. } => {
                actix_web::error::ErrorUnauthorized("Token role mismatch - please log in again")
            }
            JwtError::MissingToken => {
                actix_web::error::ErrorUnauthorized("Missing authentication token")
            }
            JwtError::InsufficientPermissions { .. } => {
                actix_web::error::ErrorForbidden("Insufficient permissions")
            }
            JwtError::InsufficientScope { .. } => actix_web::error::ErrorForbidden(
                "This action requires a full session - please log in again",
            ),
            JwtError::SessionRevoked => actix_web::error::ErrorUnauthorized(
                "Session has been revoked - please log in again",
            ),
            JwtError::SystemTime => actix_web::error::ErrorInternalServerError("Server time error"),
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

    /// Generate access token, refresh token (stored in DB), and CSRF token.
    fn create_tokens(
        user: &User,
        session_id: &uuid::Uuid,
        family_id: &uuid::Uuid,
        conn: &mut DbConnection,
    ) -> Result<LoginTokens, HttpResponse> {
        let access_token = JwtUtils::create_token(user, session_id).map_err(|_| {
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error generating token"
            }))
        })?;

        let refresh_token = JwtUtils::generate_refresh_token();
        let refresh_token_hash = JwtUtils::hash_refresh_token(&refresh_token);

        let refresh_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
        crate::repository::refresh_tokens::create_refresh_token(
            conn,
            crate::models::NewRefreshToken {
                token_hash: refresh_token_hash,
                user_uuid: user.uuid,
                expires_at: refresh_expires,
                session_id: Some(*session_id),
                family_id: *family_id,
            },
        )
        .map_err(|e| {
            tracing::error!("Failed to store refresh token: {}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create refresh token"
            }))
        })?;

        let csrf_token = crate::utils::csrf::generate_csrf_token();

        Ok(LoginTokens {
            access_token,
            refresh_token,
            csrf_token,
        })
    }

    /// Customer-portal equivalent of [`create_tokens`]: a portal-scope access
    /// token (workspace-bound) plus the same refresh + CSRF machinery. The
    /// refresh row and CSRF token are issued identically to an agent session;
    /// only the access token's scope and workspace binding differ.
    pub fn create_portal_tokens(
        user: &User,
        workspace_uuid: uuid::Uuid,
        session_id: &uuid::Uuid,
        family_id: &uuid::Uuid,
        conn: &mut DbConnection,
    ) -> Result<LoginTokens, HttpResponse> {
        let access_token = JwtUtils::create_portal_token(user, workspace_uuid, session_id)
            .map_err(|_| {
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error generating token"
                }))
            })?;

        let refresh_token = JwtUtils::generate_refresh_token();
        let refresh_token_hash = JwtUtils::hash_refresh_token(&refresh_token);
        let refresh_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
        crate::repository::refresh_tokens::create_refresh_token(
            conn,
            crate::models::NewRefreshToken {
                token_hash: refresh_token_hash,
                user_uuid: user.uuid,
                expires_at: refresh_expires,
                session_id: Some(*session_id),
                family_id: *family_id,
            },
        )
        .map_err(|e| {
            tracing::error!("Failed to store portal refresh token: {}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create refresh token"
            }))
        })?;

        let csrf_token = crate::utils::csrf::generate_csrf_token();

        Ok(LoginTokens {
            access_token,
            refresh_token,
            csrf_token,
        })
    }

    /// Create a successful login response with tokens (caller sets cookies)
    pub fn create_login_response(
        user: User,
        session_id: &uuid::Uuid,
        family_id: &uuid::Uuid,
        conn: &mut DbConnection,
    ) -> Result<(crate::models::LoginResponse, LoginTokens), HttpResponse> {
        let tokens = create_tokens(&user, session_id, family_id, conn)?;

        let response = crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            passkey_mfa_required: None,
            user_uuid: Some(user.uuid.to_string()),
            csrf_token: Some(tokens.csrf_token.clone()),
            // Build the full UserResponse (the `/auth/me` shape) rather
            // than `user.into()`, whose `From<User>` can't read
            // `user_preferences` and so leaves `theme: None`. Carrying the
            // saved theme in the login response lets the client apply it
            // immediately on sign-in instead of only after the first
            // `/auth/me` (i.e. a page refresh).
            user: Some(crate::repository::user_helpers::get_user_with_primary_email(user, conn)),
            message: Some("Login successful".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
            access_token: None,
            refresh_token: None,
        };

        Ok((response, tokens))
    }

    /// Create a response indicating TOTP MFA is required
    pub fn create_mfa_required_response(user_uuid: uuid::Uuid) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: Some(true),
            mfa_setup_required: Some(false),
            passkey_mfa_required: None,
            user_uuid: Some(user_uuid.to_string()),
            csrf_token: None,
            user: None,
            message: Some("Multi-factor authentication required".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
            access_token: None,
            refresh_token: None,
        }
    }

    /// Create a response indicating MFA setup is required
    pub fn create_mfa_setup_required_response(
        user_uuid: uuid::Uuid,
    ) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: Some(false),
            mfa_setup_required: Some(true),
            passkey_mfa_required: None,
            user_uuid: Some(user_uuid.to_string()),
            csrf_token: None,
            user: None,
            message: Some(
                "Multi-factor authentication setup required for your account type".to_string(),
            ),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
            access_token: None,
            refresh_token: None,
        }
    }

    /// Create a response indicating passkey verification is required after password login
    pub fn create_passkey_mfa_required_response(
        user_uuid: uuid::Uuid,
    ) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: None,
            mfa_setup_required: None,
            passkey_mfa_required: Some(true),
            user_uuid: Some(user_uuid.to_string()),
            csrf_token: None,
            user: None,
            message: Some("Passkey verification required".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
            access_token: None,
            refresh_token: None,
        }
    }

    /// Create a successful MFA login response with tokens (caller sets cookies)
    pub fn create_mfa_login_response(
        user: User,
        backup_code_used: bool,
        requires_regeneration: bool,
        session_id: &uuid::Uuid,
        family_id: &uuid::Uuid,
        conn: &mut DbConnection,
    ) -> Result<(crate::models::LoginResponse, LoginTokens), HttpResponse> {
        let tokens = create_tokens(&user, session_id, family_id, conn)?;

        let message = if backup_code_used && requires_regeneration {
            "Login successful using backup code. You have 2 or fewer backup codes remaining - please regenerate them soon."
        } else if backup_code_used {
            "Login successful using backup code"
        } else {
            "Login successful"
        };

        let response = crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            passkey_mfa_required: None,
            user_uuid: Some(user.uuid.to_string()),
            csrf_token: Some(tokens.csrf_token.clone()),
            // Carry the saved theme (see create_login_response).
            user: Some(crate::repository::user_helpers::get_user_with_primary_email(user, conn)),
            message: Some(message.to_string()),
            mfa_backup_code_used: Some(backup_code_used),
            requires_backup_code_regeneration: Some(requires_regeneration),
            backup_codes: None,
            access_token: None,
            refresh_token: None,
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
        unsafe {
            std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only");
        }

        // Force lazy_static initialization by accessing JWT_SECRET
        let _ = &*JWT_SECRET;

        let user = crate::models::User {
            uuid: uuid::Uuid::new_v4(),
            name: "Test User".to_string(),
            username: None,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: "user".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            password_changed_at: None,
            feature_flag_overrides: serde_json::json!({}),
            deleted_at: None,
        };

        let sid = uuid::Uuid::new_v4();
        let token = JwtUtils::create_token(&user, &sid).expect("Failed to create token");
        let claims = JwtUtils::validate_token(&token).expect("Failed to validate token");

        assert_eq!(claims.sub, user.uuid.to_string());
        assert_eq!(claims.name, "Test User");
        assert_eq!(claims.scope, "full");
        assert_eq!(claims.sid, Some(sid.to_string()));
    }

    #[test]
    fn create_token_refuses_soft_deleted_user() {
        unsafe {
            std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only");
        }
        let _ = &*JWT_SECRET;

        // Same fixture as the round-trip test, but with deleted_at
        // stamped. create_token's defence-in-depth guard should
        // refuse to mint a fresh token for the soft-deleted row
        // even though every other field is valid.
        let user = crate::models::User {
            uuid: uuid::Uuid::new_v4(),
            name: "Soft Deleted".to_string(),
            username: None,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: "user".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
            password_changed_at: None,
            feature_flag_overrides: serde_json::json!({}),
            deleted_at: Some(chrono::Utc::now().naive_utc()),
        };

        let sid = uuid::Uuid::new_v4();
        match JwtUtils::create_token(&user, &sid) {
            Err(JwtError::UserNotFound) => {}
            other => panic!("expected UserNotFound for soft-deleted user, got {other:?}"),
        }
    }

    #[test]
    /// A crypto backend is compiled in.
    ///
    /// From jsonwebtoken 10 the crate dispatches through a backend trait, and
    /// its default feature set (`use_pem`) contains no provider. Dropping the
    /// `aws_lc_rs` feature in Cargo.toml therefore still COMPILES, and then
    /// fails every sign and verify at runtime: sessions, licence checks,
    /// platform provisioning, push. Without this guard that shows up as a dozen
    /// unrelated-looking failures across four modules, which is how it reached
    /// us in the first place (Dependabot's 9 -> 11 bump, PR #203).
    ///
    /// HS256 alone is a sufficient probe: with no provider registered, every
    /// algorithm family fails identically. The per-family coverage lives with
    /// each consumer (licence EdDSA, APNs ECDSA, FCM RSA).
    #[test]
    fn a_jwt_crypto_backend_is_configured() {
        unsafe {
            std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only");
        }
        let _ = &*JWT_SECRET;

        let token = JwtUtils::create_sse_token(&uuid::Uuid::new_v4().to_string(), "member", None)
            .expect(
                "signing failed: jsonwebtoken has no crypto backend. Restore \
                 `features = [\"aws_lc_rs\"]` on the jsonwebtoken dependency in Cargo.toml.",
            );
        JwtUtils::validate_token(&token).expect(
            "verification failed: jsonwebtoken has no crypto backend. Restore \
             `features = [\"aws_lc_rs\"]` on the jsonwebtoken dependency in Cargo.toml.",
        );
    }

    #[test]
    fn create_sse_token_has_sse_scope() {
        unsafe {
            std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only");
        }
        let _ = &*JWT_SECRET;

        let user_id = uuid::Uuid::new_v4().to_string();
        let ws = uuid::Uuid::new_v4();
        let token = JwtUtils::create_sse_token(&user_id, "platform_admin", Some(ws))
            .expect("Failed to create SSE token");
        let claims = JwtUtils::validate_token(&token).expect("Failed to validate SSE token");
        assert_eq!(claims.scope, "sse");
        assert_eq!(claims.sub, user_id);
        assert_eq!(
            claims.workspace_uuid,
            Some(ws),
            "SSE token carries the workspace"
        );
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
