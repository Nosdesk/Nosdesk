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

use std::future::{ready, Ready};
use std::sync::Arc;

use actix_web::body::MessageBody;
use actix_web::cookie::Cookie;
use actix_web::dev::{Payload, ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{web, Error, FromRequest, HttpMessage, HttpRequest, HttpResponse, Responder};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::errors::{self, ApiError};
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::middleware::cookie_auth::{require_portal_membership, PORTAL_SCOPE};
use crate::models::{Claims, ContentFormat, NewComment, NewTicket, Ticket, TicketPriority, User};
use crate::repository::ticket_visibility::{
    can_view_ticket, visible_tickets_query, VisibilityContext,
};
use crate::schema::tickets;
use crate::services::search::SearchService;
use crate::utils::jwt::JwtUtils;
use crate::utils::reset_tokens::{ResetTokenUtils, TokenType};
use diesel::prelude::*;

/// Public portal sign-in routes, mounted inside the rate-limited
/// `/api/portal/auth` scope in main.rs (paths are scope-relative).
pub fn auth_config(cfg: &mut web::ServiceConfig) {
    cfg.route("/magic-link", web::post().to(request_magic_link))
        .route("/callback", web::get().to(magic_link_callback))
        .route("/ticket", web::get().to(ticket_link_callback))
        .route("/code", web::post().to(sign_in_with_code))
        // The portal refresh cookie is Path-scoped to exactly this route.
        .route("/refresh", web::post().to(refresh_portal_session));
}

/// Authenticated customer-portal routes, mounted inside the `/api/portal` scope
/// in main.rs (the scope keeps its `portal_auth_middleware` wrap).
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/me", web::get().to(get_me))
        .route("/logout", web::post().to(logout))
        .route("/tickets", web::get().to(list_my_tickets))
        .route("/tickets", web::post().to(create_my_ticket))
        .route("/request-types", web::get().to(list_request_types))
        .route("/notice", web::get().to(get_notice))
        .route(
            "/events",
            web::get().to(crate::handlers::portal_events::portal_events),
        )
        .route("/notices/{id}/follow", web::post().to(follow_notice))
        .route("/tickets/{id}", web::get().to(get_my_ticket))
        .route("/tickets/{id}/comments", web::post().to(reply_to_my_ticket))
        .route("/tickets/{id}/resolve", web::post().to(resolve_my_ticket))
        .route("/tickets/{id}/seen", web::post().to(mark_seen))
        .route(
            "/tickets/{id}/participants",
            web::post().to(add_participant),
        )
        .route(
            "/tickets/{id}/participants/{user_uuid}",
            web::delete().to(remove_participant),
        )
        .route(
            "/tickets/{id}/attachments/{attachment_id}",
            web::get().to(download_attachment),
        )
        .route("/files", web::post().to(upload_file));
}

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
/// 3. The subject is a member of that workspace in any role (the baseline
///    `Member` row a customer holds), RLS-pinned.
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
    require_portal_membership(conn, origin_ctx.workspace_id, user_uuid)?;

    Ok(PortalContext {
        user_uuid,
        workspace_id: origin_ctx.workspace_id,
        workspace_uuid: origin_ctx.workspace_uuid,
    })
}

/// The cookies + CSRF value that make up a freshly minted portal session,
/// ready to attach to either a JSON response (XHR) or a redirect (the
/// magic-link callback's top-level navigation).
pub(crate) struct PortalSessionCookies {
    pub(crate) access: Cookie<'static>,
    pub(crate) refresh: Cookie<'static>,
    pub(crate) csrf: Cookie<'static>,
    csrf_token: String,
}

/// Sign a person in to an embedded portal (the help widget, the Teams tab):
/// a short session for that host and a portal access token in the body. No
/// cookies, which a framed page can't rely on; the page keeps the token in
/// memory and asks its host to sign the person in again when it lapses.
pub(crate) fn embedded_sign_in(
    req: &HttpRequest,
    pool: &crate::db::Pool,
    host: crate::repository::active_sessions::EmbedHost,
    user_uuid: Uuid,
    workspace_uuid: Uuid,
) -> HttpResponse {
    let Ok(mut conn) = pool.get() else {
        return crate::errors::internal("Couldn't sign you in");
    };
    let Ok(user) = crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn) else {
        tracing::info!("embedded sign-in refused (inactive account)");
        return crate::errors::unauthorized("That sign-in didn't work");
    };
    let ip =
        crate::utils::client_ip::from_http_request(req).and_then(|ip| ip.to_string().parse().ok());
    let user_agent = req
        .headers()
        .get("User-Agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.chars().take(500).collect());
    let session = match crate::repository::active_sessions::embedded_session(
        &mut conn, host, user.uuid, ip, user_agent,
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = ?e, "embedded sign-in: session failed");
            return crate::errors::internal("Couldn't sign you in");
        }
    };
    match crate::utils::jwt::JwtUtils::create_portal_token(
        &user,
        workspace_uuid,
        &session.session_id,
    ) {
        Ok(access_token) => HttpResponse::Ok().json(serde_json::json!({
            "access_token": access_token,
            "expires_in": crate::repository::active_sessions::EMBEDDED_SESSION_MINUTES * 60,
        })),
        Err(_) => crate::errors::internal("Couldn't sign you in"),
    }
}

