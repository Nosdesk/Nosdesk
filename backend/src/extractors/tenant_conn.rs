//! Tenant-scoped database connection extractor.
//!
//! Owns the only handler-visible path from the r2d2 pool to a
//! `&mut PgConnection`. Every query runs inside a transaction with
//! the actor and workspace GUCs set, so the Phase 3 RLS policies on
//! tenant tables read a populated `app.workspace_id` on every read
//! and write. Handlers can't accidentally bypass tenant isolation
//! because they never see a raw connection.
//!
//! The pattern is borrowed from PostgREST: every request is one
//! transaction with `SET LOCAL` for the per-request session state.
//! This shape is PgBouncer-transaction-mode safe, doesn't depend on
//! per-connection cleanup hooks (r2d2 has none), and makes "forgot
//! to set the GUC" impossible at the type level.
//!
//! Usage:
//! ```ignore
//! pub async fn list_tickets(mut tc: TenantConn) -> impl Responder {
//!     match tc.run(|conn| repository::tickets::list(conn)) {
//!         Ok(rows) => HttpResponse::Ok().json(rows),
//!         Err(e) => errors::internal(&e.to_string()),
//!     }
//! }
//! ```
//!
//! Cross-workspace operations (registry sync, partition rotation,
//! workspace lifecycle handlers, public unauth writes) use a
//! distinct extractor [`PlatformConn`](super::PlatformConn) that
//! elevates to the `nosdesk_admin` BYPASSRLS role. The cross-
//! tenant audit surface is the **extractor name in the handler
//! signature**, not a method on this one.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::{ready, Ready};

use crate::db::{DbConnection, Pool};
use crate::middleware::RequestContext;
use crate::sync::actor::ActorContext;
use crate::sync::session;

// `Claims` and `Uuid` are only needed for the test-only fallback in
// `from_request` that reconstructs an actor from raw Claims when
// handler-level unit tests bypass the middleware chain.
#[cfg(test)]
use crate::models::Claims;
#[cfg(test)]
use uuid::Uuid;

/// Handler extractor that yields a tenant-scoped connection bound
/// to the request's actor + workspace context.
///
/// Holds the pool reference, not a checked-out connection. Each
/// `run` / `unscoped_run` call acquires a fresh connection, runs
/// the closure inside a transaction with the GUCs primed, and
/// releases the connection back to the pool. This keeps the
/// extractor cheap to construct (no pool round-trip at
/// from_request time), avoids holding a connection across
/// independent repo calls inside a handler, and plays nicely
/// with co-residing extractors that also acquire connections
/// (TicketAccess, AuthContext) — none of them block waiting for
/// a single-conn test pool.
pub struct TenantConn {
    pool: web::Data<Pool>,
    actor: ActorContext,
}

impl TenantConn {
    /// Run a closure inside a transaction with the actor GUCs set,
    /// including `app.workspace_id`. RLS policies on tenant tables
    /// see the workspace and filter accordingly. INSERT/UPDATE
    /// against a different workspace's rows fails the policy's
    /// WITH CHECK.
    pub fn run<T>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> diesel::QueryResult<T>,
    ) -> diesel::QueryResult<T> {
        let mut conn = self.pool.get().map_err(|e| {
            diesel::result::Error::QueryBuilderError(format!("pool acquire: {e}").into())
        })?;
        session::with_actor_context(&mut conn, &self.actor, f)
    }

    /// Like [`run`], but for closures that return a domain error type
    /// (rather than a bare `diesel::Error`). The only requirement is that
    /// the error can carry a `diesel::Error` (`E: From<diesel::Error>`),
    /// so pool-acquire and transaction failures still surface. Used by
    /// handlers whose repository returns a typed error the handler maps to
    /// HTTP status codes.
    ///
    /// [`run`]: Self::run
    pub fn run_result<T, E>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<diesel::result::Error>,
    {
        let mut conn = self.pool.get().map_err(|e| {
            diesel::result::Error::QueryBuilderError(format!("pool acquire: {e}").into())
        })?;
        session::with_actor_context(&mut conn, &self.actor, f)
    }

    /// Workspace the request actor is pinned to, if any. Mirrors
    /// the value set by auth middleware from `WorkspaceContext`.
    pub fn workspace_id(&self) -> Option<i32> {
        self.actor.workspace_id
    }

    /// Stamp a client-minted correlation id onto the sync actions emitted by
    /// the next `run` / `run_result` (for optimistic-create reconciliation:
    /// the client matches the echoed `correlation_id` to its pending row). The
    /// actor carries no correlation otherwise.
    pub fn set_correlation_id(&mut self, id: uuid::Uuid) {
        self.actor.correlation_id = Some(id);
    }
}

