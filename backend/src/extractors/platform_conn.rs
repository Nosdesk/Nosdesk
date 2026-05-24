//! Cross-tenant ("platform") database connection extractor.
//!
//! Companion to [`TenantConn`](super::TenantConn). Where `TenantConn`
//! confines every query to the requesting workspace via RLS,
//! `PlatformConn` runs queries as the `nosdesk_admin` role
//! (BYPASSRLS) so they see and modify rows across every workspace.
//!
//! The split is intentional: the cross-tenant audit surface is now
//! the **extractor name in the handler signature**, not a method
//! name buried in the handler body. A handler that takes
//! `pc: PlatformConn` is declaring "I read or write across
//! tenants" at code-review time and at route-registration time. A
//! handler that takes `tc: TenantConn` is declaring the opposite.
//! There's no way to flip between them at runtime without changing
//! the signature.
//!
//! Reserved for:
//!   * Workspace lifecycle handlers (create / archive / hard-delete
//!     workspace)
//!   * Registry / plugin / scheduled-job dispatch handlers that
//!     enumerate state across workspaces (not yet wired)
//!   * Public unauthenticated endpoints that must write to an RLS-
//!     enabled table without a workspace pin (csp_reports is the
//!     canonical example)
//!
//! Authentication is **not** required by this extractor — public
//! endpoints can extract it. If you want auth, also extract
//! `AuthContext` in the same handler. The actor attribution falls
//! back to a `system` actor when there's no `RequestContext` on
//! the request, so cross-tenant writes are still audit-traceable
//! to *something*.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::{ready, Ready};

use crate::db::{DbConnection, Pool};
use crate::middleware::RequestContext;
use crate::sync::actor::ActorContext;
use crate::sync::session;

/// Handler extractor that yields a connection elevated to
/// `nosdesk_admin` (BYPASSRLS) for cross-tenant work.
pub struct PlatformConn {
    pool: web::Data<Pool>,
    /// Audit attribution. Inherited from `RequestContext.actor`
    /// when present so workspace-lifecycle handlers carry the
    /// admin user's UUID; otherwise a `system` actor labelled
    /// `"platform:<reason>"` so public endpoints (csp_reports,
    /// guest paths) still attribute to something.
    actor: ActorContext,
}

impl PlatformConn {
    /// Run a closure inside a transaction with actor GUCs set and
    /// the role elevated to `nosdesk_admin` for the txn. Every
    /// query inside the closure bypasses RLS via the BYPASSRLS
    /// role attribute. On commit / rollback, `SET LOCAL ROLE`
    /// reverts; the connection returns to the pool with
    /// `nosdesk_app` semantics intact.
    pub fn run<T>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> diesel::QueryResult<T>,
    ) -> diesel::QueryResult<T> {
        let mut conn = self.pool.get().map_err(|e| {
            diesel::result::Error::QueryBuilderError(format!("pool acquire: {e}").into())
        })?;
        session::with_actor_bypass_context(&mut conn, &self.actor, f)
    }
}

/// Error type for `PlatformConn` extraction failures.
#[derive(Debug)]
pub enum PlatformConnError {
    /// Couldn't get a connection from the pool. Usually means the
    /// DB is unreachable or the pool is exhausted.
    PoolError(String),
}

impl std::fmt::Display for PlatformConnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PoolError(e) => write!(f, "Database error: {e}"),
        }
    }
}

impl actix_web::ResponseError for PlatformConnError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::HttpResponse;
        match self {
            Self::PoolError(_) => HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Internal server error"})),
        }
    }
}

impl FromRequest for PlatformConn {
    type Error = PlatformConnError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Attribution: prefer the user actor from RequestContext
        // (workspace pin doesn't matter here because we'll elevate
        // to BYPASSRLS anyway, but audit_log gets the user UUID).
        // Fall back to a system actor for public unauth endpoints.
        let actor = req
            .extensions()
            .get::<RequestContext>()
            .map(|ctx| ctx.actor.clone())
            .unwrap_or_else(|| ActorContext::system("platform:unauth"));

        let pool = match req.app_data::<web::Data<Pool>>() {
            Some(p) => p.clone(),
            None => {
                return ready(Err(PlatformConnError::PoolError(
                    "pool not configured".into(),
                )))
            }
        };

        ready(Ok(PlatformConn { pool, actor }))
    }
}