/// Mint a portal session for `user` within `workspace_uuid`: create the session
/// record and the portal token bundle, returning the cookies to set. Reuses the
/// agent session machinery wholesale (`create_session_record`, refresh-token
/// rotation, CSRF); only the access token's scope/binding and the cookie names
/// are portal-specific.
pub(crate) fn mint_portal_session(
    user: &User,
    workspace_uuid: Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<PortalSessionCookies, ApiError> {
    let session = crate::handlers::auth::create_session_record(&user.uuid, request, conn, None)
        .map_err(|e| {
            tracing::error!(error = ?e, "portal session: failed to create session record");
            ApiError::Internal("Failed to establish session".into())
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
) -> Result<HttpResponse, ApiError> {
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

/// `POST /api/portal/auth/refresh` — rotate a customer-portal session.
///
/// The portal refresh cookie has always been `Path`-scoped to this route, but
/// the route did not exist, so a portal session simply died when the 15-minute
/// access cookie expired. The route existing does not by itself fix that: no
/// portal client calls it yet, so the sessions still die until one does. What
/// it fixes now is the shape, so that when a client is written it rotates
/// through the audited path instead of growing its own.
///
/// Rotation runs through [`rotate_refresh_family`], the same code the agent
/// endpoint uses, so reuse detection, family revocation and the absolute
/// session ceiling behave identically here. The two realm-specific parts are
/// passed in: the portal audience, which refuses an agent token presented here
/// just as the agent endpoint refuses a portal one, and the portal access
/// token, which is scoped and bound to this workspace.
pub async fn refresh_portal_session(
    db_pool: web::Data<crate::db::Pool>,
    request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    // Read from extensions rather than via the extractor, matching
    // `magic_link_callback` in this same origin-resolved scope: an unknown
    // origin is a 400, not a 500.
    let Some(ctx) = request.extensions().get::<WorkspaceContext>().cloned() else {
        return Err(ApiError::BadRequest("No workspace for this origin".into()));
    };

    let Some(refresh_raw) = request
        .cookie(&crate::utils::cookies::cookie_name(
            crate::utils::cookies::PORTAL_REFRESH_TOKEN_COOKIE,
        ))
        .map(|c| c.value().to_string())
        .filter(|t| !t.is_empty())
    else {
        return Err(ApiError::Unauthorized("Refresh token not found".into()));
    };

    let mut conn = crate::handlers::helpers::db_conn(&db_pool)?;

    let workspace_uuid = ctx.workspace_uuid;
    let workspace_id = ctx.workspace_id;
    let rotated = crate::handlers::auth::rotate_refresh_family(
        &mut conn,
        &request,
        &refresh_raw,
        crate::models::REFRESH_AUDIENCE_PORTAL,
        |conn, user, session_id| {
            // A refresh cookie proves a portal session existed, not that it was
            // for the workspace this origin serves. Re-check membership on
            // every rotation so a removed customer's session dies at the next
            // refresh, and so a cookie replayed at another tenant's origin
            // never mints a token bound to that tenant. Running it here, inside
            // the mint, means the refusal lands before the family is rotated.
            //
            // Refuse without revoking the family, unlike the realm mismatch
            // above it. That one can only be a copied credential; this one
            // cannot be told apart from the ordinary case of a customer whose
            // membership was removed, and burning their family adds nothing
            // once the refresh is already refused.
            require_portal_membership(conn, workspace_id, user.uuid)
                .map_err(|_| ApiError::Unauthorized("Invalid or expired refresh token".into()))?;
            crate::utils::jwt::JwtUtils::create_portal_token(user, workspace_uuid, session_id)
                .map_err(|_| ApiError::Internal("Failed to create access token".into()))
        },
    )?;

    // Portal clients are browsers, so the rotated tokens go back as cookies
    // only; there is no bearer mode to serve here.
    let csrf_token = crate::utils::csrf::generate_csrf_token();
    Ok(HttpResponse::Ok()
        .cookie(crate::utils::cookies::create_portal_access_cookie(
            &rotated.access_token,
        ))
        .cookie(crate::utils::cookies::create_portal_refresh_cookie(
            &rotated.refresh_token,
        ))
        .cookie(crate::utils::cookies::create_portal_csrf_cookie(
            &csrf_token,
        ))
        .json(json!({
            "success": true,
            "csrf_token": csrf_token,
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
    send_sign_in_link(&pool, &ctx, &body.email);
    magic_link_accepted()
}

/// Email a sign-in link (and code) to the member of `ctx`'s workspace with
/// `email`, at most five an hour. Does nothing, silently, for an address with
/// no member here: callers answer the same way either way.
pub(crate) fn send_sign_in_link(pool: &Pool, ctx: &WorkspaceContext, email: &str) {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return;
    }

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return,
    };

    // Resolve a member of THIS workspace with that email; bail (uniformly) if
    // there is none.
    let user = match crate::repository::users::get_user_by_email(&email, &mut conn) {
        Ok(u) => u,
        Err(_) => return,
    };
    if !crate::middleware::cookie_auth::is_workspace_member(&mut conn, ctx.workspace_id, user.uuid)
    {
        return;
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
        return;
    }

    let token = ResetTokenUtils::create_reset_token(user.uuid, TokenType::PortalMagicLink);
    // The same sign-in as a 6-digit code (for the email opened on another
    // device): only its hash is stored, with an attempt counter.
    let code = new_sign_in_code();
    if crate::repository::reset_tokens::create_reset_token(
        &mut conn,
        &token.token_hash,
        user.uuid,
        TokenType::PortalMagicLink.as_str(),
        None,
        None,
        token.expires_at,
        Some(json!({ "code_hash": sign_in_code_hash(user.uuid, &code), "attempts": 0 })),
    )
    .is_err()
    {
        return;
    }

    let Some(recipient) = crate::repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
    else {
        return;
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
        Err(_) => return,
    };

    // Branding read + enqueue touch workspace-isolated tables. Run pinned as the
    // RLS-enforced runtime role so the branding read (no explicit workspace
    // filter) returns THIS workspace's settings, not an arbitrary tenant's.
    let raw_token = token.raw_token.clone();
    let user_name = user.name.clone();
    let _ = crate::sync::session::run_in_workspace(
        pool,
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
                Some(&code),
                &locale,
            )
        },
    );
}

/// `GET /api/portal/auth/callback?token=…` (portal origin, unauthenticated).
/// Consumes a single-use sign-in token, confirms the subject is a member of the
/// origin's workspace, and establishes the portal session, redirecting to the
/// portal home with the session cookies set.
pub async fn magic_link_callback(
    req: HttpRequest,
    query: web::Query<MagicLinkCallbackQuery>,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return Err(ApiError::BadRequest("No workspace for this origin".into()));
    };
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return Err(ApiError::Internal("Database connection failed".into())),
    };

    // Single-use: the token is claimed (marked used) atomically here.
    let user_uuid = match crate::repository::reset_tokens::validate_and_consume_token(
        &mut conn,
        &query.token,
        TokenType::PortalMagicLink.as_str(),
    ) {
        Ok(uuid) => uuid,
        Err(_) => return Ok(sign_in_error_redirect()),
    };

    // The link is workspace-agnostic, so confirm the subject actually belongs
    // to the workspace this origin serves before minting a session for it.
    if !crate::middleware::cookie_auth::is_workspace_member(&mut conn, ctx.workspace_id, user_uuid)
    {
        return Ok(sign_in_error_redirect());
    }

    let user = match crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn) {
        Ok(u) => u,
        Err(_) => return Ok(sign_in_error_redirect()),
    };

    // Following the emailed link proves the address, as confirming a guest
    // submission does. Best-effort: the sign-in doesn't depend on it.
    if let Err(e) = crate::repository::user_emails::mark_primary_verified(&mut conn, &user.uuid) {
        tracing::warn!(user_uuid = %user.uuid, error = ?e, "portal sign-in: could not mark email verified");
    }

    let session = mint_portal_session(&user, ctx.workspace_uuid, &req, &mut conn)?;
    Ok(HttpResponse::Found()
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .append_header(("Location", portal_path("/")))
        .finish())
}

/// Bounce a failed sign-in back to the portal with a generic error flag (a bad,
/// expired, or already-used link). Uniform regardless of the specific failure.
#[derive(Deserialize)]
pub struct TicketLinkQuery {
    t: String,
    /// The requester's answer from a resolved email ("fixed" or
    /// "not_fixed"). Carried to the page, which records it: a mail scanner
    /// fetching the link must not change the ticket.
    #[serde(default)]
    answer: Option<String>,
}

/// `GET /api/portal/auth/ticket?t=…` (portal origin, unauthenticated): the
/// "View request" link in a requester email. A valid, unexpired link for a
/// member of this workspace signs them in and opens the ticket; anything else
/// lands on sign-in with an explanation, never an error page.
pub async fn ticket_link_callback(
    req: HttpRequest,
    query: web::Query<TicketLinkQuery>,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return Err(ApiError::BadRequest("No workspace for this origin".into()));
    };
    let Some((user_uuid, ticket_id)) =
        crate::utils::portal_ticket_link::verify(ctx.workspace_id, &query.t)
    else {
        return Ok(sign_in_error_redirect());
    };
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return Err(ApiError::Internal("Database connection failed".into())),
    };
    // The link outlives a membership: re-check it, pinned (see is_workspace_member).
    if !crate::middleware::cookie_auth::is_workspace_member(&mut conn, ctx.workspace_id, user_uuid)
    {
        return Ok(sign_in_error_redirect());
    }
    let user = match crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn) {
        Ok(u) => u,
        Err(_) => return Ok(sign_in_error_redirect()),
    };
    // The link came to their inbox, which proves the address, as the magic link does.
    if let Err(e) = crate::repository::user_emails::mark_primary_verified(&mut conn, &user.uuid) {
        tracing::warn!(user_uuid = %user.uuid, error = ?e, "ticket link: could not mark email verified");
    }
    let session = mint_portal_session(&user, ctx.workspace_uuid, &req, &mut conn)?;
    Ok(HttpResponse::Found()
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .append_header((
            "Location",
            ticket_location(ticket_id, query.answer.as_deref()),
        ))
        .finish())
}

