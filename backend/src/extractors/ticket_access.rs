//! Visibility-gated ticket-scoped handler extractor.
//!
//! Replaces the call-site pattern:
//!
//! ```ignore
//! let Some(vis) = VisibilityContext::from_claims(&claims) else {
//!     return errors::not_found_msg("Ticket not found");
//! };
//! match ticket_visibility::can_view_ticket(&mut conn, &vis, ticket_id) {
//!     Ok(true) => {}
//!     Ok(false) => return errors::not_found_msg("Ticket not found"),
//!     Err(e) => return errors::internal(...),
//! }
//! ```
//!
//! with a single typed parameter:
//!
//! ```ignore
//! pub async fn my_handler(access: TicketAccess, ...) -> impl Responder {
//!     let ticket_id = access.ticket_id;
//!     let auth = &access.auth;
//!     // ... handler is unreachable unless the user can read this ticket.
//! }
//! ```
//!
//! Why an extractor, not a middleware
//! ==================================
//!
//! A middleware would have to be route-scoped (it can't know which
//! routes are ticket-scoped without configuration) and would still
//! need a way to surface the resolved `AuthContext` + `ticket_id`
//! to the handler. The extractor pattern is the canonical Actix
//! shape for "preconditions a handler depends on" — the type
//! system enforces that the gate ran before the handler body
//! executes. Forgetting the check now requires a typed handler to
//! ask for `web::Path<i32>` instead of `TicketAccess`, which shows
//! up in code review.
//!
//! Route placeholders
//! ==================
//!
//! The extractor reads the ticket id from `match_info()`, trying
//! `id` first (the bulk of ticket routes) then `ticket_id` (the
//! sub-resource routes like `/tickets/{ticket_id}/comments`). If
//! both are absent the extractor errors out with `400` rather
//! than `404`, because that's a routing wiring bug and we want
//! the operator to see it, not have it disappear into a 404.

use std::future::Future;
use std::pin::Pin;

use actix_web::{dev::Payload, web, FromRequest, HttpRequest, HttpResponse};
use tracing::error;

use crate::db::Pool;
use crate::repository::ticket_visibility::{self, VisibilityContext};

use super::AuthContext;

#[derive(Debug)]
pub struct TicketAccess {
    pub ticket_id: i32,
    pub auth: AuthContext,
}

#[derive(Debug)]
pub enum TicketAccessError {
    /// Auth extraction failed (no claims, bad uuid, user missing).
    Auth(super::auth_context::AuthContextError),
    /// The route registration is missing an `{id}` / `{ticket_id}`
    /// placeholder. Surfaced as 400 — this is a server-config bug,
    /// not a "ticket doesn't exist" condition.
    NoTicketIdInRoute,
    /// The path component was present but not an i32.
    BadTicketId,
    /// Visibility resolution failed at the data layer.
    Database(String),
    /// User exists, ticket exists, user can't read it — 404 per
    /// OWASP IDOR Cheatsheet (a 403 would leak existence).
    NotVisible,
}

impl std::fmt::Display for TicketAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth(e) => write!(f, "{e}"),
            Self::NoTicketIdInRoute => write!(f, "route missing ticket id placeholder"),
            Self::BadTicketId => write!(f, "ticket id is not a valid integer"),
            Self::Database(e) => write!(f, "{e}"),
            Self::NotVisible => write!(f, "Ticket not found"),
        }
    }
}

impl actix_web::ResponseError for TicketAccessError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::Auth(e) => e.error_response(),
            Self::NoTicketIdInRoute => HttpResponse::InternalServerError().json(
                serde_json::json!({"error": "Server misconfiguration: route is not ticket-scoped"}),
            ),
            Self::BadTicketId => HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Invalid ticket id"
            })),
            Self::Database(_) => HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to verify ticket access"
            })),
            // OWASP IDOR: deny reads with 404, never 403.
            Self::NotVisible => HttpResponse::NotFound().json(serde_json::json!({
                "error": "Ticket not found"
            })),
        }
    }
}

impl FromRequest for TicketAccess {
    type Error = TicketAccessError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        // Kick off AuthContext extraction first; it owns the DB
        // pool acquisition and the user/role lookup, and there's
        // no reason to run the visibility query before we know who
        // the caller is.
        let auth_fut = AuthContext::from_request(req, payload);
        let req = req.clone();

        Box::pin(async move {
            let auth = auth_fut.await.map_err(TicketAccessError::Auth)?;

            // Resolve the ticket id from the URL. Route registration
            // uses both `{id}` and `{ticket_id}` depending on whether
            // the ticket is the primary resource or a parent of one.
            let raw = req
                .match_info()
                .get("id")
                .or_else(|| req.match_info().get("ticket_id"))
                .ok_or(TicketAccessError::NoTicketIdInRoute)?;
            let ticket_id: i32 = raw.parse().map_err(|_| TicketAccessError::BadTicketId)?;

            // Visibility gate. Reuses the same primitive as
            // list / search filtering so single-record and list
            // surfaces can't drift.
            let pool = req
                .app_data::<web::Data<Pool>>()
                .ok_or_else(|| TicketAccessError::Database("Pool not found".into()))?;
            let mut conn = pool
                .get()
                .map_err(|e| TicketAccessError::Database(e.to_string()))?;
            let vis = VisibilityContext::from_auth(&auth);
            // Run the RLS-gated visibility check inside a workspace-pinned
            // transaction, exactly as `TenantConn` runs every tenant query.
            // `can_view_ticket` carries no explicit workspace filter and the
            // tickets policy keys off `app.workspace_id`; `with_actor_context`
            // sets it with SET LOCAL, so the read scopes to this request's
            // workspace and reverts at commit, leaving nothing on the pooled
            // connection to leak. A ticket route always resolves a workspace;
            // if one somehow didn't, the unpinned read fails closed (404).
            let allowed = match crate::handlers::helpers::request_workspace_id(&req) {
                Some(ws) => {
                    let actor =
                        crate::sync::actor::ActorContext::user_at_workspace(auth.user_uuid, ws);
                    crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
                        ticket_visibility::can_view_ticket(c, &vis, ticket_id)
                    })
                }
                None => ticket_visibility::can_view_ticket(&mut conn, &vis, ticket_id),
            }
            .map_err(|e| {
                error!(error = ?e, ticket_id, "ticket visibility check failed");
                TicketAccessError::Database(e.to_string())
            })?;
            if !allowed {
                return Err(TicketAccessError::NotVisible);
            }
            Ok(TicketAccess { ticket_id, auth })
        })
    }
}
