//! Application state construction (the composition root's state phase).
//!
//! `build_state` builds every `web::Data` the App factory registers, spawns the
//! state-bound background listeners (sync-outbox, email-queue, search
//! replicator, registry sync, channel supervisor) and the periodic scheduler,
//! and returns them bundled as [`AppState`]. Extracted from main() so the
//! composition root stays thin.

use std::sync::Arc;
use std::time::Duration;

use actix_cors::Cors;
use actix_files::Files;
use actix_limitation::{Limiter, RateLimiter};
use actix_web::dev::{fn_service, ServiceRequest, ServiceResponse};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use tracing::{error, info, warn};
use tracing_actix_web::TracingLogger;

use crate::config::Config;
use crate::db::Pool;
use crate::utils::redis_yjs_cache::create_redis_cache;
use crate::utils::storage::{create_storage, get_storage_config};
use crate::workers;

/// Every piece of shared state the App factory registers as `app_data`, plus
/// the scheduler shutdown token the composition root wires into graceful
/// shutdown. `Clone` is cheap (every field is `web::Data` (Arc) or a
/// `CancellationToken`) and required because `HttpServer` clones the factory
/// closure, which captures this, once per worker.
#[derive(Clone)]
pub struct AppState {
    pub analytics_cache: web::Data<Option<Arc<crate::utils::analytics_cache::AnalyticsCache>>>,
    pub sse_state: web::Data<crate::handlers::sse::SseState>,
    pub notification_service: web::Data<crate::services::notifications::NotificationService>,
    pub outbound_resolver_data:
        web::Data<Arc<crate::services::outbound_email::OutboundEmailResolver>>,
    pub webhook_service: web::Data<crate::services::webhooks::WebhookService>,
    pub plugin_proxy_service: web::Data<crate::services::plugins::PluginProxyService>,
    pub registry_cache: web::Data<crate::services::plugins::registry::SharedCache>,
    pub search_service: web::Data<Arc<crate::services::search::SearchService>>,
    pub yjs_app_state: web::Data<crate::handlers::collaboration::YjsAppState>,
    pub system_state: web::Data<crate::handlers::system::SystemState>,
    pub public_limiter_data: web::Data<Limiter>,
    pub auth_limiter_data: web::Data<Limiter>,
    pub frontend_logs_limiter_data: web::Data<Limiter>,
    pub storage_data: web::Data<Arc<dyn crate::utils::storage::Storage>>,
    pub inbound_s3_data: web::Data<Option<crate::services::inbound_email::s3_fetch::InboundS3>>,
    pub channel_control_data: web::Data<crate::services::channels::supervisor::ChannelControl>,
    pub scheduler_status_data: web::Data<crate::services::scheduler::StatusRegistry>,
    pub scheduler_shutdown: tokio_util::sync::CancellationToken,
}