/// Where a View request link lands, with a recognised answer carried along.
fn ticket_location(ticket_id: i32, answer: Option<&str>) -> String {
    match answer {
        Some(a @ ("fixed" | "not_fixed")) => {
            portal_path(&format!("/tickets/{ticket_id}?answer={a}"))
        }
        _ => portal_path(&format!("/tickets/{ticket_id}")),
    }
}

/// Wrong codes allowed against one sign-in email before its code stops working.
const SIGN_IN_CODE_ATTEMPTS: i64 = 5;

fn new_sign_in_code() -> String {
    format!("{:06}", rand::random::<u32>() % 1_000_000)
}

/// Salted with the user so equal codes for different people hash differently.
pub fn sign_in_code_hash(user: Uuid, code: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, format!("{user}:{code}").as_bytes());
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Deserialize)]
pub struct SignInCodeRequest {
    email: String,
    code: String,
}

/// `POST /api/portal/auth/code` (portal origin, unauthenticated): sign in with
/// the 6-digit code from the sign-in email. Every failure is the same 400 so
/// the response can't tell a known address from an unknown one; five wrong
/// codes disable the code (the link still works). A match spends the sign-in
/// (link and code are one sign-in).
pub async fn sign_in_with_code(
    req: HttpRequest,
    body: web::Json<SignInCodeRequest>,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    let invalid = || {
        Ok(errors::bad_request_with_code(
            "That code didn't work. Check it, or send a new one.",
            "invalid_code",
        ))
    };
    let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return invalid();
    };
    let email = body.email.trim().to_lowercase();
    let code: String = body.code.chars().filter(char::is_ascii_digit).collect();
    if code.len() != 6 {
        return invalid();
    }
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(_) => return Err(ApiError::Internal("Database connection failed".into())),
    };
    let Ok(user) = crate::repository::users::get_user_by_email(&email, &mut conn) else {
        return invalid();
    };
    if !crate::middleware::cookie_auth::is_workspace_member(&mut conn, ctx.workspace_id, user.uuid)
    {
        return invalid();
    }
    let live = crate::repository::reset_tokens::live_tokens(
        &mut conn,
        user.uuid,
        TokenType::PortalMagicLink.as_str(),
    )
    .unwrap_or_default();
    let wanted = sign_in_code_hash(user.uuid, &code);
    let matched = live.iter().find(|t| {
        t.metadata
            .as_ref()
            .and_then(|m| m.get("code_hash"))
            .and_then(|h| h.as_str())
            .is_some_and(|h| constant_time_eq::constant_time_eq(h.as_bytes(), wanted.as_bytes()))
    });
    let Some(token) = matched else {
        // Count the miss against every live code. At the limit the CODE stops
        // working, but the emailed link stays usable: disabling the link too
        // would let anyone who knows the address burn the requester's sign-in
        // without ever seeing their inbox.
        for t in &live {
            let Some(mut meta) = t.metadata.clone() else {
                continue;
            };
            if meta.get("code_hash").and_then(|h| h.as_str()).is_none() {
                continue;
            }
            let attempts = meta.get("attempts").and_then(|a| a.as_i64()).unwrap_or(0) + 1;
            meta["attempts"] = json!(attempts);
            if attempts >= SIGN_IN_CODE_ATTEMPTS {
                meta["code_hash"] = serde_json::Value::Null;
            }
            if let Err(e) =
                crate::repository::reset_tokens::set_metadata(&mut conn, &t.token_hash, meta)
            {
                tracing::warn!(error = ?e, "sign-in code: could not record a wrong attempt");
            }
        }
        return invalid();
    };
    // Spend it; a concurrent use that got there first wins.
    match crate::repository::reset_tokens::claim_unused(&mut conn, &token.token_hash) {
        Ok(Some(_)) => {}
        _ => return invalid(),
    }
    if let Err(e) = crate::repository::user_emails::mark_primary_verified(&mut conn, &user.uuid) {
        tracing::warn!(user_uuid = %user.uuid, error = ?e, "sign-in code: could not mark email verified");
    }
    let session = mint_portal_session(&user, ctx.workspace_uuid, &req, &mut conn)?;
    Ok(HttpResponse::Ok()
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .json(json!({ "status": "ok" })))
}

fn sign_in_error_redirect() -> HttpResponse {
    HttpResponse::Found()
        .append_header(("Location", portal_path("/login?signin_error=1")))
        .finish()
}

/// Where a portal page lives. Hosted serves the portal at the root of each
/// workspace's own origin; self-hosted shares one origin with the agent app, so
/// the portal lives under `/portal`.
pub fn portal_path(path: &str) -> String {
    if crate::middleware::workspace_context::is_hosted() {
        path.to_string()
    } else {
        format!("/portal{path}")
    }
}

/// Routes behind the agent app's own authentication (mounted in the
/// authenticated `/api` scope).
pub fn app_config(cfg: &mut web::ServiceConfig) {
    cfg.route("/me/portal", web::get().to(open_portal_from_app));
}

#[derive(Deserialize)]
pub struct OpenPortalQuery {
    /// A portal path to land on (`/tickets/12`). Anything else lands on the
    /// request list.
    #[serde(default)]
    next: Option<String>,
}

/// `GET /api/me/portal` (agent session): hand someone signed in to the app over
/// to the portal already signed in, so a requester who reached the app (a
/// password or SSO account on self-hosted) never signs in twice.
pub async fn open_portal_from_app(
    req: HttpRequest,
    auth: crate::extractors::AuthContext,
    ws: WorkspaceContext,
    pool: web::Data<Pool>,
    query: web::Query<OpenPortalQuery>,
) -> Result<HttpResponse, ApiError> {
    let next = query
        .next
        .as_deref()
        .filter(|p| p.starts_with("/tickets") && !p.contains("//") && !p.contains('\\'))
        .unwrap_or("/tickets");
    let mut conn = pool
        .get()
        .map_err(|_| ApiError::Internal("Database connection failed".into()))?;
    let user = crate::repository::users::find_active_by_uuid(&auth.user_uuid, &mut conn)
        .map_err(|_| ApiError::Unauthorized("Sign in again".into()))?;
    let session = mint_portal_session(&user, ws.workspace_uuid, &req, &mut conn)?;
    Ok(HttpResponse::Found()
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .append_header(("Location", portal_path(next)))
        .finish())
}

// --- Authenticated portal API ---

/// Extractor for the authenticated portal principal, published by
/// [`portal_auth_middleware`]. A handler that takes `PortalContext` is only
/// reachable behind that middleware.
impl FromRequest for PortalContext {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        match req.extensions().get::<PortalContext>().cloned() {
            Some(ctx) => ready(Ok(ctx)),
            None => ready(Err(actix_web::error::ErrorUnauthorized(
                "Portal authentication required",
            ))),
        }
    }
}

