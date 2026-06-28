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
use actix_web::Error;
use tracing::{error, warn};

use crate::db::DbConnection;
use crate::models::Claims;
use actix_web::HttpMessage;

/// Token scope for an authenticated CUSTOMER PORTAL session (a baseline user
/// signed in on the per-tenant portal origin). It is a distinct principal realm
/// from an agent session: a portal token must never authenticate an agent /
/// management request, even though both subjects are `users` rows. The agent
/// membership gate refuses it outright (see [`enforce_workspace_membership`]);
/// portal routes require it via their own auth. Kept here next to the gate that
/// enforces the boundary.
pub const PORTAL_SCOPE: &str = "portal";

/// Item U: resolve-then-gate the request's workspace.
///
/// The request's ORIGIN is authoritative for tenant scope, and resolution is
/// keyed on it:
///
/// - **Origin-derived** (customer portal `<slug>.nosdesk.app`, custom domains,
///   and the self-hosted bootstrap): the `WorkspaceContextMiddleware` already
///   put a `WorkspaceContext` in extensions from the Host. When present it wins
///   outright. A client cannot override its own origin's tenant via a header,
///   so on a tenant origin the selection header is never consulted (it could
///   otherwise 403 on a stray slug and is a cross-tenant confusion vector).
/// - **Selection-derived** (the single-origin agent app at `app.nosdesk.com`,
///   behind `NOSDESK_WORKSPACE_SELECTION`): consulted ONLY when the origin
///   resolved to no workspace — i.e. the agent origin, whose Host is on a
///   different base domain and so never matches a tenant. The client names the
///   workspace in the `X-Nosdesk-Workspace` header; the gate resolves it,
///   membership-checks it, and inserts the resulting `WorkspaceContext`.
///
/// Either way the same membership 403 gate fires: the user must be a member of
/// the resolved workspace or the request is `403 Forbidden` rather than falling
/// through into the app with RLS-filtered-to-empty queries.
///
/// Skipped (Ok) when neither path yields a workspace (apex / public routes that
/// pin nothing); those routes don't touch tenant tables and the strict RLS
/// policy is the secondary guard.
///
/// Called by every authentication middleware (cookie auth + dual auth) so the
/// gate fires on every authenticated entry path.
pub fn enforce_workspace_membership(
    req: &ServiceRequest,
    conn: &mut DbConnection,
    claims: &Claims,
) -> Result<(), Error> {
    // A customer-portal token is a different principal realm and must never
    // authenticate an agent / management request. This gate is the single
    // chokepoint both agent auth paths (cookie + dual) share, so refusing
    // portal scope here walls it off everywhere at once. Belt-and-suspenders
    // behind the separate portal cookie names and the separate portal origin.
    if claims.scope == PORTAL_SCOPE {
        warn!(user = %claims.sub, "Portal-scope token rejected on the agent surface");
        return Err(actix_web::error::ErrorForbidden(
            "This session cannot access the agent application",
        ));
    }

    // Origin-derived context wins when present (tenant portal / custom domain /
    // self-hosted bootstrap). Only when the origin resolved to no workspace
    // (the agent origin) do we consult the selection header. Resolving
    // selection eagerly would 403 a tenant-origin request that carries a stray
    // header, so the ordering here is load-bearing, not just precedence.
    let host_derived = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .map(|w| w.workspace_id);

    let (workspace_id, selected) = match host_derived {
        Some(id) => (id, None),
        None => match selected_workspace_context(req, conn)? {
            Some(ctx) => (ctx.workspace_id, Some(ctx)),
            // No tenant scope to authorize against (agent apex / public route).
            None => return Ok(()),
        },
    };

    // A workspace-scoped request whose subject doesn't parse is malformed; fail
    // closed rather than skip the gate (auth already validated the token, so
    // this is unreachable in practice, but the gate must never fail open).
    let user_uuid = uuid::Uuid::parse_str(&claims.sub)
        .map_err(|_| actix_web::error::ErrorForbidden("Not a member of this workspace"))?;
    require_workspace_membership(conn, workspace_id, user_uuid)?;

    // Membership confirmed: publish the selection-derived context so downstream
    // handlers and pins read the selected workspace, not a Host-derived one.
    if let Some(ctx) = selected {
        req.extensions_mut().insert(ctx);
    }
    Ok(())
}

/// Resolve the `X-Nosdesk-Workspace` selection header (the workspace **slug**,
/// as it appears in the agent app's URL) to a `WorkspaceContext`, or `Ok(None)`
/// when selection resolution is off or the header is absent. An unknown slug is
/// `403 Forbidden`, indistinguishable from a non-member so workspace existence
/// does not leak. The slug is the selection *input*; everything downstream keys
/// off the resolved workspace's id / uuid.
fn selected_workspace_context(
    req: &ServiceRequest,
    conn: &mut DbConnection,
) -> Result<Option<crate::extractors::WorkspaceContext>, Error> {
    use crate::middleware::workspace_context as wc;
    if !wc::selection_resolution_enabled() {
        return Ok(None);
    }
    let Some(slug) = req
        .headers()
        .get(wc::WORKSPACE_SELECTION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    match wc::resolve_selected_context(conn, slug) {
        Ok(Some(ctx)) => Ok(Some(ctx)),
        Ok(None) => Err(actix_web::error::ErrorForbidden(
            "Not a member of this workspace",
        )),
        Err(e) => {
            error!(error = ?e, "Selection-header workspace lookup failed");
            Err(actix_web::error::ErrorInternalServerError(
                "Workspace resolution failed",
            ))
        }
    }
}

/// Fail-closed membership check: `Ok(())` iff `user_uuid` is a member of
/// `workspace_id`, else a 403 (500 on lookup error). Shared by the request gate
/// above and the SSE / collab-WS handlers, which authenticate outside the
/// middleware and so must call this explicitly.
///
/// RLS-pinned read: `workspace_members`' policy is
/// `workspace_id = current_setting('app.workspace_id')`, so on a raw pooled
/// connection (GUC scrubbed on checkout by ResettingManager) a real member would read as "not a
/// member". Running the lookup through `with_actor_context` pins the read to
/// `workspace_id`. Callers that have already session-pinned the SAME workspace
/// (SSE/collab via `pin_request_workspace`) are unaffected: the actor workspace
/// equals the pin, so subsequent reads stay correctly scoped.
pub fn require_workspace_membership(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: uuid::Uuid,
) -> Result<(), Error> {
    let actor = crate::sync::actor::ActorContext::user_at_workspace(user_uuid, workspace_id);
    let lookup = crate::sync::session::with_actor_context(conn, &actor, |c| {
        crate::repository::workspaces::membership(c, workspace_id, user_uuid)
    });
    match lookup {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            warn!(
                user = %user_uuid,
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

/// Cookie OR session-JWT bearer (native/mobile), but NOT `nsk_` personal API
/// tokens — those need [`crate::middleware::dual_auth_middleware`] on their
/// designated routes. A thin wrapper over the shared
/// [`crate::middleware::api_token::authenticate`] path (`accept_api_tokens =
/// false`), which is the single source of truth for credential validation,
/// the scope guard, the membership gate, and context population.
pub async fn cookie_auth_middleware(
    req: ServiceRequest,
    next: actix_web::middleware::Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    crate::middleware::api_token::authenticate(req, next, false).await
}
