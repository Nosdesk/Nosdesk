//! Sync engine request context extractor.
//!
//! Pulled into every `/api/sync/*` handler. Computes the user's
//! allowed sync groups (workspace + project + group memberships) and
//! attaches a per-request correlation id so the resulting
//! `sync_actions` rows can be stitched back to the originating
//! request.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{Claims, User};

#[derive(Debug, Clone)]
pub struct SyncContext {
    pub user: User,
    /// Group strings the user is permitted to read sync events for —
    /// computed once per request from `crate::sync::groups::allowed_for_user`.
    pub allowed_groups: Vec<String>,
    /// Stable id for the request; flows into `sync_actions.correlation_id`
    /// so a single push or background job's events can be reassembled
    /// into a causal chain. Pulled from the inbound `X-Correlation-Id`
    /// header when present, generated otherwise.
    pub correlation_id: Uuid,
}

#[derive(Debug)]
pub enum SyncContextError {
    Unauthorized,
    InvalidUuid,
    UserNotFound,
    DatabaseError(String),
}

impl std::fmt::Display for SyncContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "Authentication required"),
            Self::InvalidUuid => write!(f, "Invalid user UUID in token"),
            Self::UserNotFound => write!(f, "User not found"),
            Self::DatabaseError(e) => write!(f, "Database error: {e}"),
        }
    }
}

impl actix_web::ResponseError for SyncContextError {
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

impl FromRequest for SyncContext {
    type Error = SyncContextError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let req = req.clone();

        Box::pin(async move {
            let claims = req
                .extensions()
                .get::<Claims>()
                .cloned()
                .ok_or(SyncContextError::Unauthorized)?;

            let user_uuid =
                Uuid::parse_str(&claims.sub).map_err(|_| SyncContextError::InvalidUuid)?;

            let pool = req
                .app_data::<web::Data<Pool>>()
                .ok_or_else(|| SyncContextError::DatabaseError("Pool not found".into()))?;

            let mut conn = pool
                .get()
                .map_err(|e| SyncContextError::DatabaseError(e.to_string()))?;

            // Active-only — F2C.2 H4. Soft-deleted user can't be
            // the actor for a sync session even if they hold a
            // cached auth token.
            let user = crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn)
                .map_err(|_| SyncContextError::UserNotFound)?;

            // allowed_for_user reads `projects` + `user_groups`, both RLS-
            // scoped on `app.workspace_id`. Compute the sync scope inside a
            // workspace-pinned transaction so it reflects this request's
            // workspace, not whatever lingered on the pooled connection;
            // SET LOCAL reverts at commit. No resolved workspace means no
            // sync scope to compute (unpinned read returns user-only groups).
            let allowed_groups = match crate::handlers::helpers::request_workspace_id(&req) {
                Some(ws) => {
                    let actor = crate::sync::actor::ActorContext::user_at_workspace(user_uuid, ws);
                    crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
                        crate::sync::groups::allowed_for_user(c, &user)
                    })
                }
                None => crate::sync::groups::allowed_for_user(&mut conn, &user),
            }
            .map_err(|e| SyncContextError::DatabaseError(e.to_string()))?;

            let correlation_id = req
                .headers()
                .get("X-Correlation-Id")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_else(Uuid::now_v7);

            Ok(SyncContext {
                user,
                allowed_groups,
                correlation_id,
            })
        })
    }
}