// tenant-read-exempt: authorize_portal_request's only tenant read is require_portal_membership, which pins via with_actor_context (invisible to the scanner).
/// Authenticate a portal request from its `portal_access` cookie and gate it.
/// Mirrors the agent `cookie_auth_middleware`: validate the token (and its
/// session), run the portal authorization gate, then pin the request actor to
/// the workspace so `TenantConn` queries are RLS-scoped. A non-portal token, a
/// token bound to a different tenant, or a non-member all fail here.
pub async fn portal_auth_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let pool = req
        .app_data::<web::Data<Pool>>()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("Database pool not found"))?;
    let mut conn = pool
        .get()
        .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;

    // The portal session cookie, or (the embedded help widget, which can't
    // use cookies in a third-party frame) the same portal token as a bearer.
    let token = req
        .cookie(&crate::utils::cookies::cookie_name(
            crate::utils::cookies::PORTAL_ACCESS_TOKEN_COOKIE,
        ))
        .map(|c| c.value().to_string())
        .or_else(|| {
            req.headers()
                .get(actix_web::http::header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer "))
                .map(|t| t.trim().to_string())
        })
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let (claims, _user) = JwtUtils::authenticate_with_token(&token, &mut conn)
        .await
        .map_err(|_| actix_web::error::ErrorUnauthorized("Invalid or expired token"))?;

    let portal_ctx = authorize_portal_request(&req, &mut conn, &claims)?;
    drop(conn);

    req.extensions_mut().insert(portal_ctx);
    // Pin the actor to the resolved workspace (same path the agent auth uses) so
    // TenantConn runs portal queries RLS-scoped to this tenant.
    crate::middleware::request_context::populate(&req, &claims);
    req.extensions_mut().insert(claims);

    next.call(req).await
}

/// Customer-facing projection of a [`Ticket`] for the portal. This is an
/// explicit allowlist: only the fields a ticket's own requester may see. Every
/// internal field of `Ticket` (assignee, `created_by`/`closed_by`,
/// `triage_state`, `spam_suspected`, `resolution_notes`, the SLA timers,
/// `guest_lookup_token`, `verification_state`, `category_id`,
/// `origin_channel_id`, recurrence, planning dates) is deliberately absent, so
/// serializing the raw struct can never leak them to a customer.
/// `GET /api/portal/me`: who is signed in, and the locale the portal should
/// render in (the requester's preference, else the site default).
pub async fn get_me(mut tc: TenantConn, portal: PortalContext) -> impl Responder {
    let user_uuid = portal.user_uuid;
    let result = tc.run(move |conn| {
        let user = crate::repository::users::find_active_by_uuid(&user_uuid, conn)?;
        let email = crate::repository::user_helpers::get_primary_email(&user_uuid, conn);
        let locale = crate::repository::user_locale::resolve_effective_locale(conn, user_uuid);
        Ok::<_, diesel::result::Error>((user, email, locale))
    });
    match result {
        Ok((user, email, locale)) => HttpResponse::Ok().json(json!({
            "uuid": user.uuid,
            "name": user.name,
            "email": email,
            "effective_locale": locale.to_string(),
        })),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to load /me");
            errors::internal("Failed to load profile")
        }
    }
}

/// `POST /api/portal/logout`: revoke this portal session and expire its cookies.
pub async fn logout(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    _portal: PortalContext,
) -> impl Responder {
    let sid = req
        .extensions()
        .get::<crate::models::Claims>()
        .and_then(|c| c.session_uuid());
    if let (Some(sid), Ok(mut conn)) = (sid, db_pool.get()) {
        if let Err(e) = crate::repository::active_sessions::revoke_session_by_uuid(&mut conn, &sid)
        {
            tracing::warn!(error = %e, "portal logout: failed to revoke session");
        }
    }
    let mut res = HttpResponse::NoContent();
    for cookie in crate::utils::cookies::delete_portal_cookies() {
        res.cookie(cookie);
    }
    res.finish()
}

#[derive(Debug, Serialize)]
pub struct CustomerTicket {
    pub id: i32,
    pub uuid: Uuid,
    pub title: String,
    pub priority: TicketPriority,
    pub workflow_state_id: i32,
    #[serde(rename = "created")]
    pub created_at: NaiveDateTime,
    #[serde(rename = "modified")]
    pub updated_at: NaiveDateTime,
    #[serde(rename = "closed")]
    pub closed_at: Option<NaiveDateTime>,
    /// The ticket's workflow state, as the requester sees it.
    pub state: Option<CustomerState>,
    /// Who opened it, when that isn't the viewer (a colleague's request they
    /// were added to, or one shared across their organisation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
    /// Where an approval stands (`pending`, `approved`, `declined`,
    /// `skipped`); absent when none is involved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<String>,
    /// In the list: when someone else last replied publicly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reply_at: Option<chrono::DateTime<chrono::Utc>>,
    /// In the list: that reply is newer than the viewer's last look.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unread_reply: bool,
}

#[derive(Debug, Serialize)]
pub struct CustomerState {
    pub name: String,
    pub category: crate::models::WorkflowStateCategory,
}

impl CustomerTicket {
    fn new(t: Ticket, states: &std::collections::HashMap<i32, CustomerState>) -> Self {
        let state = states.get(&t.workflow_state_id).map(|s| CustomerState {
            name: s.name.clone(),
            category: s.category,
        });
        Self {
            id: t.id,
            uuid: t.uuid,
            title: t.title,
            priority: t.priority,
            workflow_state_id: t.workflow_state_id,
            created_at: t.created_at,
            updated_at: t.updated_at,
            closed_at: t.closed_at,
            state,
            requested_by: None,
            approval_state: t.approval_state,
            last_reply_at: None,
            unread_reply: false,
        }
    }

    /// Name the requester when it isn't `viewer`.
    fn for_viewer(
        t: Ticket,
        states: &std::collections::HashMap<i32, CustomerState>,
        viewer: Uuid,
        conn: &mut DbConnection,
    ) -> Self {
        let requested_by = t
            .requester_uuid
            .filter(|r| *r != viewer)
            .and_then(|r| crate::repository::users::find_active_by_uuid(&r, conn).ok())
            .map(|u| u.name);
        Self {
            requested_by,
            ..Self::new(t, states)
        }
    }
}

/// How much of the workspace a portal user can read: their own requests and
/// the ones they were added to, plus (when the workspace shares by domain) the
/// requests of people at their verified, non-free-mail domain. Writes never
/// use this; they stay on [`VisibilityContext::requester_only`].
pub(crate) fn portal_visibility(
    conn: &mut DbConnection,
    viewer: Uuid,
) -> QueryResult<VisibilityContext> {
    use crate::schema::user_emails;
    let shares = crate::repository::site_settings::get_site_settings(conn)?.portal_share_by_domain;
    if !shares {
        return Ok(VisibilityContext::requester_only(viewer));
    }
    let verified: Option<String> = user_emails::table
        .filter(user_emails::user_uuid.eq(viewer))
        .filter(user_emails::is_primary.eq(true))
        .filter(user_emails::is_verified.eq(true))
        .select(user_emails::email)
        .first(conn)
        .optional()?;
    Ok(match verified {
        Some(email) if !crate::utils::free_mail::is_free_mail(&email) => {
            VisibilityContext::portal_sharing(viewer)
        }
        _ => VisibilityContext::requester_only(viewer),
    })
}

/// The workspace's workflow states by id (a handful of rows).
fn state_map(
    conn: &mut DbConnection,
) -> QueryResult<std::collections::HashMap<i32, CustomerState>> {
    Ok(crate::repository::workflow_states::list_all(conn)?
        .into_iter()
        .map(|s| {
            (
                s.id,
                CustomerState {
                    name: s.name,
                    category: s.category,
                },
            )
        })
        .collect())
}

