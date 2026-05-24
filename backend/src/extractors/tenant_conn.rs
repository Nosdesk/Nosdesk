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
//! workspace lifecycle handlers) use [`TenantConn::unscoped_run`]
//! which sets `app.bypass_workspace_check = 'true'` so the RLS
//! policy's bypass disjunct lets the query through. Every
//! `unscoped_run` call site is greppable and audit-reviewable.

use actix_web::{dev::Payload, web, FromRequest, HttpMessage, HttpRequest};
use std::future::{ready, Ready};

use crate::db::{DbConnection, Pool};
use crate::middleware::RequestContext;
use crate::sync::actor::ActorContext;
use crate::sync::session;

/// Handler extractor that yields a tenant-scoped connection bound
/// to the request's actor + workspace context.
pub struct TenantConn {
    conn: DbConnection,
    actor: ActorContext,
}

impl TenantConn {
    /// Run a closure inside a transaction with the actor GUCs set,
    /// including `app.workspace_id`. RLS policies on tenant tables
    /// see the workspace and filter accordingly. INSERT/UPDATE
    /// against a different workspace's rows fails the policy's
    /// WITH CHECK.
    pub fn run<T, E>(&mut self, f: impl FnOnce(&mut DbConnection) -> Result<T, E>) -> Result<T, E>
    where
        E: From<diesel::result::Error>,
    {
        session::with_actor_context(&mut self.conn, &self.actor, f)
    }

    /// Like `run`, but with `app.bypass_workspace_check = 'true'`
    /// set in the same transaction. Reserved for legitimately
    /// cross-workspace operations: workspace lifecycle handlers,
    /// background jobs that span tenants, super-admin tools.
    ///
    /// Every call site of this method is part of the audit-review
    /// surface — grep for `unscoped_run` to enumerate them.
    #[allow(dead_code)]
    pub fn unscoped_run<T, E>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<diesel::result::Error>,
    {
        session::with_actor_bypass_context(&mut self.conn, &self.actor, f)
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
}

impl std::fmt::Display for TenantConnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRequestContext => write!(f, "Authentication required"),
            Self::PoolError(e) => write!(f, "Database error: {e}"),
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
        }
    }
}

impl FromRequest for TenantConn {
    type Error = TenantConnError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let actor = match req.extensions().get::<RequestContext>().cloned() {
            Some(ctx) => ctx.actor,
            None => return ready(Err(TenantConnError::MissingRequestContext)),
        };

        let pool = match req.app_data::<web::Data<Pool>>() {
            Some(p) => p,
            None => {
                return ready(Err(TenantConnError::PoolError(
                    "pool not configured".into(),
                )))
            }
        };

        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => return ready(Err(TenantConnError::PoolError(e.to_string()))),
        };

        ready(Ok(TenantConn { conn, actor }))
    }
}
