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

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::models::Claims;
use crate::repository::feature_flags as repo;

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
pub async fn get_my_flags(pool: web::Data<Pool>, req: HttpRequest) -> impl Responder {
    let (_claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    match repo::resolve_for_user(&mut conn, &user_uuid) {
        Ok(flags) => HttpResponse::Ok().json(flags),
        Err(e) => {
            error!(error = %e, user = %user_uuid, "failed to resolve feature flags");
            errors::internal("Failed to resolve feature flags")
        }
    }
}

/// PATCH /api/admin/feature-flags — set or clear a single workspace flag.
pub async fn patch_workspace_flag(
    pool: web::Data<Pool>,
    body: web::Json<PatchFlagBody>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if body.flag.trim().is_empty() {
        return errors::bad_request("Flag name is required");
    }

    let actor_uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());

    match repo::set_workspace_flag(&mut conn, &body.flag, body.value.clone()) {
        Ok(flags) => {
            info!(
                actor = ?actor_uuid,
                flag = %body.flag,
                cleared = body.value.is_none(),
                "workspace feature flag updated"
            );
            HttpResponse::Ok().json(flags)
        }
        Err(e) => {
            error!(error = %e, flag = %body.flag, "failed to set workspace feature flag");
            errors::internal("Failed to update feature flag")
        }
    }
}

/// PUT /api/admin/feature-flags — replace the entire workspace flag map.
pub async fn put_workspace_flags(
    pool: web::Data<Pool>,
    body: web::Json<ReplaceFlagsBody>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if !body.flags.is_object() {
        return errors::bad_request("flags must be a JSON object");
    }

    let actor_uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());

    match repo::set_all_workspace_flags(&mut conn, body.flags.clone()) {
        Ok(flags) => {
            info!(actor = ?actor_uuid, "workspace feature flags replaced");
            HttpResponse::Ok().json(flags)
        }
        Err(e) => {
            error!(error = %e, "failed to replace workspace feature flags");
            errors::internal("Failed to update feature flags")
        }
    }
}

/// PATCH /api/admin/feature-flags/users/{uuid} — set or clear a per-user flag override.
pub async fn patch_user_override(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<PatchFlagBody>,
    req: HttpRequest,
) -> impl Responder {
    let target_uuid_str = path.into_inner();
    let target_uuid = match Uuid::parse_str(&target_uuid_str) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if body.flag.trim().is_empty() {
        return errors::bad_request("Flag name is required");
    }

    let actor_uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());

    match repo::set_user_override(&mut conn, &target_uuid, &body.flag, body.value.clone()) {
        Ok(overrides) => {
            info!(
                actor = ?actor_uuid,
                target = %target_uuid,
                flag = %body.flag,
                cleared = body.value.is_none(),
                "user feature flag override updated"
            );
            HttpResponse::Ok().json(overrides)
        }
        Err(diesel::result::Error::NotFound) => errors::not_found_msg("User not found"),
        Err(e) => {
            error!(error = %e, target = %target_uuid, "failed to set user feature flag override");
            errors::internal("Failed to update user feature flag override")
        }
    }
}