/// A public comment as the requester sees it: the body fields the shared
/// `CommentContent` renderer reads, the author, and its attachments.
#[derive(Debug, Serialize)]
pub struct CustomerComment {
    pub id: i32,
    pub content: String,
    pub content_format: crate::models::ContentFormat,
    pub render_kind: Option<String>,
    pub new_content: Option<String>,
    pub quoted_content: Option<String>,
    pub created_at: NaiveDateTime,
    pub author: CustomerAuthor,
    pub attachments: Vec<CustomerAttachment>,
}

#[derive(Debug, Serialize)]
pub struct CustomerAuthor {
    pub name: String,
    pub avatar_url: Option<String>,
    /// Staff reply (agent/admin/owner) rather than the requester's own.
    pub is_staff: bool,
    pub is_you: bool,
}

#[derive(Debug, Serialize)]
pub struct CustomerAttachment {
    pub id: i32,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
}

/// The ticket's public thread, oldest first, with authors (workspace persona
/// names, so an agent alias shows) and attachments.
fn customer_thread(
    conn: &mut DbConnection,
    ticket_id: i32,
    viewer: Uuid,
) -> QueryResult<Vec<CustomerComment>> {
    let mut comments =
        crate::repository::comments::get_public_comments_by_ticket_id(conn, ticket_id)?;
    comments.reverse();
    let ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
    let mut attachments = crate::repository::comments::get_attachments_for_comments(conn, &ids)?;
    let mut authors: Vec<Uuid> = comments.iter().map(|c| c.user_uuid).collect();
    authors.sort();
    authors.dedup();
    let users = crate::repository::users::get_user_map_by_uuids_with_persona(&authors, conn)?;
    let roles = crate::repository::user_helpers::workspace_roles_batch(&authors, conn);
    Ok(comments
        .into_iter()
        .map(|c| {
            let user = users.get(&c.user_uuid);
            CustomerComment {
                author: CustomerAuthor {
                    name: user.map(|u| u.name.clone()).unwrap_or_default(),
                    avatar_url: user.and_then(|u| u.avatar_thumb.clone().or(u.avatar_url.clone())),
                    is_staff: roles.get(&c.user_uuid).is_some_and(|r| r.is_staff()),
                    is_you: c.user_uuid == viewer,
                },
                attachments: attachments
                    .remove(&c.id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| CustomerAttachment {
                        id: a.id,
                        name: a.name,
                        file_size: a.file_size,
                        mime_type: a.mime_type,
                    })
                    .collect(),
                id: c.id,
                content: c.content,
                content_format: c.content_format,
                render_kind: c.render_kind,
                new_content: c.new_content,
                quoted_content: c.quoted_content,
                created_at: c.created_at,
            }
        })
        .collect())
}

/// `GET /api/portal/tickets` — the customer's own tickets in this workspace.
///
/// RLS pins to the workspace (the portal origin's tenant); the visibility
/// context is forced requester-only, so the rows are exactly the tickets this
/// customer requested or watches, never another customer's.
pub async fn list_my_tickets(mut tc: TenantConn, portal: PortalContext) -> impl Responder {
    let viewer = portal.user_uuid;
    let result = tc.run(move |conn| {
        let vis = portal_visibility(conn, viewer)?;
        let rows = visible_tickets_query(&vis)
            .order(tickets::updated_at.desc())
            .load::<Ticket>(conn)?;
        let states = state_map(conn)?;
        let ids: Vec<i32> = rows.iter().map(|t| t.id).collect();
        let replies = crate::repository::user_ticket_views::replies_for_viewer(conn, viewer, &ids)?;
        Ok::<_, diesel::result::Error>(
            rows.into_iter()
                .map(|t| {
                    let reply = replies.get(&t.id).copied();
                    CustomerTicket {
                        last_reply_at: reply.map(|(at, _)| at),
                        unread_reply: reply.is_some_and(|(_, unread)| unread),
                        ..CustomerTicket::for_viewer(t, &states, viewer, conn)
                    }
                })
                .collect::<Vec<_>>(),
        )
    });
    match result {
        Ok(dto) => HttpResponse::Ok().json(dto),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to list tickets");
            errors::internal("Failed to list tickets")
        }
    }
}

/// `GET /api/portal/tickets/{id}` — one of the customer's tickets with its
/// customer-visible thread (internal notes dropped). 404 (not 403) when the
/// ticket isn't theirs, so ticket existence doesn't leak.
pub async fn get_my_ticket(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let viewer = portal.user_uuid;
    let result = tc.run(move |conn| {
        let vis = portal_visibility(conn, viewer)?;
        if !can_view_ticket(conn, &vis, ticket_id)? {
            return Ok(None);
        }
        // Read-only when it's only shared with them (not theirs, not added).
        let can_reply =
            can_view_ticket(conn, &VisibilityContext::requester_only(viewer), ticket_id)?;
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let states = state_map(conn)?;
        let comments = customer_thread(conn, ticket_id, viewer)?;
        // The requester's own answer to "is it fixed?", if they gave one.
        let rating = crate::repository::ticket_ratings::for_ticket(conn, ticket_id)?
            .filter(|r| r.rater_uuid == viewer)
            .map(|r| json!({ "rating": r.rating, "comment": r.comment }));
        let is_requester = ticket.requester_uuid == Some(viewer);
        let participants = participants_of(conn, &ticket, viewer)?;
        Ok(Some((
            CustomerTicket::for_viewer(ticket, &states, viewer, conn),
            comments,
            rating,
            is_requester,
            participants,
            can_reply,
        )))
    });
    match result {
        Ok(Some((ticket, comments, rating, is_requester, participants, can_reply))) => {
            HttpResponse::Ok().json(json!({
                "ticket": ticket,
                "comments": comments,
                "rating": rating,
                "is_requester": is_requester,
                "participants": participants,
                "can_reply": can_reply,
            }))
        }
        Ok(None) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to load ticket");
            errors::internal("Failed to load ticket")
        }
    }
}

/// `GET /api/portal/tickets/{id}/attachments/{attachment_id}`: a file on a
/// public comment of a ticket the requester can see. The agent file routes
/// authenticate agent sessions only, so the portal serves its own.
pub async fn download_attachment(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<(i32, i32)>,
    req: HttpRequest,
    base_storage: web::Data<Arc<dyn crate::utils::storage::Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (ticket_id, attachment_id) = path.into_inner();
    let viewer = portal.user_uuid;
    let url = tc
        .run(move |conn| {
            let vis = portal_visibility(conn, viewer)?;
            if !can_view_ticket(conn, &vis, ticket_id)? {
                return Ok(None);
            }
            let attachment =
                match crate::repository::comments::get_attachment_by_id(conn, attachment_id) {
                    Ok(a) => a,
                    Err(diesel::result::Error::NotFound) => return Ok(None),
                    Err(e) => return Err(e),
                };
            let Some(comment_id) = attachment.comment_id else {
                return Ok(None);
            };
            let comment = crate::repository::comments::get_comment_by_id(conn, comment_id)?;
            let visible = comment.ticket_id == ticket_id
                && !comment.is_internal
                && comment.deleted_at.is_none();
            Ok::<_, diesel::result::Error>(visible.then_some(attachment.url))
        })
        .map_err(|e| {
            tracing::error!(error = ?e, ticket_id, "portal: attachment lookup failed");
            actix_web::error::ErrorInternalServerError("Attachment lookup failed")
        })?;
    // Ticket attachments are stored under `tickets/{ticket_id}/...` and linked
    // as `/uploads/tickets/...`; anything else isn't a ticket file.
    let Some(file_path) = url
        .as_deref()
        .and_then(|u| u.strip_prefix("/uploads/"))
        .filter(|p| p.starts_with(&format!("tickets/{ticket_id}/")))
    else {
        return Err(actix_web::error::ErrorNotFound("File not found"));
    };
    let storage = crate::utils::storage::WorkspaceScopedStorage::arc(
        base_storage.get_ref().clone(),
        portal.workspace_id,
    );
    crate::handlers::files::serve_or_not_found(
        storage,
        file_path,
        &req,
        crate::utils::storage::Caching::Private,
    )
    .await
}

