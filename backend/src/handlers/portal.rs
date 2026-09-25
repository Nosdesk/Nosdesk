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
use crate::middleware::cookie_auth::{require_workspace_membership, PORTAL_SCOPE};
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
        .route("/tickets/{id}", web::get().to(get_my_ticket))
        .route("/tickets/{id}/comments", web::post().to(reply_to_my_ticket))
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
            require_workspace_membership(conn, workspace_id, user.uuid)
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
    if !crate::middleware::cookie_auth::is_workspace_member(&mut conn, ctx.workspace_id, user.uuid)
    {
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

    // Branding read + enqueue touch workspace-isolated tables. Run pinned as the
    // RLS-enforced runtime role so the branding read (no explicit workspace
    // filter) returns THIS workspace's settings, not an arbitrary tenant's.
    let raw_token = token.raw_token.clone();
    let user_name = user.name.clone();
    let _ = crate::sync::session::run_in_workspace(
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
        .append_header(("Location", "/"))
        .finish())
}

/// Bounce a failed sign-in back to the portal with a generic error flag (a bad,
/// expired, or already-used link). Uniform regardless of the specific failure.
fn sign_in_error_redirect() -> HttpResponse {
    HttpResponse::Found()
        .append_header(("Location", "/login?signin_error=1"))
        .finish()
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

// tenant-read-exempt: authorize_portal_request's only tenant read is require_workspace_membership, which pins via with_actor_context (invisible to the scanner).
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

    let token = req
        .cookie(&crate::utils::cookies::cookie_name(
            crate::utils::cookies::PORTAL_ACCESS_TOKEN_COOKIE,
        ))
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let (claims, _user) = JwtUtils::authenticate_with_token(token.value(), &mut conn)
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
        }
    }
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
    let vis = VisibilityContext::requester_only(portal.user_uuid);
    let result = tc.run(move |conn| {
        let rows = visible_tickets_query(&vis)
            .order(tickets::updated_at.desc())
            .load::<Ticket>(conn)?;
        let states = state_map(conn)?;
        Ok::<_, diesel::result::Error>(
            rows.into_iter()
                .map(|t| CustomerTicket::new(t, &states))
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
    let vis = VisibilityContext::requester_only(portal.user_uuid);
    let result = tc.run(move |conn| {
        if !can_view_ticket(conn, &vis, ticket_id)? {
            return Ok(None);
        }
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let states = state_map(conn)?;
        let comments = customer_thread(conn, ticket_id, viewer)?;
        Ok(Some((CustomerTicket::new(ticket, &states), comments)))
    });
    match result {
        Ok(Some((ticket, comments))) => HttpResponse::Ok().json(json!({
            "ticket": ticket,
            "comments": comments,
        })),
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
    let vis = VisibilityContext::requester_only(portal.user_uuid);
    let url = tc
        .run(move |conn| {
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
    crate::handlers::files::serve_or_not_found(storage, file_path, &req).await
}

/// `POST /api/portal/files`: stage one file for the requester's next reply or
/// request. Same validation as the guest form (size cap, safe types); the row
/// is marked as theirs and claimed by [`claim_uploads`].
pub async fn upload_file(
    mut tc: TenantConn,
    portal: PortalContext,
    storage: crate::extractors::ScopedStorage,
    mut payload: actix_multipart::Multipart,
) -> Result<HttpResponse, ApiError> {
    use crate::handlers::guest::{read_validated_upload, UploadRejected};
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
}

/// `POST /api/portal/tickets` — the customer opens a ticket. Requester is the
/// portal user; the optional description lands as the first customer-visible
/// comment. Created under the pinned actor, so the activity attributes it to
/// the customer.
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
    let description = body.description.trim().to_string();
    let has_files = !attachment_ids.is_empty();
    let user_uuid = portal.user_uuid;
    let search = Arc::clone(search_service.get_ref());

    let result = tc.run(move |conn| {
        let default_state = crate::repository::workflow_states::default_state(conn)?;
        let new_ticket = NewTicket {
            title: title.clone(),
            workflow_state_id: default_state.id,
            requester_uuid: Some(user_uuid),
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
    if content.is_empty() && attachment_ids.is_empty() {
        return errors::bad_request("Reply cannot be empty");
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
