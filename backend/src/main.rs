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

use backend::config;
use backend::license;
use backend::startup;
use backend::telemetry;

use dotenvy::dotenv;
use std::env;
use tracing::{debug, info};

// Cookie-based authentication middleware lives in
// `middleware/cookie_auth.rs` so it sits next to its peer
// `middleware/api_token.rs` and uses `crate::*` paths consistently.

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
    telemetry::init();
    debug!("Tracing initialized, continuing startup");

    // === SECURITY STARTUP VALIDATION ===
    info!("Starting Nosdesk API Server");
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

    let config = config::Config::from_env()?;
    startup::run(config).await
}