/// Build all shared state, spawn the state-bound background tasks + scheduler,
/// and bundle the result. Fatal (returns `Err`) only where the original boot
/// was: the Yjs Redis cache and the search index.
pub fn build_state(
    config: &Config,
    pool: Pool,
    public_limiter: Limiter,
    auth_limiter: Limiter,
    frontend_logs_limiter: Limiter,
) -> Result<(AppState, Vec<tokio::task::JoinHandle<()>>), std::io::Error> {
    // Owned copies so the lifted body reads these by their original names.
    let redis_url = config.redis_url.clone();
    let frontend_url = config.frontend_url.clone();
    let host = config.host.clone();
    let port = config.port;
    let environment = config.environment.clone();

    // One shutdown token shared by every background task (the state-bound
    // listeners AND the periodic scheduler) so a single cancel stops them all;
    // their JoinHandles are collected so the composition root can await them.
    let scheduler_shutdown = tokio_util::sync::CancellationToken::new();
    let mut background_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    // Yjs document cache (survives backend restarts) shares the single
    // Redis URL resolved above. Used directly — no scheme rewrite — so a
    // TLS managed Redis (`rediss://`) is honoured rather than silently
    // falling back to localhost.
    let yjs_redis_url = redis_url.clone();

    let redis_cache = match create_redis_cache(&yjs_redis_url) {
        Ok(cache) => {
            info!(url = %yjs_redis_url, "Redis cache initialized for Yjs documents");
            cache
        }
        Err(e) => {
            error!(error = ?e, "Failed to initialize Redis cache for Yjs");
            error!("CRITICAL: Yjs documents will NOT persist across server restarts");
            error!("Please ensure Redis is running and REDIS_URL is configured correctly");
            return Err(std::io::Error::other(format!(
                "Redis initialization failed: {e:?}"
            )));
        }
    };

    // Short-TTL cache for dashboard analytics payloads. Best-effort:
    // a build failure here is non-fatal (the handlers fall through to
    // the live query), so unlike the Yjs cache it doesn't abort boot.
    // Always registered as `Data<Option<..>>` so the handler extractor
    // is present even when the cache itself couldn't be built.
    let analytics_cache: web::Data<
        Option<std::sync::Arc<crate::utils::analytics_cache::AnalyticsCache>>,
    > = web::Data::new(
        match crate::utils::analytics_cache::AnalyticsCache::new(&redis_url) {
            Ok(c) => {
                info!("Analytics cache initialized");
                Some(std::sync::Arc::new(c))
            }
            Err(e) => {
                warn!(error = ?e, "Analytics cache unavailable; dashboard queries will not be cached");
                None
            }
        },
    );

    // Initialize SSE state for real-time ticket updates (must be created before YjsAppState)
    let sse_state = web::Data::new(crate::handlers::sse::SseState::new());

    // Spawn the sync-actions outbox listener. Holds a dedicated
    // `tokio_postgres` LISTEN connection on `sync_actions_new` and
    // broadcasts every committed sync_actions row to SSE
    // subscribers. The DB trigger
    // `sync_actions_notify_trigger` fires the NOTIFY post-commit,
    // so any code path that emits a sync_actions row (HTTP push,
    // channel pipeline, background jobs, future write sites) auto-
    // broadcasts without per-call-site plumbing. See
    // `services/sync_outbox.rs` for the full lifecycle / recovery
    // semantics.
    if let Ok(database_url) = std::env::var("DATABASE_URL") {
        background_tasks.push(crate::services::sync_outbox::spawn(
            database_url,
            pool.clone(),
            sse_state.clone().into_inner(),
            scheduler_shutdown.clone(),
        ));
    } else {
        warn!("DATABASE_URL not set; sync outbox listener not spawned (SSE will not deliver real-time updates)");
    }

    // Build the email service once — it's reused by the notification
    // service and by the channels dispatcher for outbound ticket
    // replies. `None` means SMTP isn't configured; both callers treat
    // that as "email disabled" rather than a fatal error.
    let email_service: Option<std::sync::Arc<crate::utils::email::EmailService>> =
        match crate::utils::email::EmailService::from_env() {
            Ok(svc) => Some(std::sync::Arc::new(svc)),
            Err(e) => {
                info!(error = ?e, "Email service not configured - email notifications and channel outbound disabled");
                None
            }
        };

    // Per-workspace outbound resolver. The env `EmailService` is the
    // fallback identity, so single-tenant self-host is unchanged; the queue
    // worker resolves each row's identity (the row's workspace identity, or
    // the instance identity for auth mail) through this at send time.
    let outbound_resolver =
        std::sync::Arc::new(crate::services::outbound_email::OutboundEmailResolver::new(
            pool.clone(),
            email_service.clone(),
        ));

    // Spawn the outbound email queue listener (Item J Pass 1). Holds a
    // dedicated tokio_postgres LISTEN connection on
    // `outbound_emails_new`; on each NOTIFY, drives the worker to claim
    // a batch via SKIP LOCKED and dispatch each row through SMTP. A 30s
    // safety-net tick covers the case where a notification was missed
    // (reconnect window, etc.). The lease sweeper job (registered with
    // the periodic scheduler below) recovers rows whose worker died
    // mid-send.
    //
    // Spawned whenever DATABASE_URL is set: the resolver routes each row to
    // its workspace identity or the env fallback, and a row with no
    // configured identity is released (not failed), so the worker is safe to
    // run even before any SMTP identity exists.
    if let Ok(database_url) = std::env::var("DATABASE_URL") {
        background_tasks.push(crate::services::email_queue::spawn(
            database_url,
            pool.clone(),
            outbound_resolver.clone(),
            scheduler_shutdown.clone(),
        ));
    } else {
        warn!("DATABASE_URL not set; outbound email queue listener not spawned");
    }

    // Initialize notification service for in-app and email notifications
    let notification_service = {
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::RwLock as TokioRwLock;

        // Shared cache for notification type ID lookups (used by both service and email channel)
        let type_id_cache = Arc::new(TokioRwLock::new(HashMap::<String, i32>::new()));

        let service = crate::services::notifications::NotificationService::new(
            pool.clone(),
            type_id_cache.clone(),
        );

        // Register in-app channel. Delivery is the notification sync emit
        // in persist_notification; this registration keeps the in-app
        // preference + rate limiting in channel selection.
        let in_app_channel =
            Arc::new(crate::services::notifications::channels::in_app::InAppChannel::new());
        service.register_channel(in_app_channel);

        // Register email channel if email service is configured.
        // App name comes from the workspace branding (site_settings) at
        // send time, not env, so admin renames take effect without restart.
        if let Some(email_svc) = email_service.clone() {
            let email_channel = Arc::new(
                crate::services::notifications::channels::email::EmailChannel::new(
                    email_svc,
                    pool.clone(),
                    frontend_url.clone(),
                    type_id_cache,
                ),
            );
            service.register_channel(email_channel);
        }

        web::Data::new(service)
    };

    // Inject the outbound resolver so the comment handler can gate the
    // channel relay on whether outbound is configured at all (the worker
    // resolves the per-workspace identity at send time).
    let outbound_resolver_data = web::Data::new(outbound_resolver.clone());

    // Initialize webhook service for external integrations
    let webhook_service =
        web::Data::new(crate::services::webhooks::WebhookService::new(pool.clone()));

    // Initialize plugin proxy service for external requests
    let plugin_proxy_service = web::Data::new(crate::services::plugins::PluginProxyService::new());

    // Plugin registry cache. `None` at boot — populated by the
    // first successful sync. The admin UI reads this for the
    // browse-registry view.
    let registry_cache = web::Data::new(crate::services::plugins::registry::new_cache());

    // Kick off the background registry sync loop. Disabled when
    // `NOSDESK_REGISTRY_URL=""` (air-gapped deployments); logged and
    // skipped with no further side effects. Failures are warn-and-
    // continue — the background task retries next cycle rather
    // than unwinding.
    if let Some(registry_url) = crate::services::plugins::registry::configured_url() {
        background_tasks.push(crate::services::plugins::registry::spawn_sync_loop(
            pool.clone(),
            registry_url,
            registry_cache.as_ref().clone(),
            scheduler_shutdown.clone(),
        ));
    } else {
        info!("NOSDESK_REGISTRY_URL is empty; registry sync disabled");
    }

    // Initialize search service for full-text search
    let search_service = {
        use std::path::Path;
        use std::sync::Arc;

        let search_index_path =
            std::env::var("SEARCH_INDEX_PATH").unwrap_or_else(|_| "data/search_index".to_string());

        match crate::services::search::SearchService::new(Path::new(&search_index_path), &pool) {
            Ok(service) => {
                info!(path = %search_index_path, "Search service initialized");
                web::Data::new(Arc::new(service))
            }
            Err(e) => {
                error!(error = ?e, "Failed to initialize search service");
                error!("Search functionality will be unavailable");
                // Return a placeholder - search endpoints will fail gracefully
                // In a real deployment, you might want to fail startup here
                return Err(std::io::Error::other(format!(
                    "Search service initialization failed: {e}"
                )));
            }
        }
    };

    // Search-index replicator (S1). On >1 machine the Tantivy index is
    // per-machine local disk, so an entity indexed on one machine is
    // invisible to a search on another. When enabled, each machine tails
    // the `sync_actions` change stream and projects structured changes into
    // its own index. Off by default: a single machine (self-hosted, or the
    // single-machine first deploy) is served fully by the write-time
    // observer, so this adds nothing there. Flip it on in the hosted config
    // when running more than one machine.
    if std::env::var("NOSDESK_SEARCH_REPLICATION")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        if let Ok(database_url) = std::env::var("DATABASE_URL") {
            background_tasks.push(crate::services::search_replicator::spawn(
                database_url,
                pool.clone(),
                search_service.get_ref().clone(),
                scheduler_shutdown.clone(),
            ));
            info!("Search replication enabled (NOSDESK_SEARCH_REPLICATION=true)");
        } else {
            warn!(
                "NOSDESK_SEARCH_REPLICATION set but DATABASE_URL missing; replicator not spawned"
            );
        }
    }

    // Per-document affinity routing for multi-instance collab (Phase 2).
    // Default is single-instance: no ownership manager, routing inert,
    // behaviour identical to before. `NOSDESK_COLLAB_ROUTING` opts into
    // `fly-replay` (fly) or `direct-address` (portable / self-host).
    // `build` returns None (and we pin the mode to Single) on any setup
    // error, so a misconfig degrades rather than fails.
    use crate::services::collab_ownership::CollabRoutingMode;
    let requested_mode = CollabRoutingMode::from_env_value(
        &std::env::var("NOSDESK_COLLAB_ROUTING").unwrap_or_else(|_| "single".into()),
    );
    let collab_ownership = crate::services::collab_ownership::build(&yjs_redis_url, requested_mode);
    let collab_routing_mode = if collab_ownership.is_some() {
        requested_mode
    } else {
        CollabRoutingMode::Single
    };

    // Initialize WebSocket app state for collaborative editing (includes SseState for broadcasting)
    let yjs_app_state = web::Data::new(crate::handlers::collaboration::YjsAppState::new(
        web::Data::new(pool.clone()),
        redis_cache,
        sse_state.clone(),
        search_service.get_ref().clone(),
        collab_ownership,
        collab_routing_mode,
    ));

    // Initialize system state for tracking uptime
    let system_state = web::Data::new(crate::handlers::system::SystemState::new());

    // Share the limiters across all app instances
    let public_limiter_data = web::Data::new(public_limiter);
    let auth_limiter_data = web::Data::new(auth_limiter);
    let frontend_logs_limiter_data = web::Data::new(frontend_logs_limiter);

    if host == "0.0.0.0" {
        warn!("Server bound to all interfaces (0.0.0.0)");
    }

    // Initialize storage backend
    let storage_config = get_storage_config();
    let storage = create_storage(storage_config);
    let storage_data = web::Data::new(storage.clone());
    // Install the base storage process-wide so non-handler code paths
    // (avatar/banner image processing, the thumbnail backfill sweep, the
    // MS Graph importer) route file I/O through the same Local/S3
    // abstraction instead of writing straight to the local filesystem.
    crate::utils::storage::set_process_storage(storage.clone());

    // Inbound-email S3 reader (hosted forwarding path). `None` on self-host or
    // when `NOSDESK_INBOUND_S3_BUCKET` is unset; a bucket configured without
    // SES credentials is a hard misconfig we surface loudly but don't crash on.
    let inbound_s3 = match crate::services::inbound_email::s3_fetch::InboundS3::from_env() {
        Ok(reader) => reader,
        Err(e) => {
            tracing::error!("inbound-email S3 disabled: {e}");
            None
        }
    };
    let inbound_s3_data = web::Data::new(inbound_s3);

    info!(host = %host, port = %port, environment = %environment, "Server starting");

    // Boot the channel-worker supervisor. The supervisor owns a
    // `ChannelRegistry` and is the only task that mutates it; handlers
    // drive start/stop via an mpsc command channel exposed as
    // `web::Data<ChannelControl>`. This is the pattern Tokio docs and
    // industry tools (Vector) converge on — see
    // `crate::services::channels::supervisor` for the full rationale.
    let channel_control_data = {
        use crate::services::channels::registry::RegistryDeps;
        use crate::services::channels::supervisor;
        let deps = RegistryDeps {
            pool: pool.clone(),
            resolver: Some(outbound_resolver.clone()),
            sse: Some(sse_state.clone()),
            search: Some(search_service.get_ref().clone()),
            storage: Some(storage.clone()),
            http: None,
        };
        // `spawn` hydrates the registry from the DB before accepting
        // commands, so existing enabled channels are polling by the
        // time this line returns. The join handle is dropped — the
        // supervisor lives for the process lifetime, and the mpsc
        // senders held by `web::Data` keep it alive.
        let (control, join) = supervisor::spawn(deps, scheduler_shutdown.clone());
        background_tasks.push(join);
        web::Data::new(control)
    };

    // Boot the periodic-task scheduler on the shared shutdown token (also reused
    // as the collab shutdown token) so a single SIGTERM cancels the scheduler,
    // the listeners, and the collab background work together.
    let scheduler_status = workers::spawn_scheduled_jobs(
        pool.clone(),
        search_service.clone(),
        notification_service.clone(),
        scheduler_shutdown.clone(),
    );
    let scheduler_status_data = web::Data::new(scheduler_status);

    Ok((
        AppState {
            analytics_cache,
            sse_state,
            notification_service,
            outbound_resolver_data,
            webhook_service,
            plugin_proxy_service,
            registry_cache,
            search_service,
            yjs_app_state,
            system_state,
            public_limiter_data,
            auth_limiter_data,
            frontend_logs_limiter_data,
            storage_data,
            inbound_s3_data,
            channel_control_data,
            scheduler_status_data,
            scheduler_shutdown,
        },
        background_tasks,
    ))
}

// ---- SPA shell serving (moved from main.rs; used by configure_app below) ----

