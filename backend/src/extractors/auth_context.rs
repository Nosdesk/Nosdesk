//! Authentication context extractor
//!
//! Provides a type-safe way to access authenticated user information in handlers.
//! Automatically extracts user details from JWT claims and enriches with database info.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use futures::future::{ok, Ready};
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
    pub name: String,
    /// Group IDs the user belongs to (for future group-based permissions)
    pub group_ids: Vec<i32>,
    /// Original JWT claims (for access to other fields if needed)
    claims: Claims,
}

impl AuthContext {
    /// Check if user is an admin
    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }

    /// Check if user is a technician
    pub fn is_technician(&self) -> bool {
        self.role == UserRole::Technician
    }

    /// Check if user is a technician or admin (has elevated privileges)
    pub fn is_technician_or_admin(&self) -> bool {
        self.role == UserRole::Admin || self.role == UserRole::Technician
    }

    /// Check if user is a regular user (no elevated privileges)
    pub fn is_regular_user(&self) -> bool {
        self.role == UserRole::User
    }

    /// Get the user's email from claims
    pub fn email(&self) -> &str {
        &self.claims.email
    }

    /// Check if user can view a specific ticket
    /// Returns true if user is admin/tech, or is the requester/assignee
    pub fn can_view_ticket(&self, requester_uuid: Option<Uuid>, assignee_uuid: Option<Uuid>) -> bool {
        if self.is_technician_or_admin() {
            return true;
        }
        requester_uuid == Some(self.user_uuid) || assignee_uuid == Some(self.user_uuid)
    }

    /// Check if user belongs to a specific group
    pub fn is_in_group(&self, group_id: i32) -> bool {
        self.group_ids.contains(&group_id)
    }

    /// Check if user belongs to any of the specified groups
    pub fn is_in_any_group(&self, group_ids: &[i32]) -> bool {
        group_ids.iter().any(|id| self.group_ids.contains(id))
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
            Self::DatabaseError(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl actix_web::ResponseError for AuthContextError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::HttpResponse;
        match self {
            Self::Unauthorized => HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"})),
            Self::InvalidUuid => HttpResponse::BadRequest()
                .json(serde_json::json!({"error": "Invalid user UUID"})),
            Self::UserNotFound => HttpResponse::NotFound()
                .json(serde_json::json!({"error": "User not found"})),
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
            let user_uuid = Uuid::parse_str(&claims.sub)
                .map_err(|_| AuthContextError::InvalidUuid)?;

            // Get database pool
            let pool = req
                .app_data::<web::Data<Pool>>()
                .ok_or(AuthContextError::DatabaseError("Pool not found".into()))?;

            // Get database connection
            let mut conn = pool
                .get()
                .map_err(|e| AuthContextError::DatabaseError(e.to_string()))?;

            // Fetch user from database to get current role and groups
            let user = crate::repository::users::get_user_by_uuid(&user_uuid, &mut conn)
                .map_err(|_| AuthContextError::UserNotFound)?;

            // Fetch user's group memberships
            let group_ids = crate::repository::groups::get_group_ids_for_user(&mut conn, &user_uuid)
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

/// Optional auth context - doesn't fail if user is not authenticated
/// Useful for endpoints that behave differently for authenticated vs anonymous users
#[derive(Debug, Clone)]
pub struct OptionalAuthContext(pub Option<AuthContext>);

impl FromRequest for OptionalAuthContext {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Try to extract claims, but don't fail if not present
        let auth = req
            .extensions()
            .get::<Claims>()
            .and_then(|claims| {
                let user_uuid = Uuid::parse_str(&claims.sub).ok()?;

                // For optional auth, we use a simplified context without DB lookup
                // to avoid blocking. The role comes from claims.
                let role = match claims.role.as_str() {
                    "admin" => UserRole::Admin,
                    "technician" => UserRole::Technician,
                    _ => UserRole::User,
                };

                Some(AuthContext {
                    user_uuid,
                    role,
                    name: claims.name.clone(),
                    group_ids: vec![], // Not loaded for optional auth
                    claims: claims.clone(),
                })
            });

        ok(OptionalAuthContext(auth))
    }
}
