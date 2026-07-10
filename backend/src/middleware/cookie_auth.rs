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

/// Where a connection surface's tenant scope comes from. Each surface supplies
/// exactly ONE; [`resolve_pin_and_gate`] owns turning it into a pinned, gated
/// workspace. Adding a surface means adding a carrier arm here rather than
/// re-implementing resolve + pin + gate (and risking forgetting one).
pub enum WorkspaceCarrier<'a> {
    /// REST cookie/bearer: the Host-derived `WorkspaceContext` in extensions
    /// wins (customer portal, custom domains, self-hosted bootstrap); else the
    /// `X-Nosdesk-Workspace` selection slug (single-origin agent app). A client
    /// cannot override its own origin's tenant with a header, so the header is
    /// consulted ONLY when the origin resolved to no workspace.
    RequestOrigin(&'a ServiceRequest),
    /// A connection surface (SSE, collab WS) that authenticates outside the
    /// middleware. `token_workspace` is the workspace uuid the connection names
    /// (the SSE token's binding, or the collab docId's workspace when there is
    /// no Host context); when `None`, the Host-derived context on the request is
    /// authoritative. This encodes "an explicit workspace selection wins, else
    /// fall back to the request origin" for both surfaces.
    ConnectionToken {
        token_workspace: Option<uuid::Uuid>,
        req: &'a actix_web::HttpRequest,
    },
}

/// What to do when a carrier resolves to NO workspace. Fail-closed by default: a
/// new surface reaching for [`UnresolvedPolicy::Deny`] (the value collab uses,
/// the simplest example to copy) gets the safe behaviour.
/// [`UnresolvedPolicy::AllowRlsBackstop`] is the deliberately-named opt-in,
/// legitimate ONLY where per-query RLS is the real boundary (REST, where the
/// Host is authoritative) or where selection is off (legacy Host-derived SSE);
/// every use MUST carry a justifying comment at the call site.
pub enum UnresolvedPolicy {
    Deny,
    AllowRlsBackstop,
}

/// Outcome of [`resolve_pin_and_gate`].
pub enum GateOutcome {
    /// The caller is a member of a resolved workspace; `conn` is now pinned to
    /// it and the caller may read tenant content. Carries the resolved context.
    Scoped(crate::extractors::WorkspaceContext),
    /// Only via [`UnresolvedPolicy::AllowRlsBackstop`]: no workspace resolved,
    /// nothing pinned.
    Unscoped,
}

/// The single connection-authentication funnel: resolve the carrier's workspace,
/// pin it, then membership-gate it, in that order (no tenant read between pin
/// and gate, so pinning a client-selected workspace cannot leak). Portal-scope
/// tokens are refused (a different principal realm). This is the ONLY public
/// path to the membership gate, so a new surface cannot be wired without it.
pub fn resolve_pin_and_gate(
    conn: &mut DbConnection,
    claims: &Claims,
    carrier: WorkspaceCarrier<'_>,
    unresolved: UnresolvedPolicy,
) -> Result<GateOutcome, Error> {
    // A customer-portal token is a different principal realm and must never
    // authenticate an agent / management request; refusing it here walls it off
    // on every surface that funnels through the gate.
    if claims.scope == PORTAL_SCOPE {
        warn!(user = %claims.sub, "Portal-scope token rejected on the agent surface");
        return Err(actix_web::error::ErrorForbidden(
            "This session cannot access the agent application",
        ));
    }

    match resolve_carrier(conn, carrier)? {
        Some(ctx) => {
            // A workspace-scoped request whose subject doesn't parse is
            // malformed; fail closed (auth already validated the token, so this
            // is unreachable in practice, but the gate must never fail open).
            let user_uuid = uuid::Uuid::parse_str(&claims.sub)
                .map_err(|_| actix_web::error::ErrorForbidden("Not a member of this workspace"))?;
            crate::handlers::helpers::pin_workspace(conn, ctx.workspace_id);
            require_workspace_membership(conn, ctx.workspace_id, user_uuid)?;
            Ok(GateOutcome::Scoped(ctx))
        }
        None => match unresolved {
            UnresolvedPolicy::Deny => Err(actix_web::error::ErrorForbidden(
                "Not a member of this workspace",
            )),
            UnresolvedPolicy::AllowRlsBackstop => Ok(GateOutcome::Unscoped),
        },
    }
}

