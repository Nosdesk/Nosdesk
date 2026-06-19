//! Customer portal session: establishment and authorization.
//!
//! The portal is the authenticated surface for ticket SUBMITTERS (baseline
//! `users` rows, role `Member`), served per-tenant on `<slug>.nosdesk.app`. It
//! is a separate principal realm from the agent app: portal sessions carry the
//! `portal` token scope (refused on the agent surface, see
//! [`crate::middleware::cookie_auth::enforce_workspace_membership`]) and their
//! own cookie names.
//!
//! This module owns the two security-critical primitives:
//!
//! - [`establish_portal_session`] mints a portal session (token + refresh +
//!   CSRF, reusing the agent session machinery) and sets the portal cookies.
//!   The magic-link callback calls it once email ownership is proven.
//! - [`authorize_portal_request`] is the per-request gate: a valid portal token
//!   whose bound workspace matches the request's resolved ORIGIN, for a user
//!   who is a member of that workspace. Split out (like the agent gate) so it
//!   is unit-testable independently of the actix middleware that will wrap it.

use actix_web::dev::ServiceRequest;
use actix_web::{Error, HttpMessage, HttpRequest, HttpResponse};
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::extractors::WorkspaceContext;
use crate::middleware::cookie_auth::{require_workspace_membership, PORTAL_SCOPE};
use crate::models::{Claims, User};

/// The authenticated portal principal for a request: a customer (`user_uuid`)
/// acting within one workspace (resolved from the portal origin and confirmed
/// against the token's binding). Published into request extensions for portal
/// handlers, the portal analogue of the agent `WorkspaceContext` + `Claims`.
#[derive(Debug, Clone)]
pub struct PortalContext {
    pub user_uuid: Uuid,
    pub workspace_id: i32,
    pub workspace_uuid: Uuid,
}

/// Authorize a portal request from its validated token claims and the
/// origin-resolved workspace context.
///
/// Fail-closed checks, all collapsing to 403 so nothing about workspace or
/// membership existence leaks:
///
/// 1. The token is portal-scoped (an agent token must not act as a customer).
/// 2. The token is workspace-bound and that binding equals the workspace the
///    request's ORIGIN resolved to. This is what stops a portal token minted
///    for tenant A from being replayed onto tenant B's portal origin.
/// 3. The subject is a member of that workspace (the baseline `Member` row a
///    customer holds; reuses the agent membership check, RLS-pinned).
pub fn authorize_portal_request(
    req: &ServiceRequest,
    conn: &mut DbConnection,
    claims: &Claims,
) -> Result<PortalContext, Error> {
    if claims.scope != PORTAL_SCOPE {
        return Err(actix_web::error::ErrorForbidden("Not a portal session"));
    }

    let token_workspace = claims.workspace_uuid.ok_or_else(|| {
        actix_web::error::ErrorForbidden("Portal session is not bound to a workspace")
    })?;

    let origin_ctx = req
        .extensions()
        .get::<WorkspaceContext>()
        .cloned()
        .ok_or_else(|| actix_web::error::ErrorForbidden("No workspace for this origin"))?;

    if token_workspace != origin_ctx.workspace_uuid {
        // Token minted for a different tenant than the origin serves.
        return Err(actix_web::error::ErrorForbidden(
            "Portal session does not match this workspace",
        ));
    }

    let user_uuid = Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorForbidden("Not a member of this workspace"))?;
    require_workspace_membership(conn, origin_ctx.workspace_id, user_uuid)?;

    Ok(PortalContext {
        user_uuid,
        workspace_id: origin_ctx.workspace_id,
        workspace_uuid: origin_ctx.workspace_uuid,
    })
}

/// Establish a customer-portal session for `user` within `workspace_uuid`:
/// create the session record, mint the portal token bundle, and return a JSON
/// response carrying the portal cookies. Called once a login flow (magic-link)
/// has proven the customer's email ownership.
///
/// Reuses the agent session machinery wholesale (`create_session_record`,
/// refresh-token rotation, CSRF); only the access token's scope/binding and the
/// cookie names are portal-specific.
pub fn establish_portal_session(
    user: &User,
    workspace_uuid: Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<HttpResponse, HttpResponse> {
    let session =
        crate::handlers::auth::create_session_record(&user.uuid, request, conn).map_err(|e| {
            tracing::error!(error = ?e, "portal session: failed to create session record");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to establish session"
            }))
        })?;

    let family_id = Uuid::new_v4();
    let tokens = crate::utils::jwt::helpers::create_portal_tokens(
        user,
        workspace_uuid,
        &session.session_id,
        &family_id,
        conn,
    )?;

    Ok(HttpResponse::Ok()
        .cookie(crate::utils::cookies::create_portal_access_cookie(
            &tokens.access_token,
        ))
        .cookie(crate::utils::cookies::create_portal_refresh_cookie(
            &tokens.refresh_token,
        ))
        .cookie(crate::utils::cookies::create_portal_csrf_cookie(
            &tokens.csrf_token,
        ))
        .json(json!({
            "success": true,
            "csrf_token": tokens.csrf_token,
            "workspace_uuid": workspace_uuid,
        })))
}
