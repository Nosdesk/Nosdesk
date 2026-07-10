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
//! - **`hosted`**: parse the `Host` header, look up the workspace
//!   in two passes: (1) full-hostname match against
//!   `workspaces.custom_domain` for customers on Standard tier
//!   with their own domain; (2) subdomain match against
//!   `workspaces.slug` for the default `<slug>.nosdesk.app` shape.
//!   If neither matches, no context is attached — apex-domain
//!   routes take `Option<WorkspaceContext>` and handle that;
//!   workspace-required routes get a 404 via the extractor.
//!
//! Caching: a 60-second TTL DashMap keyed by either `host:<full
//! hostname>` or `slug:<subdomain>` so a busy tenant doesn't
//! hit Postgres on every request. The custom-domain PATCH
//! endpoint (M5 Task 5) invalidates the relevant entries so a
//! freshly-verified domain routes correctly within one request
//! rather than waiting up to 60s.

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    web, Error, HttpMessage,
};
use dashmap::DashMap;
use futures::future::LocalBoxFuture;
use once_cell::sync::Lazy;
use std::future::{ready, Ready};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;

use crate::db::Pool;
use crate::extractors::WorkspaceContext;
use crate::repository::workspaces as workspace_repo;

/// Process-wide TTL cache for workspace lookups. Keys are
/// `host:<full hostname>` (custom-domain matches) or
/// `slug:<subdomain>` (default-subdomain matches). Negative-cache
/// entries (lookups that resolved to None) are NOT stored — we'd
/// rather pay the DB roundtrip than serve "unknown workspace" from
/// a cached miss after the operator just provisioned the slug.
struct CachedContext {
    ctx: WorkspaceContext,
    expires: Instant,
}

static WORKSPACE_CACHE: Lazy<DashMap<String, CachedContext>> = Lazy::new(DashMap::new);
const CACHE_TTL: Duration = Duration::from_secs(60);

fn cache_get(key: &str) -> Option<WorkspaceContext> {
    let entry = WORKSPACE_CACHE.get(key)?;
    if entry.expires > Instant::now() {
        Some(entry.ctx.clone())
    } else {
        None
    }
}

fn cache_put(key: String, ctx: WorkspaceContext) {
    WORKSPACE_CACHE.insert(
        key,
        CachedContext {
            ctx,
            expires: Instant::now() + CACHE_TTL,
        },
    );
}

/// Drop a cache entry. Called by the custom-domain PATCH handler
/// (M5 Task 5) so a freshly-set or cleared hostname routes
/// correctly on the next request. The pattern can be either
/// `host:<hostname>` or `slug:<slug>`; cleared mappings call this
/// with the previous hostname so the stale entry doesn't linger.
pub fn invalidate_cache_key(key: &str) {
    WORKSPACE_CACHE.remove(key);
}

/// Request header carrying the agent app's selected workspace **slug**
/// (Model C), as it appears in the single-origin URL (`/acme/...`). In
/// selection mode the auth gate resolves this to a workspace and
/// membership-gates it, replacing Host-derived resolution for the agent
/// surface. The customer portal stays Host-derived and ignores this header.
pub const WORKSPACE_SELECTION_HEADER: &str = "X-Nosdesk-Workspace";

/// Whether selection-based workspace resolution is enabled.
///
/// True only when running `hosted` AND `NOSDESK_WORKSPACE_SELECTION`
/// is truthy. Read fresh from the environment (not memoised) so it is
/// operationally toggleable and the unit tests can flip it. Off by
/// default: self-hosted and current Host-derived hosted are unaffected.
pub fn selection_resolution_enabled() -> bool {
    DeploymentMode::from_env() == DeploymentMode::Hosted
        && matches!(
            std::env::var("NOSDESK_WORKSPACE_SELECTION")
                .ok()
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("1") | Some("true") | Some("yes") | Some("on")
        )
}

