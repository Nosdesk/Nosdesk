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

use crate::db::{DbConnection, Pool};
use crate::middleware::request_context;
use crate::models::Claims;
use crate::utils::jwt::JwtUtils;
use actix_web::HttpMessage;

/// Item U: workspace membership 403 gate.
///
/// If the request has a `WorkspaceContext` (hosted mode with the
/// subdomain resolved) and the authenticated user has no
/// `workspace_members` row for that workspace, return
/// `403 Forbidden` instead of letting the request fall through
/// into the app with RLS-filtered-to-empty queries.
///
/// Skipped when no `WorkspaceContext` is present (apex domain,
/// unrecognised subdomain) - those routes shouldn't touch tenant
/// tables anyway, and the strict RLS policy is the secondary
/// guard there.
///
/// Used by every authentication middleware (cookie auth + dual
/// auth) so the gate fires on every authenticated entry path.
pub(crate) fn enforce_workspace_membership(
    req: &ServiceRequest,
    conn: &mut DbConnection,
    claims: &Claims,
) -> Result<(), Error> {
    let Some(workspace_id) = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .map(|w| w.workspace_id)
    else {
        return Ok(());
    };
    let Ok(user_uuid) = uuid::Uuid::parse_str(&claims.sub) else {
        return Ok(());
    };
    // Scope the read to the request's workspace before touching
    // workspace_members. Its RLS policy is
    // `workspace_id = current_setting('app.workspace_id')`, so on a raw
    // pooled connection (where ResetAppGucs cleared the GUC) the
    // membership row is filtered out and a real member reads as "not a
    // member" — a chicken-and-egg, since the gate would establish the
    // tenant scope it needs to read. Run the lookup through
    // `with_actor_context` (the same path every tenant query uses) with the
    // actor pinned to the resolved workspace, so `app.workspace_id` is set
    // and RLS sees the row. The workspace is the one the host already
    // resolved to, not anything user-supplied.
    let actor = crate::sync::actor::ActorContext::user_at_workspace(user_uuid, workspace_id);
    let lookup = crate::sync::session::with_actor_context(conn, &actor, |c| {
        crate::repository::workspaces::membership(c, workspace_id, user_uuid)
    });
    match lookup {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            warn!(
                user = %claims.sub,
                workspace_id,
                "Workspace membership 403 gate: user is not a member; denying"
            );
            Err(actix_web::error::ErrorForbidden(
                "Not a member of this workspace",
            ))
        }
        Err(e) => {
            error!(error = ?e, "Workspace membership lookup failed");
            Err(actix_web::error::ErrorInternalServerError(
                "Workspace membership check failed",
            ))
        }
    }
}

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

    enforce_workspace_membership(&req, &mut conn, &claims)?;

    // Release the pooled connection BEFORE running the handler. Held across
    // next.call() it would pin a connection for the whole request, and since
    // handlers acquire their own, a burst of concurrent requests near the
    // pool size deadlocks (every connection held by a waiting middleware
    // while its handler blocks for a second one).
    drop(conn);

    request_context::populate(&req, &claims);
    req.extensions_mut().insert(claims);

    next.call(req).await
}
