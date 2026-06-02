//! Authentication context extractor
//!
//! Provides a type-safe way to access authenticated user information in handlers.
//! Automatically extracts user details from JWT claims and enriches with database info.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::WorkspaceContext;
use crate::models::{Claims, UserRole, WorkspaceRole};

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
    /// Legacy "global" role projection (Admin, Technician, User,
    /// AuditReviewer). Derived from `platform_role` and the caller's
    /// `workspace_members.role` in the current workspace: platform
    /// admin maps to Admin everywhere; workspace owner/admin → Admin;
    /// agent → Technician; member or no membership → User. Kept so
    /// existing handlers that branch on `auth.role` / `auth.is_admin`
    /// keep working post-W2 without code changes; the W2 split itself
    /// is the source of truth.
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
    /// True for platform admins and for workspace admins/owners in
    /// the current workspace. Existing handlers use this for
    /// admin-gated tenant operations and the gate continues to do
    /// what its name says.
    pub fn is_admin(&self) -> bool {
        self.role == UserRole::Admin
    }

    /// True for anyone at workspace-agent tier or higher (includes
    /// platform admins).
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
                platform_role: None,
                scope: "full".into(),
                sid: None,
                exp: 9999999999,
                iat: 0,
            },
        }
    }
}

/// Derive the legacy `UserRole` projection from the new W2 split:
/// platform-admin → Admin; otherwise the workspace_members role
/// (owner/admin → Admin, agent → Technician, member → User).
/// No membership in the resolved workspace falls back to User.
pub(crate) fn derive_role(
    platform_role: &str,
    workspace_role: Option<&str>,
) -> UserRole {
    if platform_role == "platform_admin" {
        return UserRole::Admin;
    }
    match workspace_role.map(WorkspaceRole::from_db) {
        Some(WorkspaceRole::Owner) | Some(WorkspaceRole::Admin) => UserRole::Admin,
        Some(WorkspaceRole::Agent) => UserRole::Technician,
        Some(WorkspaceRole::Member) | None => UserRole::User,
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

            // Fetch user from database to get platform_role + name +
            // active status. Active-only — F2C.2 H4: a soft-deleted
            // user with a cached cookie/JWT must not get a request-
            // scoped auth context. The error type collapses "row
            // missing" and "row soft-deleted" into one
            // `UserNotFound` to avoid leaking deletion state.
            let user = crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn)
                .map_err(|_| AuthContextError::UserNotFound)?;

            // Fetch user's group memberships
            let group_ids =
                crate::repository::groups::get_group_ids_for_user(&mut conn, &user_uuid)
                    .unwrap_or_default();

            // Look up the caller's role in the workspace the
            // request resolved to (set by WorkspaceContextMiddleware).
            // Apex / unscoped requests have no WorkspaceContext and
            // get a None workspace role — derive_role then falls
            // back to platform_role alone.
            let workspace_role_str = req
                .extensions()
                .get::<WorkspaceContext>()
                .map(|wc| wc.workspace_id)
                .and_then(|workspace_id| {
                    crate::repository::workspaces::membership(&mut conn, workspace_id, user_uuid)
                        .ok()
                        .flatten()
                        .map(|m| m.role)
                });

            let role = derive_role(&user.platform_role, workspace_role_str.as_deref());

            Ok(AuthContext {
                user_uuid,
                role,
                name: user.name,
                group_ids,
                claims,
            })
        })
    }
}
