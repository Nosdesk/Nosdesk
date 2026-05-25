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
    /// `"platform:fallback:<route-path>"` so any cross-tenant
    /// write that didn't go through middleware is still
    /// traceable to a specific endpoint via the audit_log.
    /// Public endpoints (csp_reports, guest paths) should
    /// override via `with_actor` with a stable
    /// `"handler:<name>"` label.
    actor: ActorContext,
}

impl PlatformConn {
    /// Override the actor attached to this extractor.
    ///
    /// Useful for public unauth handlers that need to pin a
    /// fallback workspace_id (so an RLS-enabled table whose
    /// `workspace_id` column has a `current_setting('app.workspace_id')`
    /// default still satisfies its NOT NULL constraint). The
    /// canonical case is `csp_reports`: the report intake is
    /// public, the table NOT-NULLs workspace_id with a GUC-driven
    /// default, so the handler needs to seed the GUC via an actor
    /// even though RLS itself is bypassed.
    ///
    /// For workspace-lifecycle handlers and other authenticated
    /// platform endpoints, the inherited RequestContext actor is
    /// the right answer and this method is unneeded.
    pub fn with_actor(mut self, actor: ActorContext) -> Self {
        self.actor = actor;
        self
    }

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
        // Fall back to a route-stamped system actor for handlers
        // that ran before middleware attached a RequestContext
        // (public unauth endpoints, certain test paths). Embedding
        // the route makes the fallback row debuggable in audit_log
        // instead of all collapsing into a single anonymous bucket.
        // Handlers with a stable identity should still override via
        // `with_actor` so the reference doesn't drift if the route
        // path is renamed.
        let actor = req
            .extensions()
            .get::<RequestContext>()
            .map(|ctx| ctx.actor.clone())
            .unwrap_or_else(|| ActorContext::system(format!("platform:fallback:{}", req.path())));

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

// ---- Phase 3i.7: PlatformConn extractor coverage ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::actor::ActorKind;
    use actix_web::test::TestRequest;
    use uuid::Uuid;

    /// Mirror the real handler-extraction shape: build a test
    /// HttpRequest with the test pool attached, call from_request,
    /// return the extracted PlatformConn. Centralises the pool
    /// plumbing so the tests can stay focused on actor attribution.
    async fn extract(req_builder: TestRequest) -> PlatformConn {
        let pool = crate::test_helpers::setup_test_pool();
        let req = req_builder.app_data(web::Data::new(pool)).to_http_request();
        PlatformConn::from_request(&req, &mut Payload::None)
            .await
            .expect("PlatformConn extraction succeeded")
    }

    #[actix_web::test]
    async fn fallback_actor_embeds_route_path() {
        // Without a RequestContext in extensions (the public-unauth
        // path), the actor falls back to a system actor whose
        // reference is the request path. Catches a regression where
        // the fallback collapses every unauthenticated cross-tenant
        // write into a single anonymous "platform:unauth" bucket.
        let pc = extract(TestRequest::default().uri("/api/csp-report")).await;
        assert_eq!(pc.actor.kind, ActorKind::System);
        assert_eq!(
            pc.actor.reference.as_deref(),
            Some("platform:fallback:/api/csp-report")
        );
    }

    #[actix_web::test]
    async fn inherits_request_context_actor_when_present() {
        // Authenticated platform handlers (workspace lifecycle,
        // admin sync dispatchers) inherit the user actor from
        // RequestContext so audit_log records who initiated the
        // cross-tenant op rather than an anonymous fallback.
        let user_uuid = Uuid::now_v7();
        let ctx_actor = ActorContext::user(user_uuid, None);
        let req_ctx = RequestContext::new(Uuid::now_v7(), ctx_actor);

        let req = TestRequest::default()
            .uri("/api/admin/workspaces/42/archive")
            .app_data(web::Data::new(crate::test_helpers::setup_test_pool()))
            .to_http_request();
        req.extensions_mut().insert(req_ctx);

        let pc = PlatformConn::from_request(&req, &mut Payload::None)
            .await
            .expect("extract");
        assert_eq!(pc.actor.kind, ActorKind::User);
        assert_eq!(pc.actor.uuid, Some(user_uuid));
    }

    #[actix_web::test]
    async fn with_actor_overrides_inherited_actor() {
        // Public unauth handlers should override the fallback with
        // a stable "handler:<name>" label so the audit trail
        // doesn't drift if a route path is renamed. Verifies the
        // builder swaps the actor without rebuilding the whole
        // extractor.
        let pc = extract(TestRequest::default().uri("/api/csp-report")).await;
        let pinned = ActorContext::system("handler:csp_report").with_workspace(1);
        let pc = pc.with_actor(pinned);
        assert_eq!(
            pc.actor.reference.as_deref(),
            Some("handler:csp_report"),
            "with_actor must replace the fallback reference"
        );
        assert_eq!(pc.actor.workspace_id, Some(1));
    }
}
