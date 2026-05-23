//! Workspace-context resolution middleware.
//!
//! Sits in the actix middleware stack (after CSRF, before
//! auth) and resolves the workspace for the current request,
//! stashing it into request extensions for the
//! `WorkspaceContext` extractor to read.
//!
//! Two deployment modes, driven by the `NOSDESK_DEPLOYMENT_MODE`
//! env var:
//!
//! - **`self_hosted`** (default): the single bootstrap
//!   workspace (id=1, slug=`default`) is loaded once at
//!   middleware construction and attached to every request.
//!   No per-request DB lookup. This matches Phase 1's bootstrap
//!   and lets existing single-tenant deployments upgrade
//!   without any operational change.
//!
//! - **`hosted`**: parse the `Host` header, extract the
//!   subdomain segment, look the workspace up by slug. If the
//!   subdomain is empty (apex domain) or doesn't resolve to a
//!   workspace, no context is attached — apex-domain routes
//!   take `Option<WorkspaceContext>` and handle that case;
//!   workspace-required routes get a 404 via the extractor.
//!
//! 2a is the skeleton: middleware runs, context lands in
//! extensions, but no handler reads it yet. Behaviour is
//! preserved end-to-end. 2c-d wire handlers + repos.
//!
//! Caching: per-request DB lookups on every hosted request
//! are wasteful but tractable for 2a. 2e introduces a moka
//! cache keyed by slug with a short TTL.

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    web, Error, HttpMessage,
};
use futures::future::LocalBoxFuture;
use std::future::{ready, Ready};
use std::sync::Arc;
use tracing::warn;

use crate::db::Pool;
use crate::extractors::WorkspaceContext;
use crate::repository::workspaces as workspace_repo;

/// Deployment topology. Drives whether workspace context comes
/// from a process-wide bootstrap (self-hosted) or per-request
/// subdomain resolution (hosted SaaS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    SelfHosted,
    Hosted,
}

impl DeploymentMode {
    /// Read from the `NOSDESK_DEPLOYMENT_MODE` env var. Defaults
    /// to `SelfHosted` to preserve behaviour for existing
    /// installs that don't set the variable.
    pub fn from_env() -> Self {
        match std::env::var("NOSDESK_DEPLOYMENT_MODE")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("hosted") => DeploymentMode::Hosted,
            _ => DeploymentMode::SelfHosted,
        }
    }
}

/// Configuration produced once at server start and shared
/// across all per-request middleware invocations.
#[derive(Clone)]
pub struct WorkspaceContextConfig {
    pub mode: DeploymentMode,
    /// For self-hosted mode, the bootstrap workspace loaded at
    /// startup. Shared across all requests via Arc so we
    /// don't re-query.
    pub bootstrap: Option<Arc<WorkspaceContext>>,
}

impl WorkspaceContextConfig {
    /// Initialise from env + the database. In self-hosted mode
    /// this loads the bootstrap workspace once; in hosted mode
    /// no bootstrap is loaded (subdomain resolution per request).
    /// Returns an error only if self-hosted bootstrap fails —
    /// hosted mode initialises lazily.
    pub fn initialise(pool: &Pool) -> Result<Self, String> {
        let mode = DeploymentMode::from_env();
        let bootstrap = match mode {
            DeploymentMode::SelfHosted => {
                let mut conn = pool
                    .get()
                    .map_err(|e| format!("workspace bootstrap: pool acquire failed: {e}"))?;
                let ws = workspace_repo::find_by_id(&mut conn, 1)
                    .map_err(|e| format!("workspace bootstrap: query failed: {e}"))?
                    .ok_or_else(|| {
                        "workspace bootstrap: id=1 not found. Phase 1 \
                         migration must run before the server starts."
                            .to_string()
                    })?;
                Some(Arc::new(WorkspaceContext {
                    workspace_id: ws.id,
                    workspace_uuid: ws.uuid,
                    slug: ws.slug,
                    name: ws.name,
                    organisation_id: ws.organisation_id,
                }))
            }
            DeploymentMode::Hosted => None,
        };
        Ok(Self { mode, bootstrap })
    }
}

/// The middleware constructor. Built once at server start and
/// applied to every request via `.wrap()`.
pub struct WorkspaceContextMiddleware {
    config: Arc<WorkspaceContextConfig>,
}

impl WorkspaceContextMiddleware {
    pub fn new(config: WorkspaceContextConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for WorkspaceContextMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = WorkspaceContextService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(WorkspaceContextService {
            service: Arc::new(service),
            config: self.config.clone(),
        }))
    }
}

