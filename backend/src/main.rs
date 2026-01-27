mod db;
mod extractors;
mod handlers;
mod models;
mod repository;
mod schema;
mod config_utils;
mod utils;
mod middleware;
mod oidc;
mod services;

use actix_cors::Cors;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder, Error, HttpMessage};
use actix_web::dev::{ServiceRequest, ServiceResponse, fn_service};
use actix_files::Files;
use actix_limitation::{Limiter, RateLimiter};
use dotenv::dotenv;
use std::env;
use std::time::Duration;
use tracing::{info, warn, error, debug};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use utils::storage::{get_storage_config, create_storage};
use utils::redis_yjs_cache::create_redis_cache;

async fn health_check() -> impl Responder {
    HttpResponse::Ok().body("Helpdesk API is running!")
}

// Custom rate limit error handler
#[allow(dead_code)]
async fn rate_limit_handler() -> impl Responder {
    HttpResponse::TooManyRequests().json(serde_json::json!({
        "status": "error",
        "message": "Rate limit exceeded. Please slow down your requests.",
        "code": "RATE_LIMIT_EXCEEDED",
        "retry_after_seconds": 60
    }))
}

/// Handle missing assets in development mode
/// When frontend rebuilds, old asset hashes become invalid - this helps developers
fn handle_missing_asset(path: &str) -> HttpResponse {
    let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

    if environment != "production" {
        // Log helpful message for developers
        log::warn!(
            "Asset not found: {path} - Frontend may have been rebuilt. Try refreshing the page."
        );

        // For JS files, return code that triggers a page reload
        if path.ends_with(".js") {
            return HttpResponse::Ok()
                .content_type("application/javascript")
                .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
                .body(r#"console.warn('[Nosdesk Dev] Asset hash mismatch - frontend was rebuilt. Reloading...');setTimeout(()=>location.reload(),500);"#);
        }

        // For CSS files, return empty CSS with proper MIME type
        if path.ends_with(".css") {
            return HttpResponse::Ok()
                .content_type("text/css")
                .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
                .body("/* Asset hash mismatch - frontend was rebuilt */");
        }
    }

    HttpResponse::NotFound().finish()
}

/// Serve the SPA index.html for all non-API routes (SPA routing)
/// This follows Actix best practices for SPA applications
async fn serve_spa(_req: HttpRequest) -> HttpResponse {
    // Check if this is a static asset request (has file extension and not HTML)
    let path = _req.path();

    // If it's a hashed asset request (contains hash pattern), handle as missing asset
    if path.starts_with("/assets/") && path.contains('-') {
        return handle_missing_asset(path);
    }

    // If it's a static asset request, return 404 to let the Files service handle it
    if path.contains('.') && !path.ends_with(".html") {
        return HttpResponse::NotFound().finish();
    }

    // For all other routes (SPA routes), serve index.html
    // Use no-cache so browsers always check for updated versions after deployments
    match tokio::fs::read("./public/index.html").await {
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
            let environment = std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
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

// Cookie-based authentication middleware
async fn cookie_auth_middleware(
    req: actix_web::dev::ServiceRequest,
    next: actix_web::middleware::Next<impl actix_web::body::MessageBody>,
) -> Result<actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>, Error> {
    let pool = req.app_data::<web::Data<crate::db::Pool>>()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("Database pool not found"))?;

    let mut conn = pool.get()
        .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;

    use crate::utils::jwt::JwtUtils;

    // Debug logging
    let cookie_names: Vec<String> = req.cookies()
        .map(|jar| jar.iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    debug!(path = %req.path(), cookies = ?cookie_names, "Cookie auth middleware processing request");

    // Extract access token from httpOnly cookie
    let token = req.cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| {
            warn!(path = %req.path(), "Cookie auth: no access_token cookie found");
            actix_web::error::ErrorUnauthorized("Authentication required")
        })?;

    debug!("Cookie auth: validating token from cookie");

    // Validate token and get claims
    let (claims, _user) = JwtUtils::authenticate_with_token(token.value(), &mut conn).await
        .map_err(|err| {
            error!(error = ?err, "Cookie auth: token validation failed");
            actix_web::error::ErrorUnauthorized("Invalid or expired token")
        })?;

    info!(user = %claims.sub, "Cookie auth: user authenticated successfully");

    // Insert claims into request extensions
    req.extensions_mut().insert(claims);

    // Continue to the handler
    next.call(req).await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Early startup logging before tracing is initialized
    // Using eprintln! here since tracing isn't set up yet
    eprintln!("BACKEND STARTING - Current dir: {:?}", std::env::current_dir());
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

    // Initialize tracing/logging subsystem with better error handling
    let log_level = env::var("RUST_LOG")
        .unwrap_or_else(|_| {
            if env::var("ENVIRONMENT").unwrap_or_default() == "production" {
                "info".to_string()
            } else {
                "debug".to_string()
            }
        });

    // Ignore tracing init errors (might already be initialized by cargo watch)
    // Docker best practice: log to stdout (not files), Docker daemon handles log forwarding
    let _ = tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_target(true)
                .with_line_number(true)
                .with_writer(std::io::stdout)
        )
        .with(EnvFilter::new(&log_level))
        .try_init();

    debug!("Tracing initialized, continuing startup");

    // === SECURITY STARTUP VALIDATION ===
    info!("Starting Nosdesk API Server");
    info!(log_level = %log_level, "Log level configured");
    
    // Debug: Print some environment variables to see what's available
    debug!("Environment check:");
    debug!("  DATABASE_URL is set: {}", env::var("DATABASE_URL").is_ok());
    debug!("  JWT_SECRET is set: {}", env::var("JWT_SECRET").is_ok());
    debug!("  HOST: {}", env::var("HOST").unwrap_or("NOT_SET".to_string()));
    debug!("  PORT: {}", env::var("PORT").unwrap_or("NOT_SET".to_string()));
    
    // Get environment early for validation
    let environment = env::var("ENVIRONMENT").unwrap_or("development".to_string());
    info!("Environment: {}", environment);

    // Validate that JWT_SECRET is set and secure
    let _jwt_secret = match std::env::var("JWT_SECRET") {
        Ok(secret) => {
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
        },
        Err(e) => {
            error!(error = %e, "JWT_SECRET environment variable must be set");
            error!("Generate a secure key with: openssl rand -base64 32");
            std::process::exit(1);
        }
    };
    info!("JWT_SECRET validated");
    
    // Validate that MFA_ENCRYPTION_KEY is set for production
    if environment == "production" {
        match std::env::var("MFA_ENCRYPTION_KEY") {
            Ok(key) => {
                if key.len() != 64 {
                    error!("MFA_ENCRYPTION_KEY must be exactly 64 hex characters (32 bytes)");
                    error!("Generate a secure key with: openssl rand -hex 32");
                    std::process::exit(1);
                }
                // Validate it's valid hex
                if hex::decode(&key).is_err() {
                    error!("MFA_ENCRYPTION_KEY must be valid hexadecimal");
                    error!("Generate a secure key with: openssl rand -hex 32");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                error!(error = %e, "MFA_ENCRYPTION_KEY environment variable must be set in production");
                error!("Generate a secure key with: openssl rand -hex 32");
                std::process::exit(1);
            }
        }
    } else if std::env::var("MFA_ENCRYPTION_KEY").is_err() {
        warn!("MFA_ENCRYPTION_KEY not set - MFA features will be disabled");
        warn!("Generate with: openssl rand -hex 32");
    }
    
    // Security: Validate environment (already declared above)
    if environment == "production" {
        // Check for HTTPS in production URLs
        if let Ok(frontend_url) = env::var("FRONTEND_URL") {
            if !frontend_url.starts_with("https://") && !frontend_url.starts_with("http://localhost") {
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

    // Create rate limiter with Redis backend (fallback to in-memory for development)
    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| {
        if environment == "production" {
            warn!("REDIS_URL not configured in production - using in-memory rate limiting");
        }
        "memory://".to_string()
    });

    // Build the public limiter (for unauthenticated requests)
    let public_limiter = Limiter::builder(&redis_url)
        .key_by(|req: &actix_web::dev::ServiceRequest| {
            // Use IP address as the key for rate limiting
            req.peer_addr()
                .map(|addr| format!("public:{}", addr.ip()))
        })
        .limit(rate_limit_per_minute as usize)
        .period(Duration::from_secs(60)) // 1 minute window
        .build();

    // Build the authenticated limiter (for authenticated requests)
    let auth_limiter = Limiter::builder(&redis_url)
        .key_by(|req: &actix_web::dev::ServiceRequest| {
            // Use IP address with auth prefix for higher limits
            req.peer_addr()
                .map(|addr| format!("auth:{}", addr.ip()))
        })
        .limit(auth_rate_limit_per_minute as usize)
        .period(Duration::from_secs(60)) // 1 minute window
        .build();

    let public_limiter = match public_limiter {
        Ok(limiter) => limiter,
        Err(e) => {
            warn!(error = %e, "Rate limiter fallback to in-memory");

            // Fallback to memory limiter
            let fallback = Limiter::builder("memory://")
                .key_by(|req: &actix_web::dev::ServiceRequest| {
                    req.peer_addr()
                        .map(|addr| format!("public:{}", addr.ip()))
                })
                .limit(rate_limit_per_minute as usize)
                .period(Duration::from_secs(60))
                .build();

            match fallback {
                Ok(limiter) => limiter,
                Err(fallback_err) => {
                    error!(error = %fallback_err, "Failed to initialize fallback rate limiter");
                    return Err(std::io::Error::other("Rate limiter initialization failed"));
                }
            }
        }
    };

    let auth_limiter = match auth_limiter {
        Ok(limiter) => limiter,
        Err(e) => {
            warn!(error = %e, "Auth rate limiter fallback to in-memory");

            // Fallback to memory limiter
            let fallback = Limiter::builder("memory://")
                .key_by(|req: &actix_web::dev::ServiceRequest| {
                    req.peer_addr()
                        .map(|addr| format!("auth:{}", addr.ip()))
                })
                .limit(auth_rate_limit_per_minute as usize)
                .period(Duration::from_secs(60))
                .build();

            match fallback {
                Ok(limiter) => limiter,
                Err(fallback_err) => {
                    error!(error = %fallback_err, "Failed to initialize fallback auth rate limiter");
                    return Err(std::io::Error::other("Auth rate limiter initialization failed"));
                }
            }
        }
    };

    // Get host and port from environment variables
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or("8080".to_string()).parse::<u16>().map_err(|e| {
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
                "FRONTEND_URL environment variable is required in production"
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
        Ok(_) => {},
        Err(e) => {
            error!(error = %e, "Database initialization failed");
            return Err(std::io::Error::other(format!("Database initialization failed: {e}")));
        }
    }

    // Security: Verify initialization was successful
    if !db::is_initialized() {
        error!("Database initialization verification failed");
        return Err(std::io::Error::other("Database initialization verification failed"));
    }

    // Create uploads directory structure if it doesn't exist
    let uploads_dir = "/app/uploads";
    let directories = ["", "temp", "tickets", "users", "users/avatars", "users/banners", "users/thumbs", "plugins"];
    for dir in directories.iter() {
        let full_path = format!("{uploads_dir}/{dir}");
        match std::fs::create_dir_all(&full_path) {
            Ok(_) => {},
            Err(e) => {
                error!(path = %full_path, error = %e, "Failed to create directory");
                return Err(std::io::Error::other(format!("Failed to create directory: {full_path}")));
            }
        }
    }

    // Provision plugins from /app/plugins/ directory
    {
        let mut conn = pool.get().expect("Failed to get connection for plugin provisioning");
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

    // Initialize Redis cache for Yjs documents (survives backend restarts)
    // Use the same Redis URL as rate limiting, but fall back to localhost if using memory://
    let yjs_redis_url = if redis_url.starts_with("redis://") {
        redis_url.clone()
    } else {
        warn!("Using in-memory rate limiting - Yjs cache will use localhost Redis");
        env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
    };

    let redis_cache = match create_redis_cache(&yjs_redis_url) {
        Ok(cache) => {
            info!(url = %yjs_redis_url, "Redis cache initialized for Yjs documents");
            cache
        },
        Err(e) => {
            error!(error = ?e, "Failed to initialize Redis cache for Yjs");
            error!("CRITICAL: Yjs documents will NOT persist across server restarts");
            error!("Please ensure Redis is running and REDIS_URL is configured correctly");
            return Err(std::io::Error::other(format!("Redis initialization failed: {e:?}")));
        }
    };

    // Initialize SSE state for real-time ticket updates (must be created before YjsAppState)
    let sse_state = web::Data::new(handlers::sse::SseState::new());

    // Initialize notification service for in-app and email notifications
    let notification_service = {
        use std::sync::Arc;
        let service = services::notifications::NotificationService::new(pool.clone());

        // Register in-app channel (SSE)
        // web::Data<T> wraps Arc<T>, so we can get the inner Arc directly
        let sse_state_arc: Arc<handlers::sse::SseState> = sse_state.clone().into_inner();
        let in_app_channel = Arc::new(services::notifications::channels::in_app::InAppChannel::new(sse_state_arc));
        service.register_channel(in_app_channel);

        // Register email channel if email service is configured
        match utils::email::EmailService::from_env() {
            Ok(email_service) => {
                let app_name = std::env::var("APP_NAME").unwrap_or_else(|_| "Nosdesk".to_string());
                let email_channel = Arc::new(services::notifications::channels::email::EmailChannel::new(
                    Arc::new(email_service),
                    pool.clone(),
                    frontend_url.clone(),
                    app_name,
                ));
                service.register_channel(email_channel);
            }
            Err(e) => {
                info!(error = ?e, "Email service not configured - email notifications disabled");
            }
        }

        web::Data::new(service)
    };

    // Initialize webhook service for external integrations
    let webhook_service = {
        use std::sync::Arc;
        let sse_state_arc: Arc<handlers::sse::SseState> = sse_state.clone().into_inner();
        web::Data::new(services::webhooks::WebhookService::new(pool.clone(), sse_state_arc))
    };

    // Initialize plugin proxy service for external requests
    let plugin_proxy_service = web::Data::new(services::plugins::PluginProxyService::new());

    // Initialize search service for full-text search
    let search_service = {
        use std::sync::Arc;
        use std::path::Path;

        let search_index_path = env::var("SEARCH_INDEX_PATH")
            .unwrap_or_else(|_| "data/search_index".to_string());

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
                return Err(std::io::Error::other(format!("Search service initialization failed: {e}")));
            }
        }
    };

    // Initialize WebSocket app state for collaborative editing (includes SseState for broadcasting)
    let yjs_app_state = web::Data::new(handlers::collaboration::YjsAppState::new(web::Data::new(pool.clone()), redis_cache, sse_state.clone()));

    // Initialize system state for tracking uptime
    let system_state = web::Data::new(handlers::system::SystemState::new());

    // Share the limiters across all app instances
    let public_limiter_data = web::Data::new(public_limiter);
    let auth_limiter_data = web::Data::new(auth_limiter);

    if host == "0.0.0.0" {
        warn!("Server bound to all interfaces (0.0.0.0)");
    }

    // Initialize storage backend
    let storage_config = get_storage_config();
    let storage = create_storage(storage_config);
    let storage_data = web::Data::new(storage.clone());

    info!(host = %host, port = %port, environment = %environment, "Server starting");
    
    let server_result = HttpServer::new(move || {
        // Configure CORS with specific allowed origins
        let mut cors = Cors::default()
            .allowed_origin(&frontend_url)
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

        // Add additional allowed origins if specified
        for origin in &additional_origins {
            cors = cors.allowed_origin(origin);
        }

        // Configure JSON payload limits for file uploads
        let json_config = web::JsonConfig::default()
            .limit(max_payload_size);

        // Configure multipart form limits for file uploads
        let multipart_config = web::FormConfig::default()
            .limit(max_payload_size);

        App::new()
            .wrap(cors)
            .wrap(crate::middleware::SecurityHeaders) // Apply security headers globally
            .wrap(crate::utils::csrf::CsrfProtection)
            .app_data(public_limiter_data.clone())
            .app_data(auth_limiter_data.clone())
            .app_data(web::Data::new(pool.clone()))
            .app_data(yjs_app_state.clone())
            .app_data(sse_state.clone())
            .app_data(system_state.clone())
            .app_data(storage_data.clone())
            .app_data(notification_service.clone())
            .app_data(webhook_service.clone())
            .app_data(plugin_proxy_service.clone())
            .app_data(search_service.clone())
            .app_data(json_config)
            .app_data(multipart_config)
            
            // === PUBLIC ROUTES (NO AUTHENTICATION REQUIRED) ===
            .route("/health", web::get().to(health_check))

            // Debug endpoint for frontend log forwarding (dev mode only)
            .route("/api/debug/frontend-logs", web::post().to(handlers::debug::receive_frontend_logs))

            // Public file serving - ONLY user avatars, banners, thumbs, and branding (no sensitive data)
            .route("/uploads/users/avatars/{filename:.*}", web::get().to(handlers::serve_public_file))
            .route("/uploads/users/banners/{filename:.*}", web::get().to(handlers::serve_public_file))
            .route("/uploads/users/thumbs/{filename:.*}", web::get().to(handlers::serve_public_file))
            .route("/uploads/branding/{filename:.*}", web::get().to(handlers::branding::serve_branding_file))

            // Public branding config (needed for favicon/logo before login)
            .route("/api/branding", web::get().to(handlers::branding::get_public_branding))
            
            // Public WebSocket for collaboration (auth handled in WebSocket handler)
            .service(
                web::scope("/api/collaboration")
                    .configure(handlers::collaboration::config)
            )
            
            // Public file serving with token-based auth for attachments
            .route("/api/files/tickets/{ticket_id}/notes/{filename:.*}", web::get().to(handlers::serve_ticket_note_image))
            .route("/api/files/tickets/{filename:.*}", web::get().to(handlers::serve_ticket_file))
            .route("/api/files/temp/{filename:.*}", web::get().to(handlers::serve_temp_file))
            
            // SSE endpoints (with custom token-based auth)
            // Main event stream for all real-time updates (tickets, documentation, devices, etc.)
            .route("/api/events/stream", web::get().to(handlers::sse::ticket_events_stream))
            .route("/api/events/status", web::get().to(handlers::sse::sse_status))
            
            // Authentication routes (public by design)
            .service(
                web::scope("/api/auth")
                    .wrap(RateLimiter::default())
                    .service(
                        web::scope("/setup")
                            .route("/status", web::get().to(handlers::check_setup_status))
                            .route("/admin", web::post().to(handlers::setup_initial_admin))
                            .route("/restore/upload", web::post().to(handlers::backup::onboarding_upload_restore))
                            .route("/restore/execute", web::post().to(handlers::backup::onboarding_execute_restore))
                    )
                                            .route("/login", web::post().to(handlers::login))
                        .route("/logout", web::post().to(handlers::logout))
                        .route("/mfa-login", web::post().to(handlers::mfa_login))
                        .route("/mfa-setup-login", web::post().to(handlers::mfa_setup_login))
                        .route("/mfa-enable-login", web::post().to(handlers::mfa_enable_login))
                        .route("/register", web::post().to(handlers::register))
                        .route("/refresh", web::post().to(handlers::refresh_token))
                    // Password reset routes (public, rate-limited)
                    .route("/password-reset/request", web::post().to(handlers::password_reset::request_password_reset))
                    .route("/password-reset/complete", web::post().to(handlers::password_reset::reset_password_with_token))
                    // MFA reset routes (public, rate-limited)
                    .route("/mfa-reset/request", web::post().to(handlers::mfa_reset::request_mfa_reset))
                    .route("/mfa-reset/complete", web::post().to(handlers::mfa_reset::complete_mfa_reset))
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
                            .route("/{id}", web::delete().to(handlers::revoke_session))
                            .route("/others", web::delete().to(handlers::revoke_all_other_sessions))
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
            
            // === PROTECTED ROUTES (AUTHENTICATION REQUIRED) ===
            // Supports both cookie-based auth (browser) and Bearer token auth (API clients)
            .service(
                web::scope("/api")
                    .wrap(actix_web::middleware::from_fn(middleware::dual_auth_middleware))
                    
                    // Authentication Provider management (admin only) - simplified for environment-based config
                    .route("/admin/auth/providers", web::get().to(handlers::get_auth_providers))

                    // Email configuration (admin only) - environment-based config
                    .route("/admin/email/config", web::get().to(handlers::email::get_email_config))
                    .route("/admin/email/test", web::post().to(handlers::email::send_test_email))

                    // System information (admin only)
                    .route("/admin/system/info", web::get().to(handlers::system::get_system_info))
                    .route("/admin/system/updates", web::get().to(handlers::system::check_system_updates))

                    // Branding configuration (admin only)
                    .route("/admin/branding/config", web::get().to(handlers::branding::get_branding_config))
                    .route("/admin/branding/config", web::patch().to(handlers::branding::update_branding_config))
                    .route("/admin/branding/image", web::post().to(handlers::branding::upload_branding_image))
                    .route("/admin/branding/image", web::delete().to(handlers::branding::delete_branding_image))

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

                    // ===== TICKET MANAGEMENT =====
                    .route("/tickets", web::get().to(handlers::get_tickets))
                    .route("/tickets/paginated", web::get().to(handlers::get_paginated_tickets))
                    .route("/tickets/recent", web::get().to(handlers::get_recent_tickets))
                    .route("/tickets", web::post().to(handlers::create_ticket))
                    .route("/tickets/empty", web::post().to(handlers::create_empty_ticket))
                    .route("/tickets/bulk", web::post().to(handlers::bulk_tickets))
                    .route("/tickets/{id}", web::get().to(handlers::get_ticket))
                    .route("/tickets/{id}", web::put().to(handlers::update_ticket))
                    .route("/tickets/{id}", web::patch().to(handlers::update_ticket_partial))
                    .route("/tickets/{id}", web::delete().to(handlers::delete_ticket))
                    .route("/tickets/{id}/view", web::post().to(handlers::record_ticket_view))
                    .route("/import/file", web::post().to(handlers::import_tickets_from_json))
                    .route("/import/json", web::post().to(handlers::import_tickets_from_json_string))
                    .route("/tickets/{ticket_id}/link/{linked_ticket_id}", web::post().to(handlers::link_tickets))
                    .route("/tickets/{ticket_id}/unlink/{linked_ticket_id}", web::delete().to(handlers::unlink_tickets))
                    .route("/tickets/{ticket_id}/devices/{device_id}", web::post().to(handlers::add_device_to_ticket))
                    .route("/tickets/{ticket_id}/devices/{device_id}", web::delete().to(handlers::remove_device_from_ticket))
                    .route("/tickets/{ticket_id}/comments", web::get().to(handlers::get_comments_by_ticket_id))
                    .route("/tickets/{ticket_id}/comments", web::post().to(handlers::add_comment_to_ticket))
                    .route("/tickets/{ticket_id}/notes/images", web::post().to(handlers::upload_ticket_note_image))
                    .route("/comments/{id}", web::delete().to(handlers::delete_comment))
                    .route("/comments/{comment_id}/attachments", web::post().to(handlers::add_attachment_to_comment))
                    .route("/attachments/{id}", web::delete().to({
                        let storage = storage.clone();
                        move |path, pool| {
                            handlers::delete_attachment(path, pool, storage.clone())
                        }
                    }))
                    
                    // ===== PROJECT MANAGEMENT =====
                    .route("/projects", web::get().to(handlers::get_all_projects))
                    .route("/projects", web::post().to(handlers::create_project))
                    .route("/projects/{id}", web::get().to(handlers::get_project))
                    .route("/projects/{id}", web::put().to(handlers::update_project))
                    .route("/projects/{id}", web::delete().to(handlers::delete_project))
                    .route("/projects/{id}/tickets", web::get().to(handlers::get_project_tickets))
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
                    .route("/groups/{id}/devices", web::put().to(handlers::groups::set_group_devices))
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
                    .route("/admin/plugins", web::get().to(handlers::plugins::list_plugins))
                    .route("/admin/plugins", web::post().to(handlers::plugins::install_plugin))
                    .route("/admin/plugins/{uuid}", web::get().to(handlers::plugins::get_plugin))
                    .route("/admin/plugins/{uuid}", web::put().to(handlers::plugins::update_plugin))
                    .route("/admin/plugins/{uuid}", web::delete().to(handlers::plugins::uninstall_plugin))
                    .route("/admin/plugins/{uuid}/settings", web::get().to(handlers::plugins::get_plugin_settings))
                    .route("/admin/plugins/{uuid}/settings", web::post().to(handlers::plugins::set_plugin_setting))
                    .route("/admin/plugins/{uuid}/settings/{key}", web::delete().to(handlers::plugins::delete_plugin_setting))
                    .route("/admin/plugins/{uuid}/activity", web::get().to(handlers::plugins::get_plugin_activity))
                    .route("/admin/plugins/{uuid}/bundle", web::post().to(handlers::plugins::upload_plugin_bundle))
                    .route("/admin/plugins/install", web::post().to(handlers::plugins::install_plugin_from_zip))

                    // ===== PLUGIN API (For plugins to use) =====
                    .route("/plugins/enabled", web::get().to(handlers::plugins::list_enabled_plugins))
                    .route("/plugins/{uuid}/bundle", web::get().to(handlers::plugins::serve_plugin_bundle))
                    .route("/plugins/{uuid}/storage/{key}", web::get().to(handlers::plugins::get_plugin_storage))
                    .route("/plugins/{uuid}/storage", web::post().to(handlers::plugins::set_plugin_storage))
                    .route("/plugins/{uuid}/storage/{key}", web::delete().to(handlers::plugins::delete_plugin_storage))
                    .route("/plugins/{uuid}/proxy", web::post().to(handlers::plugins::proxy_plugin_request))

                    // ===== USER MANAGEMENT =====
                    // Note: Specific routes must come BEFORE generic {uuid} routes to avoid matching conflicts
                    .route("/users", web::get().to(handlers::get_users))
                    .route("/users/paginated", web::get().to(handlers::get_paginated_users))
                    .route("/users/batch", web::post().to(handlers::get_users_batch))
                    .route("/users/bulk", web::post().to(handlers::bulk_users))
                    .route("/users/cleanup-images", web::post().to(handlers::cleanup_stale_images))
                    .route("/files/cleanup-temp", web::post().to(handlers::cleanup_temp_files))
                    .route("/users/auth-identities", web::get().to(handlers::get_user_auth_identities))
                    .route("/users/auth-identities/{id}", web::delete().to(handlers::delete_user_auth_identity))
                    .route("/users", web::post().to(handlers::create_user))
                    .route("/users/{uuid}", web::get().to(handlers::get_user_by_uuid))
                    .route("/users/{uuid}", web::put().to(handlers::update_user_by_uuid))
                    .route("/users/{uuid}", web::delete().to(handlers::delete_user))
                    .route("/users/{uuid}/image", web::post().to(handlers::upload_user_image))
                    .route("/users/{uuid}/emails", web::get().to(handlers::get_user_emails))
                    .route("/users/{uuid}/emails", web::post().to(handlers::add_user_email))
                    .route("/users/{uuid}/emails/{email_id}", web::put().to(handlers::update_user_email))
                    .route("/users/{uuid}/emails/{email_id}", web::delete().to(handlers::delete_user_email))
                    .route("/users/{uuid}/with-emails", web::get().to(handlers::get_user_with_emails))
                    .route("/users/{uuid}/auth-identities", web::get().to(handlers::get_user_auth_identities_by_uuid))
                    .route("/users/{uuid}/auth-identities/{id}", web::delete().to(handlers::delete_user_auth_identity_by_uuid))
                    .route("/users/{uuid}/resend-invitation", web::post().to(handlers::resend_invitation))
                    
                    // ===== DEVICE MANAGEMENT =====
                    .route("/devices", web::get().to(handlers::get_all_devices))
                    .route("/devices/paginated", web::get().to(handlers::get_paginated_devices))
                    .route("/devices/paginated/excluding", web::get().to(handlers::get_paginated_devices_excluding))
                    .route("/devices/bulk", web::post().to(handlers::bulk_devices))
                    .route("/devices", web::post().to(handlers::create_device))
                    .route("/devices/{id}", web::get().to(handlers::get_device_by_id))
                    .route("/devices/{id}", web::put().to(handlers::update_device))
                    .route("/devices/{id}", web::delete().to(handlers::delete_device))
                    .route("/devices/{id}/unmanage", web::post().to(handlers::unmanage_device))
                    .route("/users/{uuid}/devices", web::get().to(handlers::get_user_devices))
                    
                    // ===== DOCUMENTATION SYSTEM =====
                    .route("/documentation/pages", web::get().to(handlers::get_documentation_pages))
                    .route("/documentation/pages/export", web::get().to(handlers::export_documentation_pages))
                    .route("/documentation/pages", web::post().to(handlers::create_documentation_page))
                    .route("/documentation/pages/{id}", web::get().to(handlers::get_documentation_page))
                    .route("/documentation/pages/{id}", web::put().to(handlers::update_documentation_page))
                    .route("/documentation/pages/{id}", web::delete().to(handlers::delete_documentation_page))
                    .route("/documentation/pages/top-level", web::get().to(handlers::get_top_level_documentation_pages))
                    .route("/documentation/pages/parent/{parent_id}", web::get().to(handlers::get_documentation_pages_by_parent_id))
                    .route("/documentation/pages/slug/{slug}", web::get().to(handlers::get_documentation_page_by_slug))
                    .route("/documentation/pages/slug/{slug}/with-children", web::get().to(handlers::get_documentation_page_by_slug_with_children))
                    .route("/documentation/pages/{id}/with-children-by-parent", web::get().to(handlers::get_page_with_children_by_parent_id))
                    .route("/documentation/pages/ordered/top-level", web::get().to(handlers::get_ordered_top_level_pages))
                    .route("/documentation/pages/ordered/parent/{parent_id}", web::get().to(handlers::get_ordered_pages_by_parent_id))
                    .route("/documentation/pages/{id}/with-ordered-children", web::get().to(handlers::get_page_with_ordered_children))
                    .route("/documentation/pages/reorder", web::post().to(handlers::reorder_pages))
                    .route("/documentation/pages/move", web::post().to(handlers::move_page_to_parent))
                    .route("/tickets/{ticket_id}/documentation", web::get().to(handlers::get_documentation_pages_by_ticket_id))
                    .route("/tickets/{ticket_id}/documentation/create", web::post().to(handlers::create_documentation_page_from_ticket))
                    .route("/documentation/{id}", web::put().to(handlers::update_documentation_page))
                    .route("/documentation/{id}", web::delete().to(handlers::delete_documentation_page))
            )
            
            // Unified file serving using storage abstraction (protected routes)
            .route("/uploads/tickets/{path:.*}", web::get().to(handlers::serve_protected_file))
            .route("/uploads/temp/{path:.*}", web::get().to(handlers::serve_protected_file))
            
            // === FRONTEND STATIC FILES ===
            // Serve static frontend files with SPA fallback using default_handler
            // This is the recommended actix-web pattern for SPAs
            .service(
                Files::new("/assets", "./public/assets")
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
    .run()
    .await;

    server_result
}