use actix_web::{web, HttpRequest, HttpResponse, HttpMessage};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::models::{Claims, User};
use crate::repository;
use crate::utils;

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
    let claims = req.extensions().get::<Claims>().cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))?;
    let conn = db_conn(pool)?;
    let user_uuid = Uuid::parse_str(&claims.sub)
        .map_err(|_| errors::internal("Invalid user UUID"))?;
    Ok((claims, user_uuid, conn))
}

/// Admin-only helper with no target user: enforce admin role, return a
/// pooled DB connection. Use this for admin-settings endpoints that
/// act on *singletons* (site_settings, channels, etc.) rather than a
/// specific target user — the target-user variant [`admin_user_conn`]
/// is for endpoints like "admin updates user X's role."
pub fn admin_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
) -> Result<DbConnection, HttpResponse> {
    let claims = req.extensions().get::<Claims>().cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))?;
    if !crate::utils::rbac::is_admin(&claims) {
        return Err(errors::forbidden("Admin required"));
    }
    db_conn(pool)
}

/// Admin-only helper: authenticate caller, enforce admin role, parse target UUID, load target user.
/// Returns (admin Claims, target User, DbConnection) or an appropriate error response.
pub fn admin_user_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
    target_uuid_str: &str,
) -> Result<(Claims, User, DbConnection), HttpResponse> {
    let (claims, _caller_uuid, mut conn) = auth_conn(req, pool)?;

    if claims.role != "admin" {
        return Err(errors::forbidden("Admin access required"));
    }

    let target_uuid = utils::parse_uuid(target_uuid_str)
        .map_err(|_| errors::bad_request("Invalid UUID format"))?;

    let user = repository::get_user_by_uuid(&target_uuid, &mut conn)
        .map_err(|_| errors::not_found("User"))?;

    Ok((claims, user, conn))
}