/// `POST /api/portal/files`: stage one file for the requester's next reply or
/// request. Same validation as the guest form (size cap, safe types); the row
/// is marked as theirs and claimed by [`claim_uploads`].
/// Longest request description or reply, in characters.
const MAX_TEXT_CHARS: usize = 20_000;
/// Per person, per hour: generous for anyone typing, a wall for a script.
const REQUESTS_PER_HOUR: u32 = 20;
const REPLIES_PER_HOUR: u32 = 120;
const UPLOADS_PER_HOUR: u32 = 60;

/// `Some(429)` when `user` has used up this hour's allowance of `kind`.
/// Fails open if the limiter's store is unreachable.
async fn over_write_limit(kind: &str, user: Uuid, per_hour: u32) -> Option<HttpResponse> {
    use crate::utils::rate_limit::{get_redis_url, RateLimiter};
    let key = format!("portal_{kind}:{user}");
    match RateLimiter::check_rate_limit(&get_redis_url(), &key, per_hour, 3600).await {
        Ok(true) => None,
        Ok(false) => Some(errors::too_many_requests(
            "You've sent a lot in the last hour. Please try again a little later.",
            3600,
        )),
        Err(e) => {
            tracing::warn!(error = %e, "portal write limiter unavailable; allowing");
            None
        }
    }
}

pub async fn upload_file(
    mut tc: TenantConn,
    portal: PortalContext,
    storage: crate::extractors::ScopedStorage,
    mut payload: actix_multipart::Multipart,
) -> Result<HttpResponse, ApiError> {
    use crate::handlers::guest::{read_validated_upload, UploadRejected};
    if let Some(limited) = over_write_limit("upload", portal.user_uuid, UPLOADS_PER_HOUR).await {
        return Ok(limited);
    }
    let upload = match read_validated_upload(&mut payload).await {
        Ok(u) => u,
        Err(UploadRejected::Invalid(msg)) => return Err(ApiError::BadRequest(msg)),
        Err(UploadRejected::TooLarge(msg)) => return Ok(errors::payload_too_large(msg)),
    };
    let stored = storage
        .0
        .store_file(&upload.data, &upload.filename, &upload.mime, "temp")
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "portal: failed to store upload");
            ApiError::Internal("Failed to store file".into())
        })?;
    let new_attachment = crate::models::NewAttachment {
        url: stored.url.clone(),
        name: upload.filename.clone(),
        file_size: Some(upload.data.len() as i64),
        mime_type: Some(upload.mime.clone()),
        checksum: Some(upload.checksum),
        comment_id: None,
        uploaded_by: Some(portal.user_uuid),
        transcription: None,
    };
    match tc.run(|conn| crate::repository::create_attachment(conn, new_attachment)) {
        Ok(att) => Ok(HttpResponse::Created().json(json!({
            "id": att.id,
            "name": att.name,
            "file_size": att.file_size,
            "mime_type": att.mime_type,
        }))),
        Err(e) => {
            let _ = storage.0.delete_file(&stored.path).await;
            tracing::error!(error = ?e, "portal: failed to record upload");
            Err(ApiError::Internal("Failed to save attachment".into()))
        }
    }
}

/// Attach the requester's own staged uploads to their comment: move each file
/// into the ticket's folder and point the row at the comment. Ids that aren't
/// theirs, are already attached, or have expired are skipped; a failed move
/// never fails the reply.
async fn claim_uploads(
    tc: &mut TenantConn,
    storage: &crate::extractors::ScopedStorage,
    ids: &[i32],
    owner: Uuid,
    ticket_id: i32,
    comment_id: i32,
) {
    use crate::utils::file_validation::{GUEST_ATTACHMENT_TTL_MINUTES, GUEST_MAX_FILES_PER_TICKET};
    if ids.is_empty() {
        return;
    }
    let ids: Vec<i32> = ids
        .iter()
        .copied()
        .take(GUEST_MAX_FILES_PER_TICKET)
        .collect();
    let since =
        chrono::Utc::now().naive_utc() - chrono::Duration::minutes(GUEST_ATTACHMENT_TTL_MINUTES);
    let candidates = match tc
        .run(|conn| crate::repository::comments::claimable_uploads(conn, &ids, owner, since))
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = ?e, ticket_id, "portal: failed to load uploads to attach");
            return;
        }
    };
    for att in candidates {
        let temp_path = att.url.trim_start_matches("/uploads/").to_string();
        let file_only = temp_path.trim_start_matches("temp/").to_string();
        let new_path = format!("tickets/{ticket_id}/{file_only}");
        if let Err(e) = storage.0.move_file(&temp_path, &new_path).await {
            tracing::warn!(error = ?e, attachment_id = att.id, "portal: failed to move upload");
            continue;
        }
        let url = format!("/uploads/{new_path}");
        if let Err(e) = tc.run(|conn| {
            crate::repository::comments::reparent_attachment(conn, att.id, &url, comment_id, owner)
        }) {
            tracing::warn!(error = ?e, attachment_id = att.id, "portal: failed to attach upload");
        }
    }
}

#[derive(Deserialize)]
pub struct NewPortalTicket {
    pub title: String,
    /// One of the request types from `GET /api/portal/request-types`.
    #[serde(default)]
    pub category_id: Option<i32>,
    #[serde(default)]
    pub description: String,
    /// Ids from `POST /api/portal/files`.
    #[serde(default)]
    pub attachment_ids: Vec<i32>,
}

#[derive(Deserialize)]
pub struct NewPortalReply {
    #[serde(default)]
    pub content: String,
    /// Ids from `POST /api/portal/files`.
    #[serde(default)]
    pub attachment_ids: Vec<i32>,
    /// The requester is answering "is it fixed?" with no: recorded as a
    /// rating alongside the reply (which reopens a closed request).
    #[serde(default)]
    pub still_needs_help: bool,
}

/// `POST /api/portal/tickets` — the customer opens a ticket. Requester is the
/// portal user; the optional description lands as the first customer-visible
/// comment. Created under the pinned actor, so the activity attributes it to
/// the customer.
/// `GET /api/portal/notice`: the live known-issue notice, and whether the
/// requester already follows its incident.
pub async fn get_notice(mut tc: TenantConn, portal: PortalContext) -> impl Responder {
    let viewer = portal.user_uuid;
    let result = tc.run(move |conn| {
        let Some(live) = crate::repository::workspace_notices::active(conn, chrono::Utc::now())?
        else {
            return Ok(json!({ "notice": null, "following": false }));
        };
        let following = match live.incident_ticket_id {
            Some(ticket) => {
                crate::repository::ticket_watchers::watcher_uuids(conn, ticket)?.contains(&viewer)
            }
            None => false,
        };
        Ok::<_, diesel::result::Error>(json!({
            "notice": crate::handlers::notices::PublicNotice::from(live),
            "following": following,
        }))
    });
    match result {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to load the notice");
            errors::internal("Failed to load the notice")
        }
    }
}

