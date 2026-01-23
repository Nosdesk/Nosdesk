use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use diesel::RunQueryDsl;
use dotenv::dotenv;
use std::env;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn, error};

pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;
pub type DbConnection = r2d2::PooledConnection<ConnectionManager<PgConnection>>;

// Embed migrations at compile time
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

// Simple flag to ensure initialization only happens once
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize the database by running migrations
/// This function is designed to be called only once
pub async fn initialize_database(pool: &Pool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    let mut conn = pool.get()
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
        },
        Err(e) => {
            error!(error = %e, "DATABASE_URL environment variable must be set");
            std::process::exit(1);
        }
    };

    info!("Attempting to create database connection pool");
    let manager = ConnectionManager::<PgConnection>::new(database_url);

    match r2d2::Pool::builder().build(manager) {
        Ok(pool) => {
            info!("Database connection pool created successfully");
            pool
        },
        Err(e) => {
            error!(
                error = %e,
                "Failed to create database connection pool. This usually means the database is not accessible or DATABASE_URL is incorrect"
            );
            std::process::exit(1);
        }
    }
} 