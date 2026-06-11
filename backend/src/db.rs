use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel::RunQueryDsl;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use dotenvy::dotenv;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{error, info, warn};

pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;
pub type DbConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;

/// Connection customizer that clears every app-level GUC when a
/// connection is established.
///
/// `with_actor_context` scopes its GUCs to a transaction (via
/// `set_config(_, _, true)`), so they normally die at commit/rollback.
/// This is belt-and-braces: r2d2 runs `on_acquire` each time it gets a
/// backend from the connection manager, so if a deployment fronts
/// Postgres with a pooler (PgBouncer/pgcat) that hands back a server
/// connection another client left with a session-scoped `app.*` GUC
/// still set, this clears it before the connection enters our pool
/// rather than letting stale workspace / actor attribution leak in.
/// Empty-string is equivalent to unset everywhere these GUCs are read
/// (every reader goes through `NULLIF(current_setting(...), '')`),
/// matching `sync::session::reset_session_role`.
#[derive(Debug)]
struct ResetAppGucs;

impl r2d2::CustomizeConnection<PgConnection, r2d2::Error> for ResetAppGucs {
    fn on_acquire(&self, conn: &mut PgConnection) -> Result<(), r2d2::Error> {
        for key in [
            "app.workspace_id",
            "app.actor_uuid",
            "app.actor_kind",
            "app.actor_ref",
            "app.correlation_id",
            "app.client_tx_id",
        ] {
            diesel::sql_query("SELECT set_config($1, '', false)")
                .bind::<diesel::sql_types::Text, _>(key)
                .execute(conn)
                .map_err(r2d2::Error::QueryError)?;
        }
        Ok(())
    }
}

// Embed migrations at compile time
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

// Simple flag to ensure initialization only happens once
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize the database by running migrations
/// This function is designed to be called only once
pub async fn initialize_database(
    pool: &Pool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if INITIALIZED.load(Ordering::Acquire) {
        return Ok(());
    }

    // Wait for database to be ready
    let mut attempts = 0;
    while attempts < 30 {
        if let Ok(mut conn) = pool.get() {
            if diesel::sql_query("SELECT 1").execute(&mut conn).is_ok() {
                break;
            }
        }

        attempts += 1;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    if attempts >= 30 {
        error!(
            attempts = 30,
            timeout_seconds = 60,
            "Database not ready after timeout. Check that PostgreSQL is running and DATABASE_URL is correct"
        );
        return Err("Database not ready after 60 seconds".into());
    }

    // Run migrations
    let mut conn = pool
        .get()
        .map_err(|e| format!("Failed to get database connection: {e}"))?;

    match conn.run_pending_migrations(MIGRATIONS) {
        Ok(migrations) => {
            if !migrations.is_empty() {
                info!("Applied {} database migration(s)", migrations.len());
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to run migrations");
            return Err(e);
        }
    }

    // Stamp the binary's schema hash into system_meta so the bootstrap
    // protocol can detect client/server schema mismatches. `build.rs`
    // computes a stable hash of the embedded migrations directory and
    // bakes it in via the NOSDESK_SCHEMA_HASH env var; the hash
    // changes deterministically whenever any migration is added or
    // modified.
    let schema_hash = env!("NOSDESK_SCHEMA_HASH");
    if let Err(e) = crate::sync::system_meta::set_schema_hash(&mut conn, schema_hash) {
        warn!(error = %e, "Failed to write schema_hash to system_meta");
    }

    // Mint the per-database instance id if absent. Stable for the life
    // of the database, regenerated only on a fresh init; clients use it
    // as an epoch fence to wipe local caches that belong to a different
    // database generation (see docs/plans/collab-stale-cache-fence.md).
    match crate::sync::system_meta::ensure_instance_id(&mut conn) {
        Ok(id) => info!(instance_id = %id, "Database instance id ready"),
        Err(e) => warn!(error = %e, "Failed to ensure instance_id in system_meta"),
    }

    // Check if this is the first run
    match crate::repository::count_users(&mut conn) {
        Ok(count) => {
            if count == 0 {
                info!("Initial setup required - no users found");
            } else {
                info!(user_count = count, "System ready");
            }
        }
        Err(e) => {
            warn!(error = %e, "Could not check user count");
        }
    }

    INITIALIZED.store(true, Ordering::Release);
    Ok(())
}

/// Check if database has been initialized
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

pub fn establish_connection_pool() -> Pool {
    dotenv().ok();

    let database_url = match env::var("DATABASE_URL") {
        Ok(url) => {
            info!(url_prefix = %url.chars().take(30).collect::<String>(), "DATABASE_URL found");
            url
        }
        Err(e) => {
            error!(error = %e, "DATABASE_URL environment variable must be set");
            std::process::exit(1);
        }
    };

    info!("Attempting to create database connection pool");
    let manager = ConnectionManager::<PgConnection>::new(database_url);

    match r2d2::Pool::builder()
        .connection_customizer(Box::new(ResetAppGucs))
        .build(manager)
    {
        Ok(pool) => {
            info!("Database connection pool created successfully");
            pool
        }
        Err(e) => {
            error!(
                error = %e,
                "Failed to create database connection pool. This usually means the database is not accessible or DATABASE_URL is incorrect"
            );
            std::process::exit(1);
        }
    }
}