/// Handle missing assets in development mode
/// When frontend rebuilds, old asset hashes become invalid - this helps developers.
/// Uses a single-attempt reload with cache-busting to avoid infinite loops.
fn handle_missing_asset(path: &str) -> HttpResponse {
    let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

    if environment != "production" {
        log::warn!(
            "Asset not found: {path} - Frontend may have been rebuilt. Try refreshing the page."
        );

        // For JS files, return code that triggers a single cache-busted reload.
        // Uses sessionStorage to track reload attempts and prevent infinite loops.
        if path.ends_with(".js") {
            return HttpResponse::Ok()
                .content_type("application/javascript")
                .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
                .body(r#"(function(){
  var key = '__nosdesk_reload_' + location.pathname;
  if (sessionStorage.getItem(key)) {
    sessionStorage.removeItem(key);
    console.error('[Nosdesk Dev] Asset still missing after reload. Frontend may still be building — try refreshing manually in a few seconds.');
  } else {
    sessionStorage.setItem(key, '1');
    console.warn('[Nosdesk Dev] Asset hash mismatch — frontend was rebuilt. Reloading...');
    location.replace(location.pathname + location.search);
  }
})();"#);
        }

        if path.ends_with(".css") {
            return HttpResponse::Ok()
                .content_type("text/css")
                .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
                .body("/* Asset hash mismatch - frontend was rebuilt */");
        }
    }

    HttpResponse::NotFound().finish()
}

/// The SPA shell file for a request's surface: the customer portal
/// (`portal.html`) on a hosted per-tenant origin (the workspace middleware
/// host-resolved a `WorkspaceContext` from `<slug>.nosdesk.app` or a verified
/// custom domain), the agent app (`index.html`) otherwise. Self-host always
/// serves the agent app, ignoring its ever-present bootstrap workspace.
fn spa_shell_path(
    mode: crate::middleware::DeploymentMode,
    host_resolved_workspace: bool,
) -> &'static str {
    if mode == crate::middleware::DeploymentMode::Hosted && host_resolved_workspace {
        "./public/portal.html"
    } else {
        "./public/index.html"
    }
}

