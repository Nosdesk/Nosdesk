use actix_web::{web, HttpRequest, HttpResponse, HttpMessage};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::models::Claims;

/// Get a database connection from the pool, returning a standard error response on failure.
pub fn db_conn(pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    pool.get().map_err(|_| HttpResponse::InternalServerError().json("Database connection error"))
}

/// Extract claims + user UUID + DB connection from a request.
/// Combines the three most common boilerplate blocks into one call.
pub fn auth_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
) -> Result<(Claims, Uuid, DbConnection), HttpResponse> {
    let claims = req.extensions().get::<Claims>().cloned()
        .ok_or_else(|| HttpResponse::Unauthorized().json("Authentication required"))?;
    let conn = db_conn(pool)?;
    let user_uuid = Uuid::parse_str(&claims.sub)
        .map_err(|_| HttpResponse::InternalServerError().json("Invalid user UUID"))?;
    Ok((claims, user_uuid, conn))
}
