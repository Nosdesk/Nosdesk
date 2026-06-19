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

use actix_web::cookie::Cookie;
use actix_web::dev::ServiceRequest;
use actix_web::{web, Error, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::extractors::WorkspaceContext;
use crate::handlers::errors;
use crate::middleware::cookie_auth::{require_workspace_membership, PORTAL_SCOPE};
use crate::models::{Claims, User};
use crate::utils::reset_tokens::{ResetTokenUtils, TokenType};

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

/// The cookies + CSRF value that make up a freshly minted portal session,
/// ready to attach to either a JSON response (XHR) or a redirect (the
/// magic-link callback's top-level navigation).
struct PortalSessionCookies {
    access: Cookie<'static>,
    refresh: Cookie<'static>,
    csrf: Cookie<'static>,
    csrf_token: String,
}

/// Mint a portal session for `user` within `workspace_uuid`: create the session
/// record and the portal token bundle, returning the cookies to set. Reuses the
/// agent session machinery wholesale (`create_session_record`, refresh-token
/// rotation, CSRF); only the access token's scope/binding and the cookie names
/// are portal-specific.
fn mint_portal_session(
    user: &User,
    workspace_uuid: Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<PortalSessionCookies, HttpResponse> {
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

    Ok(PortalSessionCookies {
        access: crate::utils::cookies::create_portal_access_cookie(&tokens.access_token),
        refresh: crate::utils::cookies::create_portal_refresh_cookie(&tokens.refresh_token),
        csrf: crate::utils::cookies::create_portal_csrf_cookie(&tokens.csrf_token),
        csrf_token: tokens.csrf_token,
    })
}

/// Establish a portal session and return a JSON response carrying the portal
/// cookies. Called once a login flow has proven the customer's email ownership.
pub fn establish_portal_session(
    user: &User,
    workspace_uuid: Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<HttpResponse, HttpResponse> {
    let session = mint_portal_session(user, workspace_uuid, request, conn)?;
    Ok(HttpResponse::Ok()
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .json(json!({
            "success": true,
            "csrf_token": session.csrf_token,
            "workspace_uuid": workspace_uuid,
        })))
}

// --- Magic-link sign-in ---

#[derive(Deserialize)]
pub struct MagicLinkRequest {
    pub email: String,
}

#[derive(Deserialize)]
pub struct MagicLinkCallbackQuery {
    pub token: String,
}

/// Uniform response for the request endpoint: the same body whether or not an
/// account exists, so the portal can't be used to enumerate which addresses are
/// customers of a workspace.
fn magic_link_accepted() -> HttpResponse {
    HttpResponse::Ok().json(json!({
        "status": "ok",
        "message": "If an account exists for that email, a sign-in link has been sent."
    }))
}

/// `POST /api/portal/auth/magic-link` (portal origin, unauthenticated). Issues a
/// single-use sign-in link to a baseline member of the origin's workspace.
///
/// Always returns the same uniform body. Work happens only when the email
/// belongs to a member of THIS workspace; everything else (unknown email,
/// non-member, rate-limited, no primary email) silently no-ops behind the same
/// response.
pub async fn request_magic_link(
    req: HttpRequest,
    body: web::Json<MagicLinkRequest>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() else {
        // No workspace resolved for this origin: nothing to sign in to.
        return magic_link_accepted();
    };
    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return magic_link_accepted();
    }

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return magic_link_accepted(),
    };

    // Resolve a member of THIS workspace with that email; bail (uniformly) if
    // there is none.
    let user = match crate::repository::users::get_user_by_email(&email, &mut conn) {
        Ok(u) => u,
        Err(_) => return magic_link_accepted(),
    };
    let is_member = matches!(
        crate::repository::workspaces::membership(&mut conn, ctx.workspace_id, user.uuid),
        Ok(Some(_))
    );
    if !is_member {
        return magic_link_accepted();
    }

    // Rate-limit: cap sign-in links per user per hour.
    let since = chrono::Utc::now() - chrono::Duration::hours(1);
    let recent = crate::repository::reset_tokens::count_recent_tokens(
        &mut conn,
        user.uuid,
        TokenType::PortalMagicLink.as_str(),
        since,
    )
    .unwrap_or(0);
    if recent >= 5 {
        return magic_link_accepted();
    }

    let token = ResetTokenUtils::create_reset_token(user.uuid, TokenType::PortalMagicLink);
    if crate::repository::reset_tokens::create_reset_token(
        &mut conn,
        &token.token_hash,
        user.uuid,
        TokenType::PortalMagicLink.as_str(),
        None,
        None,
        token.expires_at,
        None,
    )
    .is_err()
    {
        return magic_link_accepted();
    }

    let Some(recipient) = crate::repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
    else {
        return magic_link_accepted();
    };

    // Link base is the workspace's own canonical origin (the portal host), so
    // the emailed link lands back on the same origin the request came from.
    let base_url = crate::utils::tenant_origin::canonical_host_for(
        &ctx.slug,
        ctx.custom_domain.as_deref(),
        crate::utils::tenant_origin::tenant_domain().as_deref(),
    )
    .map(|host| format!("https://{host}"))
    .or_else(|| crate::utils::tenant_origin::email_link_base(None))
    .unwrap_or_default();

    let email_service = match crate::utils::email::EmailService::from_env() {
        Ok(s) => s,
        Err(_) => return magic_link_accepted(),
    };

    // Branding read + enqueue touch workspace-isolated tables, so run pinned +
    // elevated (the standard background path for tenant-table writes).
    let raw_token = token.raw_token.clone();
    let user_name = user.name.clone();
    let _ = crate::sync::session::background_run_in_workspace(
        &pool,
        "background:portal_magic_link",
        ctx.workspace_id,
        move |conn| {
            let branding = crate::utils::email_branding::get_email_branding(conn, &base_url);
            let locale = crate::repository::user_locale::resolve_effective_locale(conn, user.uuid);
            crate::services::transactional_email::enqueue_portal_magic_link(
                conn,
                &email_service,
                &branding,
                &recipient,
                &user_name,
                &raw_token,
                &locale,
            )
        },
    );

    magic_link_accepted()
}

/// `GET /api/portal/auth/callback?token=…` (portal origin, unauthenticated).
/// Consumes a single-use sign-in token, confirms the subject is a member of the
/// origin's workspace, and establishes the portal session, redirecting to the
/// portal home with the session cookies set.
pub async fn magic_link_callback(
    req: HttpRequest,
    query: web::Query<MagicLinkCallbackQuery>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return errors::bad_request("No workspace for this origin");
    };
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return errors::internal("Database connection failed"),
    };

    // Single-use: the token is claimed (marked used) atomically here.
    let user_uuid = match crate::repository::reset_tokens::validate_and_consume_token(
        &mut conn,
        &query.token,
        TokenType::PortalMagicLink.as_str(),
    ) {
        Ok(uuid) => uuid,
        Err(_) => return sign_in_error_redirect(),
    };

    // The link is workspace-agnostic, so confirm the subject actually belongs
    // to the workspace this origin serves before minting a session for it.
    if !matches!(
        crate::repository::workspaces::membership(&mut conn, ctx.workspace_id, user_uuid),
        Ok(Some(_))
    ) {
        return sign_in_error_redirect();
    }

    let user = match crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn) {
        Ok(u) => u,
        Err(_) => return sign_in_error_redirect(),
    };

    match mint_portal_session(&user, ctx.workspace_uuid, &req, &mut conn) {
        Ok(session) => HttpResponse::Found()
            .cookie(session.access)
            .cookie(session.refresh)
            .cookie(session.csrf)
            .append_header(("Location", "/"))
            .finish(),
        Err(resp) => resp,
    }
}

/// Bounce a failed sign-in back to the portal with a generic error flag (a bad,
/// expired, or already-used link). Uniform regardless of the specific failure.
fn sign_in_error_redirect() -> HttpResponse {
    HttpResponse::Found()
        .append_header(("Location", "/?signin_error=1"))
        .finish()
}