pub struct WorkspaceContextService<S> {
    service: Arc<S>,
    config: Arc<WorkspaceContextConfig>,
}

impl<S, B> Service<ServiceRequest> for WorkspaceContextService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let config = self.config.clone();

        Box::pin(async move {
            let ctx = resolve_context(&req, &config).await;
            if let Some(ctx) = ctx {
                req.extensions_mut().insert(ctx);
            }
            service.call(req).await
        })
    }
}

/// Resolve the workspace for this request.
///
/// Self-hosted: returns the bootstrap workspace verbatim.
/// Hosted: parses the Host header, extracts the subdomain, and
/// looks up the workspace by slug. Returns `None` if no
/// workspace matches — apex-domain routes and unknown
/// subdomains both end up here.
async fn resolve_context(
    req: &ServiceRequest,
    config: &WorkspaceContextConfig,
) -> Option<WorkspaceContext> {
    match config.mode {
        DeploymentMode::SelfHosted => config.bootstrap.as_deref().cloned(),
        DeploymentMode::Hosted => {
            let host = req
                .headers()
                .get(actix_web::http::header::HOST)?
                .to_str()
                .ok()?;
            let slug = subdomain_from_host(host)?;
            let pool = req.app_data::<web::Data<Pool>>()?;
            let mut conn = match pool.get() {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "workspace resolve: pool acquire failed");
                    return None;
                }
            };
            let ws = workspace_repo::find_by_slug(&mut conn, slug)
                .ok()
                .flatten()?;
            Some(WorkspaceContext {
                workspace_id: ws.id,
                workspace_uuid: ws.uuid,
                slug: ws.slug,
                name: ws.name,
                organisation_id: ws.organisation_id,
            })
        }
    }
}

/// Extract the subdomain label from a Host header value.
///
/// `acme.nosdesk.com` -> `Some("acme")`. `nosdesk.com` -> `None`
/// (apex). `localhost:8080` -> `None` (no subdomain to extract).
/// Port suffixes are stripped. Returns `None` for any host with
/// fewer than three labels — the apex domain plus its subdomain
/// is exactly three (foo + nosdesk + com).
///
/// Reserved subdomains (`www`, `api`, `app`) should be filtered
/// out at this layer too eventually; for now they fall through
/// to the slug lookup and naturally return `None` because no
/// workspace will have those slugs (the slug-format CHECK
/// allows them today; a reserved-list CHECK lands in Phase 5
/// alongside subdomain routing).
fn subdomain_from_host(host: &str) -> Option<&str> {
    let host_no_port = host.split(':').next()?;
    let labels: Vec<&str> = host_no_port.split('.').collect();
    if labels.len() < 3 {
        return None;
    }
    Some(labels[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomain_extracts_first_label() {
        assert_eq!(subdomain_from_host("acme.nosdesk.com"), Some("acme"));
        assert_eq!(subdomain_from_host("acme.nosdesk.com:8080"), Some("acme"));
    }

    #[test]
    fn subdomain_apex_returns_none() {
        assert_eq!(subdomain_from_host("nosdesk.com"), None);
        assert_eq!(subdomain_from_host("nosdesk.com:443"), None);
    }

    #[test]
    fn subdomain_localhost_returns_none() {
        assert_eq!(subdomain_from_host("localhost"), None);
        assert_eq!(subdomain_from_host("localhost:8080"), None);
    }

    #[test]
    fn deployment_mode_defaults_to_self_hosted() {
        // Snapshot + restore — std::env mutation is process-
        // wide and tests run in the same process, so we have
        // to put the variable back.
        let prev = std::env::var("NOSDESK_DEPLOYMENT_MODE").ok();
        std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
        assert_eq!(DeploymentMode::from_env(), DeploymentMode::SelfHosted);
        if let Some(v) = prev {
            std::env::set_var("NOSDESK_DEPLOYMENT_MODE", v);
        }
    }

    #[test]
    fn deployment_mode_hosted_recognised() {
        let prev = std::env::var("NOSDESK_DEPLOYMENT_MODE").ok();
        std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
        assert_eq!(DeploymentMode::from_env(), DeploymentMode::Hosted);
        if let Some(v) = prev {
            std::env::set_var("NOSDESK_DEPLOYMENT_MODE", v);
        } else {
            std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
        }
    }
}
