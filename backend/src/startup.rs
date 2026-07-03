//! Application state construction (the composition root's state phase).
//!
//! `build_state` builds every `web::Data` the App factory registers, spawns the
//! state-bound background listeners (sync-outbox, email-queue, search
//! replicator, registry sync, channel supervisor) and the periodic scheduler,
//! and returns them bundled as [`AppState`]. Extracted from main() so the
//! composition root stays thin (see docs/plans/main-bootstrap-refactor.md).

use std::sync::Arc;

use actix_limitation::Limiter;
use actix_web::web;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::db::Pool;
use crate::utils::redis_yjs_cache::create_redis_cache;
use crate::utils::storage::{create_storage, get_storage_config};
use crate::workers;

/// Every piece of shared state the App factory registers as `app_data`, plus
/// the scheduler shutdown token the composition root wires into graceful
/// shutdown. Destructured back into locals in main() so the App builder reads
/// them by their original names.
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
) -> Result<AppState, std::io::Error> {
    // Owned copies so the lifted body reads these by their original names.
    let redis_url = config.redis_url.clone();
    let frontend_url = config.frontend_url.clone();
    let host = config.host.clone();
    let port = config.port;
    let environment = config.environment.clone();

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
        crate::services::sync_outbox::spawn(
            database_url,
            pool.clone(),
            sse_state.clone().into_inner(),
        );
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
        crate::services::email_queue::spawn(database_url, pool.clone(), outbound_resolver.clone());
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
        crate::services::plugins::registry::spawn_sync_loop(
            pool.clone(),
            registry_url,
            registry_cache.as_ref().clone(),
        );
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
            crate::services::search_replicator::spawn(
                database_url,
                pool.clone(),
                search_service.get_ref().clone(),
            );
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
    // error, so a misconfig degrades rather than fails. See
    // `docs/realtime-collab-affinity-design.md`.
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
        let (control, _join) = supervisor::spawn(deps);
        web::Data::new(control)
    };

    // Boot the periodic-task scheduler (see workers::spawn_scheduled_jobs). The
    // returned `scheduler_shutdown` token is shared with the collab shutdown
    // path below so a single SIGTERM cancels both.
    let (scheduler_shutdown, scheduler_status) = workers::spawn_scheduled_jobs(
        pool.clone(),
        search_service.clone(),
        notification_service.clone(),
    );
    let scheduler_status_data = web::Data::new(scheduler_status);

    Ok(AppState {
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
    })
}
