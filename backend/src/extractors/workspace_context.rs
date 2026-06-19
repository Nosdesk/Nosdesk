//! Workspace context extractor.
//!
//! Provides handler-friendly access to the workspace resolved
//! for the current request. Pair with the
//! `WorkspaceContextMiddleware` (which does the actual
//! resolution and stuffs the context into the request
//! extensions) — this extractor pulls it back out per handler.
//!
//! Usage:
//! ```ignore
//! pub async fn list_tickets(
//!     ws: WorkspaceContext,
//!     auth: AuthContext,
//!     pool: web::Data<Pool>,
//! ) -> impl Responder {
//!     let tickets = repository::tickets::list(&mut conn, ws.workspace_id, ...)?;
//!     ...
//! }
//! ```
//!
//! For routes that are valid on the apex domain (signup,
//! marketing, password reset to the apex), use
//! `Option<WorkspaceContext>` instead — `None` then means the
//! request didn't carry a resolvable workspace context.

use actix_web::{dev::Payload, FromRequest, HttpMessage, HttpRequest};
use std::future::{ready, Ready};
use uuid::Uuid;

/// Resolved workspace for the current request. Cloned from the
/// request extensions on extraction (the middleware stuffs it
/// there). Cheap to clone — small struct with a couple of short
/// `String`s.
#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    pub workspace_id: i32,
    pub workspace_uuid: Uuid,
    pub slug: String,
    pub name: String,
    /// Verified custom domain (`workspaces.custom_domain`) if the tenant set
    /// one. Drives `canonical_origin`: a custom domain is the workspace's
    /// browser host instead of `<slug>.<tenant_domain>`.
    pub custom_domain: Option<String>,
    /// Nullable seam for a future org-as-parent-of-workspaces
    /// tier. NULL on every workspace today.
    pub organisation_id: Option<i32>,
}

impl WorkspaceContext {
    /// The workspace's canonical browser origin (`https://<host>`), for
    /// building tenant-facing URLs (password-reset / invite links, WebAuthn,
    /// OIDC). A verified `custom_domain` wins; otherwise
    /// `<slug>.<NOSDESK_TENANT_DOMAIN>`. Returns `None` in self-hosted mode or
    /// when no tenant base domain is configured, the caller then falls back to
    /// `FRONTEND_URL` or the request host. Delegates to `utils::tenant_origin`,
    /// the single source of truth shared with the `Workspace`-keyed callers.
    pub fn canonical_origin(&self) -> Option<String> {
        use crate::utils::tenant_origin;
        let host = tenant_origin::canonical_host_for(
            &self.slug,
            self.custom_domain.as_deref(),
            tenant_origin::tenant_domain().as_deref(),
        );
        tenant_origin::origin_from_host(host)
    }
}

/// Error type for `WorkspaceContext` extraction failures.
#[derive(Debug)]
pub enum WorkspaceContextError {
    /// The middleware didn't run or didn't resolve a workspace.
    /// In hosted mode this usually means the request came to
    /// the apex domain or to an unknown subdomain; in self-
    /// hosted it means the bootstrap workspace couldn't be
    /// loaded (configuration / migration issue).
    Missing,
}

impl std::fmt::Display for WorkspaceContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "No workspace context for this request"),
        }
    }
}

impl actix_web::ResponseError for WorkspaceContextError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::HttpResponse;
        match self {
            Self::Missing => {
                HttpResponse::NotFound().json(serde_json::json!({"error": "Workspace not found"}))
            }
        }
    }
}

impl FromRequest for WorkspaceContext {
    type Error = WorkspaceContextError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let ctx = req.extensions().get::<WorkspaceContext>().cloned();
        ready(ctx.ok_or(WorkspaceContextError::Missing))
    }
}

// `Option<WorkspaceContext>` is automatically supported via
// actix's blanket `impl FromRequest for Option<T> where
// T: FromRequest`: extraction failure yields `Ok(None)`. Apex-
// domain routes (signup, marketing) take `Option<_>` and the
// handler branches on `Some` / `None`.
