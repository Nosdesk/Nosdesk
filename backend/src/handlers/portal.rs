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
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::handlers::errors;
use crate::middleware::cookie_auth::{require_workspace_membership, PORTAL_SCOPE};
use crate::models::{Claims, ContentFormat, NewComment, NewTicket, Ticket, User};
use crate::repository::ticket_visibility::{
    can_view_ticket, visible_tickets_query, VisibilityContext,
};
use crate::schema::tickets;
use crate::services::search::SearchService;
use crate::utils::jwt::JwtUtils;
use crate::utils::reset_tokens::{ResetTokenUtils, TokenType};
use diesel::prelude::*;

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
        .cookie(crate::utils::cookies::PORTAL_ACCESS_TOKEN_COOKIE)
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

/// `GET /api/portal/tickets` — the customer's own tickets in this workspace.
///
/// RLS pins to the workspace (the portal origin's tenant); the visibility
/// context is forced requester-only, so the rows are exactly the tickets this
/// customer requested or watches, never another customer's.
pub async fn list_my_tickets(mut tc: TenantConn, portal: PortalContext) -> impl Responder {
    let vis = VisibilityContext::requester_only(portal.user_uuid);
    let result = tc.run(move |conn| {
        visible_tickets_query(&vis)
            .order(tickets::updated_at.desc())
            .load::<Ticket>(conn)
    });
    match result {
        Ok(rows) => HttpResponse::Ok().json(rows),
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
    let vis = VisibilityContext::requester_only(portal.user_uuid);
    let result = tc.run(move |conn| {
        if !can_view_ticket(conn, &vis, ticket_id)? {
            return Ok(None);
        }
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let comments =
            crate::repository::comments::get_public_comments_by_ticket_id(conn, ticket_id)?;
        Ok(Some((ticket, comments)))
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

#[derive(Deserialize)]
pub struct NewPortalTicket {
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
pub struct NewPortalReply {
    pub content: String,
}

/// `POST /api/portal/tickets` — the customer opens a ticket. Requester is the
/// portal user; the optional description lands as the first customer-visible
/// comment. Created under the pinned actor, so the activity attributes it to
/// the customer.
pub async fn create_my_ticket(
    mut tc: TenantConn,
    portal: PortalContext,
    search_service: web::Data<Arc<SearchService>>,
    body: web::Json<NewPortalTicket>,
) -> impl Responder {
    let body = body.into_inner();
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return errors::bad_request("Title is required");
    }
    let description = body.description.trim().to_string();
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

        // First customer-visible comment carries the description. Non-fatal
        // (mirrors the guest portal): the ticket is the primary artefact.
        if !description.is_empty() {
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
            if let Err(e) = crate::repository::comments::create_comment_with_annotation(
                conn,
                new_comment,
                annotation,
                Some(&search),
            ) {
                tracing::warn!(error = ?e, ticket_id = ticket.id, "portal: failed to persist initial comment");
            }
        }
        Ok(ticket)
    });

    match result {
        Ok(ticket) => HttpResponse::Created().json(ticket),
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
    path: web::Path<i32>,
    body: web::Json<NewPortalReply>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let content = body.into_inner().content.trim().to_string();
    if content.is_empty() {
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

    match result {
        Ok(Some(comment)) => HttpResponse::Created().json(comment),
        Ok(None) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to post reply");
            errors::internal("Failed to post reply")
        }
    }
}