/// Serve the SPA shell for all non-API routes (SPA routing)
/// This follows Actix best practices for SPA applications
async fn serve_spa(req: HttpRequest) -> HttpResponse {
    use actix_web::HttpMessage as _;

    // Check if this is a static asset request (has file extension and not HTML)
    let path = req.path();

    // If it's a hashed asset request (contains hash pattern), handle as missing asset.
    // Frontend assets live under `/static/` (Vite's `assetsDir`),
    // not `/assets/` (now a SPA route prefix).
    if path.starts_with("/static/") && path.contains('-') {
        return handle_missing_asset(path);
    }

    // If it's a static asset request, return 404 to let the Files service handle it
    if path.contains('.') && !path.ends_with(".html") {
        return HttpResponse::NotFound().finish();
    }

    // Pick the SPA shell by surface (see `spa_shell_path`). The portal origin
    // host-resolves to a `WorkspaceContext` in hosted mode; the agent origin
    // resolves to none; self-host always serves the agent app.
    let host_resolved_workspace = req
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .is_some();
    let shell = spa_shell_path(
        crate::middleware::DeploymentMode::current(),
        host_resolved_workspace,
    );

    // For all other routes (SPA routes), serve the chosen shell.
    // Use no-cache so browsers always check for updated versions after deployments
    match tokio::fs::read(shell).await {
        Ok(content) => {
            HttpResponse::Ok()
                .content_type("text/html; charset=utf-8")
                // no-cache: browser may cache but must revalidate with server before using
                // This ensures users get new frontend builds while still benefiting from 304s
                .insert_header(("Cache-Control", "no-cache"))
                .body(content)
        }
        Err(_) => {
            // Fallback if index.html doesn't exist
            let environment =
                std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
            if environment != "production" {
                HttpResponse::NotFound()
                    .content_type("text/html")
                    .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
                    .body(r#"<!DOCTYPE html>
<html>
<head>
    <title>Building...</title>
    <meta http-equiv="refresh" content="3">
</head>
<body style="margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#1a1a2e;font-family:system-ui,sans-serif;">
    <div style="text-align:center;color:#fff;">
        <div style="width:40px;height:40px;border:3px solid #333;border-top-color:#6366f1;border-radius:50%;margin:0 auto 16px;animation:spin 1s linear infinite;"></div>
        <p style="margin:0;font-size:16px;opacity:0.8;">Frontend is rebuilding...</p>
    </div>
    <style>@keyframes spin{to{transform:rotate(360deg)}}</style>
</body>
</html>"#)
            } else {
                HttpResponse::NotFound()
                    .content_type("text/plain")
                    .body("Frontend not found")
            }
        }
    }
}

/// Register all shared `app_data` and the full route tree (public routes, the
/// portal/auth/public/internal scopes, the authenticated `/api` scope, and the
/// SPA fallback) onto the App. The app-level wraps + CORS stay in main()'s
/// factory closure; everything a `ServiceConfig` can carry lives here.
pub fn configure_app(
    cfg: &mut web::ServiceConfig,
    state: &AppState,
    pool: &Pool,
    workspace_config: &crate::middleware::WorkspaceContextConfig,
    config: &Config,
) {
    let json_config = web::JsonConfig::default().limit(config.max_payload_size);
    let multipart_config = web::FormConfig::default().limit(config.max_payload_size);
    cfg.app_data(json_config)
        .app_data(multipart_config)
            .app_data(state.auth_limiter_data.clone())
            .app_data(web::Data::new(pool.clone()))
            .app_data(state.yjs_app_state.clone())
            .app_data(state.sse_state.clone())
            .app_data(state.system_state.clone())
            .app_data(state.storage_data.clone())
            .app_data(state.notification_service.clone())
            .app_data(state.outbound_resolver_data.clone())
            .app_data(state.channel_control_data.clone())
            .app_data(state.scheduler_status_data.clone())
            .app_data(state.webhook_service.clone())
            .app_data(state.plugin_proxy_service.clone())
            .app_data(state.registry_cache.clone())
            .app_data(state.search_service.clone())
            .app_data(state.analytics_cache.clone())
            .app_data(state.inbound_s3_data.clone())

            // === PUBLIC ROUTES (NO AUTHENTICATION REQUIRED) ===
            .route("/health", web::get().to(crate::handlers::health::liveness))
            .route("/readiness", web::get().to(crate::handlers::health::readiness))

            // Forwarded frontend console logs: gated inside the handler unless
            // debug build or `NOSDESK_ALLOW_FRONTEND_DEBUG_LOGS=1`. Rate limited
            // and JSON-capped separately from global config.
            .service(
                web::resource("/api/debug/frontend-logs")
                    .app_data(web::JsonConfig::default().limit(512 * 1024))
                    // Own bucket (felogs:{ip}); shadows the app-level limiter
                    // for this resource so a log flood can't drain the public
                    // / auth quotas that gate login and MFA.
                    .app_data(state.frontend_logs_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .route(web::post().to(crate::handlers::debug::receive_frontend_logs)),
            )

            // Public file serving - ONLY user avatars, banners, thumbs, and branding (no sensitive data)
            .route("/uploads/users/avatars/{filename:.*}", web::get().to(crate::handlers::serve_public_file))
            .route("/uploads/users/banners/{filename:.*}", web::get().to(crate::handlers::serve_public_file))
            .route("/uploads/users/thumbs/{filename:.*}", web::get().to(crate::handlers::serve_public_file))
            .route("/uploads/branding/{filename:.*}", web::get().to(crate::handlers::branding::serve_branding_file))

            // Public branding config (needed for favicon/logo before login)
            .route("/api/branding", web::get().to(crate::handlers::branding::get_public_branding))

            // Public instance config (routing topology) read at SPA startup
            .route("/api/config", web::get().to(crate::handlers::app_config::get_public_config))

            // Inbound-email webhook (hosted): AWS SNS POSTs here when SES
            // receives forwarded mail. Unauthenticated by necessity (SNS is
            // server-to-server); the handler verifies the SNS signature.
            .route("/api/inbound/email", web::post().to(crate::handlers::inbound_email::receive))

            // Public (unauthenticated) guest endpoints — feature flags checked per handler.
            // Tighter JSON payload limit here than the app-wide default: the only
            // write endpoint in this scope is /tickets with a 10KB description cap,
            // so 32KB is generous headroom without expanding the DoS surface.
            .service(
                web::scope("/api/public")
                    .app_data(web::JsonConfig::default().limit(32 * 1024))
                    .app_data(state.public_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .configure(crate::handlers::guest::config)
            )

            // Public WebSocket for collaboration (auth handled in WebSocket handler)
            .service(
                web::scope("/api/collaboration")
                    .configure(crate::handlers::collaboration::config)
            )

            // Public CSP violation report intake. Browsers POST
            // here without credentials when the page's CSP blocks
            // a subresource. Body is small (<8 KB in practice) but
            // we cap at 16 KB to absorb chunky `original-policy`
            // strings without becoming a DoS surface. Rate-limited
            // through the public limiter; reports are deduplicated
            // server-side by the handler so a misbehaving page
            // can't blow up the table either.
            .service(
                web::resource("/api/csp-report")
                    .app_data(web::PayloadConfig::new(16 * 1024))
                    .app_data(state.public_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .route(web::post().to(crate::handlers::csp_reports::report_violation))
            )

            // Authenticated, workspace-scoped tenant file serving. Its own
            // scope so the wrap stack matches the main /api scope: dual_auth
            // (cookie or Bearer + workspace-membership gate) registered last so
            // it runs first and puts Claims in extensions, then token_scope
            // enforces API-token scopes. These routes map to `Full`, so a
            // narrowed token 403s here instead of slipping past scope
            // enforcement (they used to sit outside any scope-wrapped tree).
            // The handlers add a per-ticket/asset visibility check via
            // TenantConn so a caller only reads files it can see in its own
            // workspace. Registered before the main /api scope so /api/files/*
            // resolves here; the notes route precedes the generic ticket route
            // so the `{filename:.*}` tail can't swallow it.
            .service(
                web::scope("/api/files")
                    .wrap(actix_web::middleware::from_fn(
                        crate::middleware::token_scope::token_scope_middleware,
                    ))
                    .wrap(actix_web::middleware::from_fn(crate::middleware::dual_auth_middleware))
                    .route("/tickets/{ticket_id}/notes/{filename:.*}", web::get().to(crate::handlers::serve_ticket_note_image))
                    .route("/tickets/{filename:.*}", web::get().to(crate::handlers::serve_ticket_file))
                    .route("/assets/{asset_id}/media/{filename:.*}", web::get().to(crate::handlers::asset_media::serve_asset_media_file))
                    .route("/temp/{filename:.*}", web::get().to(crate::handlers::serve_temp_file))
            )

            // SSE endpoints (with custom token-based auth)
            // Main event stream for all real-time updates (tickets, documentation, devices, etc.)
            .route("/api/events/stream", web::get().to(crate::handlers::sse::sse_events_stream))
            .route("/api/events/status", web::get().to(crate::handlers::sse::sse_status))

            // Customer portal sign-in (public by design). Unauthenticated;
            // the workspace is resolved from the portal origin by the app-wide
            // workspace-context middleware. Rate-limited like the auth scope to
            // bound magic-link sends.
            .service(
                web::scope("/api/portal/auth")
                    .wrap(RateLimiter::default())
                    .configure(crate::handlers::portal::auth_config),
            )
            // Authenticated customer portal API. Registered AFTER the public
            // `/api/portal/auth` scope so the sign-in routes match there first.
            // The portal session is ownership-scoped: every handler reads its
            // own tickets only, RLS-pinned to the origin's workspace.
            .service(
                web::scope("/api/portal")
                    .wrap(actix_web::middleware::from_fn(
                        crate::handlers::portal::portal_auth_middleware,
                    ))
                    .configure(crate::handlers::portal::config),
            )
            // Authentication routes (public by design)
            .service(
                web::scope("/api/auth")
                    // Brute-force defence: auth endpoints get the
                    // stricter public limit (shadows the app-level auth
                    // limiter for this scope). See security-audit-2026-06.
                    .app_data(state.public_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .service({
                        // Restore moved to `nosdesk-cli db restore` (AUD-005).
                        // The self-serve initial-admin route only exists in
                        // self-hosted mode; in hosted mode the control plane
                        // provisions admins, so /setup/admin is never mounted
                        // (a request to it 404s rather than 403s).
                        let setup = web::scope("/setup")
                            .route("/status", web::get().to(crate::handlers::check_setup_status));
                        match workspace_config.mode {
                            crate::middleware::DeploymentMode::SelfHosted => setup
                                .route("/admin", web::post().to(crate::handlers::setup_initial_admin)),
                            crate::middleware::DeploymentMode::Hosted => setup,
                        }
                    })
                    .configure(crate::handlers::auth::config)
            )

            // === INTERNAL PROVISIONING SURFACE (M5) ===
            // /api/internal/v1/* is reachable only by the control plane
            // (`~/dev/nosdesk-com`), which presents a short-lived EdDSA
            // JWT. `platform_auth_middleware` verifies it against
            // PLATFORM_PUBLIC_KEY / PLATFORM_ISSUER and 404s the surface
            // on self-hosted instances. No api_token / cookie auth runs
            // here (an EdDSA JWT isn't an `nsk_` token, so dual_auth would
            // reject it).
            //
            // Wrap order matters: actix runs the last-registered wrap
            // first, so platform_auth sits OUTSIDE idempotency and
            // authenticates before any idempotency-cache work. An
            // unauthenticated request is rejected before it can touch the
            // cache. The per-handler `PlatformAuth` extractor then reads
            // the verified marker (defense-in-depth, fails closed).
            .service(
                web::scope("/api/internal/v1")
                    .wrap(actix_web::middleware::from_fn(crate::middleware::idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(crate::extractors::platform_auth_middleware))
                    .configure(crate::handlers::internal_workspaces::config)
            )

            // === PROTECTED ROUTES (AUTHENTICATION REQUIRED) ===
            // Supports both cookie-based auth (browser) and Bearer token auth (API clients)
            .service(
                web::scope("/api")
                    // Throttle authenticated traffic (per-IP, 600/min via
                    // the auth limiter) so expensive endpoints
                    // (/search/rebuild, /sync/bootstrap, /admin/backup/export)
                    // can't be hammered unthrottled. Registered after the
                    // auth wrap so it runs first and rejects before the
                    // auth/DB work. SSE (/api/events/stream) and the
                    // collaboration WS are separate top-level services, so
                    // this does not throttle long-lived connections. See
                    // security-audit-2026-06.
                    .wrap(RateLimiter::default())
                    // Enforce API-token scopes. Registered before (so it
                    // runs after) dual_auth, which puts Claims in
                    // extensions. Cookie sessions and un-narrowed tokens
                    // carry `full` and short-circuit; platform tokens are
                    // exempt; a narrowed token must satisfy the route's
                    // required scope.
                    .wrap(actix_web::middleware::from_fn(
                        crate::middleware::token_scope::token_scope_middleware,
                    ))
                    .wrap(actix_web::middleware::from_fn(crate::middleware::dual_auth_middleware))

                    // Authentication Provider management (admin only) - simplified for environment-based config
                    .configure(crate::handlers::auth_providers::config)

                    // Email configuration (admin only) - environment-based config
                    .configure(crate::handlers::email::config)

                    // System information (admin only)
                    .configure(crate::handlers::system::config)

                    // Branding configuration + image upload (admin only)
                    .configure(crate::handlers::branding::config)

                    // Workspace lifecycle (admin only, Phase 4 W1).
                    // GET / POST list + create; per-id rename /
                    // archive / restore / hard-delete. Hard-delete
                    // requires ?confirm=<slug> matching the row.
                    .configure(crate::handlers::admin_workspaces::config)
                    .configure(crate::handlers::workspace_export::config)

                    // Guest access controls (admin only)
                    .configure(crate::handlers::guest_settings::config)

                    // Multi-channel ingestion (admin only). Phase-1 UI
                    // surfaces only the email_imap provider; backend
                    // is generic over channel rows.
                    .configure(crate::handlers::channels::config)

                    // Periodic-task scheduler status (read-only).
                    .configure(crate::handlers::scheduler::config)

                    // CSP violation reports — admins inspect what's
                    // being blocked under the live policy. Drives
                    // safe rollouts: tighten in report-only mode,
                    // observe here, then enforce.
                    .configure(crate::handlers::csp_reports::config)

                    // Audit log — admin-gated read of audit_log rows
                    // produced by the per-table triggers. See
                    // handlers/audit_log.rs for query shape and
                    // 2026-05-11-210000_attach_audit_tier1 for the
                    // tables that participate.
                    .configure(crate::handlers::audit::config)

                    // Outbound email queue — Item J Pass 1 admin
                    // surface. List rows + per-row actions (retry now,
                    // cancel) for operators to investigate why a
                    // notification didn't fire.
                    .configure(crate::handlers::email_queue::config)

                    // Consolidated dashboard stats. The frontend's
                    // widget registry derives an `include` set
                    // from the user's active widgets and passes
                    // it here so we only compute what's about to
                    // be displayed. Replaces three independent
                    // full-list ticket fetches per dashboard load.
                    .configure(crate::handlers::analytics::config)

                    // Canned responses — reads open to any authenticated
                    // user (composer picker); writes admin-only. The
                    // insertions log endpoint is open (any user can
                    // record their own picker use, workspace-local);
                    // the starter catalog is admin-only since it's
                    // only consumed by the admin create flow.
                    .configure(crate::handlers::canned_responses::config)

                    // Rules engine (Phase 1: manual rules + recent
                    // activity log). The `/state` and `/apply` literal
                    // sub-paths register before the wildcard `/{id}`
                    // sibling to avoid the actix route-shadowing
                    // gotcha; same pattern as `/canned-response-starters`
                    // above. `/apply` itself is wired in Wave 6.
                    // Starter catalog lives at a distinct path
                    // (`/admin/rule-starters`) rather than nested
                    // under `/rules/...` so the wildcard
                    // `/rules/{id}` GET can't absorb it. Same
                    // precaution and same shape as the canned-
                    // response-starters route above; the
                    // `project_actix_route_shadowing` memory note
                    // documents why ordering alone isn't enough.
                    .configure(crate::handlers::rules::config)


                    // Workflow states — read open to any authenticated user;
                    // admin writes (create, rename, recolor, reorder,
                    // promote default, archive) for the customisation UI.
                    // Sync engine — local-first protocol behind /api/sync.
                    // bootstrap streams a per-user NDJSON snapshot; delta
                    // pulls incremental changes from a sync_id cursor;
                    // push applies an array of optimistic transactions.
                    // Saved views (workspace / project / private)
                    .configure(crate::handlers::saved_views::config)

                    // Cycles. Project-scoped list + create live under
                    // /projects/{id}/cycles; per-cycle ops use the
                    // cycle uuid for stable bookmarkable URLs.
                    .configure(crate::handlers::cycles::config)

                    // SLA admin: policies + working calendars CRUD.
                    // Reads open to any authenticated user (the
                    // admin UI lists them); writes gate on admin
                    // inside the handler.
                    .configure(crate::handlers::sla::config)

                    .configure(crate::handlers::sync::config)

                    .configure(crate::handlers::workflow_states::config)

                    // Asset-kind registry. Admin-only CRUD over the
                    // runtime discriminator table that drives
                    // `assets.kind` validation.
                    .configure(crate::handlers::asset_kinds::config)

                    // Bulk CSV import — admin-only. The template
                    // route is declared before /{id} so the literal
                    // "template" path segment doesn't get matched
                    // as a job-uuid by the catch-all.
                    .configure(crate::handlers::imports::config)

                    // Feature flags — staged rollout machinery (Phase 0 of the
                    // projects-v2 architecture). Read endpoint open to any
                    // authenticated user; write endpoints admin-only.
                    .configure(crate::handlers::feature_flags::config)

                    // Backup and restore (admin only)
                    .configure(crate::handlers::backup::config)

                    // Microsoft Graph API + integration endpoints
                    .configure(crate::handlers::microsoft_graph::config)
                    .configure(crate::handlers::msgraph_integration::config)

                    // File upload endpoint
                    .configure(crate::handlers::files::config)

                    // ===== SSE / SEARCH / NOTIFICATIONS / BUG REPORTS =====
                    .configure(crate::handlers::sse::config)
                    .configure(crate::handlers::search::config)
                    .configure(crate::handlers::notifications::config)
                    .configure(crate::handlers::bug_reports::config)

                    // ===== TICKET MANAGEMENT =====
                    .configure(crate::handlers::tickets::config)
                    // ===== PROJECT MANAGEMENT =====
                    .configure(crate::handlers::projects::config)
                    // ===== GROUP DETAIL (All authenticated users) =====
                    .configure(crate::handlers::groups::config)
                    // ===== CATEGORY MANAGEMENT =====
                    .configure(crate::handlers::categories::config)
                    // ===== ASSIGNMENT RULES MANAGEMENT =====
                    .configure(crate::handlers::assignment_rules::config)
                    // ===== API TOKEN MANAGEMENT =====
                    .configure(crate::handlers::api_tokens::config)
                    // ===== WEBHOOK MANAGEMENT =====
                    .configure(crate::handlers::webhooks::config)

                    // ===== PLUGIN MANAGEMENT (Admin) =====
                    .configure(crate::handlers::plugins::config)
                    // ===== USER MANAGEMENT =====
                    .configure(crate::handlers::users::config)
                    // ===== ASSET MANAGEMENT =====
                    .configure(crate::handlers::assets::config)
                    // ===== DOCUMENTATION SYSTEM =====
                    .configure(crate::handlers::documentation::config)
                    // ===== DOCUMENTATION COLLECTIONS =====
                    .configure(crate::handlers::documentation_collections::config)
            )

            // Catch-all for /uploads/* that wasn't matched by the explicit
            // public-asset routes above. Previously this served any object
            // unauthenticated; it now 404s. Tenant files are served only via
            // the authenticated, workspace-scoped /api/files/* routes.
            .route("/uploads/{path:.*}", web::get().to(crate::handlers::reject_legacy_upload_path))

            // === FRONTEND STATIC FILES ===
            // Serve static frontend files with SPA fallback using default_handler
            // This is the recommended actix-web pattern for SPAs
            .service(
                // Vite-output assets live under `/static/` so the
                // SPA can keep `/assets/*` as a route prefix
                // (inventory list, create, detail). If you move
                // them back, update `vite.config.ts.assetsDir`,
                // `serve_spa`, and the security-headers middleware
                // in lockstep.
                Files::new("/static", "./public/static")
                    .use_last_modified(true)
                    .use_etag(true)
                    // Handle missing assets gracefully in development (frontend rebuild scenario)
                    .default_handler(fn_service(|req: ServiceRequest| async move {
                        let path = req.path().to_string();
                        let (req, _) = req.into_parts();
                        let res = handle_missing_asset(&path);
                        Ok(ServiceResponse::new(req, res))
                    }))
            )
            .service(
                Files::new("/pdfjs", "./public/pdfjs")
                    .use_last_modified(true)
                    .use_etag(true)
            )
            // Root path handler - serves index.html or rebuilding message
            .route("/", web::get().to(serve_spa))
            .service(
                Files::new("/", "./public")
                    .use_last_modified(true)
                    .use_etag(true)
                    // SPA fallback: serve index.html for any path not found
                    .default_handler(fn_service(|req: ServiceRequest| async move {
                        let (req, _) = req.into_parts();
                        // Use no-cache so browsers always check for updated frontend builds
                        match tokio::fs::read("./public/index.html").await {
                            Ok(content) => {
                                let res = HttpResponse::Ok()
                                    .content_type("text/html; charset=utf-8")
                                    .insert_header(("Cache-Control", "no-cache"))
                                    .body(content);
                                Ok(ServiceResponse::new(req, res))
                            }
                            Err(_) => {
                                // Frontend not built yet - show friendly rebuilding message
                                let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
                                let body = if environment != "production" {
                                    r#"<!DOCTYPE html>
<html>
<head>
    <title>Building...</title>
    <meta http-equiv="refresh" content="3">
</head>
<body style="margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;background:#1a1a2e;font-family:system-ui,sans-serif;">
    <div style="text-align:center;color:#fff;">
        <div style="width:40px;height:40px;border:3px solid #333;border-top-color:#6366f1;border-radius:50%;margin:0 auto 16px;animation:spin 1s linear infinite;"></div>
        <p style="margin:0;font-size:16px;opacity:0.8;">Frontend is rebuilding...</p>
    </div>
    <style>@keyframes spin{to{transform:rotate(360deg)}}</style>
</body>
</html>"#
                                } else {
                                    "Frontend not found"
                                };
                                let res = HttpResponse::ServiceUnavailable()
                                    .content_type("text/html")
                                    .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
                                    .body(body);
                                Ok(ServiceResponse::new(req, res))
                            }
                        }
                    }))
            );
}

// ---- composition root (moved from main.rs) ----

/// SIGTERM (Fly deploy / `docker stop`) + SIGINT (Ctrl-C) streams, kept
/// installed for the whole shutdown so a *second* signal during the graceful
/// drain can escalate to an immediate stop rather than falling through to the
/// default handler (which hard-kills mid-flush). We `disable_signals()` on the
/// server and drive shutdown ourselves so the collab flush runs before the
/// server stops.
struct ShutdownSignals {
    term: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    /// Install the handlers. `None` if either can't be installed, in which case
    /// the caller falls back to running without signal-driven shutdown.
    fn install() -> Option<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Some(Self {
            term: signal(SignalKind::terminate()).ok()?,
            interrupt: signal(SignalKind::interrupt()).ok()?,
        })
    }

    /// Resolve on the next SIGTERM or SIGINT.
    async fn recv(&mut self) {
        tokio::select! {
            _ = self.term.recv() => info!("Received SIGTERM"),
            _ = self.interrupt.recv() => info!("Received SIGINT"),
        }
    }
}

/// A bound, un-awaited server plus the handles the caller needs to drive
/// graceful shutdown. Returned by [`build_server`].
pub struct BuiltServer {
    /// The un-awaited actix server. `await` it (or spawn it) to serve.
    pub server: actix_web::dev::Server,
    /// Cancels the periodic scheduler + all background listeners + collab work.
    pub scheduler_shutdown: tokio_util::sync::CancellationToken,
    /// The Yjs app state, so the shutdown path can flush dirty docs.
    yjs: actix_web::web::Data<crate::handlers::collaboration::YjsAppState>,
    /// Background task handles, awaited (bounded) on shutdown after the token is
    /// cancelled so the listeners finish cleanly rather than being aborted.
    background_tasks: Vec<tokio::task::JoinHandle<()>>,
}

/// Build everything from a validated [`Config`] up to a bound, un-awaited
/// server: keyring, database, background workers, shared state, and the App
/// factory bound to `listener`. Taking a pre-bound listener (rather than
/// binding host:port internally) lets tests bind `127.0.0.1:0` and drive the
/// real server over HTTP. The binary's [`run`] wraps this with signal-driven
/// graceful shutdown.
pub async fn build_server(
    config: Config,
    listener: std::net::TcpListener,
) -> std::io::Result<BuiltServer> {
    // Rebind the values downstream setup still reads by their old names;
    // build_state / configure_app take `&config` directly.
    let environment = config.environment.clone();
    let rate_limit_per_minute = config.rate_limit_per_minute;
    let auth_rate_limit_per_minute = config.auth_rate_limit_per_minute;
    let redis_url = config.redis_url.clone();
    let frontend_url = config.frontend_url.clone();
    // Read before `config` moves into the factory closure below.
    let shutdown_timeout_secs = config.shutdown_timeout_secs;
    let additional_origins = config.additional_origins.clone();
    let tenant_domain = config.tenant_domain.clone();

    // Initialise the at-rest encryption keyring (versioned KEK,
    // self-describing framed-blob shape; see
    // `crate::utils::encryption::Keyring`). MFA secrets, channel credentials, plugin signing
    // keys, and plugin secret settings all decrypt through this.
    //
    // Hard cutover at v1: refusal to boot is unconditional. There
    // is no longer a "MFA disabled in dev if unset" path — every
    // install must export at least `MFA_KEK_V1`. The legacy
    // `ENCRYPTION_KEY` / `MFA_ENCRYPTION_KEY` names are no longer
    // read; rename them at upgrade time.
    let placeholder_kek_v1 = std::env::var("MFA_KEK_V1")
        .ok()
        .is_some_and(|k| crate::config::looks_like_placeholder(&k));
    if placeholder_kek_v1 {
        if environment == "production" {
            error!("MFA_KEK_V1 appears to be the docker.env.example placeholder");
            error!("Refusing to start in production with a placeholder KEK");
            error!("Generate a secure key with: openssl rand -hex 32");
            std::process::exit(1);
        } else {
            warn!("MFA_KEK_V1 looks like a placeholder; using it as-is in dev");
        }
    }
    match crate::utils::encryption::init_keyring() {
        Ok(kr) => {
            info!(versions = ?kr.versions(), current = kr.current_version(), "Keyring initialised")
        }
        Err(e) => {
            error!(error = %e, "Keyring initialisation failed");
            if std::env::var("ENCRYPTION_KEY").is_ok()
                || std::env::var("MFA_ENCRYPTION_KEY").is_ok()
            {
                error!("Note: ENCRYPTION_KEY and MFA_ENCRYPTION_KEY are no longer read.");
                error!("Rename your existing key to MFA_KEK_V1 and set MFA_KEK_VERSION=1.");
            } else {
                error!("Generate a key with: openssl rand -hex 32");
                error!("Export it as MFA_KEK_V1 and set MFA_KEK_VERSION=1.");
            }
            std::process::exit(1);
        }
    }

    // Open the optional GeoIP database (GEOIP_DB_PATH) once at startup so
    // session creation can attach a coarse location. No-op + info log when
    // unset; never fatal.
    crate::utils::geoip::init_from_env();

    // Rate-limit keying goes through the trusted-proxy-aware client_ip
    // helper. Behind a reverse proxy (the standard production shape) per-IP
    // limits track the real client; on a direct connection X-Forwarded-For
    // is ignored so an attacker can't rotate spoofed headers to bypass the
    // limit. A build failure (a malformed REDIS_URL) is fatal everywhere —
    // there's no in-memory fallback to silently degrade to.
    let public_limiter = Limiter::builder(&redis_url)
        .key_by(|req: &actix_web::dev::ServiceRequest| {
            crate::utils::client_ip::from_service_request(req).map(|ip| format!("public:{ip}"))
        })
        .limit(rate_limit_per_minute as usize)
        .period(Duration::from_secs(60))
        .build()
        .map_err(|e| {
            error!(error = %e, "Failed to build the public rate limiter (check REDIS_URL)");
            std::io::Error::other("Public rate limiter initialization failed")
        })?;

    let auth_limiter = Limiter::builder(&redis_url)
        .key_by(|req: &actix_web::dev::ServiceRequest| {
            crate::utils::client_ip::from_service_request(req).map(|ip| format!("auth:{ip}"))
        })
        .limit(auth_rate_limit_per_minute as usize)
        .period(Duration::from_secs(60))
        .build()
        .map_err(|e| {
            error!(error = %e, "Failed to build the auth rate limiter (check REDIS_URL)");
            std::io::Error::other("Auth rate limiter initialization failed")
        })?;

    // Forwarded browser-console logs get their OWN per-IP bucket
    // (`felogs:{ip}`), separate from the `public:`/`auth:` quotas that
    // gate login and MFA. A chatty or looping client (e.g. the frontend
    // remote logger retrying against a disabled endpoint) can then only
    // exhaust this bucket, never lock a user out of auth. See the MFA-429
    // incident where a log storm on the shared public limiter 429'd
    // /api/auth/mfa-setup-login.
    let frontend_logs_limiter = Limiter::builder(&redis_url)
        .key_by(|req: &actix_web::dev::ServiceRequest| {
            crate::utils::client_ip::from_service_request(req).map(|ip| format!("felogs:{ip}"))
        })
        .limit(rate_limit_per_minute as usize)
        .period(Duration::from_secs(60))
        .build()
        .map_err(|e| {
            error!(error = %e, "Failed to build the frontend-logs rate limiter (check REDIS_URL)");
            std::io::Error::other("Frontend-logs rate limiter initialization failed")
        })?;

    // Set up database connection pool
    let pool = match std::panic::catch_unwind(crate::db::establish_connection_pool) {
        Ok(pool) => pool,
        Err(e) => {
            error!(error = ?e, "Database connection pool initialization panicked");
            return Err(std::io::Error::other("Database connection pool failed"));
        }
    };

    // === DATABASE INITIALIZATION ===
    match crate::db::initialize_database(&pool).await {
        Ok(_) => {}
        Err(e) => {
            error!(error = %e, "Database initialization failed");
            return Err(std::io::Error::other(format!(
                "Database initialization failed: {e}"
            )));
        }
    }

    // Security: Verify initialization was successful
    if !crate::db::is_initialized() {
        error!("Database initialization verification failed");
        return Err(std::io::Error::other(
            "Database initialization verification failed",
        ));
    }

    // === DATABASE ROLE / RLS POSTURE (P0.2) ===
    // In hosted (multi-tenant) mode, tenant isolation rests entirely on
    // row-level security. A login role that bypasses RLS (a superuser or
    // BYPASSRLS role) sees and writes every tenant's rows even with FORCE
    // RLS on the table, so the production DATABASE_URL must authenticate as
    // the NOBYPASSRLS `nosdesk_app` role. Refuse to boot a production
    // hosted deployment on a bypass role; warn loudly elsewhere so dev /
    // staging stacks on the superuser keep working. Self-hosted is single
    // tenant, where RLS is moot, so this check doesn't apply there.
    if crate::middleware::DeploymentMode::current() == crate::middleware::DeploymentMode::Hosted {
        match crate::db::inspect_role_rls_posture(&pool) {
            Ok(posture) if posture.bypasses_rls => {
                if environment == "production" {
                    error!(
                        role = %posture.role_name,
                        "Hosted mode is connected to Postgres as a role that BYPASSES RLS; tenant isolation is disabled. Pin DATABASE_URL to the NOBYPASSRLS 'nosdesk_app' role."
                    );
                    std::process::exit(1);
                } else {
                    warn!(
                        role = %posture.role_name,
                        "Hosted mode connected as an RLS-bypassing role (superuser/BYPASSRLS); acceptable outside production only. Pin DATABASE_URL to 'nosdesk_app' before going live."
                    );
                }
            }
            Ok(posture) => {
                info!(
                    role = %posture.role_name,
                    "DB role enforces RLS (NOBYPASSRLS); hosted tenant isolation active"
                );
            }
            Err(e) => {
                // Don't fail open on an inconclusive check in production.
                if environment == "production" {
                    error!(error = %e, "Could not verify DB role RLS posture in hosted production mode");
                    std::process::exit(1);
                } else {
                    warn!(error = %e, "Could not verify DB role RLS posture");
                }
            }
        }
    }

    // W6c: eagerly provision sync_actions / audit_log partitions
    // at startup, before binding the listener. The daily scheduler
    // job below keeps the runway rolling forward, but a deployment
    // that's been offline past its last provisioned month would
    // reject the first INSERT into either partitioned table on the
    // very first request. Doing it synchronously here self-heals
    // that gap. Fail loud — refuse to bind the listener rather than
    // serve a process that will 500 on the first audit write.
    if let Err(e) = crate::services::scheduled_jobs::ensure_sync_partitions(pool.clone()).await {
        error!(error = %e, "Startup partition provisioning failed");
        return Err(std::io::Error::other(format!(
            "Startup partition provisioning failed: {e}"
        )));
    }

    // Create uploads directory structure if it doesn't exist. The base path is
    // `/app/uploads` in the container image; `NOSDESK_UPLOAD_DIR` overrides it
    // so a host / test boot can point it somewhere writable.
    let uploads_dir =
        std::env::var("NOSDESK_UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_string());
    let uploads_dir = uploads_dir.as_str();
    let directories = [
        "",
        "temp",
        "tickets",
        "users",
        "users/avatars",
        "users/banners",
        "users/thumbs",
        "plugins",
    ];
    for dir in directories.iter() {
        let full_path = format!("{uploads_dir}/{dir}");
        match std::fs::create_dir_all(&full_path) {
            Ok(_) => {}
            Err(e) => {
                error!(path = %full_path, error = %e, "Failed to create directory");
                return Err(std::io::Error::other(format!(
                    "Failed to create directory: {full_path}"
                )));
            }
        }
    }

    // Bootstrap local plugin signing key before provisioning so any
    // verification path can resolve the `local` trust tier. The
    // singleton row's INSERT fires the workspace-scoped audit trigger,
    // which requires `app.workspace_id` to be set; pin to the
    // bootstrap workspace (same pattern as `run_seeds` below).
    {
        let mut conn = pool
            .get()
            .expect("Failed to get connection for local key bootstrap");
        let actor =
            crate::sync::actor::ActorContext::system("startup:plugin_local_key").with_workspace(1);
        let result = crate::sync::session::with_actor_context::<_, anyhow::Error>(
            &mut conn,
            &actor,
            |conn| {
                crate::services::plugins::local_key::ensure_local_signing_key(conn)
                    .map(|_| ())
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
        );
        if let Err(e) = result {
            error!(error = %e, "Failed to bootstrap plugin local signing key");
            return Err(std::io::Error::other(format!(
                "Failed to bootstrap plugin local signing key: {e}"
            )));
        }
    }

    // Bootstrap admin paths, in priority order:
    //   1. INITIAL_ADMIN_* env vars (GitOps / declarative). If
    //      set + users empty, seed and we're done.
    //   2. Otherwise, mint a one-shot bootstrap token so the
    //      operator can complete setup via the web URL or the
    //      `nosdesk-cli admin create` subcommand.
    //
    // Misconfigured env vars (set but unusable — bad hash, missing
    // password, etc.) are a refuse-to-boot condition; an operator
    // who set INITIAL_ADMIN_* clearly meant the env path and a
    // silent fallback would mask the mistake.
    {
        let mut conn = pool
            .get()
            .expect("Failed to get connection for bootstrap reconcile");

        match crate::services::admin_setup::seed_from_env(&mut conn) {
            Ok(true) => {
                // Env-var seed succeeded; reconcile() will see
                // users and clean up any stale token file.
                if let Err(e) = crate::utils::bootstrap_token::reconcile(
                    &mut conn,
                    crate::middleware::DeploymentMode::current(),
                ) {
                    error!(error = ?e, "bootstrap token reconcile failed");
                }
            }
            Ok(false) => {
                // No env-seed; fall through to the token path.
                if let Err(e) = crate::utils::bootstrap_token::reconcile(
                    &mut conn,
                    crate::middleware::DeploymentMode::current(),
                ) {
                    error!(error = ?e, "bootstrap token reconcile failed");
                }
            }
            Err(e) => {
                error!(error = %e, "INITIAL_ADMIN_* env vars are set but unusable");
                return Err(std::io::Error::other(format!(
                    "refusing to start with misconfigured INITIAL_ADMIN_*: {e}"
                )));
            }
        }
    }

    // Log + clean up an unconsumed bootstrap token when it expires. A
    // no-op when no token is on disk (setup already done, or hosted).
    crate::utils::bootstrap_token::spawn_expiry_logger();

    // AUD-007: prewarm the dummy bcrypt hash so the first real
    // login attempt doesn't pay the one-shot init cost.
    crate::utils::login_timing::prewarm();

    // Surface the compiled-in Nosdesk root pubkey fingerprint on
    // every startup. Operators can diff this against what they
    // know the real root to be; an attacker who swaps the backend
    // binary with one linking a different root will announce the
    // substitution in the logs on the next boot.
    match crate::services::plugins::signing::root_pubkey() {
        Some(root_b64) => {
            use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
            match BASE64.decode(root_b64.as_bytes()) {
                Ok(bytes) if bytes.len() == 32 => {
                    info!(
                        fingerprint = %crate::services::plugins::signing::fingerprint(&bytes),
                        "Nosdesk plugin root pubkey loaded"
                    );
                }
                _ => {
                    warn!(
                        "NOSDESK_ROOT_PUBKEY is set but not a valid base64-encoded 32-byte Ed25519 pubkey; official-tier installs will fail"
                    );
                }
            }
        }
        None => {
            warn!(
                "NOSDESK_ROOT_PUBKEY is not baked in; registry sync and official-tier installs will fail"
            );
        }
    }

    // Provision plugins from /app/plugins/ directory
    {
        let mut conn = pool
            .get()
            .expect("Failed to get connection for plugin provisioning");
        let results = crate::services::plugins::provision_plugins(&mut conn);
        for result in results {
            match result {
                crate::services::plugins::provisioning::ProvisionResult::Created(name) => {
                    info!(plugin = %name, "Provisioned new plugin");
                }
                crate::services::plugins::provisioning::ProvisionResult::Updated(name) => {
                    info!(plugin = %name, "Updated provisioned plugin");
                }
                crate::services::plugins::provisioning::ProvisionResult::Failed(name, err) => {
                    warn!(plugin = %name, error = %err, "Failed to provision plugin");
                }
                _ => {}
            }
        }
    }

    // Run startup seeds (idempotent - only creates content if missing).
    // Pin the seed connection to the bootstrap workspace: post-3d
    // every tenant table NOT-NULLs workspace_id from the
    // app.workspace_id GUC default, so seeding the welcome doc page /
    // Getting Started collection with an unset GUC trips the
    // NOT-NULL constraint. with_actor_context sets the GUC for the
    // seed transaction; run_seeds keeps its own warn-on-failure
    // handling, so the closure just returns Ok.
    {
        let mut conn = pool.get().expect("Failed to get connection for seeding");
        let actor = crate::sync::actor::ActorContext::system("startup:seed").with_workspace(1);
        let _ = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |conn| {
                crate::services::seed::run_seeds(conn);
                Ok(())
            },
        );
    }

    let (state, background_tasks) = build_state(
        &config,
        pool.clone(),
        public_limiter,
        auth_limiter,
        frontend_logs_limiter,
    )?;

    // Pre-create the static-asset directories so `Files::new` can
    // canonicalize them at startup. Without this, if the backend
    // boots before the frontend build has populated `./public/static`
    // (a common race in `compose up` where backend and frontend-watch
    // start in parallel), Actix's `Files` service fails its initial
    // canonicalize and silently falls through to the default_handler
    // for every request, even after the directory later appears.
    // The "Asset still missing after reload" dev fallback then takes
    // over and reload-loops the browser indefinitely.
    //
    // These must match the actual `Files::new` mounts below
    // (`/static`, `/pdfjs`). Pre-creating `./public/assets` instead was
    // a stale leftover from the old Vite `assetsDir`: it both skipped
    // the real `static` dir AND shadowed the `/assets` SPA route, so a
    // hard refresh on `/assets` resolved to an empty directory and
    // returned "unable to render directory without index file".
    //
    // Idempotent: `create_dir_all` is a no-op when the directory
    // already exists. Safe in production where the build pipeline
    // populates these directories long before the binary starts.
    for static_dir in ["./public/static", "./public/pdfjs"] {
        if let Err(e) = std::fs::create_dir_all(static_dir) {
            error!(path = %static_dir, error = %e, "Failed to ensure static directory exists");
            return Err(std::io::Error::other(format!(
                "Failed to ensure static directory {static_dir}: {e}"
            )));
        }
    }

    // === MULTI-TENANT BOOTSTRAP ===
    // Phase 2a: resolve a workspace per request via middleware.
    // Self-hosted mode loads the bootstrap workspace once here
    // and reuses it for every request; hosted mode resolves
    // subdomain -> slug -> workspace lazily inside the
    // middleware itself. Failing fast at startup if the
    // bootstrap workspace is missing surfaces a misconfigured
    // deployment before any traffic hits.
    let workspace_config = match crate::middleware::WorkspaceContextConfig::initialise(&pool) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!(error = %e, "Workspace context bootstrap failed");
            return Err(std::io::Error::other(e));
        }
    };
    info!(
        mode = ?workspace_config.mode,
        bootstrap_slug = workspace_config.bootstrap.as_ref().map(|w| w.slug.as_str()),
        "Workspace context middleware initialised"
    );

    // M5 Task 6. Build the allowlist once at boot and share it via
    // Arc into each worker's CORS closure. The previous code used
    // `Cors::allowed_origin(&frontend_url)` which is exact-string;
    // every tenant subdomain preflight failed. `CorsAllowlist`
    // adds an anchored tenant-subdomain regex; substring bypasses
    // (`https://acme.nosdesk.app.attacker.com`) can't slip in.
    let allow_native_app = crate::utils::cors_allowlist::native_app_allowed_from_env();
    let cors_allowlist = crate::utils::cors_allowlist::CorsAllowlist::new(
        std::iter::once(frontend_url.as_str()).chain(additional_origins.iter().map(|s| s.as_str())),
        tenant_domain.as_deref(),
        allow_native_app,
    );
    info!(
        host_count = cors_allowlist.exact_count(),
        tenant_domain = ?tenant_domain,
        allow_native_app,
        "CORS allowlist initialised"
    );
    // Install as the process-wide allowlist. Both the CORS layer below and
    // the collab WebSocket origin guard (crate::handlers::collaboration) read it
    // via `cors_allowlist::global()` — no per-App `web::Data` to wire (or
    // forget in tests). The set runs before any worker starts.
    crate::utils::cors_allowlist::set_global(cors_allowlist);

    // Cloned out before the factory closure moves `yjs_app_state` in, so
    // the shutdown handler below can flush collab docs on SIGTERM.
    let yjs_for_shutdown = state.yjs_app_state.clone();
    let collab_shutdown_token = state.scheduler_shutdown.clone();

    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin_fn(|origin, _req_head| {
                origin
                    .to_str()
                    .ok()
                    .is_some_and(|o| crate::utils::cors_allowlist::global().allows(o))
            })
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"])
            .allowed_headers(vec![
                "Authorization",
                "Content-Type",
                "Accept",
                "Origin",
                "X-Requested-With",
                "X-CSRF-Token",
                // Bearer mode + Model-C workspace selection: the native app's
                // sync engine uses cross-origin (tauri://localhost) raw fetch,
                // which preflights these. The REST path goes through the native
                // HTTP plugin and bypasses CORS, but the streaming sync fetch
                // does not, so the preflight must allow them.
                "X-Auth-Mode",
                "X-Nosdesk-Workspace",
            ])
            .expose_headers(vec!["content-disposition"])
            .supports_credentials()
            .max_age(3600);

        App::new()
            // TracingLogger is the outermost wrap so its root span
            // covers every other middleware (CORS preflight, CSRF
            // rejections, security headers). Auth middlewares record
            // user_uuid / actor_kind onto this span post-hoc.
            .wrap(TracingLogger::<crate::middleware::NosdeskRootSpanBuilder>::new())
            .wrap(cors)
            .wrap(crate::middleware::SecurityHeaders) // Apply security headers globally
            .wrap(crate::utils::csrf::CsrfProtection)
            // Workspace context resolution. Sits before the auth
            // middlewares because authenticated routes need the
            // workspace identified first (a Phase 2e change will
            // wire the membership check into auth using this
            // context). Self-hosted attaches the bootstrap
            // workspace to every request; hosted resolves
            // subdomain per request.
            .wrap(crate::middleware::WorkspaceContextMiddleware::new(
                workspace_config.clone(),
            ))
            // The authenticated limiter is the app-level default that
            // `RateLimiter::default()` resolves by type (it looks up
            // `web::Data<Limiter>`). The stricter public limiter is
            // registered at the scope level on each public / auth
            // surface below so it shadows this one there. Registering
            // both app-level would make the second silently win for
            // every scope (both are the same `web::Data<Limiter>`
            // type), which is the bug that left the public limit
            // unused. See security-audit-2026-06.
            .configure(|cfg| configure_app(cfg, &state, &pool, &workspace_config, &config))
    })
    .listen(listener)?
    // Bound graceful-drain budget for in-flight requests (actix defaults to
    // 30s). Keep it below the deploy grace period so the drain finishes before
    // SIGKILL. See Config::shutdown_timeout_secs.
    .shutdown_timeout(shutdown_timeout_secs)
    .disable_signals()
    .run();

    Ok(BuiltServer {
        server,
        scheduler_shutdown: collab_shutdown_token,
        yjs: yjs_for_shutdown,
        background_tasks,
    })
}

/// The binary entrypoint's composition root: bind the configured host:port and
/// serve with signal-driven graceful shutdown until SIGTERM / SIGINT drains it.
pub async fn run(config: Config) -> std::io::Result<()> {
    let listener = std::net::TcpListener::bind((config.host.as_str(), config.port))?;
    let BuiltServer {
        server,
        scheduler_shutdown,
        yjs,
        background_tasks,
    } = build_server(config, listener).await?;

    // We own the signals (`disable_signals` in build_server) and drive shutdown
    // from this flow so post-drain work (the collab flush + task drain) is
    // guaranteed to finish before the process exits. Run the server as a task so
    // we can wait on a signal alongside it.
    let server_handle = server.handle();
    let mut server_task = actix_web::rt::spawn(server);

    // Keep the signal handlers installed for the whole shutdown so a *second*
    // signal escalates to an immediate stop rather than hard-killing us.
    let mut signals = match ShutdownSignals::install() {
        Some(s) => s,
        None => {
            error!("Failed to install signal handlers; graceful shutdown disabled");
            return server_task.await.unwrap_or(Ok(()));
        }
    };

    tokio::select! {
        _ = signals.recv() => {}
        // The server ended on its own (a fatal error, not a signal): cancel
        // background work and propagate its result.
        res = &mut server_task => {
            scheduler_shutdown.cancel();
            return res.unwrap_or(Ok(()));
        }
    }

    // Graceful shutdown, ordered per the k8s / Fly guidance:
    //   1. Fail readiness so the proxy / orchestrator drains this instance.
    //   2. Pause briefly so that drain propagates before we stop accepting.
    //   3. Stop accepting new connections and drain in-flight requests (bounded
    //      by the shutdown_timeout set in build_server).
    //   4. Connections closed, docs final: flush collab, then cancel the shared
    //      token and await the background listeners/jobs (bounded) so they finish
    //      cleanly rather than being aborted by the runtime dropping.
    // A second signal escalates to an immediate stop. The deploy grace period
    // (Fly `kill_timeout`) must exceed drain pause + shutdown_timeout + flush +
    // task-drain, or the process is SIGKILLed mid-shutdown.
    info!("Shutdown signal received; draining traffic");
    crate::handlers::health::begin_shutdown();

    let sh = server_handle.clone();
    let token = scheduler_shutdown.clone();
    let graceful = async move {
        tokio::time::sleep(SHUTDOWN_DRAIN_PAUSE).await;
        sh.stop(true).await;
        info!("HTTP drained; flushing collaborative documents");
        yjs.flush_all_dirty(std::time::Duration::from_secs(4)).await;
        token.cancel();
        // Bounded: the tasks are crash-safe, so a timeout just proceeds to exit.
        if tokio::time::timeout(
            BACKGROUND_DRAIN_TIMEOUT,
            futures::future::join_all(background_tasks),
        )
        .await
        .is_err()
        {
            warn!("Background tasks did not finish within the drain window; exiting anyway");
        }
    };

    tokio::select! {
        _ = graceful => {}
        _ = signals.recv() => {
            warn!("Second shutdown signal received; stopping immediately");
            scheduler_shutdown.cancel();
            server_handle.stop(false).await;
        }
    }
    let _ = server_task.await;
    Ok(())
}

/// How long to keep serving after failing readiness before we stop accepting
/// connections, so the load balancer / orchestrator notices the drain first.
const SHUTDOWN_DRAIN_PAUSE: std::time::Duration = std::time::Duration::from_secs(3);

/// Bounded window to wait for background listeners/jobs to finish after the
/// shutdown token is cancelled, before exiting regardless (they're crash-safe).
const BACKGROUND_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::spa_shell_path;
    use crate::middleware::DeploymentMode;

    #[test]
    fn portal_shell_only_on_a_hosted_tenant_origin() {
        // Hosted + a host-resolved workspace => the customer portal.
        assert_eq!(
            spa_shell_path(DeploymentMode::Hosted, true),
            "./public/portal.html"
        );
        // Hosted agent origin (no host-resolved workspace) => the agent app.
        assert_eq!(
            spa_shell_path(DeploymentMode::Hosted, false),
            "./public/index.html"
        );
        // Self-host always serves the agent app, even though its bootstrap
        // workspace makes a context ever-present.
        assert_eq!(
            spa_shell_path(DeploymentMode::SelfHosted, true),
            "./public/index.html"
        );
        assert_eq!(
            spa_shell_path(DeploymentMode::SelfHosted, false),
            "./public/index.html"
        );
    }
}
