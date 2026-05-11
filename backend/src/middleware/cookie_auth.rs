//! Cookie-based authentication middleware.
//!
//! Reads the access-token httpOnly cookie set by `/api/auth/login`,
//! validates it with [`crate::utils::jwt::JwtUtils`], inserts the
//! resulting [`Claims`] + a [`RequestContext`] into request extensions,
//! and records user attribution on the active tracing span.
//!
//! Routes that need to accept either a cookie OR a Bearer token use
//! [`crate::middleware::dual_auth_middleware`] instead. The two auth
//! flows share the same context-population path
//! ([`crate::middleware::request_context::populate`]) so attribution is
//! uniform across both surfaces.

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{web, Error};
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::middleware::request_context;
use crate::utils::jwt::JwtUtils;
use actix_web::HttpMessage;

pub async fn cookie_auth_middleware(
    req: ServiceRequest,
    next: actix_web::middleware::Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let pool = req
        .app_data::<web::Data<Pool>>()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("Database pool not found"))?;

    let mut conn = pool
        .get()
        .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;

    let cookie_names: Vec<String> = req
        .cookies()
        .map(|jar| jar.iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    debug!(
        path = %req.path(),
        cookies = ?cookie_names,
        "Cookie auth middleware processing request"
    );

    let token = req
        .cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| {
            warn!(path = %req.path(), "Cookie auth: no access_token cookie found");
            actix_web::error::ErrorUnauthorized("Authentication required")
        })?;

    debug!("Cookie auth: validating token from cookie");

    let (claims, _user) = JwtUtils::authenticate_with_token(token.value(), &mut conn)
        .await
        .map_err(|err| {
            error!(error = ?err, "Cookie auth: token validation failed");
            actix_web::error::ErrorUnauthorized("Invalid or expired token")
        })?;

    info!(user = %claims.sub, "Cookie auth: user authenticated successfully");

    request_context::populate(&req, &claims);
    req.extensions_mut().insert(claims);

    next.call(req).await
}
