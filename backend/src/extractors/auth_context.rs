//! Authentication context extractor
//!
//! Provides a type-safe way to access authenticated user information in handlers.
//! Automatically extracts user details from JWT claims and enriches with database info.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{Claims, UserRole};

/// Authentication context containing user information and permissions.
///
/// Use this as a handler parameter to get automatic authentication:
/// ```ignore
/// pub async fn my_handler(auth: AuthContext, ...) -> impl Responder {
///     if auth.is_admin() {
///         // admin-only logic
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User's UUID
    pub user_uuid: Uuid,
    /// User's role (Admin, Technician, User)
    pub role: UserRole,
    /// User's display name
    #[allow(dead_code)]
    pub name: String,
    /// Group IDs the user belongs to (for future group-based permissions)
    pub group_ids: Vec<i32>,
    /// Original JWT claims (for access to other fields if needed)
    #[allow(dead_code)]
    claims: Claims,
}

impl AuthContext {
    /// Check if user is an admin
    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }

    /// Check if user is a technician or admin (has elevated privileges)
    pub fn is_technician_or_admin(&self) -> bool {
        self.role == UserRole::Admin || self.role == UserRole::Technician
    }

    /// Construct an AuthContext for tests.
    #[cfg(test)]
    pub fn test_context(user_uuid: Uuid, role: UserRole, group_ids: Vec<i32>) -> Self {
        Self {
            user_uuid,
            role,
            name: "test-user".into(),
            group_ids,
            claims: Claims {
                sub: user_uuid.to_string(),
                name: "test-user".into(),
                email: "test@example.com".into(),
                role: role.as_str().to_string(),
                scope: "full".into(),
                sid: None,
                exp: 9999999999,
                iat: 0,
            },
        }
    }
}

/// Error type for AuthContext extraction failures
#[derive(Debug)]
pub enum AuthContextError {
    /// No authentication token provided
    Unauthorized,
    /// Invalid user UUID in token
    InvalidUuid,
    /// User not found in database
    UserNotFound,
    /// Database error
    DatabaseError(String),
}

impl std::fmt::Display for AuthContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "Authentication required"),
            Self::InvalidUuid => write!(f, "Invalid user UUID in token"),
            Self::UserNotFound => write!(f, "User not found"),
            Self::DatabaseError(e) => write!(f, "Database error: {e}"),
        }
    }
}

impl actix_web::ResponseError for AuthContextError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::HttpResponse;
        match self {
            Self::Unauthorized => HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"})),
            Self::InvalidUuid => {
                HttpResponse::BadRequest().json(serde_json::json!({"error": "Invalid user UUID"}))
            }
            Self::UserNotFound => {
                HttpResponse::NotFound().json(serde_json::json!({"error": "User not found"}))
            }
            Self::DatabaseError(_) => HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Internal server error"})),
        }
    }
}

impl FromRequest for AuthContext {
    type Error = AuthContextError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let req = req.clone();

        Box::pin(async move {
            // Extract claims from request extensions (set by JWT middleware)
            let claims = req
                .extensions()
                .get::<Claims>()
                .cloned()
                .ok_or(AuthContextError::Unauthorized)?;

            // Parse user UUID
            let user_uuid =
                Uuid::parse_str(&claims.sub).map_err(|_| AuthContextError::InvalidUuid)?;

            // Get database pool
            let pool = req
                .app_data::<web::Data<Pool>>()
                .ok_or(AuthContextError::DatabaseError("Pool not found".into()))?;

            // Get database connection
            let mut conn = pool
                .get()
                .map_err(|e| AuthContextError::DatabaseError(e.to_string()))?;

            // Fetch user from database to get current role and groups.
            // Active-only — F2C.2 H4: a soft-deleted user with a
            // cached cookie/JWT must not get a request-scoped auth
            // context. The error type collapses "row missing" and
            // "row soft-deleted" into one `UserNotFound` to avoid
            // leaking deletion state.
            let user = crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn)
                .map_err(|_| AuthContextError::UserNotFound)?;

            // Fetch user's group memberships
            let group_ids =
                crate::repository::groups::get_group_ids_for_user(&mut conn, &user_uuid)
                    .unwrap_or_default();

            Ok(AuthContext {
                user_uuid,
                role: user.role,
                name: user.name,
                group_ids,
                claims,
            })
        })
    }
}