/// `POST /api/portal/notices/{id}/follow`: follow the incident behind a live
/// notice instead of filing a duplicate. The requester joins the incident
/// ticket as a participant, so it shows in their portal and its public updates
/// reach them by email.
pub async fn follow_notice(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
) -> impl Responder {
    let notice_id = path.into_inner();
    let viewer = portal.user_uuid;
    let result = tc.run(move |conn| {
        let live = crate::repository::workspace_notices::active(conn, chrono::Utc::now())?;
        let Some(ticket) = live
            .filter(|n| n.id == notice_id)
            .and_then(|n| n.incident_ticket_id)
        else {
            return Ok(None);
        };
        crate::repository::ticket_watchers::add_watcher(conn, ticket, viewer, false)?;
        Ok::<_, diesel::result::Error>(Some(ticket))
    });
    match result {
        Ok(Some(ticket)) => {
            HttpResponse::Ok().json(json!({ "following": true, "ticket_id": ticket }))
        }
        Ok(None) => errors::not_found("That notice isn't live"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to follow a notice");
            errors::internal("Failed to follow the issue")
        }
    }
}

/// A request type as a requester sees it.
#[derive(Serialize)]
pub struct RequestType {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

impl From<crate::models::TicketCategory> for RequestType {
    fn from(c: crate::models::TicketCategory) -> Self {
        Self {
            id: c.id,
            name: c.name,
            description: c.description,
            icon: c.icon,
            color: c.color,
        }
    }
}

/// `GET /api/portal/request-types`: what a requester can pick when opening a
/// request. Empty when the workspace offers none.
pub async fn list_request_types(mut tc: TenantConn, _portal: PortalContext) -> impl Responder {
    match tc.run(crate::repository::categories::requester_request_types) {
        Ok(types) => {
            HttpResponse::Ok().json(types.into_iter().map(RequestType::from).collect::<Vec<_>>())
        }
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to list request types");
            errors::internal("Failed to load request types")
        }
    }
}

pub async fn create_my_ticket(
    mut tc: TenantConn,
    portal: PortalContext,
    search_service: web::Data<Arc<SearchService>>,
    storage: crate::extractors::ScopedStorage,
    body: web::Json<NewPortalTicket>,
) -> impl Responder {
    let body = body.into_inner();
    let attachment_ids = body.attachment_ids.clone();
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return errors::bad_request("Title is required");
    }
    if title.chars().count() > 255 {
        return errors::bad_request("Keep the title to 255 characters");
    }
    let description = body.description.trim().to_string();
    if description.chars().count() > MAX_TEXT_CHARS {
        return errors::bad_request("That's too long. Attach a file for anything longer");
    }
    if let Some(limited) = over_write_limit("request", portal.user_uuid, REQUESTS_PER_HOUR).await {
        return limited;
    }
    let has_files = !attachment_ids.is_empty();
    let user_uuid = portal.user_uuid;
    let search = Arc::clone(search_service.get_ref());

    let requested_type = body.category_id;
    let result = tc.run(move |conn| {
        let default_state = crate::repository::workflow_states::default_state(conn)?;
        let category_id =
            crate::repository::categories::offered_request_type(conn, requested_type)?;
        let new_ticket = NewTicket {
            title: title.clone(),
            workflow_state_id: default_state.id,
            requester_uuid: Some(user_uuid),
            category_id,
            submitted_via: Some("portal".to_string()),
            ..Default::default()
        };
        let annotation = crate::repository::tickets::TicketCreationAnnotation {
            source: Some("portal".to_string()),
            subject: Some(title.clone()),
            ..Default::default()
        };
        let ticket = crate::repository::tickets::create_ticket_with_annotation(
            conn, new_ticket, annotation, None,
        )?;

        // First customer-visible comment carries the description (and any
        // files). Non-fatal (mirrors the guest portal): the ticket is the
        // primary artefact.
        let mut first_comment = None;
        if !description.is_empty() || has_files {
            let new_comment = NewComment {
                content: description.clone(),
                ticket_id: ticket.id,
                user_uuid,
                is_internal: false,
                content_format: ContentFormat::Plaintext,
                ..Default::default()
            };
            let annotation = crate::repository::comments::CommentCreationAnnotation {
                source: Some("portal".to_string()),
                ..Default::default()
            };
            match crate::repository::comments::create_comment_with_annotation(
                conn,
                new_comment,
                annotation,
                Some(&search),
            ) {
                Ok(c) => first_comment = Some(c.id),
                Err(e) => {
                    tracing::warn!(error = ?e, ticket_id = ticket.id, "portal: failed to persist initial comment");
                }
            }
        }
        Ok((ticket, first_comment))
    });

    if let Ok((ticket, Some(comment_id))) = &result {
        claim_uploads(
            &mut tc,
            &storage,
            &attachment_ids,
            user_uuid,
            ticket.id,
            *comment_id,
        )
        .await;
    }

    match result {
        Ok((ticket, _)) => HttpResponse::Created().json(CustomerTicket::new(
            ticket,
            &std::collections::HashMap::new(),
        )),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to create ticket");
            errors::internal("Failed to create ticket")
        }
    }
}

/// `POST /api/portal/tickets/{id}/comments` — the customer replies on one of
/// their own tickets. Ownership is checked first (404 otherwise), and the reply
/// is always a customer-visible (non-internal) comment authored by the customer.
pub async fn reply_to_my_ticket(
    mut tc: TenantConn,
    portal: PortalContext,
    search_service: web::Data<Arc<SearchService>>,
    storage: crate::extractors::ScopedStorage,
    path: web::Path<i32>,
    body: web::Json<NewPortalReply>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let body = body.into_inner();
    let content = body.content.trim().to_string();
    let attachment_ids = body.attachment_ids;
    let still_needs_help = body.still_needs_help;
    if content.is_empty() && attachment_ids.is_empty() {
        return errors::bad_request("Reply cannot be empty");
    }
    if content.chars().count() > MAX_TEXT_CHARS {
        return errors::bad_request("That's too long. Attach a file for anything longer");
    }
    if let Some(limited) = over_write_limit("reply", portal.user_uuid, REPLIES_PER_HOUR).await {
        return limited;
    }
    let vis = VisibilityContext::requester_only(portal.user_uuid);
    let user_uuid = portal.user_uuid;
    let search = Arc::clone(search_service.get_ref());

    let result = tc.run(move |conn| {
        if !can_view_ticket(conn, &vis, ticket_id)? {
            return Ok(None);
        }
        let new_comment = NewComment {
            content,
            ticket_id,
            user_uuid,
            is_internal: false,
            content_format: ContentFormat::Plaintext,
            ..Default::default()
        };
        let annotation = crate::repository::comments::CommentCreationAnnotation {
            source: Some("portal".to_string()),
            ..Default::default()
        };
        let comment = crate::repository::comments::create_comment_with_annotation(
            conn,
            new_comment,
            annotation,
            Some(&search),
        )?;
        if still_needs_help && is_requester(conn, ticket_id, user_uuid)? {
            crate::repository::ticket_ratings::record(
                conn,
                ticket_id,
                user_uuid,
                crate::repository::ticket_ratings::Rating::Bad,
                None,
            )?;
        }
        Ok(Some(comment))
    });

    if let Ok(Some(comment)) = &result {
        claim_uploads(
            &mut tc,
            &storage,
            &attachment_ids,
            user_uuid,
            ticket_id,
            comment.id,
        )
        .await;
    }

    match result {
        Ok(Some(comment)) => HttpResponse::Created().json(comment),
        Ok(None) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to post reply");
            errors::internal("Failed to post reply")
        }
    }
}

/// Most people a requester can add to one request.
const MAX_PARTICIPANTS: usize = 10;

/// Someone on a request, as the portal shows them. `email` only for the
/// requester, who added them.
#[derive(Serialize)]
pub struct Participant {
    pub uuid: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub is_requester: bool,
}