/// Error type for `TenantConn` extraction failures.
#[derive(Debug)]
pub enum TenantConnError {
    /// No `RequestContext` on the request — auth middleware didn't
    /// run, or the route is wired up wrong. `TenantConn` is only
    /// valid on authenticated routes.
    MissingRequestContext,
    /// Couldn't get a connection from the pool. Usually means the
    /// DB is unreachable or the pool is exhausted.
    PoolError(String),
    /// Selection mode is active but the request resolved no workspace to pin.
    /// A `TenantConn` route is always workspace-scoped, so an unpinned actor
    /// would read every tenant query as empty (RLS) — fail closed with a clear
    /// 400 rather than degrade into empty results or a downstream panic.
    NoWorkspaceSelected,
}

impl std::fmt::Display for TenantConnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRequestContext => write!(f, "Authentication required"),
            Self::PoolError(e) => write!(f, "Database error: {e}"),
            Self::NoWorkspaceSelected => write!(f, "No workspace selected"),
        }
    }
}

impl actix_web::ResponseError for TenantConnError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::HttpResponse;
        match self {
            Self::MissingRequestContext => HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "Authentication required"})),
            Self::PoolError(_) => HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Internal server error"})),
            Self::NoWorkspaceSelected => HttpResponse::BadRequest()
                .json(serde_json::json!({"error": "No workspace selected"})),
        }
    }
}

impl FromRequest for TenantConn {
    type Error = TenantConnError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Primary path (production + integration tests): the auth
        // middlewares populate RequestContext and it carries the
        // workspace-pinned actor.
        //
        // Fallback path (handler-level unit tests only): some tests
        // insert raw Claims into request extensions without going
        // through the middleware chain. The fallback reconstructs a
        // best-effort actor; the workspace pin comes from the
        // ambient GUC that setup_test_connection pre-sets. The
        // fallback is gated behind cfg(test) so it cannot execute
        // in a release build — if a production route is ever wired
        // with an auth middleware but without
        // WorkspaceContextMiddleware, the request fails fast with
        // 401 rather than silently degrading to a None-workspace
        // actor that returns empty results from every tenant
        // query.
        let actor = if let Some(ctx) = req.extensions().get::<RequestContext>().cloned() {
            ctx.actor
        } else {
            #[cfg(test)]
            {
                if let Some(claims) = req.extensions().get::<Claims>().cloned() {
                    match Uuid::parse_str(&claims.sub) {
                        Ok(uuid) => ActorContext::user(uuid, None),
                        Err(_) => return ready(Err(TenantConnError::MissingRequestContext)),
                    }
                } else {
                    return ready(Err(TenantConnError::MissingRequestContext));
                }
            }
            #[cfg(not(test))]
            {
                return ready(Err(TenantConnError::MissingRequestContext));
            }
        };

        // Under selection mode (hosted single-origin agent app), the workspace
        // is carried per-request via the `X-Nosdesk-Workspace` header. A
        // `TenantConn` route with no resolved workspace means the header was
        // absent or didn't resolve: fail closed with 400 here, before any
        // RLS-empty read reaches a handler. Host mode (self-hosted / subdomain)
        // always has a Host-derived workspace, so this branch is unreachable
        // there and self-hosted is unaffected.
        if actor.workspace_id.is_none()
            && crate::middleware::workspace_context::selection_resolution_enabled()
        {
            return ready(Err(TenantConnError::NoWorkspaceSelected));
        }

        let pool = match req.app_data::<web::Data<Pool>>() {
            Some(p) => p.clone(),
            None => {
                return ready(Err(TenantConnError::PoolError(
                    "pool not configured".into(),
                )))
            }
        };

        ready(Ok(TenantConn { pool, actor }))
    }
}