/// Resolve a carrier to its workspace context. `Ok(None)` = no workspace
/// identifier at all (the caller's [`UnresolvedPolicy`] decides); `Err(403)` = a
/// PROVIDED identifier that is unknown (existence not leaked).
fn resolve_carrier(
    conn: &mut DbConnection,
    carrier: WorkspaceCarrier<'_>,
) -> Result<Option<crate::extractors::WorkspaceContext>, Error> {
    match carrier {
        WorkspaceCarrier::RequestOrigin(req) => {
            if let Some(ctx) = req
                .extensions()
                .get::<crate::extractors::WorkspaceContext>()
                .cloned()
            {
                return Ok(Some(ctx));
            }
            selected_workspace_context(req, conn)
        }
        WorkspaceCarrier::ConnectionToken {
            token_workspace,
            req,
        } => match token_workspace {
            Some(uuid) => resolve_provided_uuid(conn, uuid).map(Some),
            None => Ok(req
                .extensions()
                .get::<crate::extractors::WorkspaceContext>()
                .cloned()),
        },
    }
}

/// Resolve a provided workspace uuid, mapping an unknown uuid to the same 403 a
/// non-member gets (no existence leak).
fn resolve_provided_uuid(
    conn: &mut DbConnection,
    uuid: uuid::Uuid,
) -> Result<crate::extractors::WorkspaceContext, Error> {
    match crate::middleware::workspace_context::resolve_workspace_uuid(conn, uuid) {
        Ok(Some(ctx)) => Ok(ctx),
        Ok(None) => Err(actix_web::error::ErrorForbidden(
            "Not a member of this workspace",
        )),
        Err(e) => {
            error!(error = ?e, "Workspace uuid resolution failed");
            Err(actix_web::error::ErrorInternalServerError(
                "Workspace resolution failed",
            ))
        }
    }
}

/// The REST membership gate: resolve the request's workspace (Host-derived, else
/// selection slug), pin it, and gate it. Fail-open when no workspace resolves —
/// the Host is authoritative for tenant scope and the per-query RLS policy is
/// the backstop, so apex / public routes fall through. Called by both agent auth
/// middlewares (cookie + dual) so the gate fires on every authenticated entry.
pub fn enforce_workspace_membership(
    req: &ServiceRequest,
    conn: &mut DbConnection,
    claims: &Claims,
) -> Result<(), Error> {
    match resolve_pin_and_gate(
        conn,
        claims,
        WorkspaceCarrier::RequestOrigin(req),
        // AllowRlsBackstop: REST is Host-authoritative + RLS-backstopped, so an
        // unresolved workspace is a legitimate apex / public route.
        UnresolvedPolicy::AllowRlsBackstop,
    )? {
        // Publish the resolved (possibly selection-derived) context so
        // downstream handlers + pins read the selected workspace. Re-inserting a
        // Host-derived context is a harmless no-op.
        GateOutcome::Scoped(ctx) => {
            req.extensions_mut().insert(ctx);
        }
        GateOutcome::Unscoped => {}
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
/// `workspace_id`, else a 403 (500 on lookup error). This is the raw gate behind
/// [`resolve_pin_and_gate`]; **agent surfaces (REST / SSE / collab) reach it only
/// through that funnel**, so a new connection surface can't be wired without
/// resolve + pin + gate. The customer portal (a distinct principal realm with
/// its own origin + `PORTAL_SCOPE`) is the one intentional direct caller.
///
/// RLS-pinned read: `workspace_members`' policy is
/// `workspace_id = current_setting('app.workspace_id')`, so on a raw pooled
/// connection (GUC scrubbed on checkout by ResettingManager) a real member would read as "not a
/// member". Running the lookup through `with_actor_context` pins the read to
/// `workspace_id`. Callers that have already session-pinned the SAME workspace
/// (the funnel via `pin_workspace`) are unaffected: the actor workspace
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