/// The requester and the other non-staff people on a request, for `viewer`.
fn participants_of(
    conn: &mut DbConnection,
    ticket: &crate::models::Ticket,
    viewer: Uuid,
) -> QueryResult<Vec<Participant>> {
    use crate::repository::user_helpers;
    let show_emails = ticket.requester_uuid == Some(viewer);
    let mut uuids: Vec<Uuid> = ticket.requester_uuid.into_iter().collect();
    for w in crate::repository::ticket_watchers::watcher_uuids(conn, ticket.id)? {
        let staff = user_helpers::workspace_role(conn, w).is_some_and(|r| r.is_staff());
        if !staff && !uuids.contains(&w) {
            uuids.push(w);
        }
    }
    let mut out = Vec::new();
    for uuid in uuids {
        let Ok(user) = crate::repository::users::find_active_by_uuid(&uuid, conn) else {
            continue;
        };
        out.push(Participant {
            uuid,
            name: user.name,
            email: show_emails
                .then(|| user_helpers::get_primary_email(&uuid, conn))
                .flatten(),
            is_requester: ticket.requester_uuid == Some(uuid),
        });
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
pub struct AddParticipantRequest {
    pub email: String,
}

/// `POST /api/portal/tickets/{id}/participants`: the requester adds someone
/// by email. They join the workspace as a requester if they're new, can see
/// the request in their portal, get the team's replies by email, and are told
/// they were added. 404 for anyone but the requester.
pub async fn add_participant(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
    body: web::Json<AddParticipantRequest>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let requester = portal.user_uuid;
    let email = body.into_inner().email.trim().to_lowercase();
    if email.len() > 254 || !email.contains('@') || email.contains(char::is_whitespace) {
        return errors::bad_request("Enter a valid email address");
    }
    let email_service = crate::utils::email::EmailService::from_env().ok();
    let result = tc.run(move |conn| {
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        if ticket.requester_uuid != Some(requester) {
            return Ok(Err(errors::not_found("Ticket not found")));
        }
        let current = participants_of(conn, &ticket, requester)?;
        if current.len() > MAX_PARTICIPANTS {
            return Ok(Err(errors::bad_request("This request already has the most people it can")));
        }
        let name = email.split('@').next().unwrap_or(&email).to_string();
        let person = crate::repository::user_helpers::find_or_provision_requester(
            &email, &name, conn, None,
        )?;
        if person.uuid == requester {
            return Ok(Err(errors::bad_request("You're already on this request")));
        }
        // Already a member: a no-op (ON CONFLICT DO NOTHING).
        crate::repository::workspaces::add_membership(
            conn,
            ticket.workspace_id,
            person.uuid,
            "member",
            crate::repository::workspaces::SeatWriteAuthority::Product,
        )?;
        let added = crate::repository::ticket_watchers::add_watcher(conn, ticket_id, person.uuid, false)?;
        if added {
            if let (Some(svc), Some(url)) = (
                email_service.as_ref(),
                crate::utils::portal_ticket_link::view_request_url(
                    conn,
                    ticket.workspace_id,
                    person.uuid,
                    ticket_id,
                ),
            ) {
                let adder = crate::repository::users::find_active_by_uuid(&requester, conn)
                    .map(|u| u.name)
                    .unwrap_or_default();
                let base = url
                    .find("/api/")
                    .map(|i| url[..i].to_string())
                    .unwrap_or_default();
                let branding = crate::utils::email_branding::get_email_branding(conn, &base);
                let locale =
                    crate::repository::user_locale::resolve_effective_locale(conn, person.uuid);
                if let Err(e) = crate::services::transactional_email::enqueue_participant_added(
                    conn,
                    svc,
                    &branding,
                    &email,
                    person.uuid,
                    &adder,
                    ticket_id,
                    &ticket.title,
                    &url,
                    &locale,
                ) {
                    tracing::warn!(error = ?e, ticket_id, "portal: could not queue the participant email");
                }
            }
        }
        Ok(Ok(participants_of(conn, &ticket, requester)?))
    });
    match result {
        Ok(Ok(people)) => HttpResponse::Ok().json(json!({ "participants": people })),
        Ok(Err(resp)) => resp,
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to add participant");
            errors::internal("Failed to add that person")
        }
    }
}

/// `DELETE /api/portal/tickets/{id}/participants/{user_uuid}`: the requester
/// removes someone, or a participant leaves. The requester stays.
pub async fn remove_participant(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<(i32, Uuid)>,
) -> impl Responder {
    let (ticket_id, target) = path.into_inner();
    let viewer = portal.user_uuid;
    let result = tc.run(move |conn| {
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let is_requester = ticket.requester_uuid == Some(viewer);
        if Some(target) == ticket.requester_uuid || !(is_requester || target == viewer) {
            return Ok(None);
        }
        crate::repository::ticket_watchers::remove_watcher(conn, ticket_id, &target)?;
        Ok(Some(participants_of(conn, &ticket, viewer)?))
    });
    match result {
        Ok(Some(people)) => HttpResponse::Ok().json(json!({ "participants": people })),
        Ok(None) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to remove participant");
            errors::internal("Failed to remove that person")
        }
    }
}

/// `POST /api/portal/tickets/{id}/seen`: the requester is looking at the
/// request right now (the page is visible). Emails held for them because they
/// were watching live are dropped once they've seen the ticket.
pub async fn mark_seen(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let viewer = portal.user_uuid;
    let result = tc.run(move |conn| {
        let vis = portal_visibility(conn, viewer)?;
        if !can_view_ticket(conn, &vis, ticket_id)? {
            return Ok(false);
        }
        crate::repository::user_ticket_views::record_view(conn, viewer, ticket_id)?;
        Ok::<_, diesel::result::Error>(true)
    });
    match result {
        Ok(true) => HttpResponse::NoContent().finish(),
        Ok(false) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to record a view");
            errors::internal("Failed to record the view")
        }
    }
}

/// Whether `user` is the ticket's requester (the one who can answer
/// "is it fixed?"; watchers see the request but don't rate it).
fn is_requester(conn: &mut DbConnection, ticket_id: i32, user: Uuid) -> QueryResult<bool> {
    use crate::schema::tickets;
    diesel::select(diesel::dsl::exists(
        tickets::table
            .find(ticket_id)
            .filter(tickets::requester_uuid.eq(user)),
    ))
    .get_result(conn)
}

#[derive(Debug, Deserialize)]
pub struct ResolveRequest {
    /// An optional note with the "fixed" answer.
    #[serde(default)]
    pub comment: Option<String>,
}

/// `POST /api/portal/tickets/{id}/resolve`: the requester says it's fixed.
/// An open request is resolved (moved to the first done state), and the
/// answer is recorded as a good rating. 404 for anyone but the requester.
pub async fn resolve_my_ticket(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
    body: web::Json<ResolveRequest>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let user_uuid = portal.user_uuid;
    let comment = body
        .into_inner()
        .comment
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    let result = tc.run(move |conn| {
        if !crate::repository::ticket_ratings::resolve_as_requester(
            conn,
            ticket_id,
            user_uuid,
            comment.as_deref(),
        )? {
            return Ok(None);
        }
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let states = state_map(conn)?;
        Ok(Some(CustomerTicket::new(ticket, &states)))
    });
    match result {
        Ok(Some(ticket)) => HttpResponse::Ok().json(json!({ "ticket": ticket })),
        Ok(None) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to resolve ticket");
            errors::internal("Failed to resolve ticket")
        }
    }
}