/// Resolve a selection-header workspace slug to a [`WorkspaceContext`].
/// `Ok(None)` for an unknown / soft-archived workspace; the caller maps
/// that to the same 403 a non-member gets so workspace existence does
/// not leak. The `workspaces` table is resolvable without a pinned GUC
/// (it is the resolution table), the same `find_by_slug` Host-derived
/// resolution uses above.
pub fn resolve_selected_context(
    conn: &mut crate::db::DbConnection,
    slug: &str,
) -> diesel::QueryResult<Option<WorkspaceContext>> {
    Ok(workspace_repo::find_by_slug(conn, slug)?.map(workspace_to_context))
}

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
        Self::from_value(std::env::var("NOSDESK_DEPLOYMENT_MODE").ok().as_deref())
    }

    /// Pure parse of a `NOSDESK_DEPLOYMENT_MODE` value, split out so config
    /// validation can resolve the mode through its injectable env getter
    /// instead of reading process env directly. `hosted` (case/whitespace
    /// insensitive) is the only value that selects hosted; anything else,
    /// including absent, is `SelfHosted`.
    pub fn from_value(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_lowercase).as_deref() {
            Some("hosted") => DeploymentMode::Hosted,
            _ => DeploymentMode::SelfHosted,
        }
    }

    /// Process-wide deployment mode, read from the environment once
    /// and cached for the lifetime of the process.
    ///
    /// This is the single source of truth: anywhere that branches on
    /// hosted vs. self-hosted should call this rather than re-parsing
    /// `NOSDESK_DEPLOYMENT_MODE`, so every branch agrees and the env
    /// var is read exactly once. (`from_env` stays public for the unit
    /// tests, which need to observe different env values within one
    /// process.)
    pub fn current() -> Self {
        static MODE: std::sync::OnceLock<DeploymentMode> = std::sync::OnceLock::new();
        *MODE.get_or_init(Self::from_env)
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
                    custom_domain: ws.custom_domain,
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
            let host_raw = req
                .headers()
                .get(actix_web::http::header::HOST)?
                .to_str()
                .ok()?;
            // Normalise: strip port, lowercase. Host headers are
            // case-insensitive per RFC 7230 §5.4 but we store
            // `custom_domain` lowercase, so normalise the lookup
            // key to match.
            let host_no_port = host_raw.split(':').next()?.to_ascii_lowercase();

            // --- Pass 1: custom-domain full-hostname match ---
            let host_key = format!("host:{host_no_port}");
            if let Some(ctx) = cache_get(&host_key) {
                return Some(ctx);
            }

            let pool = req.app_data::<web::Data<Pool>>()?;
            let mut conn = match pool.get() {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "workspace resolve: pool acquire failed");
                    return None;
                }
            };

            if let Ok(Some(ws)) = workspace_repo::find_by_custom_domain(&mut conn, &host_no_port) {
                let ctx = workspace_to_context(ws);
                cache_put(host_key, ctx.clone());
                return Some(ctx);
            }

            // --- Pass 2: subdomain match against slug ---
            // Scope slug resolution to the configured tenant base domain so a
            // host on a DIFFERENT base domain (the agent origin
            // `app.nosdesk.com` vs tenants on `*.nosdesk.app`) never resolves
            // to a tenant workspace. With no tenant domain configured (legacy /
            // single-base-domain hosted), fall back to the bare first-label
            // extraction.
            let slug = match crate::utils::tenant_origin::tenant_domain() {
                Some(td) => slug_under_tenant_domain(&host_no_port, &td)?,
                None => subdomain_from_host(&host_no_port)?,
            };
            let slug_key = format!("slug:{slug}");
            if let Some(ctx) = cache_get(&slug_key) {
                return Some(ctx);
            }
            let ws = workspace_repo::find_by_slug(&mut conn, slug)
                .ok()
                .flatten()?;
            let ctx = workspace_to_context(ws);
            cache_put(slug_key, ctx.clone());
            Some(ctx)
        }
    }
}

