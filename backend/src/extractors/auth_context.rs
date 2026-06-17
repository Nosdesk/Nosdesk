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
use crate::models::{Claims, PlatformRole, WorkspaceRole};

/// Authentication context containing user information and permissions.
///
/// Use this as a handler parameter to get automatic authentication:
/// ```ignore
/// pub async fn my_handler(auth: AuthContext, ...) -> impl Responder {
///     if auth.is_workspace_admin() {
///         // admin-only logic
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User's UUID
    pub user_uuid: Uuid,
    /// Platform-wide privilege role (platform_admin / audit_reviewer /
    /// user), read from `users.platform_role`. Governs instance-wide
    /// gates (workspace lifecycle, audit surface).
    pub platform_role: PlatformRole,
    /// The caller's role in the workspace this request resolved to
    /// (set by WorkspaceContextMiddleware). `None` for apex / unscoped
    /// requests with no WorkspaceContext, or when the user has no
    /// membership in the resolved workspace.
    pub workspace_role: Option<WorkspaceRole>,
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
    /// True only for instance-wide platform admins. Use for ops that
    /// transcend a single workspace (workspace lifecycle, instance
    /// settings).
    pub fn is_platform_admin(&self) -> bool {
        self.platform_role.is_platform_admin()
    }

    /// Workspace-admin tier: platform admins, plus workspace
    /// owners/admins in the resolved workspace. Use for managing a
    /// workspace's members, settings, and entities.
    pub fn is_workspace_admin(&self) -> bool {
        self.is_platform_admin()
            || self
                .workspace_role
                .is_some_and(|r| r.meets(WorkspaceRole::Admin))
    }

    /// Agent tier or higher: anyone who can handle tickets in the
    /// resolved workspace (agent / admin / owner), plus platform
    /// admins.
    pub fn can_handle_tickets(&self) -> bool {
        self.is_platform_admin()
            || self
                .workspace_role
                .is_some_and(|r| r.meets(WorkspaceRole::Agent))
    }

    /// Role half of the audit-read gate (platform admin or audit
    /// reviewer). Callers still AND this with the `audit:read` scope.
    pub fn can_read_audit(&self) -> bool {
        self.platform_role.can_read_audit()
    }

    /// Construct an AuthContext for tests from a legacy role token.
    /// Maps the token onto the platform + bootstrap-workspace role
    /// split, preserving the pre-W2 fixture semantics: admin ->
    /// platform_admin + ws admin; technician -> ws agent;
    /// audit_reviewer -> platform audit reviewer; everything else ->
    /// plain ws member.
    #[cfg(test)]
    pub fn test_context(user_uuid: Uuid, role: &str, group_ids: Vec<i32>) -> Self {
        let (platform_role, workspace_role) = match role {
            "admin" => (PlatformRole::PlatformAdmin, Some(WorkspaceRole::Admin)),
            "technician" => (PlatformRole::User, Some(WorkspaceRole::Agent)),
            "audit_reviewer" => (PlatformRole::AuditReviewer, Some(WorkspaceRole::Member)),
            _ => (PlatformRole::User, Some(WorkspaceRole::Member)),
        };
        Self {
            user_uuid,
            platform_role,
            workspace_role,
            name: "test-user".into(),
            group_ids,
            claims: Claims {
                sub: user_uuid.to_string(),
                name: "test-user".into(),
                email: "test@example.com".into(),
                platform_role: platform_role.as_str().to_string(),
                scope: "full".into(),
                sid: None,
                workspace_uuid: None,
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
            let workspace_role = req
                .extensions()
                .get::<WorkspaceContext>()
                .map(|wc| wc.workspace_id)
                .and_then(|workspace_id| {
                    crate::repository::workspaces::membership(&mut conn, workspace_id, user_uuid)
                        .ok()
                        .flatten()
                        .map(|m| WorkspaceRole::from_db(&m.role))
                });

            Ok(AuthContext {
                user_uuid,
                platform_role: PlatformRole::from_db(&user.platform_role),
                workspace_role,
                name: user.name,
                group_ids,
                claims,
            })
        })
    }
}
