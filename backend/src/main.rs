// Mirror the crate-level allows in lib.rs so the binary build picks
// up the same posture. See lib.rs for the rationale.
#![allow(
    clippy::too_many_arguments,
    clippy::large_enum_variant,
    clippy::type_complexity,
    clippy::should_implement_trait,
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items,
    clippy::field_reassign_with_default,
    clippy::manual_strip
)]

use backend::db;
use backend::handlers;
use backend::license;
use backend::middleware;
use backend::services;
use backend::utils;

use actix_cors::Cors;
use actix_files::Files;
use actix_limitation::{Limiter, RateLimiter};
use actix_web::dev::{fn_service, ServiceRequest, ServiceResponse};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use dotenvy::dotenv;
use std::env;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use tracing_actix_web::TracingLogger;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use utils::redis_yjs_cache::create_redis_cache;
use utils::storage::{create_storage, get_storage_config};

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
fn spa_shell_path(mode: middleware::DeploymentMode, host_resolved_workspace: bool) -> &'static str {
    if mode == middleware::DeploymentMode::Hosted && host_resolved_workspace {
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
        .get::<backend::extractors::WorkspaceContext>()
        .is_some();
    let shell = spa_shell_path(
        middleware::DeploymentMode::current(),
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

// Cookie-based authentication middleware lives in
// `middleware/cookie_auth.rs` so it sits next to its peer
// `middleware/api_token.rs` and uses `crate::*` paths consistently.
// Re-exported from the middleware module for the same `from_fn`
// usage in route registration below.
use middleware::cookie_auth_middleware;

/// Resolve when the process receives a termination signal: SIGTERM (Fly
/// deploy / `docker stop`) or SIGINT (Ctrl-C in dev). We `disable_signals()`
/// on the server and drive shutdown ourselves so our own graceful work
/// (flushing collaborative documents) runs before the server stops;
/// actix's built-in handling would stop the server without that flush. If
/// the handlers can't be installed, this never resolves, leaving today's
/// behaviour (the OS kills the process, no flush).
async fn await_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let (mut term, mut interrupt) = match (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) {
        (Ok(t), Ok(i)) => (t, i),
        _ => {
            error!(
                "Failed to install termination signal handlers; graceful shutdown flush disabled"
            );
            std::future::pending::<()>().await;
            return;
        }
    };
    tokio::select! {
        _ = term.recv() => info!("Received SIGTERM"),
        _ = interrupt.recv() => info!("Received SIGINT"),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Early startup logging before tracing is initialized
    // Using eprintln! here since tracing isn't set up yet
    eprintln!(
        "BACKEND STARTING - Current dir: {:?}",
        std::env::current_dir()
    );
    std::io::Write::flush(&mut std::io::stderr()).ok();

    // Load .env file if it exists (for local development), but don't fail if it doesn't exist
    // In Docker, environment variables are already loaded via docker-compose
    if let Err(e) = dotenv() {
        eprintln!("Could not load .env file: {e}. This is normal in Docker environments.");
    }

    // Critical check: Verify DATABASE_URL exists
    if std::env::var("DATABASE_URL").is_err() {
        eprintln!("FATAL ERROR: DATABASE_URL environment variable is not set!");
        eprintln!("Cannot proceed without database connection");
        std::process::exit(1);
    }
    eprintln!("DATABASE_URL is set");

    // JWT_SECRET presence is validated after tracing init (see below)
    eprintln!("JWT_SECRET will be validated after tracing initialization");

    eprintln!("Initializing tracing...");

    // Initialize tracing/logging subsystem with better error handling.
    //
    // Third-party crates that emit per-operation log lines are pinned
    // below the default level so our own code stays grep-able. Tantivy
    // in particular logs every segment open / commit at INFO + DEBUG,
    // which floods the dev backend log (saw ~90% of recent lines from
    // tantivy alone). h2 / hyper / rustls / mio / want similarly emit
    // connection lifecycle chatter that obscures real signal at
    // debug.  Operators can still bump any of these by setting
    // `RUST_LOG` explicitly.
    let log_level = env::var("RUST_LOG").unwrap_or_else(|_| {
        let base = if env::var("ENVIRONMENT").unwrap_or_default() == "production" {
            "info"
        } else {
            "debug"
        };
        format!(
            "{base},tantivy=warn,h2=info,hyper=info,hyper_util=info,rustls=info,mio=info,want=info"
        )
    });

    // Production (LOG_FORMAT=json) emits via `RedactingJsonLayer` —
    // a field-allowlist JSON serializer that drops anything outside
    // the policy in `utils::tracing_redact`. Local dev keeps the
    // pretty formatter because developer laptops aren't
    // sub-processors of anyone's data. See the "Log redaction"
    // section of `SECURITY.md` for the policy.
    //
    // `EnvFilter` is attached as a per-layer filter via
    // `Layer::with_filter`, not on the registry. This keeps the
    // registry's span store unfiltered, so `LookupSpan` /
    // `ctx.event_scope` always see `tracing_actix_web`'s
    // per-request span (carrying `request_id`) — even when the
    // filter would drop the "served" event. The bare event is
    // suppressed inside the layer by target check.
    use tracing_subscriber::Layer as _;
    let json_logs = std::env::var("LOG_FORMAT").ok().as_deref() == Some("json");
    let env_filter = || EnvFilter::new(&log_level);
    let registry = tracing_subscriber::registry();
    let _ = if json_logs {
        registry
            .with(utils::tracing_redact::RedactingJsonLayer.with_filter(env_filter()))
            .try_init()
    } else {
        registry
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_line_number(true)
                    .with_writer(std::io::stdout)
                    .with_filter(env_filter()),
            )
            .try_init()
    };

    debug!("Tracing initialized, continuing startup");

    // === SECURITY STARTUP VALIDATION ===
    info!("Starting Nosdesk API Server");
    info!(log_level = %log_level, "Log level configured");
    // Resolve + log the edition once at boot (verifies NOSDESK_LICENSE_KEY,
    // if any). Community caps self-hosted deployments at one workspace.
    info!(
        edition = crate::license::current().name(),
        max_workspaces = crate::license::current().max_workspaces(),
        "Edition resolved"
    );

    // Debug: Print some environment variables to see what's available
    debug!("Environment check:");
    debug!(
        "  DATABASE_URL is set: {}",
        env::var("DATABASE_URL").is_ok()
    );
    debug!("  JWT_SECRET is set: {}", env::var("JWT_SECRET").is_ok());
    debug!(
        "  HOST: {}",
        env::var("HOST").unwrap_or("NOT_SET".to_string())
    );
    debug!(
        "  PORT: {}",
        env::var("PORT").unwrap_or("NOT_SET".to_string())
    );

    // Get environment early for validation
    let environment = env::var("ENVIRONMENT").unwrap_or("development".to_string());
    info!("Environment: {}", environment);

    // Detects values that look like docker.env.example placeholders
    // (e.g. "your-super-secret-jwt-key-change-this-in-production").
    // Applied to every production secret check below so an operator
    // who forgets to override the example file gets a fast hard
    // failure rather than a forged-token incident.
    fn looks_like_placeholder(value: &str) -> bool {
        let lower = value.to_ascii_lowercase();
        const NEEDLES: &[&str] = &[
            "change-this",
            "change-me",
            "your-super-secret",
            "your-64-character",
            "your-",
            "placeholder",
            "example",
        ];
        NEEDLES.iter().any(|n| lower.contains(n))
    }

    // Validate that JWT_SECRET is set and secure
    let _jwt_secret = match std::env::var("JWT_SECRET") {
        Ok(secret) => {
            if environment == "production" && looks_like_placeholder(&secret) {
                error!("JWT_SECRET appears to be the docker.env.example placeholder");
                error!("Refusing to start in production with a placeholder JWT_SECRET");
                error!("Generate a secure key with: openssl rand -base64 32");
                std::process::exit(1);
            }
            if secret.len() < 32 {
                if environment == "production" {
                    error!("JWT_SECRET must be at least 32 characters in production");
                    error!("Generate a secure key with: openssl rand -base64 32");
                    std::process::exit(1);
                } else {
                    warn!("JWT_SECRET is less than 32 characters - this would be rejected in production");
                }
            }
            secret
        }
        Err(e) => {
            error!(error = %e, "JWT_SECRET environment variable must be set");
            error!("Generate a secure key with: openssl rand -base64 32");
            std::process::exit(1);
        }
    };
    info!("JWT_SECRET validated");

    // Initialise the at-rest encryption keyring (versioned KEK,
    // self-describing framed-blob shape; see
    // `utils::encryption::Keyring` and docs/auth-convergence.md
    // items 1-3). MFA secrets, channel credentials, plugin signing
    // keys, and plugin secret settings all decrypt through this.
    //
    // Hard cutover at v1: refusal to boot is unconditional. There
    // is no longer a "MFA disabled in dev if unset" path — every
    // install must export at least `MFA_KEK_V1`. The legacy
    // `ENCRYPTION_KEY` / `MFA_ENCRYPTION_KEY` names are no longer
    // read; rename them at upgrade time.
    let placeholder_kek_v1 = std::env::var("MFA_KEK_V1")
        .ok()
        .is_some_and(|k| looks_like_placeholder(&k));
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

    // NOSDESK_ROOT_PUBKEY is baked into the binary at build time
    // via option_env! (see services/plugins/signing.rs). Without it
    // the plugin trust chain cannot verify Official or Verified
    // tiers; only `local` (CLI-installed) plugins work. That's
    // acceptable for an unconfigured fork but not for a Nosdesk
    // production deployment.
    if environment == "production" && crate::services::plugins::signing::root_pubkey().is_none() {
        error!("NOSDESK_ROOT_PUBKEY was not set at build time");
        error!("Refusing to start in production without a plugin trust root");
        error!("Rebuild with: docker build --build-arg NOSDESK_ROOT_PUBKEY=<base64> ...");
        error!("(Forks running their own registry should override with their own root key.)");
        std::process::exit(1);
    }

    // --- docker.env.example default credentials must never ship to production ---
    if environment.eq_ignore_ascii_case("production") {
        const EX_POSTGRES_PASSWORD: &str = "nosdesk_password";
        const EX_REDIS_PASSWORD: &str = "nosdesk_redis_password";

        let insecure_defaults_allowed = matches!(
            env::var("ALLOW_INSECURE_DEFAULT_SECRETS").as_deref(),
            Ok("1" | "true" | "yes")
        );

        if !insecure_defaults_allowed {
            if env::var("POSTGRES_PASSWORD").as_deref() == Ok(EX_POSTGRES_PASSWORD) {
                error!(
                    "POSTGRES_PASSWORD matches docker.env.example default ({EX_POSTGRES_PASSWORD})"
                );
                error!("Refusing to start in production with documented sample credentials");
                error!("Change POSTGRES_PASSWORD or set ALLOW_INSECURE_DEFAULT_SECRETS=1 only for isolated labs");
                std::process::exit(1);
            }
            if env::var("REDIS_PASSWORD").as_deref() == Ok(EX_REDIS_PASSWORD) {
                error!("REDIS_PASSWORD matches docker.env.example default ({EX_REDIS_PASSWORD})");
                error!("Refusing to start in production with documented sample credentials");
                error!("Change REDIS_PASSWORD or set ALLOW_INSECURE_DEFAULT_SECRETS=1 only for isolated labs");
                std::process::exit(1);
            }
        } else {
            warn!("ALLOW_INSECURE_DEFAULT_SECRETS enabled — example Postgres/Redis passwords accepted (labs only)");
        }
    }

    // Security: Validate environment (already declared above)
    if environment == "production" {
        // Check for HTTPS in production URLs
        if let Ok(frontend_url) = env::var("FRONTEND_URL") {
            if !frontend_url.starts_with("https://")
                && !frontend_url.starts_with("http://localhost")
            {
                warn!("FRONTEND_URL should use HTTPS in production");
            }
        }

        // Check database SSL in production
        if let Ok(db_url) = env::var("DATABASE_URL") {
            if !db_url.contains("sslmode=require") && !db_url.contains("localhost") {
                warn!("DATABASE_URL should use sslmode=require in production");
            }
        }
    }

    // === RATE LIMITING CONFIGURATION ===
    // Get rate limiting configuration from environment with reasonable defaults
    let rate_limit_per_minute = env::var("RATE_LIMIT_PER_MINUTE")
        .unwrap_or("60".to_string()) // Conservative limit for public endpoints
        .parse::<u64>()
        .unwrap_or(60)
        .clamp(30, 1000); // Reasonable limits: 30-1000 requests per minute

    let auth_rate_limit_per_minute = env::var("AUTH_RATE_LIMIT_PER_MINUTE")
        .unwrap_or("600".to_string()) // Higher limit for authenticated users (10x public rate)
        .parse::<u64>()
        .unwrap_or(600)
        .clamp(120, 5000); // Higher limits for authenticated users: 120-5000 requests per minute

    // Redis is a hard dependency: HTTP + auth/MFA rate limiting, the Yjs
    // collab cache, and the `/readiness` probe all require it. Resolve ONE
    // URL for all of them (the same shape as `utils::rate_limit::get_redis_url`).
    // Production requires it explicitly — in-memory rate limiting would be
    // per-machine, an N× silent bypass across the fleet — while dev defaults
    // to localhost. There is no `memory://` fallback: it only ever masked a
    // misconfigured single dev box where readiness and auth lockout were
    // already broken anyway.
    let redis_url = match env::var("REDIS_URL") {
        Ok(url) => url,
        Err(_) => {
            if environment == "production" {
                error!(
                    "REDIS_URL is required in production: rate limiting, the collab cache, and the readiness probe all depend on Redis, and in-memory limiting is a per-machine (N×) silent bypass. Configure Redis."
                );
                std::process::exit(1);
            }
            "redis://localhost:6379".to_string()
        }
    };

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

    // Get host and port from environment variables
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("PORT")
        .unwrap_or("8080".to_string())
        .parse::<u16>()
        .map_err(|e| {
            error!(error = %e, "Invalid PORT value");
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid PORT")
        })?;

    // Security: Get file upload limits from environment
    let max_file_size_mb = env::var("MAX_FILE_SIZE_MB")
        .unwrap_or("50".to_string())
        .parse::<usize>()
        .unwrap_or(50)
        .clamp(1, 500); // 1MB to 500MB limit

    let max_payload_size = max_file_size_mb * 1024 * 1024; // Convert to bytes

    // Validate CORS configuration - FRONTEND_URL required in production
    let frontend_url = match env::var("FRONTEND_URL") {
        Ok(url) => url,
        Err(_) if environment == "production" => {
            error!("FRONTEND_URL must be set in production for CORS security");
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "FRONTEND_URL environment variable is required in production",
            ));
        }
        Err(_) => "http://localhost:3000".to_string(),
    };

    // Parse additional CORS origins if provided
    let additional_origins: Vec<String> = env::var("ADDITIONAL_CORS_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect();

    // Tenant domain suffix for hosted-mode CORS (M5 Task 6). When
    // set (e.g. `nosdesk.app`), every `<slug>.<tenant_domain>` origin
    // passes the CORS check. Self-hosted leaves this unset and relies
    // on FRONTEND_URL alone. Built as an anchored regex below so a
    // substring-only match (`s.ends_with(".nosdesk.app")`) — the
    // classic CORS bypass — is impossible.
    let tenant_domain = env::var("NOSDESK_TENANT_DOMAIN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Set up database connection pool
    let pool = match std::panic::catch_unwind(db::establish_connection_pool) {
        Ok(pool) => pool,
        Err(e) => {
            error!(error = ?e, "Database connection pool initialization panicked");
            return Err(std::io::Error::other("Database connection pool failed"));
        }
    };

    // === DATABASE INITIALIZATION ===
    match db::initialize_database(&pool).await {
        Ok(_) => {}
        Err(e) => {
            error!(error = %e, "Database initialization failed");
            return Err(std::io::Error::other(format!(
                "Database initialization failed: {e}"
            )));
        }
    }

    // Security: Verify initialization was successful
    if !db::is_initialized() {
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
        match db::inspect_role_rls_posture(&pool) {
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
    if let Err(e) = services::scheduled_jobs::ensure_sync_partitions(pool.clone()).await {
        error!(error = %e, "Startup partition provisioning failed");
        return Err(std::io::Error::other(format!(
            "Startup partition provisioning failed: {e}"
        )));
    }

    // Create uploads directory structure if it doesn't exist
    let uploads_dir = "/app/uploads";
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
        let actor = backend::sync::actor::ActorContext::system("startup:plugin_local_key")
            .with_workspace(1);
        let result = backend::sync::session::with_actor_context::<_, anyhow::Error>(
            &mut conn,
            &actor,
            |conn| {
                services::plugins::local_key::ensure_local_signing_key(conn)
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

        match services::admin_setup::seed_from_env(&mut conn) {
            Ok(true) => {
                // Env-var seed succeeded; reconcile() will see
                // users and clean up any stale token file.
                if let Err(e) = utils::bootstrap_token::reconcile(
                    &mut conn,
                    crate::middleware::DeploymentMode::current(),
                ) {
                    error!(error = ?e, "bootstrap token reconcile failed");
                }
            }
            Ok(false) => {
                // No env-seed; fall through to the token path.
                if let Err(e) = utils::bootstrap_token::reconcile(
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
    utils::bootstrap_token::spawn_expiry_logger();

    // AUD-007: prewarm the dummy bcrypt hash so the first real
    // login attempt doesn't pay the one-shot init cost.
    utils::login_timing::prewarm();

    // Surface the compiled-in Nosdesk root pubkey fingerprint on
    // every startup. Operators can diff this against what they
    // know the real root to be; an attacker who swaps the backend
    // binary with one linking a different root will announce the
    // substitution in the logs on the next boot.
    match services::plugins::signing::root_pubkey() {
        Some(root_b64) => {
            use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
            match BASE64.decode(root_b64.as_bytes()) {
                Ok(bytes) if bytes.len() == 32 => {
                    info!(
                        fingerprint = %services::plugins::signing::fingerprint(&bytes),
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
        let results = services::plugins::provision_plugins(&mut conn);
        for result in results {
            match result {
                services::plugins::provisioning::ProvisionResult::Created(name) => {
                    info!(plugin = %name, "Provisioned new plugin");
                }
                services::plugins::provisioning::ProvisionResult::Updated(name) => {
                    info!(plugin = %name, "Updated provisioned plugin");
                }
                services::plugins::provisioning::ProvisionResult::Failed(name, err) => {
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
        let actor = backend::sync::actor::ActorContext::system("startup:seed").with_workspace(1);
        let _ = backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |conn| {
                services::seed::run_seeds(conn);
                Ok(())
            },
        );
    }

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
    let analytics_cache: web::Data<Option<std::sync::Arc<utils::analytics_cache::AnalyticsCache>>> =
        web::Data::new(
            match utils::analytics_cache::AnalyticsCache::new(&redis_url) {
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
    let sse_state = web::Data::new(handlers::sse::SseState::new());

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
        services::sync_outbox::spawn(database_url, pool.clone(), sse_state.clone().into_inner());
    } else {
        warn!("DATABASE_URL not set; sync outbox listener not spawned (SSE will not deliver real-time updates)");
    }

    // Build the email service once — it's reused by the notification
    // service and by the channels dispatcher for outbound ticket
    // replies. `None` means SMTP isn't configured; both callers treat
    // that as "email disabled" rather than a fatal error.
    let email_service: Option<std::sync::Arc<utils::email::EmailService>> =
        match utils::email::EmailService::from_env() {
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
    let outbound_resolver = std::sync::Arc::new(
        services::outbound_email::OutboundEmailResolver::new(pool.clone(), email_service.clone()),
    );

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
        services::email_queue::spawn(database_url, pool.clone(), outbound_resolver.clone());
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

        let service =
            services::notifications::NotificationService::new(pool.clone(), type_id_cache.clone());

        // Register in-app channel. Delivery is the notification sync emit
        // in persist_notification; this registration keeps the in-app
        // preference + rate limiting in channel selection.
        let in_app_channel =
            Arc::new(services::notifications::channels::in_app::InAppChannel::new());
        service.register_channel(in_app_channel);

        // Register email channel if email service is configured.
        // App name comes from the workspace branding (site_settings) at
        // send time, not env, so admin renames take effect without restart.
        if let Some(email_svc) = email_service.clone() {
            let email_channel =
                Arc::new(services::notifications::channels::email::EmailChannel::new(
                    email_svc,
                    pool.clone(),
                    frontend_url.clone(),
                    type_id_cache,
                ));
            service.register_channel(email_channel);
        }

        web::Data::new(service)
    };

    // Inject the outbound resolver so the comment handler can gate the
    // channel relay on whether outbound is configured at all (the worker
    // resolves the per-workspace identity at send time).
    let outbound_resolver_data = web::Data::new(outbound_resolver.clone());

    // Initialize webhook service for external integrations
    let webhook_service = web::Data::new(services::webhooks::WebhookService::new(pool.clone()));

    // Initialize plugin proxy service for external requests
    let plugin_proxy_service = web::Data::new(services::plugins::PluginProxyService::new());

    // Plugin registry cache. `None` at boot — populated by the
    // first successful sync. The admin UI reads this for the
    // browse-registry view.
    let registry_cache = web::Data::new(services::plugins::registry::new_cache());

    // Kick off the background registry sync loop. Disabled when
    // `NOSDESK_REGISTRY_URL=""` (air-gapped deployments); logged and
    // skipped with no further side effects. Failures are warn-and-
    // continue — the background task retries next cycle rather
    // than unwinding.
    if let Some(registry_url) = services::plugins::registry::configured_url() {
        services::plugins::registry::spawn_sync_loop(
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
            env::var("SEARCH_INDEX_PATH").unwrap_or_else(|_| "data/search_index".to_string());

        match services::search::SearchService::new(Path::new(&search_index_path), &pool) {
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
    if env::var("NOSDESK_SEARCH_REPLICATION")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        if let Ok(database_url) = env::var("DATABASE_URL") {
            services::search_replicator::spawn(
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
    use services::collab_ownership::CollabRoutingMode;
    let requested_mode = CollabRoutingMode::from_env_value(
        &env::var("NOSDESK_COLLAB_ROUTING").unwrap_or_else(|_| "single".into()),
    );
    let collab_ownership = services::collab_ownership::build(&yjs_redis_url, requested_mode);
    let collab_routing_mode = if collab_ownership.is_some() {
        requested_mode
    } else {
        CollabRoutingMode::Single
    };

    // Initialize WebSocket app state for collaborative editing (includes SseState for broadcasting)
    let yjs_app_state = web::Data::new(handlers::collaboration::YjsAppState::new(
        web::Data::new(pool.clone()),
        redis_cache,
        sse_state.clone(),
        search_service.get_ref().clone(),
        collab_ownership,
        collab_routing_mode,
    ));

    // Initialize system state for tracking uptime
    let system_state = web::Data::new(handlers::system::SystemState::new());

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
    utils::storage::set_process_storage(storage.clone());

    // Inbound-email S3 reader (hosted forwarding path). `None` on self-host or
    // when `NOSDESK_INBOUND_S3_BUCKET` is unset; a bucket configured without
    // SES credentials is a hard misconfig we surface loudly but don't crash on.
    let inbound_s3 = match services::inbound_email::s3_fetch::InboundS3::from_env() {
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
    // `services::channels::supervisor` for the full rationale.
    let channel_control_data = {
        use services::channels::registry::RegistryDeps;
        use services::channels::supervisor;
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

    // Boot the periodic-task scheduler. Each `spawn_periodic` returns
    // a JoinHandle we intentionally drop — tasks live for the runtime's
    // lifetime, and cancellation (when we add a SIGTERM handler) fires
    // through the shared `scheduler_shutdown` token.
    //
    // See `services::scheduled_jobs` for the concrete job bodies and
    // `services::scheduler` for the rationale behind rolling this
    // rather than pulling in `tokio-cron-scheduler` (short version:
    // idiomatic Rust + avoids documented footguns in that crate).
    let scheduler_status = services::scheduler::status_registry();
    let scheduler_shutdown = tokio_util::sync::CancellationToken::new();
    {
        use services::scheduled_jobs as jobs;
        use services::scheduler::spawn_periodic;
        use std::time::Duration;

        // Hourly: prune expired auth sessions + refresh tokens so the
        // tables don't accrete dead rows indefinitely.
        let p = pool.clone();
        spawn_periodic(
            "active_sessions.cleanup",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::cleanup_expired_sessions(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "refresh_tokens.cleanup",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::cleanup_expired_refresh_tokens(p.clone()),
        );

        // Every 30 min: Microsoft Graph delta sync (skipped at runtime
        // when the provider isn't configured).
        let p = pool.clone();
        spawn_periodic(
            "msgraph.delta_sync",
            Duration::from_secs(30 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::msgraph_delta_sync(p.clone()),
        );

        // Daily: roll the sync_actions / audit_log monthly partitions
        // forward. Inserts after the last provisioned month would
        // otherwise fail; the substrate migration provides the first
        // four months and this job extends the window.
        let p = pool.clone();
        spawn_periodic(
            "sync.partition_provisioner",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::ensure_sync_partitions(p.clone()),
        );

        // Daily: prune CSP violation reports past the retention
        // window so a noisy reporter (browser extension etc.) can't
        // grow the table unbounded. Retention defaults to 30 days.
        let p = pool.clone();
        spawn_periodic(
            "csp_reports.prune",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_csp_reports(p.clone()),
        );

        // Hourly: prune Idempotency-Key cache rows past the retention
        // horizon (default 24h). M5 provisioning retries either
        // succeed in minutes or escalate to ops; old keys serve no
        // purpose and shouldn't accumulate. Hourly instead of daily
        // because the table is small and the sweep is cheap.
        let p = pool.clone();
        spawn_periodic(
            "idempotency_keys.prune",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_idempotency_keys(p.clone()),
        );

        // Every 60s: sweep expired leases on the outbound email queue.
        // A worker that crashed mid-send leaves a row in `sending` with
        // a 5-minute lease; the sweep moves expired-lease rows back to
        // `failed` so the next claim cycle picks them up. Cheap (the
        // partial outbound_emails_lease_idx keeps the scan tiny).
        let p = pool.clone();
        spawn_periodic(
            "outbound_emails.sweep_leases",
            Duration::from_secs(60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::sweep_outbound_email_leases(p.clone()),
        );

        // Hourly: re-verify workspace DKIM sending domains. A `verified`
        // domain whose published record disappears flips back to `pending`
        // so sends fall back to the platform identity instead of shipping
        // mail that fails DKIM/DMARC at the receiver.
        let p = pool.clone();
        spawn_periodic(
            "dkim.reverify_domains",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::reverify_dkim_domains(p.clone()),
        );

        // Daily: row-level retention for security_events and
        // webhook_deliveries; partition-level retention for audit_log
        // and sync_actions. Partition drops use DETACH CONCURRENTLY so
        // the parent's lock window stays at SHARE UPDATE EXCLUSIVE
        // (W6a's lock-friendly attach in reverse).
        let p = pool.clone();
        spawn_periodic(
            "security_events.prune",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_security_events(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "webhook_deliveries.prune",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_webhook_deliveries(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "audit_log.drop_old_partitions",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_audit_log_partitions(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "sync_actions.drop_old_partitions",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_sync_actions_partitions(p.clone()),
        );

        // Daily: hard-delete soft-deleted users past the retention
        // window. The cascade in repository::users::purge_user is
        // destructive (comments / tickets get NULLed or reassigned)
        // so the grace window (default 30 days, set via
        // NOSDESK_USER_PURGE_GRACE_DAYS) is the operator-facing
        // safety net. The worker re-tries failed rows on the next
        // tick rather than aborting the sweep.
        let p = pool.clone();
        let s = search_service.get_ref().clone();
        spawn_periodic(
            "users.purge_soft_deleted",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::purge_soft_deleted_users(p.clone(), s.clone()),
        );

        // Daily: hard-delete workspaces whose archive grace window
        // (default 30 days, `WORKSPACE_HARD_DELETE_GRACE_DAYS` to
        // override) has elapsed. Mirrors purge_soft_deleted_users;
        // BYPASSRLS role for the cross-tenant cascade.
        let p = pool.clone();
        spawn_periodic(
            "workspaces.purge_archived",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::purge_archived_workspaces(p.clone()),
        );

        // Daily: backfill avatar thumbnails missing on disk or unset in
        // the DB. Restores rebuild thumbnails eagerly (they're not in the
        // backup payload); this is the idempotent safety net that heals
        // any later drift and does no work in steady state.
        let p = pool.clone();
        spawn_periodic(
            "users.backfill_thumbnails",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::backfill_user_thumbnails(p.clone()),
        );

        // Every 60s: detect SLA breaches and flip the pill live. Scans
        // the materialised `sla_response_target_at` /
        // `sla_resolution_target_at` columns (cheap partial indexes),
        // atomically stamps `*_breached_at`, emits a ticket.sla_updated
        // sync_action (pill repaint) plus a ticket.sla_breached
        // sync_action (webhook delivery via the outbox), and notifies the
        // assignee + watchers via NotificationService.
        let p = pool.clone();
        let ns = notification_service.clone().into_inner();
        spawn_periodic(
            "sla.detect_breaches",
            Duration::from_secs(60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::detect_sla_breaches(p.clone(), ns.clone()),
        );

        // Daily: remind borrowers about device loans due back soon or
        // overdue, via NotificationService. Advisory-locked; scans all
        // workspaces under BYPASSRLS and stamps each loan so a reminder
        // fires once.
        let p = pool.clone();
        let ns = notification_service.clone().into_inner();
        spawn_periodic(
            "asset_loans.due_reminders",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::loan_due_reminders(p.clone(), ns.clone()),
        );

        info!("scheduler: periodic jobs spawned");
    }
    let scheduler_status_data = web::Data::new(scheduler_status);

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
    let cors_allowlist = crate::utils::cors_allowlist::CorsAllowlist::new(
        std::iter::once(frontend_url.as_str()).chain(additional_origins.iter().map(|s| s.as_str())),
        tenant_domain.as_deref(),
    );
    info!(
        host_count = cors_allowlist.exact_count(),
        tenant_domain = ?tenant_domain,
        "CORS allowlist initialised"
    );
    // Install as the process-wide allowlist. Both the CORS layer below and
    // the collab WebSocket origin guard (handlers::collaboration) read it
    // via `cors_allowlist::global()` — no per-App `web::Data` to wire (or
    // forget in tests). The set runs before any worker starts.
    crate::utils::cors_allowlist::set_global(cors_allowlist);

    // Cloned out before the factory closure moves `yjs_app_state` in, so
    // the shutdown handler below can flush collab docs on SIGTERM.
    let yjs_for_shutdown = yjs_app_state.clone();
    let collab_shutdown_token = scheduler_shutdown.clone();

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
                "X-CSRF-Token"
            ])
            .expose_headers(vec!["content-disposition"])
            .supports_credentials()
            .max_age(3600);

        // Configure JSON payload limits for file uploads
        let json_config = web::JsonConfig::default()
            .limit(max_payload_size);

        // Configure multipart form limits for file uploads
        let multipart_config = web::FormConfig::default()
            .limit(max_payload_size);

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
            .app_data(auth_limiter_data.clone())
            .app_data(web::Data::new(pool.clone()))
            .app_data(yjs_app_state.clone())
            .app_data(sse_state.clone())
            .app_data(system_state.clone())
            .app_data(storage_data.clone())
            .app_data(notification_service.clone())
            .app_data(outbound_resolver_data.clone())
            .app_data(channel_control_data.clone())
            .app_data(scheduler_status_data.clone())
            .app_data(webhook_service.clone())
            .app_data(plugin_proxy_service.clone())
            .app_data(registry_cache.clone())
            .app_data(search_service.clone())
            .app_data(analytics_cache.clone())
            .app_data(inbound_s3_data.clone())
            .app_data(json_config)
            .app_data(multipart_config)

            // === PUBLIC ROUTES (NO AUTHENTICATION REQUIRED) ===
            .route("/health", web::get().to(handlers::health::liveness))
            .route("/readiness", web::get().to(handlers::health::readiness))

            // Forwarded frontend console logs: gated inside the handler unless
            // debug build or `NOSDESK_ALLOW_FRONTEND_DEBUG_LOGS=1`. Rate limited
            // and JSON-capped separately from global config.
            .service(
                web::resource("/api/debug/frontend-logs")
                    .app_data(web::JsonConfig::default().limit(512 * 1024))
                    // Own bucket (felogs:{ip}); shadows the app-level limiter
                    // for this resource so a log flood can't drain the public
                    // / auth quotas that gate login and MFA.
                    .app_data(frontend_logs_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .route(web::post().to(handlers::debug::receive_frontend_logs)),
            )

            // Public file serving - ONLY user avatars, banners, thumbs, and branding (no sensitive data)
            .route("/uploads/users/avatars/{filename:.*}", web::get().to(handlers::serve_public_file))
            .route("/uploads/users/banners/{filename:.*}", web::get().to(handlers::serve_public_file))
            .route("/uploads/users/thumbs/{filename:.*}", web::get().to(handlers::serve_public_file))
            .route("/uploads/branding/{filename:.*}", web::get().to(handlers::branding::serve_branding_file))

            // Public branding config (needed for favicon/logo before login)
            .route("/api/branding", web::get().to(handlers::branding::get_public_branding))

            // Public instance config (routing topology) read at SPA startup
            .route("/api/config", web::get().to(handlers::app_config::get_public_config))

            // Inbound-email webhook (hosted): AWS SNS POSTs here when SES
            // receives forwarded mail. Unauthenticated by necessity (SNS is
            // server-to-server); the handler verifies the SNS signature.
            .route("/api/inbound/email", web::post().to(handlers::inbound_email::receive))

            // Public (unauthenticated) guest endpoints — feature flags checked per handler.
            // Tighter JSON payload limit here than the app-wide default: the only
            // write endpoint in this scope is /tickets with a 10KB description cap,
            // so 32KB is generous headroom without expanding the DoS surface.
            .service(
                web::scope("/api/public")
                    .app_data(web::JsonConfig::default().limit(32 * 1024))
                    .app_data(public_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .route("/settings", web::get().to(handlers::guest::get_public_settings))
                    .route("/tickets", web::post().to(handlers::guest::submit_guest_ticket))
                    .route("/tickets/{token}", web::get().to(handlers::guest::get_guest_ticket_status))
                    .route("/files/temp", web::post().to(handlers::guest::upload_guest_attachment))
                    .route("/docs", web::get().to(handlers::guest::list_public_docs))
                    .route("/docs/search", web::get().to(handlers::guest::search_public_docs))
                    .route("/docs/{slug}", web::get().to(handlers::guest::get_public_doc))
                    // One-click email unsubscribe (RFC 8058): POST is the mail
                    // client's automatic one-click, GET the human-followed link.
                    .route("/unsubscribe", web::post().to(handlers::unsubscribe::one_click))
                    .route("/unsubscribe", web::get().to(handlers::unsubscribe::landing))
            )

            // Public WebSocket for collaboration (auth handled in WebSocket handler)
            .service(
                web::scope("/api/collaboration")
                    .configure(handlers::collaboration::config)
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
                    .app_data(public_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .route(web::post().to(handlers::csp_reports::report_violation))
            )

            // Public file serving with token-based auth for attachments
            // Authenticated, workspace-scoped tenant file serving. Each route
            // is wrapped with dual_auth_middleware (cookie or Bearer, and a
            // workspace-membership gate); the handlers add a per-ticket
            // visibility check via TenantConn so a caller can only read files
            // for tickets they can see in their own workspace.
            .route("/api/files/tickets/{ticket_id}/notes/{filename:.*}", web::get().to(handlers::serve_ticket_note_image).wrap(actix_web::middleware::from_fn(middleware::dual_auth_middleware)))
            .route("/api/files/tickets/{filename:.*}", web::get().to(handlers::serve_ticket_file).wrap(actix_web::middleware::from_fn(middleware::dual_auth_middleware)))
            .route("/api/files/assets/{asset_id}/media/{filename:.*}", web::get().to(handlers::asset_media::serve_asset_media_file).wrap(actix_web::middleware::from_fn(middleware::dual_auth_middleware)))
            .route("/api/files/temp/{filename:.*}", web::get().to(handlers::serve_temp_file).wrap(actix_web::middleware::from_fn(middleware::dual_auth_middleware)))

            // SSE endpoints (with custom token-based auth)
            // Main event stream for all real-time updates (tickets, documentation, devices, etc.)
            .route("/api/events/stream", web::get().to(handlers::sse::sse_events_stream))
            .route("/api/events/status", web::get().to(handlers::sse::sse_status))

            // Customer portal sign-in (public by design). Unauthenticated;
            // the workspace is resolved from the portal origin by the app-wide
            // workspace-context middleware. Rate-limited like the auth scope to
            // bound magic-link sends.
            .service(
                web::scope("/api/portal/auth")
                    .wrap(RateLimiter::default())
                    .route(
                        "/magic-link",
                        web::post().to(handlers::portal::request_magic_link),
                    )
                    .route(
                        "/callback",
                        web::get().to(handlers::portal::magic_link_callback),
                    ),
            )
            // Authenticated customer portal API. Registered AFTER the public
            // `/api/portal/auth` scope so the sign-in routes match there first.
            // The portal session is ownership-scoped: every handler reads its
            // own tickets only, RLS-pinned to the origin's workspace.
            .service(
                web::scope("/api/portal")
                    .wrap(actix_web::middleware::from_fn(
                        handlers::portal::portal_auth_middleware,
                    ))
                    .route("/tickets", web::get().to(handlers::portal::list_my_tickets))
                    .route("/tickets", web::post().to(handlers::portal::create_my_ticket))
                    .route(
                        "/tickets/{id}",
                        web::get().to(handlers::portal::get_my_ticket),
                    )
                    .route(
                        "/tickets/{id}/comments",
                        web::post().to(handlers::portal::reply_to_my_ticket),
                    ),
            )
            // Authentication routes (public by design)
            .service(
                web::scope("/api/auth")
                    // Brute-force defence: auth endpoints get the
                    // stricter public limit (shadows the app-level auth
                    // limiter for this scope). See security-audit-2026-06.
                    .app_data(public_limiter_data.clone())
                    .wrap(RateLimiter::default())
                    .service({
                        // Restore moved to `nosdesk-cli db restore` (AUD-005).
                        // The self-serve initial-admin route only exists in
                        // self-hosted mode; in hosted mode the control plane
                        // provisions admins, so /setup/admin is never mounted
                        // (a request to it 404s rather than 403s).
                        let setup = web::scope("/setup")
                            .route("/status", web::get().to(handlers::check_setup_status));
                        match workspace_config.mode {
                            crate::middleware::DeploymentMode::SelfHosted => setup
                                .route("/admin", web::post().to(handlers::setup_initial_admin)),
                            crate::middleware::DeploymentMode::Hosted => setup,
                        }
                    })
                                            .route("/login", web::post().to(handlers::login))
                        .route("/logout", web::post().to(handlers::logout))
                        .route("/mfa-login", web::post().to(handlers::mfa_login))
                        .route("/recovery-login", web::post().to(handlers::recovery_login))
                        .route("/mfa-setup-login", web::post().to(handlers::mfa_setup_login))
                        .route("/mfa-enable-login", web::post().to(handlers::mfa_enable_login))
                        .route("/passkey-setup-login/start", web::post().to(handlers::start_passkey_setup_login))
                        .route("/passkey-setup-login/finish", web::post().to(handlers::finish_passkey_setup_login))
                        .route("/refresh", web::post().to(handlers::refresh_token))
                    // Password reset routes (public, rate-limited)
                    .route("/password-reset/request", web::post().to(handlers::password_reset::request_password_reset))
                    .route("/password-reset/complete", web::post().to(handlers::password_reset::reset_password_with_token))
                    // Invitation routes (public)
                    .route("/invitation/validate", web::post().to(handlers::invitation::validate_invitation))
                    .route("/invitation/accept", web::post().to(handlers::invitation::accept_invitation))
                    .route("/providers", web::get().to(handlers::get_enabled_auth_providers))
                    .route("/oauth/authorize", web::post().to(handlers::oauth_authorize))
                    .route("/oauth/callback", web::get().to(handlers::oauth_callback))
                    .route("/oauth/logout", web::post().to(handlers::oauth_logout))
                    // Protected auth routes
                    .route("/me", web::get().to(handlers::get_current_user).wrap(actix_web::middleware::from_fn(cookie_auth_middleware)))
                    .route("/change-password", web::post().to(handlers::change_password).wrap(actix_web::middleware::from_fn(cookie_auth_middleware)))
                    .route("/oauth/connect", web::post().to(handlers::oauth_connect).wrap(actix_web::middleware::from_fn(cookie_auth_middleware)))
                    // Session Management endpoints
                    .service(
                        web::scope("/sessions")
                            .wrap(actix_web::middleware::from_fn(cookie_auth_middleware))
                            .route("", web::get().to(handlers::get_user_sessions))
                            .route("/others", web::delete().to(handlers::revoke_all_other_sessions))
                            .route("/{id}", web::delete().to(handlers::revoke_session))
                    )
                    // MFA (Multi-Factor Authentication) endpoints
                    .service(
                        web::scope("/mfa")
                            .wrap(actix_web::middleware::from_fn(cookie_auth_middleware))
                            .route("/setup", web::post().to(handlers::mfa_setup))
                            .route("/verify-setup", web::post().to(handlers::mfa_verify_setup))
                            .route("/enable", web::post().to(handlers::mfa_enable))
                            .route("/disable", web::post().to(handlers::mfa_disable))
                            .route("/regenerate-backup-codes", web::post().to(handlers::mfa_regenerate_backup_codes))
                            .route("/status", web::get().to(handlers::mfa_status))
                    )
                    // Passkey login endpoints (public - no auth required)
                    .route("/passkeys/login/start", web::post().to(handlers::start_passkey_login))
                    .route("/passkeys/login/finish", web::post().to(handlers::finish_passkey_login))
                    // Passkey management endpoints (protected - requires cookie auth)
                    .service(
                        web::scope("/passkeys")
                            .wrap(actix_web::middleware::from_fn(cookie_auth_middleware))
                            .route("/register/start", web::post().to(handlers::start_passkey_registration))
                            .route("/register/finish", web::post().to(handlers::finish_passkey_registration))
                            .route("", web::get().to(handlers::list_passkeys))
                            .route("/{credential_id}", web::patch().to(handlers::rename_passkey))
                            .route("/{credential_id}", web::delete().to(handlers::delete_passkey))
                    )
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
                    .wrap(actix_web::middleware::from_fn(middleware::idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(backend::extractors::platform_auth_middleware))
                    .route(
                        "/workspaces/create",
                        web::post().to(handlers::internal_workspaces::create_workspace),
                    )
                    .route(
                        "/workspaces/{slug}/upsert_projected_user",
                        web::post().to(handlers::internal_workspaces::upsert_projected_user),
                    )
                    .route(
                        "/workspaces/{slug}/seat_limit",
                        web::post().to(handlers::internal_workspaces::set_seat_limit),
                    )
                    .route(
                        "/workspaces/{slug}/members/set_role",
                        web::post().to(handlers::internal_workspaces::set_member_role),
                    )
                    .route(
                        "/workspaces/{slug}/custom-domain",
                        web::patch().to(handlers::internal_workspaces::set_custom_domain),
                    )
                    .route(
                        "/workspaces/{slug}/provisioning",
                        web::get().to(handlers::internal_workspaces::workspace_provisioning),
                    )
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
                    // required scope. See docs/plans/api-token-scopes-plan.md.
                    .wrap(actix_web::middleware::from_fn(
                        middleware::token_scope::token_scope_middleware,
                    ))
                    .wrap(actix_web::middleware::from_fn(middleware::dual_auth_middleware))

                    // Authentication Provider management (admin only) - simplified for environment-based config
                    .route("/admin/auth/providers", web::get().to(handlers::get_auth_providers))

                    // Email configuration (admin only) - environment-based config
                    .route("/admin/email/config", web::get().to(handlers::email::get_email_config))
                    .route("/admin/email/test", web::post().to(handlers::email::send_test_email))
                    // Per-workspace verified sending domain (DKIM)
                    .route("/admin/email/outbound", web::get().to(handlers::workspace_email::get_outbound))
                    .route("/admin/email/outbound", web::delete().to(handlers::workspace_email::reset))
                    .route("/admin/email/outbound/domain", web::put().to(handlers::workspace_email::set_domain))
                    .route("/admin/email/outbound/verify", web::post().to(handlers::workspace_email::verify_domain))
                    .route("/admin/email/outbound/dns-check", web::get().to(handlers::workspace_email::dns_check))
                    .route("/admin/email/outbound/test", web::post().to(handlers::workspace_email::test_send))

                    // System information (admin only)
                    .route("/admin/system/info", web::get().to(handlers::system::get_system_info))
                    .route("/admin/system/updates", web::get().to(handlers::system::check_system_updates))

                    // Branding configuration (admin only)
                    .route("/admin/branding/config", web::get().to(handlers::branding::get_branding_config))
                    .route("/admin/branding/config", web::patch().to(handlers::branding::update_branding_config))

                    // Workspace lifecycle (admin only, Phase 4 W1).
                    // GET / POST list + create; per-id rename /
                    // archive / restore / hard-delete. Hard-delete
                    // requires ?confirm=<slug> matching the row.
                    .route("/admin/workspaces", web::get().to(handlers::admin_workspaces::list_workspaces))
                    .route("/admin/workspaces", web::post().to(handlers::admin_workspaces::create_workspace))
                    .route("/admin/edition", web::get().to(handlers::admin_workspaces::get_edition))
                    .route("/admin/workspaces/{id}", web::patch().to(handlers::admin_workspaces::rename_workspace))
                    .route("/admin/workspaces/{id}", web::delete().to(handlers::admin_workspaces::hard_delete_workspace))
                    .route("/admin/workspaces/{id}/archive", web::post().to(handlers::admin_workspaces::archive_workspace))
                    .route("/admin/workspaces/{id}/restore", web::post().to(handlers::admin_workspaces::restore_workspace))

                    // Workspace membership (admin only, Phase 4 W3).
                    // Cross-tenant membership management for the
                    // platform admin. Workspace-admin self-service
                    // member management is a separate route under
                    // /api/workspaces/{id}/members (later workstream).
                    .route("/admin/workspaces/{id}/members", web::get().to(handlers::admin_workspaces::list_members))
                    .route("/admin/workspaces/{id}/members", web::post().to(handlers::admin_workspaces::add_member))
                    .route("/admin/workspaces/{id}/members/{user_uuid}", web::patch().to(handlers::admin_workspaces::update_member_role))
                    .route("/admin/workspaces/{id}/members/{user_uuid}", web::delete().to(handlers::admin_workspaces::remove_member))

                    // Caller's own workspace memberships — backs
                    // the frontend workspace switcher. Authenticated,
                    // no admin gate. Phase 4 W3.
                    .route("/me/workspaces", web::get().to(handlers::admin_workspaces::list_my_workspaces))

                    // Tenant self-serve member management for the caller's
                    // OWN workspace (context-scoped, no id in the path).
                    // Workspace-admin gated; distinct from the platform-
                    // admin operator console at /admin/workspaces/{id}/members.
                    // Phase 4 W3 / P1.3.
                    .route("/workspace/members", web::get().to(handlers::workspace_members::list_members))
                    .route("/workspace/members/{user_uuid}", web::patch().to(handlers::workspace_members::update_member_role))
                    .route("/workspace/members/{user_uuid}", web::delete().to(handlers::workspace_members::remove_member))

                    // Guest access controls (admin only)
                    .route("/admin/guest-settings", web::get().to(handlers::guest_settings::get_guest_settings))
                    .route("/admin/guest-settings", web::patch().to(handlers::guest_settings::update_guest_settings))

                    // Multi-channel ingestion (admin only). Phase-1 UI
                    // surfaces only the email_imap provider; backend
                    // is generic over channel rows.
                    .route("/admin/channels", web::get().to(handlers::channels::list_channels))
                    .route("/admin/channels", web::post().to(handlers::channels::create_channel))
                    .route("/admin/channels/{id}", web::get().to(handlers::channels::get_channel))
                    .route("/admin/channels/{id}", web::patch().to(handlers::channels::update_channel))
                    .route("/admin/channels/{id}", web::delete().to(handlers::channels::delete_channel))
                    .route("/admin/channels/{id}/credentials", web::delete().to(handlers::channels::clear_credential))
                    .route("/admin/channels/{id}/test-connection", web::post().to(handlers::channels::test_connection))

                    // Periodic-task scheduler status (read-only).
                    .route("/admin/scheduler/status", web::get().to(handlers::scheduler::get_status))

                    // CSP violation reports — admins inspect what's
                    // being blocked under the live policy. Drives
                    // safe rollouts: tighten in report-only mode,
                    // observe here, then enforce.
                    .route("/admin/csp-reports", web::get().to(handlers::csp_reports::list_violations))

                    // Audit log — admin-gated read of audit_log rows
                    // produced by the per-table triggers. See
                    // handlers/audit_log.rs for query shape and
                    // 2026-05-11-210000_attach_audit_tier1 for the
                    // tables that participate.
                    .route("/admin/audit-log", web::get().to(handlers::audit_log::list))
                    // Item C/W5: unified audit feed over all three
                    // substrates (sync_actions + security_events +
                    // audit_log), gated by the audit:read scope and the
                    // admin / audit-reviewer roles.
                    .route("/admin/audit", web::get().to(handlers::audit::list))
                    .route(
                        "/admin/audit/export",
                        web::get().to(handlers::audit::export),
                    )

                    // Outbound email queue — Item J Pass 1 admin
                    // surface. List rows + per-row actions (retry now,
                    // cancel) for operators to investigate why a
                    // notification didn't fire.
                    .route("/admin/email-queue", web::get().to(handlers::email_queue::list))
                    .route("/admin/email-queue/stats", web::get().to(handlers::email_queue::stats))

                    // Inbound dead-letter log — platform-admin only. Cross-tenant
                    // operator view of mail forwarded to an unknown token.
                    .route("/admin/inbound/dead-letters", web::get().to(handlers::inbound_dead_letters::list))
                    .route("/admin/email-queue/{id}/retry", web::post().to(handlers::email_queue::retry_now))
                    .route("/admin/email-queue/{id}/cancel", web::post().to(handlers::email_queue::cancel))
                    .route("/admin/email-suppressions", web::get().to(handlers::email_suppressions::list))
                    .route("/admin/email-suppressions", web::post().to(handlers::email_suppressions::create))
                    .route("/admin/email-suppressions/{email}", web::delete().to(handlers::email_suppressions::delete))

                    // Consolidated dashboard stats. The frontend's
                    // widget registry derives an `include` set
                    // from the user's active widgets and passes
                    // it here so we only compute what's about to
                    // be displayed. Replaces three independent
                    // full-list ticket fetches per dashboard load.
                    .route("/dashboard/stats", web::get().to(handlers::dashboard::get_stats))
                    // Analytics endpoints (Phase 4). Both run via
                    // TenantConn so the RLS policy on tickets
                    // restricts the aggregation source rows to the
                    // active workspace before any aggregation
                    // happens. Inputs are validated against the same
                    // allowlists the chart-config form enforces
                    // client-side.
                    .route("/dashboard/kpi", web::get().to(handlers::analytics::get_kpi))
                    .route(
                        "/dashboard/kpi-summary",
                        web::get().to(handlers::analytics::get_kpi_summary),
                    )
                    .route(
                        "/dashboard/timeseries",
                        web::get().to(handlers::analytics::get_timeseries),
                    )
                    .route(
                        "/dashboard/breakdown",
                        web::get().to(handlers::analytics::get_breakdown),
                    )
                    .route(
                        "/dashboard/heatmap",
                        web::get().to(handlers::analytics::get_heatmap),
                    )
                    .route(
                        "/dashboard/leaderboard",
                        web::get().to(handlers::analytics::get_leaderboard),
                    )
                    .route(
                        "/dashboard/audit-annotations",
                        web::get().to(handlers::analytics::get_audit_annotations),
                    )

                    // Canned responses — reads open to any authenticated
                    // user (composer picker); writes admin-only. The
                    // insertions log endpoint is open (any user can
                    // record their own picker use, workspace-local);
                    // the starter catalog is admin-only since it's
                    // only consumed by the admin create flow.
                    .route("/canned-responses", web::get().to(handlers::canned_responses::list_canned))
                    .route("/canned-responses/{id}/insertions", web::post().to(handlers::canned_responses::record_insertion))
                    .route("/admin/canned-responses", web::post().to(handlers::canned_responses::create_canned))
                    // Sits at its own path (not nested under
                    // /admin/canned-responses/) so the `{id}` route
                    // below can't shadow it via wildcard matching.
                    // Actix's `.route()` chain registers each call as
                    // a separate Resource and the wildcard sibling
                    // wins over the literal sibling on the same path
                    // level; keeping the starters at a distinct path
                    // sidesteps that ambiguity entirely.
                    .route("/admin/canned-response-starters", web::get().to(handlers::canned_responses::starter_catalog))
                    .route("/admin/canned-responses/{id}", web::patch().to(handlers::canned_responses::update_canned))
                    .route("/admin/canned-responses/{id}", web::delete().to(handlers::canned_responses::delete_canned))

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
                    .route("/admin/rule-starters", web::get().to(handlers::rules::list_starter_catalog))
                    .route("/rules", web::get().to(handlers::rules::list_rules))
                    .route("/rules", web::post().to(handlers::rules::create_rule))
                    .route("/rules/{id}/apply", web::post().to(handlers::rules::apply_rule))
                    .route("/rules/{id}/state", web::patch().to(handlers::rules::transition_state))
                    .route("/rules/{id}/versions", web::get().to(handlers::rules::list_rule_versions))
                    .route("/rules/{rule_id}/versions/{version}", web::get().to(handlers::rules::get_rule_version))
                    .route("/rules/{id}", web::get().to(handlers::rules::get_rule))
                    .route("/rules/{id}", web::put().to(handlers::rules::update_rule))
                    .route("/rules/{id}", web::delete().to(handlers::rules::delete_rule))
                    .route("/rule-applications", web::get().to(handlers::rules::list_rule_applications))
                    .route("/rule-applications/{id}", web::get().to(handlers::rules::get_rule_application))

                    .route("/admin/branding/image", web::post().to(handlers::branding::upload_branding_image))
                    .route("/admin/branding/image", web::delete().to(handlers::branding::delete_branding_image))

                    // Workflow states — read open to any authenticated user;
                    // admin writes (create, rename, recolor, reorder,
                    // promote default, archive) for the customisation UI.
                    // Sync engine — local-first protocol behind /api/sync.
                    // bootstrap streams a per-user NDJSON snapshot; delta
                    // pulls incremental changes from a sync_id cursor;
                    // push applies an array of optimistic transactions.
                    // Saved views (workspace / project / private)
                    .route("/saved-views", web::get().to(handlers::saved_views::list))
                    .route("/saved-views", web::post().to(handlers::saved_views::create))
                    .route("/saved-views/{uuid}", web::get().to(handlers::saved_views::get_one))
                    .route("/saved-views/{uuid}", web::patch().to(handlers::saved_views::patch))
                    .route("/saved-views/{uuid}", web::delete().to(handlers::saved_views::delete))

                    // Cycles. Project-scoped list + create live under
                    // /projects/{id}/cycles; per-cycle ops use the
                    // cycle uuid for stable bookmarkable URLs.
                    .route("/cycles", web::get().to(handlers::cycles::list_workspace))
                    .route("/projects/{project_id}/cycles", web::get().to(handlers::cycles::list))
                    .route("/projects/{project_id}/cycles", web::post().to(handlers::cycles::create))
                    .route("/cycles/{uuid}", web::get().to(handlers::cycles::get_one))
                    .route("/cycles/{uuid}", web::patch().to(handlers::cycles::patch))
                    .route("/cycles/{uuid}", web::delete().to(handlers::cycles::archive))
                    .route("/cycles/{uuid}/complete", web::post().to(handlers::cycles::complete))
                    .route("/cycles/{uuid}/stats", web::get().to(handlers::cycles::stats))
                    .route("/cycles/{uuid}/burnup", web::get().to(handlers::cycles::burnup))
                    .route("/cycles/{uuid}/tickets", web::get().to(handlers::cycles::tickets))
                    .route("/cycles/{uuid}/tickets/{ticket_id}", web::post().to(handlers::cycles::add_ticket))
                    .route("/cycles/{uuid}/tickets/{ticket_id}", web::delete().to(handlers::cycles::remove_ticket))

                    // SLA admin: policies + working calendars CRUD.
                    // Reads open to any authenticated user (the
                    // admin UI lists them); writes gate on admin
                    // inside the handler.
                    .route("/admin/sla/policies", web::get().to(handlers::sla::list_policies))
                    .route("/admin/sla/policies", web::post().to(handlers::sla::create_policy))
                    // Static path must precede /{id} so "matches" isn't parsed as an id.
                    .route("/admin/sla/policies/matches", web::get().to(handlers::sla::policy_match_counts))
                    .route("/admin/sla/policies/{id}", web::patch().to(handlers::sla::update_policy))
                    .route("/admin/sla/policies/{id}", web::delete().to(handlers::sla::delete_policy))
                    .route("/admin/sla/calendars", web::get().to(handlers::sla::list_calendars))
                    .route("/admin/sla/calendars", web::post().to(handlers::sla::create_calendar))
                    .route("/admin/sla/calendars/{id}", web::patch().to(handlers::sla::update_calendar))
                    .route("/admin/sla/calendars/{id}", web::delete().to(handlers::sla::delete_calendar))
                    .route("/admin/sla/calendars/{id}/holidays", web::get().to(handlers::sla::list_holidays))
                    .route("/admin/sla/calendars/{id}/holidays", web::post().to(handlers::sla::create_holiday))
                    .route("/admin/sla/holidays/{id}", web::delete().to(handlers::sla::delete_holiday))
                    .route("/tickets/{id}/sla/explain", web::get().to(handlers::sla::explain_for_ticket))
                    .route("/sla/workspace-summary", web::get().to(handlers::sla::workspace_summary))

                    .route("/sync/bootstrap", web::get().to(handlers::sync::bootstrap::bootstrap))
                    .route("/sync/delta", web::get().to(handlers::sync::delta::delta))
                    .route("/sync/push", web::post().to(handlers::sync::push::push))
                    .route("/sync/schema", web::get().to(handlers::sync::schema::schema))

                    .route("/workflow-states", web::get().to(handlers::workflow_states::list))
                    .route("/admin/workflow-states", web::post().to(handlers::workflow_states::create))
                    .route("/admin/workflow-states/{id}", web::patch().to(handlers::workflow_states::patch))
                    .route("/admin/workflow-states/{id}", web::delete().to(handlers::workflow_states::archive))

                    // Asset-kind registry. Admin-only CRUD over the
                    // runtime discriminator table that drives
                    // `assets.kind` validation.
                    .route("/admin/asset-kinds/{id}", web::get().to(handlers::asset_kinds::get))
                    .route("/admin/asset-kinds", web::post().to(handlers::asset_kinds::create))
                    // Usage stat. Sits at /usage suffix on a numeric
                    // {id} path so the wildcard-vs-literal shadowing
                    // hazard (see project_actix_route_shadowing memory)
                    // doesn't apply: both segments are constrained
                    // numerics, no literal-vs-wildcard ambiguity.
                    .route("/admin/asset-kinds/{id}/usage", web::get().to(handlers::asset_kinds::usage))
                    .route("/admin/asset-kinds/{id}", web::put().to(handlers::asset_kinds::update))
                    .route("/admin/asset-kinds/{id}", web::delete().to(handlers::asset_kinds::delete))

                    // Bulk CSV import — admin-only. The template
                    // route is declared before /{id} so the literal
                    // "template" path segment doesn't get matched
                    // as a job-uuid by the catch-all.
                    .route("/admin/import", web::post().to(handlers::imports::upload))
                    .route("/admin/import/template/{type}", web::get().to(handlers::imports::template))
                    .route("/admin/import/{id}/commit", web::post().to(handlers::imports::commit))
                    .route("/admin/import/{id}", web::get().to(handlers::imports::get_job))

                    // Feature flags — staged rollout machinery (Phase 0 of the
                    // projects-v2 architecture). Read endpoint open to any
                    // authenticated user; write endpoints admin-only.
                    .route("/feature-flags", web::get().to(handlers::feature_flags::get_my_flags))
                    .route("/admin/feature-flags", web::patch().to(handlers::feature_flags::patch_workspace_flag))
                    .route("/admin/feature-flags", web::put().to(handlers::feature_flags::put_workspace_flags))
                    .route("/admin/feature-flags/users/{uuid}", web::patch().to(handlers::feature_flags::patch_user_override))

                    // Backup and restore (admin only)
                    .route("/admin/backup/export", web::post().to(handlers::backup::start_export))
                    .route("/admin/backup/jobs", web::get().to(handlers::backup::get_jobs))
                    .route("/admin/backup/jobs/{id}", web::get().to(handlers::backup::get_job))
                    .route("/admin/backup/jobs/{id}", web::delete().to(handlers::backup::delete_job))
                    .route("/admin/backup/download/{id}", web::get().to(handlers::backup::download_backup))
                    .route("/admin/backup/restore/upload", web::post().to(handlers::backup::upload_restore))
                    .route("/admin/backup/restore/{id}/preview", web::get().to(handlers::backup::preview_restore))
                    .route("/admin/backup/restore/{id}/execute", web::post().to(handlers::backup::execute_restore))

                    // Microsoft Graph API endpoints
                    .route("/auth/microsoft/graph", web::post().to(handlers::process_graph_request))
                    .service(
                        web::scope("/msgraph")
                            .route("/request", web::post().to(handlers::process_graph_request))
                            .route("/users", web::get().to(handlers::get_graph_users))
                            .route("/devices", web::get().to(handlers::get_graph_devices))
                            .route("/groups", web::get().to(handlers::get_graph_groups))
                            .route("/directory-objects", web::get().to(handlers::get_graph_directory_objects))
                    )

                    // Microsoft Graph Integration endpoints
                    .service(
                        web::scope("/integrations/graph")
                            // Auth already handled by parent /api scope
                            .route("/config", web::get().to(handlers::get_config_validation))
                            .route("/status", web::get().to(handlers::get_connection_status))
                            .route("/test", web::post().to(handlers::test_connection))
                            .route("/sync", web::post().to(handlers::sync_data))
                            .route("/progress/{session_id}", web::get().to(handlers::get_sync_progress_endpoint))
                            .route("/active-syncs", web::get().to(handlers::get_active_syncs))
                            .route("/last-sync", web::get().to(handlers::get_last_sync))
                            .route("/cancel/{session_id}", web::post().to(handlers::cancel_sync_session))
                            .route("/entra-object-id/{azure_ad_device_id}", web::get().to(handlers::get_entra_object_id))
                    )

                    // File upload endpoint
                    .route("/upload", web::post().to(handlers::upload_files))

                    // ===== SERVER-SENT EVENTS (SSE) =====
                    .route("/events/token", web::post().to(handlers::sse::get_sse_token))

                    // ===== SEARCH =====
                    .route("/search", web::get().to(handlers::search::search))
                    .route("/search/rebuild", web::post().to(handlers::search::rebuild_index))
                    .route("/search/stats", web::get().to(handlers::search::get_stats))

                    // ===== NOTIFICATIONS =====
                    .route("/notifications", web::get().to(handlers::notifications::get_notifications))
                    .route("/notifications/count", web::get().to(handlers::notifications::get_unread_count))
                    .route("/notifications/read", web::post().to(handlers::notifications::mark_notifications_read))
                    .route("/notifications/read-all", web::post().to(handlers::notifications::mark_all_notifications_read))
                    .route("/notifications/preferences", web::get().to(handlers::notifications::get_preferences))
                    .route("/notifications/preferences", web::put().to(handlers::notifications::update_preference))
                    .route("/notifications/delete", web::post().to(handlers::notifications::delete_notifications))

                    // ===== BUG REPORTS =====
                    // User-submitted from the in-app "Report a problem" modal.
                    // Workspace-scoped via the standard TenantConn flow.
                    .route("/bug-reports", web::post().to(handlers::bug_reports::create_bug_report))

                    // ===== TICKET MANAGEMENT =====
                    .route("/tickets", web::get().to(handlers::get_tickets))
                    .route("/tickets/paginated", web::get().to(handlers::get_paginated_tickets))
                    .route("/tickets/recent", web::get().to(handlers::get_recent_tickets))
                    .route("/tickets", web::post().to(handlers::create_ticket))
                    .route("/tickets/empty", web::post().to(handlers::create_empty_ticket))
                    .route("/tickets/bulk", web::post().to(handlers::bulk_tickets))
                    // Literal /tickets/merge before /tickets/{id}; there is no
                    // /tickets/{id} POST, so no wildcard can absorb it.
                    .route("/tickets/merge", web::post().to(handlers::ticket_merge::merge_tickets))
                    .route("/tickets/{id}/merge-history", web::get().to(handlers::ticket_merge::get_merge_history))
                    .route("/tickets/{ticket_id}/rule-applications", web::get().to(handlers::rules::list_ticket_rule_applications))
                    .route("/tickets/{id}/applicable-actions", web::get().to(handlers::rules::list_applicable_actions))
                    .route("/tickets/{id}", web::get().to(handlers::get_ticket))
                    .route("/tickets/{id}", web::put().to(handlers::update_ticket))
                    .route("/tickets/{id}", web::patch().to(handlers::update_ticket_partial))
                    .route("/tickets/{id}", web::delete().to(handlers::delete_ticket))
                    .route("/tickets/{id}/view", web::post().to(handlers::record_ticket_view))
                    .route("/tickets/{id}/view", web::delete().to(handlers::remove_recent_ticket))
                    .route("/tickets/{id}/activity", web::get().to(handlers::get_ticket_activity))
                    .route("/tickets/{id}/loans", web::get().to(handlers::asset_loans::list_for_ticket))
                    .route("/tickets/{id}/field-preview", web::post().to(handlers::preview_ticket_field))
                    .route("/tickets/{id}/tags", web::put().to(handlers::tags::set_ticket_tags))
                    .route("/tickets/{id}/watchers", web::get().to(handlers::ticket_watchers::list_watchers))
                    .route("/tickets/{id}/watch", web::post().to(handlers::ticket_watchers::watch_ticket))
                    .route("/tickets/{id}/watch", web::delete().to(handlers::ticket_watchers::unwatch_ticket))
                    .route("/tickets/{id}/watch/me", web::get().to(handlers::ticket_watchers::my_watch_state))
                    .route("/tickets/{id}/watch/preferences", web::patch().to(handlers::ticket_watchers::update_my_watch_preferences))
                    .route("/tags", web::get().to(handlers::tags::list_tags))
                    .route("/tags", web::post().to(handlers::tags::create_tag))
                    .route("/tags/{id}", web::patch().to(handlers::tags::update_tag))
                    .route("/tags/{id}", web::delete().to(handlers::tags::archive_tag))
                    .route("/import/file", web::post().to(handlers::import_tickets_from_json))
                    .route("/import/json", web::post().to(handlers::import_tickets_from_json_string))
                    .route("/tickets/{ticket_id}/link/{linked_ticket_id}", web::post().to(handlers::link_tickets))
                    .route("/tickets/{ticket_id}/unlink/{linked_ticket_id}", web::delete().to(handlers::unlink_tickets))
                    .route("/tickets/{ticket_id}/assets/{asset_id}", web::post().to(handlers::add_device_to_ticket))
                    .route("/tickets/{ticket_id}/assets/{asset_id}", web::delete().to(handlers::remove_device_from_ticket))
                    .route("/tickets/{id}/asset-usage", web::get().to(handlers::asset_usage::list_for_ticket))
                    .route("/tickets/{ticket_id}/comments", web::get().to(handlers::get_comments_by_ticket_id))
                    .route("/tickets/{ticket_id}/comments", web::post().to(handlers::add_comment_to_ticket))
                    .route("/tickets/{ticket_id}/notes/images", web::post().to(handlers::upload_ticket_note_image))
                    .route("/comments/{id}", web::delete().to(handlers::delete_comment))
                    .route("/comments/{id}/raw.eml", web::get().to(handlers::get_comment_raw_eml))
                    // Image proxy for inbound email rendering. Path-positional
                    // {sig}/{encoded_url} keeps the URL self-describing and
                    // cache-friendly (browsers cache by full URL). HMAC sig
                    // is derived from JWT_SECRET; see handlers::image_proxy.
                    .route("/image-proxy/{sig}/{encoded_url}", web::get().to(handlers::image_proxy::proxy_image))
                    .route("/comments/{comment_id}/attachments", web::post().to(handlers::add_attachment_to_comment))
                    .route("/attachments/{id}", web::delete().to(handlers::delete_attachment))

                    // ===== PROJECT MANAGEMENT =====
                    .route("/projects", web::get().to(handlers::get_all_projects))
                    .route("/projects", web::post().to(handlers::create_project))
                    .route("/projects/{id}", web::get().to(handlers::get_project))
                    .route("/projects/{id}", web::put().to(handlers::update_project))
                    .route("/projects/{id}", web::delete().to(handlers::delete_project))
                    .route("/projects/{id}/tickets", web::get().to(handlers::get_project_tickets))
                    .route("/projects/{id}/dependencies", web::get().to(handlers::projects::get_project_dependencies))
                    // Literal /tickets/new before the /tickets/{ticket_id}
                    // POST so the wildcard can't absorb the quick-add create.
                    .route("/projects/{project_id}/tickets/new", web::post().to(handlers::projects::create_ticket_in_project))
                    .route("/projects/{project_id}/tickets/{ticket_id}", web::post().to(handlers::add_ticket_to_project))
                    .route("/projects/{project_id}/tickets/{ticket_id}", web::delete().to(handlers::remove_ticket_from_project))
                    .route("/projects/{id}/tickets/order", web::put().to(handlers::update_ticket_order))

                    // ===== GROUP DETAIL (All authenticated users) =====
                    .route("/groups/details/{uuid}", web::get().to(handlers::groups::get_group_details))

                    // ===== GROUP MANAGEMENT (Admin Only) =====
                    .route("/groups", web::get().to(handlers::groups::get_all_groups))
                    .route("/groups", web::post().to(handlers::groups::create_group))
                    .route("/groups/{id}", web::get().to(handlers::groups::get_group))
                    .route("/groups/{id}", web::put().to(handlers::groups::update_group))
                    .route("/groups/{id}", web::delete().to(handlers::groups::delete_group))
                    .route("/groups/{id}/members", web::put().to(handlers::groups::set_group_members))
                    .route("/groups/{id}/assets", web::put().to(handlers::groups::set_group_devices))
                    .route("/groups/{id}/includes", web::get().to(handlers::groups::get_group_includes))
                    .route("/groups/{id}/includes", web::put().to(handlers::groups::set_group_includes))
                    .route("/groups/{id}/unmanage", web::post().to(handlers::groups::unmanage_group))
                    .route("/users/{uuid}/groups", web::get().to(handlers::groups::get_user_groups))
                    .route("/users/{uuid}/groups", web::put().to(handlers::groups::set_user_groups))

                    // ===== CATEGORY MANAGEMENT =====
                    // User-facing categories endpoint (respects visibility)
                    .route("/categories", web::get().to(handlers::categories::get_categories))
                    // Admin category endpoints
                    .route("/admin/categories", web::get().to(handlers::categories::get_all_categories_admin))
                    .route("/admin/categories", web::post().to(handlers::categories::create_category))
                    .route("/admin/categories/reorder", web::put().to(handlers::categories::reorder_categories))
                    .route("/admin/categories/{id}", web::get().to(handlers::categories::get_category_admin))
                    .route("/admin/categories/{id}", web::put().to(handlers::categories::update_category))
                    .route("/admin/categories/{id}", web::delete().to(handlers::categories::delete_category))
                    .route("/admin/categories/{id}/visibility", web::put().to(handlers::categories::set_category_visibility))

                    // ===== ASSIGNMENT RULES MANAGEMENT =====
                    .route("/admin/assignment-rules", web::get().to(handlers::assignment_rules::get_all_rules))
                    .route("/admin/assignment-rules", web::post().to(handlers::assignment_rules::create_rule))
                    .route("/admin/assignment-rules/reorder", web::put().to(handlers::assignment_rules::reorder_rules))
                    .route("/admin/assignment-rules/preview", web::post().to(handlers::assignment_rules::preview_assignment))
                    .route("/admin/assignment-rules/logs", web::get().to(handlers::assignment_rules::get_assignment_logs))
                    .route("/admin/assignment-rules/{id}", web::get().to(handlers::assignment_rules::get_rule))
                    .route("/admin/assignment-rules/{id}", web::patch().to(handlers::assignment_rules::update_rule))
                    .route("/admin/assignment-rules/{id}", web::delete().to(handlers::assignment_rules::delete_rule))

                    // ===== API TOKEN MANAGEMENT =====
                    .route("/admin/api-tokens", web::get().to(handlers::api_tokens::list_api_tokens))
                    .route("/admin/api-tokens", web::post().to(handlers::api_tokens::create_api_token))
                    .route("/admin/api-tokens/{uuid}", web::get().to(handlers::api_tokens::get_api_token))
                    .route("/admin/api-tokens/{uuid}", web::delete().to(handlers::api_tokens::revoke_api_token))

                    // ===== WEBHOOK MANAGEMENT =====
                    .route("/admin/webhooks", web::get().to(handlers::webhooks::list_webhooks))
                    .route("/admin/webhooks", web::post().to(handlers::webhooks::create_webhook))
                    .route("/admin/webhooks/event-types", web::get().to(handlers::webhooks::get_event_types))
                    .route("/admin/webhooks/{uuid}", web::get().to(handlers::webhooks::get_webhook))
                    .route("/admin/webhooks/{uuid}", web::put().to(handlers::webhooks::update_webhook))
                    .route("/admin/webhooks/{uuid}", web::delete().to(handlers::webhooks::delete_webhook))
                    .route("/admin/webhooks/{uuid}/deliveries", web::get().to(handlers::webhooks::get_deliveries))
                    .route("/admin/webhooks/{uuid}/test", web::post().to(handlers::webhooks::test_webhook))

                    // ===== PLUGIN MANAGEMENT (Admin) =====
                    // Literal paths MUST be registered before the
                    // `{uuid}` paths: actix matches in registration
                    // order and a `web::Path<Uuid>` extractor on the
                    // generic route would 400 trying to parse
                    // "registry" or "install" as a UUID, never
                    // falling through to the literal handlers.
                    .route("/admin/plugins", web::get().to(handlers::plugins::list_plugins))
                    .route("/admin/plugins/config", web::get().to(handlers::plugins::get_admin_config))
                    .route("/admin/plugins/signing-overview", web::get().to(handlers::plugins::get_signing_overview))
                    .route("/admin/plugins/install", web::post().to(handlers::plugins::install_plugin_from_zip))
                    .route("/admin/plugins/registry", web::get().to(handlers::plugins::get_registry))
                    .route("/admin/plugins/registry/refresh", web::post().to(handlers::plugins::refresh_registry))
                    .route("/admin/plugins/registry/install", web::post().to(handlers::plugins::install_from_registry))
                    .route("/admin/plugins/{uuid}", web::get().to(handlers::plugins::get_plugin))
                    .route("/admin/plugins/{uuid}", web::put().to(handlers::plugins::update_plugin))
                    .route("/admin/plugins/{uuid}", web::delete().to(handlers::plugins::uninstall_plugin))
                    .route("/admin/plugins/{uuid}/settings", web::get().to(handlers::plugins::get_plugin_settings))
                    .route("/admin/plugins/{uuid}/settings", web::post().to(handlers::plugins::set_plugin_setting))
                    .route("/admin/plugins/{uuid}/settings/{key}", web::delete().to(handlers::plugins::delete_plugin_setting))
                    .route("/admin/plugins/{uuid}/activity", web::get().to(handlers::plugins::get_plugin_activity))

                    // ===== PLUGIN API (For plugins to use) =====
                    .route("/plugins/enabled", web::get().to(handlers::plugins::list_enabled_plugins))
                    .route("/plugins/{uuid}/bundle", web::get().to(handlers::plugins::serve_plugin_bundle))
                    .route("/plugins/{uuid}/icon", web::get().to(handlers::plugins::serve_plugin_icon))
                    .route("/plugins/{uuid}/storage/{key}", web::get().to(handlers::plugins::get_plugin_storage))
                    .route("/plugins/{uuid}/storage", web::post().to(handlers::plugins::set_plugin_storage))
                    .route("/plugins/{uuid}/storage/{key}", web::delete().to(handlers::plugins::delete_plugin_storage))
                    .route("/plugins/{uuid}/proxy", web::post().to(handlers::plugins::proxy_plugin_request))

                    // ===== PLUGIN EVENT EMISSION =====
                    // Authenticated user iframes can call this to record a
                    // plugin-emitted event in sync_actions with
                    // actor_kind = 'plugin'. Aggregate must be a registered
                    // variant; plugins extend behaviour through event_type
                    // strings, not by inventing new aggregates.
                    .route("/plugins/{uuid}/events", web::post().to(handlers::plugin_events::emit_plugin_event))

                    // ===== PLUGIN COLLECTIONS =====
                    .route("/plugins/{uuid}/collections", web::get().to(handlers::plugin_collections::list_collections))
                    .route("/plugins/{uuid}/collections/{name}", web::get().to(handlers::plugin_collections::get_collection_schema))
                    .route("/plugins/{uuid}/collections/{name}/rows", web::get().to(handlers::plugin_collections::list_collection_rows))
                    .route("/plugins/{uuid}/collections/{name}/rows", web::post().to(handlers::plugin_collections::create_collection_row))
                    .route("/plugins/{uuid}/collections/{name}/rows/{row_uuid}", web::get().to(handlers::plugin_collections::get_collection_row))
                    .route("/plugins/{uuid}/collections/{name}/rows/{row_uuid}", web::put().to(handlers::plugin_collections::update_collection_row))
                    .route("/plugins/{uuid}/collections/{name}/rows/{row_uuid}", web::delete().to(handlers::plugin_collections::delete_collection_row))

                    // ===== USER MANAGEMENT =====
                    // Note: Specific routes must come BEFORE generic {uuid} routes to avoid matching conflicts
                    .route("/users", web::get().to(handlers::get_users))
                    .route("/users/paginated", web::get().to(handlers::get_paginated_users))
                    .route("/users/batch", web::post().to(handlers::get_users_batch))
                    .route("/users/bulk", web::post().to(handlers::bulk_users))
                    .route("/users/cleanup-images", web::post().to(handlers::cleanup_stale_images))
                    .route("/users/regenerate-thumbnails", web::post().to(handlers::regenerate_avatar_thumbnails))
                    .route("/files/cleanup-temp", web::post().to(handlers::cleanup_temp_files))
                    .route("/users/auth-identities", web::get().to(handlers::get_user_auth_identities))
                    .route("/users/auth-identities/{id}", web::delete().to(handlers::delete_user_auth_identity))
                    .route("/users", web::post().to(handlers::create_user))
                    .route("/users/{uuid}", web::get().to(handlers::get_user_by_uuid))
                    .route("/users/{uuid}", web::put().to(handlers::update_user_by_uuid))
                    .route("/users/{uuid}", web::delete().to(handlers::delete_user))
                    .route("/users/{uuid}/restore", web::post().to(handlers::restore_user))
                    .route("/users/{uuid}/purge", web::delete().to(handlers::purge_user_now))
                    .route("/users/{uuid}/image", web::post().to(handlers::upload_user_image))
                    .route("/users/{uuid}/emails", web::get().to(handlers::get_user_emails))
                    .route("/users/{uuid}/emails", web::post().to(handlers::add_user_email))
                    .route("/users/{uuid}/emails/{email_id}", web::put().to(handlers::update_user_email))
                    .route("/users/{uuid}/emails/{email_id}", web::delete().to(handlers::delete_user_email))
                    // User contact profile (standard cols + custom-field values).
                    .route("/users/{uuid}/profile-fields", web::get().to(handlers::user_contact::get_user_profile_fields))
                    .route("/users/{uuid}/profile-fields", web::put().to(handlers::user_contact::set_user_profile_fields))
                    // Multi-valued contact: phones + addresses (self or admin; synced rows read-only).
                    .route("/users/{uuid}/phones", web::get().to(handlers::user_contact::list_user_phones))
                    .route("/users/{uuid}/phones", web::post().to(handlers::user_contact::add_user_phone))
                    .route("/users/{uuid}/phones/{id}", web::put().to(handlers::user_contact::update_user_phone))
                    .route("/users/{uuid}/phones/{id}", web::delete().to(handlers::user_contact::delete_user_phone))
                    .route("/users/{uuid}/addresses", web::get().to(handlers::user_contact::list_user_addresses))
                    .route("/users/{uuid}/addresses", web::post().to(handlers::user_contact::add_user_address))
                    .route("/users/{uuid}/addresses/{id}", web::put().to(handlers::user_contact::update_user_address))
                    .route("/users/{uuid}/addresses/{id}", web::delete().to(handlers::user_contact::delete_user_address))
                    // Workspace user custom-field schema (read staff, write admin).
                    .route("/admin/user-fields", web::get().to(handlers::user_contact::get_user_field_schema))
                    .route("/admin/user-fields", web::put().to(handlers::user_contact::set_user_field_schema))
                    // Per-workspace LDAP/directory config (admin-gated in the handlers).
                    .route("/ldap/settings", web::get().to(handlers::ldap_integration::get_ldap_settings))
                    .route("/ldap/settings", web::put().to(handlers::ldap_integration::set_ldap_settings))
                    .route("/ldap/presets", web::get().to(handlers::ldap_integration::get_ldap_presets))
                    .route("/ldap/test-connection", web::post().to(handlers::ldap_integration::test_ldap_connection))
                    .route("/ldap/sync", web::post().to(handlers::ldap_integration::run_ldap_sync))
                    .route("/users/{uuid}/with-emails", web::get().to(handlers::get_user_with_emails))
                    .route("/users/{uuid}/profile", web::get().to(handlers::users::get_user_profile_bundle))
                    .route("/users/{uuid}/auth-identities", web::get().to(handlers::get_user_auth_identities_by_uuid))
                    .route("/users/{uuid}/auth-identities/{id}", web::delete().to(handlers::delete_user_auth_identity_by_uuid))
                    .route("/users/{uuid}/resend-invitation", web::post().to(handlers::resend_invitation))
                    .route("/users/{uuid}/security-info", web::get().to(handlers::get_user_security_info))
                    .route("/users/{uuid}/reset-password", web::post().to(handlers::admin_reset_user_password))
                    .route("/users/{uuid}/disable-mfa", web::post().to(handlers::admin_disable_user_mfa))
                    .route("/users/{uuid}/passkeys/{credential_id}", web::delete().to(handlers::admin_delete_user_passkey))

                    // ===== ASSET MANAGEMENT =====
                    .route("/assets", web::get().to(handlers::get_all_devices))
                    .route("/assets/paginated", web::get().to(handlers::get_paginated_devices))
                    .route("/assets/paginated/excluding", web::get().to(handlers::get_paginated_devices_excluding))
                    .route("/assets/bulk", web::post().to(handlers::bulk_devices))
                    .route("/assets/calendar-overlay", web::get().to(handlers::assets::calendar_overlay))
                    .route("/assets/export", web::get().to(handlers::export_assets))
                    .route("/assets/locations", web::get().to(handlers::get_asset_locations))
                    .route("/assets/grouping-dataset", web::get().to(handlers::assets::asset_grouping_dataset))
                    .route("/assets/rollouts", web::post().to(handlers::assets::create_rollout))
                    // Read-only kind registry for the asset create/edit
                    // pickers. Technician-gated (matches asset create);
                    // admin CRUD lives under /admin/asset-kinds.
                    .route("/asset-kinds", web::get().to(handlers::asset_kinds::list_for_picker))
                    // Asset model catalog (manufacturers + models). Technician-gated CRUD.
                    .route("/manufacturers", web::get().to(handlers::manufacturers::list))
                    .route("/manufacturers", web::post().to(handlers::manufacturers::create))
                    .route("/manufacturers/{id:\\d+}", web::get().to(handlers::manufacturers::get))
                    .route("/manufacturers/{id:\\d+}", web::put().to(handlers::manufacturers::update))
                    .route("/manufacturers/{id:\\d+}", web::delete().to(handlers::manufacturers::delete))
                    .route("/asset-models", web::get().to(handlers::asset_models::list))
                    .route("/asset-models", web::post().to(handlers::asset_models::create))
                    .route("/asset-models/{id:\\d+}", web::get().to(handlers::asset_models::get))
                    .route("/asset-models/{id:\\d+}", web::put().to(handlers::asset_models::update))
                    .route("/asset-models/{id:\\d+}", web::delete().to(handlers::asset_models::delete))
                    .route("/assets", web::post().to(handlers::create_device))
                    .route("/assets/empty", web::post().to(handlers::create_empty_device))
                    .route("/assets/{id:\\d+}/model", web::post().to(handlers::set_asset_model))
                    .route("/assets/{id:\\d+}/model", web::delete().to(handlers::clear_asset_model))
                    .route("/assets/{id:\\d+}", web::get().to(handlers::get_device_by_id))
                    .route("/assets/{id:\\d+}", web::put().to(handlers::update_device))
                    .route("/assets/{id:\\d+}", web::delete().to(handlers::delete_device))
                    .route("/assets/{id:\\d+}/unmanage", web::post().to(handlers::unmanage_device))
                    .route("/assets/{id:\\d+}/lifecycle", web::get().to(handlers::asset_lifecycle::list_for_asset))
                    .route("/assets/{id:\\d+}/lifecycle", web::post().to(handlers::asset_lifecycle::create_transition))
                    .route("/assets/{id:\\d+}/loans", web::get().to(handlers::asset_loans::list_for_asset))
                    .route("/assets/{id:\\d+}/loans", web::post().to(handlers::asset_loans::issue))
                    .route("/assets/{id:\\d+}/loans/{loan_id:\\d+}", web::patch().to(handlers::asset_loans::edit))
                    .route("/assets/{id:\\d+}/loans/{loan_id:\\d+}/return", web::post().to(handlers::asset_loans::return_loan))
                    .route("/assets/{id:\\d+}/media", web::get().to(handlers::asset_media::list_for_asset))
                    .route("/assets/{id:\\d+}/media", web::post().to(handlers::asset_media::upload_for_asset))
                    .route("/assets/{id:\\d+}/media/{media_id}", web::put().to(handlers::asset_media::update_media))
                    .route("/assets/{id:\\d+}/media/{media_id}", web::delete().to(handlers::asset_media::delete_media))
                    .route("/assets/{id:\\d+}/usage", web::post().to(handlers::asset_usage::record))
                    .route("/assets/{id:\\d+}/usage", web::get().to(handlers::asset_usage::list_for_asset))
                    .route("/assets/{id:\\d+}/audit", web::post().to(handlers::asset_audits::record))
                    .route("/assets/{id:\\d+}/audits", web::get().to(handlers::asset_audits::list_for_asset))
                    .route("/users/{uuid}/assets", web::get().to(handlers::get_user_devices))

                    // ===== DOCUMENTATION SYSTEM =====
                    // Literal paths MUST come before {id} wildcard to avoid being swallowed
                    .route("/documentation/pages", web::get().to(handlers::get_documentation_pages))
                    .route("/documentation/pages", web::post().to(handlers::create_documentation_page))
                    .route("/documentation/pages/export", web::get().to(handlers::export_documentation_pages))
                    .route("/documentation/pages/top-level", web::get().to(handlers::get_top_level_documentation_pages))
                    .route("/documentation/pages/uncollected", web::get().to(handlers::documentation_collections::get_uncollected_pages))
                    .route("/documentation/pages/reorder", web::post().to(handlers::reorder_pages))
                    .route("/documentation/pages/move", web::post().to(handlers::move_page_to_parent))
                    .route("/documentation/pages/ordered/top-level", web::get().to(handlers::get_ordered_top_level_pages))
                    .route("/documentation/pages/ordered/parent/{parent_id}", web::get().to(handlers::get_ordered_pages_by_parent_id))
                    .route("/documentation/pages/parent/{parent_id}", web::get().to(handlers::get_documentation_pages_by_parent_id))
                    .route("/documentation/pages/uuid/{uuid}/content", web::get().to(handlers::get_documentation_page_content_by_uuid))
                    .route("/documentation/pages/slug/{slug}", web::get().to(handlers::get_documentation_page_by_slug))
                    .route("/documentation/pages/slug/{slug}/with-children", web::get().to(handlers::get_documentation_page_by_slug_with_children))
                    .route("/documentation/pages/archived", web::get().to(handlers::get_archived_pages))
                    .route("/documentation/pages/trash", web::get().to(handlers::get_trashed_pages))
                    .route("/documentation/starred", web::get().to(handlers::get_starred_pages))
                    // {id} wildcard routes AFTER all literal paths
                    .route("/documentation/pages/{id}", web::get().to(handlers::get_documentation_page))
                    .route("/documentation/pages/{id}", web::put().to(handlers::update_documentation_page))
                    .route("/documentation/pages/{id}", web::delete().to(handlers::delete_documentation_page))
                    .route("/documentation/pages/{id}/with-children-by-parent", web::get().to(handlers::get_page_with_children_by_parent_id))
                    .route("/documentation/pages/{id}/with-ordered-children", web::get().to(handlers::get_page_with_ordered_children))
                    .route("/documentation/pages/{id}/embeddings", web::put().to(handlers::sync_page_embeddings))
                    .route("/documentation/pages/{id}/export/markdown", web::get().to(handlers::export_page_as_markdown))
                    .route("/documentation/pages/{id}/collections", web::get().to(handlers::documentation_collections::get_collections_for_page))
                    .route("/documentation/pages/{id}/collections", web::put().to(handlers::documentation_collections::set_page_collections))
                    .route("/documentation/pages/{id}/visibility", web::get().to(handlers::get_page_visibility))
                    .route("/documentation/pages/{id}/visibility", web::put().to(handlers::set_page_visibility))
                    .route("/documentation/pages/{id}/subscription", web::get().to(handlers::get_page_subscription))
                    .route("/documentation/pages/{id}/subscribe", web::post().to(handlers::subscribe_to_page))
                    .route("/documentation/pages/{id}/subscribe", web::delete().to(handlers::unsubscribe_from_page))
                    .route("/documentation/pages/{id}/starred", web::get().to(handlers::get_page_starred))
                    .route("/documentation/pages/{id}/star", web::post().to(handlers::star_page))
                    .route("/documentation/pages/{id}/star", web::delete().to(handlers::unstar_page))
                    .route("/documentation/pages/{id}/restore", web::post().to(handlers::restore_page))
                    .route("/documentation/pages/{id}/permanent", web::delete().to(handlers::permanently_delete_page))
                    .route("/tickets/{ticket_id}/documentation", web::get().to(handlers::get_documentation_pages_by_ticket_id))
                    .route("/tickets/{ticket_id}/documentation/create", web::post().to(handlers::create_documentation_page_from_ticket))
                    .route("/tickets/{ticket_id}/flag-as-gap", web::post().to(handlers::knowledge_gaps::flag_ticket_as_gap))
                    .route("/tickets/{ticket_id}/flag-as-gap", web::delete().to(handlers::knowledge_gaps::unflag_ticket_as_gap))
                    .route("/knowledge-gaps", web::get().to(handlers::knowledge_gaps::list_knowledge_gaps))
                    .route("/knowledge-gaps/detect-clusters", web::post().to(handlers::knowledge_gaps::detect_clusters))
                    .route("/knowledge-gaps/detect-failed-searches", web::post().to(handlers::knowledge_gaps::detect_failed_searches))
                    .route("/knowledge-gaps/detect-stale-docs", web::post().to(handlers::knowledge_gaps::detect_stale_docs))
                    .route("/knowledge-gaps/{id}", web::get().to(handlers::knowledge_gaps::get_knowledge_gap))
                    .route("/knowledge-gaps/{id}/dismiss", web::post().to(handlers::knowledge_gaps::dismiss_knowledge_gap))
                    .route("/knowledge-gaps/{id}/resolve", web::post().to(handlers::knowledge_gaps::resolve_knowledge_gap))
                    .route("/tickets/{ticket_id}/documentation-pages", web::get().to(handlers::list_ticket_doc_links))
                    .route("/documentation/pages/{id}/tickets", web::get().to(handlers::list_page_tickets))
                    .route("/documentation/pages/{id}/tickets", web::post().to(handlers::create_page_ticket_link))
                    .route("/documentation/pages/{page_id}/tickets/{ticket_id}", web::delete().to(handlers::delete_page_ticket_link))
                    .route("/documentation/pages/{id}/verification", web::post().to(handlers::verify_page))
                    .route("/documentation/pages/{id}/verification", web::delete().to(handlers::unverify_page))

                    // ===== DOCUMENTATION COLLECTIONS =====
                    .route("/documentation/collections", web::get().to(handlers::documentation_collections::get_collections))
                    .route("/documentation/collections", web::post().to(handlers::documentation_collections::create_collection))
                    .route("/documentation/collections/reorder", web::post().to(handlers::documentation_collections::reorder_collections))
                    .route("/documentation/collections/slug/{slug}", web::get().to(handlers::documentation_collections::get_collection_by_slug))
                    .route("/documentation/collections/{id}", web::get().to(handlers::documentation_collections::get_collection))
                    .route("/documentation/collections/{id}", web::put().to(handlers::documentation_collections::update_collection))
                    .route("/documentation/collections/{id}", web::delete().to(handlers::documentation_collections::delete_collection))
                    .route("/documentation/collections/{id}/pages", web::post().to(handlers::documentation_collections::add_page_to_collection))
                    .route("/documentation/collections/{id}/pages/{page_id}", web::delete().to(handlers::documentation_collections::remove_page_from_collection))
                    .route("/documentation/collections/{id}/visibility", web::get().to(handlers::documentation_collections::get_collection_visibility))
                    .route("/documentation/collections/{id}/visibility", web::put().to(handlers::documentation_collections::set_collection_visibility))
                    .route("/documentation/collections/{id}/page-overrides", web::get().to(handlers::documentation_collections::get_page_overrides_in_collection))
                    .route("/documentation/{id}", web::put().to(handlers::update_documentation_page))
                    .route("/documentation/{id}", web::delete().to(handlers::delete_documentation_page))
            )

            // Catch-all for /uploads/* that wasn't matched by the explicit
            // public-asset routes above. Previously this served any object
            // unauthenticated; it now 404s. Tenant files are served only via
            // the authenticated, workspace-scoped /api/files/* routes.
            .route("/uploads/{path:.*}", web::get().to(handlers::reject_legacy_upload_path))

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
            )
    })
    .bind((host, port))?
    .disable_signals()
    .run();

    // Graceful shutdown. We own the signals (`disable_signals` above) so
    // the collab flush completes before the server tears down: on SIGTERM
    // (Fly deploy / `docker stop`) or SIGINT (Ctrl-C), flush in-memory
    // documents to durable storage, cancel background jobs, then stop the
    // server gracefully. Without this, a deploy drops every edit made
    // since the last periodic save.
    let server_handle = server.handle();
    actix_web::rt::spawn(async move {
        await_shutdown_signal().await;
        info!("Shutdown signal received; flushing collaborative documents before stop");
        yjs_for_shutdown
            .flush_all_dirty(std::time::Duration::from_secs(4))
            .await;
        collab_shutdown_token.cancel();
        server_handle.stop(true).await;
    });

    server.await
}

#[cfg(test)]
mod tests {
    use super::spa_shell_path;
    use backend::middleware::DeploymentMode;

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
