use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::models::{Claims, User};
use crate::repository;
use crate::sync::actor::ActorContext;
use crate::utils;

/// Default page size for list endpoints when the caller doesn't
/// specify one. Twenty is a sensible default for tabular UIs.
pub const DEFAULT_LIMIT: i64 = 20;

/// Cap on caller-supplied limits. Anything larger gets clamped down
/// so a single request can't run an unbounded query.
pub const MAX_LIMIT: i64 = 200;

/// Apply default + cap to a caller-supplied limit. Every list
/// endpoint should funnel its `?limit=` through this so the
/// worst-case query cost is bounded uniformly.
pub fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// Clamp a caller-supplied offset to a non-negative value. Pairs
/// with [`clamp_limit`] for offset-based pagination.
pub fn clamp_offset(offset: Option<i64>) -> i64 {
    offset.unwrap_or(0).max(0)
}

/// Get a database connection from the pool. Re-exports the
/// canonical implementation in [`errors::db_conn`] so existing call
/// sites keep working — pool exhaustion now returns a 503 with a
/// structured error body and a Retry-After header instead of a
/// generic 500.
pub fn db_conn(pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    errors::db_conn(pool)
}

/// Extract claims + user UUID + DB connection from a request.
/// Combines the three most common boilerplate blocks into one call.
pub fn auth_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
) -> Result<(Claims, Uuid, DbConnection), HttpResponse> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))?;
    let conn = db_conn(pool)?;
    let user_uuid =
        Uuid::parse_str(&claims.sub).map_err(|_| errors::internal("Invalid user UUID"))?;
    Ok((claims, user_uuid, conn))
}

/// Admin-only helper with no target user: enforce admin role, return a
/// pooled DB connection. Use this for admin-settings endpoints that
/// act on *singletons* (site_settings, channels, etc.) rather than a
/// specific target user — the target-user variant [`admin_user_conn`]
/// is for endpoints like "admin updates user X's role."
pub fn admin_conn(req: &HttpRequest, pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))?;
    let mut conn = db_conn(pool)?;
    // Admin tier = platform admin, or workspace admin/owner in the
    // bootstrap workspace (the claims carry only the platform role, so
    // the workspace half is looked up). Mirrors the old derived-admin
    // gate now that the legacy UserRole projection is gone.
    let is_admin = crate::utils::rbac::is_platform_admin(&claims)
        || crate::utils::parse_uuid(&claims.sub)
            .ok()
            .and_then(|uuid| {
                crate::repository::user_helpers::bootstrap_workspace_role(&mut conn, uuid)
            })
            .is_some_and(|r| r.meets(crate::models::WorkspaceRole::Admin));
    if !is_admin {
        return Err(errors::forbidden("Admin required"));
    }
    Ok(conn)
}

/// Admin-only helper: authenticate caller, enforce admin role, parse target UUID, load target user.
/// Returns (admin Claims, target User, DbConnection) or an appropriate error response.
pub fn admin_user_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
    target_uuid_str: &str,
) -> Result<(Claims, User, DbConnection), HttpResponse> {
    let (claims, _caller_uuid, mut conn) = auth_conn(req, pool)?;

    if !crate::utils::rbac::is_platform_admin(&claims) {
        return Err(errors::forbidden("Admin access required"));
    }

    let target_uuid = utils::parse_uuid(target_uuid_str)
        .map_err(|_| errors::bad_request("Invalid UUID format"))?;

    let user = repository::get_user_by_uuid(&target_uuid, &mut conn)
        .map_err(|_| errors::not_found("User"))?;

    Ok((claims, user, conn))
}

/// Build an `ActorContext` from the JWT claims attached to the
/// request. Falls back to a system actor named after `system_ref`
/// when no claims are present (background tasks, public endpoints
/// that wandered into a write path, etc.) — this guarantees every
/// emit gets attributed to *something* rather than producing rows
/// with nil-UUID actors that pollute audit queries.
///
/// The actor's `workspace_id` is sourced from the `RequestContext`
/// that the auth middleware populates, which carries the pin set
/// by `WorkspaceContextMiddleware` ahead of auth. Without this,
/// downstream `with_actor_context` calls leave `app.workspace_id`
/// unset and the strict RLS policy returns zero rows (the failure
/// mode that hit the user-purge handlers before this fix). Tests
/// that inject `Claims` directly without `RequestContext` get the
/// ambient workspace GUC set by `setup_test_connection` instead.
pub fn actor_for(req: &HttpRequest, system_ref: &'static str) -> ActorContext {
    let uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());
    // Prefer the auth-populated RequestContext actor pin; fall back to the
    // WorkspaceContext that WorkspaceContextMiddleware sets on EVERY route
    // (including public, unauthenticated ones), so workspace-scoped reads
    // work outside the auth middleware too.
    let workspace_id = req
        .extensions()
        .get::<crate::middleware::RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id)
        .or_else(|| {
            req.extensions()
                .get::<crate::extractors::WorkspaceContext>()
                .map(|w| w.workspace_id)
        });
    let mut actor = match uuid {
        Some(u) => ActorContext::user(u, None),
        None => ActorContext::system(system_ref),
    };
    if let Some(ws) = workspace_id {
        actor = actor.with_workspace(ws);
    }
    actor
}
