use actix_web::{HttpResponse, Error as ActixError};
use actix_web_httpauth::extractors::bearer::BearerAuth;
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
    /// Create a JWT token for a user
    pub fn create_token(user: &User) -> Result<String, JwtError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| JwtError::SystemTime)?
            .as_secs() as usize;

        let claims = Claims {
            sub: uuid_to_string(&user.uuid),
            name: user.name.clone(),
            email: user.email.clone(),
            role: role_to_string(&user.role),
            exp: now + 24 * 60 * 60, // 24 hours from now
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

        Ok((claims, user))
    }

    /// Extract and validate JWT from BearerAuth
    pub async fn authenticate_request(
        auth: &BearerAuth,
        conn: &mut DbConnection,
    ) -> Result<(Claims, User), ActixError> {
        let token = auth.token();
        
        match Self::validate_token_with_user_check(token, conn).await {
            Ok((claims, user)) => Ok((claims, user)),
            Err(jwt_error) => Err(jwt_error.into()),
        }
    }

    /// Check if user has required role
    pub fn check_role_permission(claims: &Claims, required_role: &str) -> Result<(), JwtError> {
        match (claims.role.as_str(), required_role) {
            ("admin", _) => Ok(()), // Admin can access everything
            (user_role, required) if user_role == required => Ok(()),
            ("technician", "user") => Ok(()), // Technician can access user-level resources
            _ => Err(JwtError::InsufficientPermissions {
                required: required_role.to_string(),
                actual: claims.role.clone(),
            }),
        }
    }

    /// Extract token from query parameters (for SSE where headers aren't supported)
    pub fn extract_token_from_query(query_token: Option<&String>) -> Result<&str, JwtError> {
        query_token
            .map(|s| s.as_str())
            .ok_or(JwtError::MissingToken)
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
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EncodingError(e) => write!(f, "JWT encoding error: {}", e),
            Self::SystemTime => write!(f, "System time error"),
            Self::InvalidUserUuid => write!(f, "Invalid user UUID in token"),
            Self::UserNotFound => write!(f, "User not found or inactive"),
            Self::RoleMismatch { token_role, current_role } => {
                write!(f, "Role mismatch - token has '{}', current role is '{}'", token_role, current_role)
            }
            Self::MissingToken => write!(f, "Missing authentication token"),
            Self::InsufficientPermissions { required, actual } => {
                write!(f, "Insufficient permissions - required: {}, actual: {}", required, actual)
            }
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

    /// Create a successful login response with JWT token
    pub fn create_login_response(user: User) -> Result<crate::models::LoginResponse, HttpResponse> {
        let token = JwtUtils::create_token(&user)
            .map_err(|_| HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error generating token"
            })))?;

        Ok(crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            user_uuid: Some(user.uuid.to_string()),
            token: Some(token),
            user: Some(user.into()),
            message: Some("Login successful".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
        })
    }

    /// Create a response indicating MFA is required
    pub fn create_mfa_required_response(user_uuid: uuid::Uuid) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: Some(true),
            mfa_setup_required: Some(false),
            user_uuid: Some(user_uuid.to_string()),
            token: None,
            user: None,
            message: Some("Multi-factor authentication required".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
        }
    }

    /// Create a response indicating MFA setup is required
    pub fn create_mfa_setup_required_response(user_uuid: uuid::Uuid) -> crate::models::LoginResponse {
        crate::models::LoginResponse {
            success: false,
            mfa_required: Some(false),
            mfa_setup_required: Some(true),
            user_uuid: Some(user_uuid.to_string()),
            token: None,
            user: None,
            message: Some("Multi-factor authentication setup required for your account type".to_string()),
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
        }
    }



    /// Create a successful MFA login response
    pub fn create_mfa_login_response(
        user: User,
        backup_code_used: bool,
        requires_regeneration: bool,
    ) -> Result<crate::models::LoginResponse, HttpResponse> {
        let token = JwtUtils::create_token(&user)
            .map_err(|_| HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error generating token"
            })))?;

        let mut message = "Login successful".to_string();
        if backup_code_used {
            message = if requires_regeneration {
                "Login successful using backup code. You have 2 or fewer backup codes remaining - please regenerate them soon.".to_string()
            } else {
                "Login successful using backup code".to_string()
            };
        }

        Ok(crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            user_uuid: Some(user.uuid.to_string()),
            token: Some(token),
            user: Some(user.into()),
            message: Some(message),
            mfa_backup_code_used: Some(backup_code_used),
            requires_backup_code_regeneration: Some(requires_regeneration),
        })
    }

    /// Middleware helper for role-based access control
    pub async fn require_role(
        auth: &BearerAuth,
        conn: &mut DbConnection,
        required_role: &str,
    ) -> Result<(Claims, User), ActixError> {
        let (claims, user) = JwtUtils::authenticate_request(auth, conn).await?;
        JwtUtils::check_role_permission(&claims, required_role)?;
        Ok((claims, user))
    }

    /// Middleware helper for admin-only access
    pub async fn require_admin(
        auth: &BearerAuth,
        conn: &mut DbConnection,
    ) -> Result<(Claims, User), ActixError> {
        require_role(auth, conn, "admin").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;

    #[test]
    fn test_role_permission_check() {
        let admin_claims = Claims {
            sub: "test-uuid".to_string(),
            name: "Admin User".to_string(),
            email: "admin@test.com".to_string(),
            role: "admin".to_string(),
            exp: 9999999999,
            iat: 1000000000,
        };

        let user_claims = Claims {
            sub: "test-uuid".to_string(),
            name: "Regular User".to_string(),
            email: "user@test.com".to_string(),
            role: "user".to_string(),
            exp: 9999999999,
            iat: 1000000000,
        };

        // Admin can access everything
        assert!(JwtUtils::check_role_permission(&admin_claims, "admin").is_ok());
        assert!(JwtUtils::check_role_permission(&admin_claims, "technician").is_ok());
        assert!(JwtUtils::check_role_permission(&admin_claims, "user").is_ok());

        // User can only access user-level resources
        assert!(JwtUtils::check_role_permission(&user_claims, "user").is_ok());
        assert!(JwtUtils::check_role_permission(&user_claims, "technician").is_err());
        assert!(JwtUtils::check_role_permission(&user_claims, "admin").is_err());
    }
} 