fn workspace_to_context(ws: crate::models::Workspace) -> WorkspaceContext {
    WorkspaceContext {
        workspace_id: ws.id,
        workspace_uuid: ws.uuid,
        slug: ws.slug,
        name: ws.name,
        custom_domain: ws.custom_domain,
        organisation_id: ws.organisation_id,
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

/// Extract the tenant slug from a host that sits DIRECTLY under the configured
/// tenant base domain: `acme.nosdesk.app` with tenant domain `nosdesk.app` ->
/// `Some("acme")`.
///
/// Returns `None` when the host is not exactly `<label>.<tenant_domain>`: a
/// different base domain (the agent origin `app.nosdesk.com`), the apex domain
/// itself, or a multi-level subdomain (`x.acme.nosdesk.app`). Scoping slug
/// resolution to the tenant domain is what keeps the agent origin (served on a
/// different base domain) from ever resolving to a tenant workspace, which is
/// the origin boundary the surface model relies on. `host` is expected already
/// port-stripped and lowercased.
fn slug_under_tenant_domain<'a>(host: &'a str, tenant_domain: &str) -> Option<&'a str> {
    let label = host.strip_suffix(tenant_domain)?.strip_suffix('.')?;
    if label.is_empty() || label.contains('.') {
        return None;
    }
    Some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests that mutate the process-global
    /// `NOSDESK_DEPLOYMENT_MODE` / `NOSDESK_WORKSPACE_SELECTION` env vars.
    /// `std::env` is process-wide and cargo runs tests in parallel threads, so
    /// without this their set -> read -> restore sequences interleave and read
    /// each other's values (the flaky `left: SelfHosted, right: Hosted`).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Acquire the env lock, recovering from poisoning so one failing test
    /// doesn't cascade-fail every other env-dependent test.
    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
    fn tenant_slug_extracted_under_tenant_domain() {
        assert_eq!(
            slug_under_tenant_domain("acme.nosdesk.app", "nosdesk.app"),
            Some("acme")
        );
    }

    #[test]
    fn tenant_slug_none_on_different_base_domain() {
        // The agent origin lives on a DIFFERENT base domain; it must never
        // resolve to a tenant, no matter what slugs exist.
        assert_eq!(
            slug_under_tenant_domain("app.nosdesk.com", "nosdesk.app"),
            None
        );
        assert_eq!(
            slug_under_tenant_domain("nosdesk-dev.fly.dev", "nosdesk.app"),
            None
        );
    }

    #[test]
    fn tenant_slug_none_for_apex_and_multilevel() {
        // The tenant apex itself has no slug label.
        assert_eq!(slug_under_tenant_domain("nosdesk.app", "nosdesk.app"), None);
        // Multi-level subdomains are not provisioned and must not resolve.
        assert_eq!(
            slug_under_tenant_domain("x.acme.nosdesk.app", "nosdesk.app"),
            None
        );
        // A host that merely ends with the domain string but isn't under it.
        assert_eq!(
            slug_under_tenant_domain("evilnosdesk.app", "nosdesk.app"),
            None
        );
    }

    #[test]
    fn deployment_mode_defaults_to_self_hosted() {
        let _env = lock_env();
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
        let _env = lock_env();
        let prev = std::env::var("NOSDESK_DEPLOYMENT_MODE").ok();
        std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
        assert_eq!(DeploymentMode::from_env(), DeploymentMode::Hosted);
        if let Some(v) = prev {
            std::env::set_var("NOSDESK_DEPLOYMENT_MODE", v);
        } else {
            std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
        }
    }

    /// Snapshot + restore both env vars selection resolution reads, run `body`
    /// with them set to the given values. std::env is process-wide, so tests
    /// that touch it must put it back.
    fn with_selection_env(mode: Option<&str>, flag: Option<&str>, body: impl FnOnce()) {
        let _env = lock_env();
        let prev_mode = std::env::var("NOSDESK_DEPLOYMENT_MODE").ok();
        let prev_flag = std::env::var("NOSDESK_WORKSPACE_SELECTION").ok();
        match mode {
            Some(v) => std::env::set_var("NOSDESK_DEPLOYMENT_MODE", v),
            None => std::env::remove_var("NOSDESK_DEPLOYMENT_MODE"),
        }
        match flag {
            Some(v) => std::env::set_var("NOSDESK_WORKSPACE_SELECTION", v),
            None => std::env::remove_var("NOSDESK_WORKSPACE_SELECTION"),
        }
        body();
        match prev_mode {
            Some(v) => std::env::set_var("NOSDESK_DEPLOYMENT_MODE", v),
            None => std::env::remove_var("NOSDESK_DEPLOYMENT_MODE"),
        }
        match prev_flag {
            Some(v) => std::env::set_var("NOSDESK_WORKSPACE_SELECTION", v),
            None => std::env::remove_var("NOSDESK_WORKSPACE_SELECTION"),
        }
    }

    #[test]
    fn selection_off_by_default_and_requires_both_hosted_and_flag() {
        // Default: neither hosted nor flag.
        with_selection_env(None, None, || {
            assert!(!selection_resolution_enabled());
        });
        // Hosted but flag absent: still off.
        with_selection_env(Some("hosted"), None, || {
            assert!(!selection_resolution_enabled());
        });
        // Flag set but self-hosted: off (single-tenant ignores selection).
        with_selection_env(Some("self_hosted"), Some("1"), || {
            assert!(!selection_resolution_enabled());
        });
        // Both present: on.
        with_selection_env(Some("hosted"), Some("1"), || {
            assert!(selection_resolution_enabled());
        });
        // Truthy spellings accepted; junk is not.
        with_selection_env(Some("hosted"), Some("true"), || {
            assert!(selection_resolution_enabled());
        });
        with_selection_env(Some("hosted"), Some("maybe"), || {
            assert!(!selection_resolution_enabled());
        });
    }
}
