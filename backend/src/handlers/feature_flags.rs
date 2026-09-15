//! Feature flag endpoints.
//!
//! - `GET  /api/feature-flags` — current user's resolved flags. Auth required.
//! - `PATCH /api/admin/feature-flags` — set workspace-level flags (admin).
//! - `PATCH /api/admin/feature-flags/users/{uuid}` — set per-user overrides (admin).
//!
//! The resolved-flags endpoint is fetched once after auth and cached
//! by the frontend store; the SSE bus broadcasts a `feature_flags_changed`
//! event when admin endpoints write, prompting affected clients to
//! refetch. Bus wiring is a Phase 2 concern; v1 callers refresh on
//! navigation.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::errors::ApiError;
use crate::handlers::helpers;
use crate::models::Claims;
use crate::repository::feature_flags as repo;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/feature-flags",
        web::get().to(crate::handlers::feature_flags::get_my_flags),
    )
    .route(
        "/admin/feature-flags",
        web::patch().to(crate::handlers::feature_flags::patch_workspace_flag),
    )
    .route(
        "/admin/feature-flags",
        web::put().to(crate::handlers::feature_flags::put_workspace_flags),
    )
    .route(
        "/admin/feature-flags/users/{uuid}",
        web::patch().to(crate::handlers::feature_flags::patch_user_override),
    );
}

#[derive(Debug, Deserialize)]
pub struct PatchFlagBody {
    pub flag: String,
    /// `null` clears the flag back to the next layer's value.
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ReplaceFlagsBody {
    pub flags: Value,
}

/// GET /api/feature-flags — resolved flag map for the current user.
pub async fn get_my_flags(
    pool: web::Data<Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let (_claims, user_uuid, mut conn) = helpers::auth_conn(&req, &pool)?;

    // Scope the read to the request's workspace: site_settings is
    // RLS-isolated by workspace_id, so the resolver must run with
    // app.workspace_id set or it reads zero rows (the hosted-mode 500).
    let actor = helpers::actor_for(&req, "feature_flags:read");
    let resolved = crate::sync::session::with_actor_context(&mut conn, &actor, |conn| {
        repo::resolve_for_user(conn, &user_uuid)
    });
    match resolved {
        Ok(flags) => Ok(HttpResponse::Ok().json(flags)),
        Err(e) => {
            error!(error = %e, user = %user_uuid, "failed to resolve feature flags");
            Err(ApiError::Internal("Failed to resolve feature flags".into()))
        }
    }
}

/// PATCH /api/admin/feature-flags — set or clear a single workspace flag.
pub async fn patch_workspace_flag(
    pool: web::Data<Pool>,
    body: web::Json<PatchFlagBody>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::admin_conn(&req, &pool)?;

    if body.flag.trim().is_empty() {
        return Err(ApiError::BadRequest("Flag name is required".into()));
    }

    let actor_uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());

    let actor = helpers::actor_for(&req, "feature_flags:write");
    let updated = crate::sync::session::with_actor_context(&mut conn, &actor, |conn| {
        repo::set_workspace_flag(conn, &body.flag, body.value.clone())
    });
    match updated {
        Ok(flags) => {
            info!(
                actor = ?actor_uuid,
                flag = %body.flag,
                cleared = body.value.is_none(),
                "workspace feature flag updated"
            );
            Ok(HttpResponse::Ok().json(flags))
        }
        Err(e) => {
            error!(error = %e, flag = %body.flag, "failed to set workspace feature flag");
            Err(ApiError::Internal("Failed to update feature flag".into()))
        }
    }
}

/// PUT /api/admin/feature-flags — replace the entire workspace flag map.
pub async fn put_workspace_flags(
    pool: web::Data<Pool>,
    body: web::Json<ReplaceFlagsBody>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::admin_conn(&req, &pool)?;

    if !body.flags.is_object() {
        return Err(ApiError::BadRequest("flags must be a JSON object".into()));
    }

    let actor_uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());

    let actor = helpers::actor_for(&req, "feature_flags:write");
    let replaced = crate::sync::session::with_actor_context(&mut conn, &actor, |conn| {
        repo::set_all_workspace_flags(conn, body.flags.clone())
    });
    match replaced {
        Ok(flags) => {
            info!(actor = ?actor_uuid, "workspace feature flags replaced");
            Ok(HttpResponse::Ok().json(flags))
        }
        Err(e) => {
            error!(error = %e, "failed to replace workspace feature flags");
            Err(ApiError::Internal("Failed to update feature flags".into()))
        }
    }
}

/// PATCH /api/admin/feature-flags/users/{uuid} — set or clear a per-user flag override.
pub async fn patch_user_override(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<PatchFlagBody>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let target_uuid_str = path.into_inner();

    // `admin_conn` proves the caller is an admin, but says nothing about WHOSE
    // admin. `users` carries no RLS, and `set_user_override` writes by uuid, so
    // that alone let an admin of one workspace set flags on a member of
    // another. `admin_user_conn` is the established gate for "an admin acting on
    // a target user": it adds the membership check, resolves the target, and is
    // the same helper the MFA and passkey recovery routes already use.
    let (claims, target, mut conn) = helpers::admin_user_conn(&req, &pool, &target_uuid_str)?;
    let target_uuid = target.uuid;

    if body.flag.trim().is_empty() {
        return Err(ApiError::BadRequest("Flag name is required".into()));
    }

    let actor_uuid = Uuid::parse_str(&claims.sub).ok();

    match repo::set_user_override(&mut conn, &target_uuid, &body.flag, body.value.clone()) {
        Ok(overrides) => {
            info!(
                actor = ?actor_uuid,
                target = %target_uuid,
                flag = %body.flag,
                cleared = body.value.is_none(),
                "user feature flag override updated"
            );
            Ok(HttpResponse::Ok().json(overrides))
        }
        Err(diesel::result::Error::NotFound) => Err(ApiError::NotFoundMsg("User not found".into())),
        Err(e) => {
            error!(error = %e, target = %target_uuid, "failed to set user feature flag override");
            Err(ApiError::Internal(
                "Failed to update user feature flag override".into(),
            ))
        }
    }
}
