use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel::{Connection, RunQueryDsl};
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
            // Defense-in-depth for a retired isolation switch: no RLS
            // policy reads `app.bypass_workspace_check` anymore (the
            // 2026-06-12 migration dropped the last five), so this is
            // inert today. Clearing it when the connection is established
            // means a fresh backend can never start out carrying a stale
            // `true` even if a future change reintroduces a reader.
            "app.bypass_workspace_check",
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

/// Apply pending migrations on an already-open connection (pooled or raw).
fn apply_pending_migrations<C>(conn: &mut C) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    C: MigrationHarness<diesel::pg::Pg>,
{
    match conn.run_pending_migrations(MIGRATIONS) {
        Ok(applied) => {
            if !applied.is_empty() {
                info!("Applied {} database migration(s)", applied.len());
            }
            Ok(())
        }
        Err(e) => {
            error!(error = %e, "Failed to run migrations");
            Err(e)
        }
    }
}

/// Run pending migrations, using a *privileged* role when
/// `MIGRATION_DATABASE_URL` is set and falling back to the runtime pool
/// otherwise.
///
/// The schema migrations are designed to be applied by a superuser / owner:
/// they `CREATE ROLE`, `ALTER … OWNER TO nosdesk_admin`, `CREATE EXTENSION`,
/// and `GRANT … TO nosdesk_app`. The runtime role (`nosdesk_app` —
/// `NOBYPASSRLS`, no `CREATEROLE`) intentionally can't do any of those, so
/// running migrations through the app's own pool fails on a fresh or changed
/// schema and silently leaves it drifted (the failure mode that stranded the
/// hosted-test instance).
///
/// Point `MIGRATION_DATABASE_URL` at a privileged role (the cluster superuser)
/// to apply migrations cleanly. The runtime pool always stays on
/// `DATABASE_URL` (`nosdesk_app`), so RLS enforcement and the hosted-mode
/// role-posture guard are unaffected — the privileged connection is opened
/// only for the migration run and dropped immediately after. When unset,
/// behaviour is unchanged (single-role dev / self-hosted setups).
fn run_migrations(pool: &Pool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match env::var("MIGRATION_DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => {
            let url = url.trim();
            info!(
                database = %redact_db_url(url),
                "Running migrations via MIGRATION_DATABASE_URL (privileged role)"
            );
            let mut conn = PgConnection::establish(url)
                .map_err(|e| format!("MIGRATION_DATABASE_URL connect failed: {e}"))?;
            apply_pending_migrations(&mut conn)
            // `conn` drops here — the privileged connection never enters the pool.
        }
        _ => {
            info!(
                "Running migrations via the runtime pool (DATABASE_URL); \
                 MIGRATION_DATABASE_URL not set"
            );
            let mut conn = pool
                .get()
                .map_err(|e| format!("Failed to get database connection: {e}"))?;
            apply_pending_migrations(&mut conn)
        }
    }
}

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

    // Run migrations. Uses MIGRATION_DATABASE_URL (a privileged role) when set,
    // since the schema migrations need CREATE ROLE / ALTER OWNER / CREATE
    // EXTENSION / GRANT that the runtime nosdesk_app role intentionally lacks.
    run_migrations(pool)?;

    // Post-migration bookkeeping runs as the runtime role on the pool — the
    // migrations have already granted nosdesk_app access to these tables.
    let mut conn = pool
        .get()
        .map_err(|e| format!("Failed to get database connection: {e}"))?;

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

/// The login role's RLS posture, as seen by the app's pool.
pub struct RoleRlsPosture {
    /// The role the connection authenticates as (`current_user`).
    pub role_name: String,
    /// True when the role bypasses row-level security — either a
    /// superuser (`rolsuper`) or an explicit `BYPASSRLS`
    /// (`rolbypassrls`). Such a role sees and writes every tenant's
    /// rows even with `FORCE` RLS on the table, so it must never back
    /// a hosted multi-tenant deployment.
    pub bypasses_rls: bool,
}

/// Inspect the role the pool authenticates as. Used by the hosted-mode
/// startup guard (P0.2) to refuse booting on a role that bypasses RLS,
/// which would silently disable tenant isolation.
pub fn inspect_role_rls_posture(pool: &Pool) -> Result<RoleRlsPosture, String> {
    use diesel::sql_types::{Bool, Text};

    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        role_name: String,
        #[diesel(sql_type = Bool)]
        bypasses_rls: bool,
    }

    let mut conn = pool.get().map_err(|e| format!("pool acquire: {e}"))?;
    let row: Row = diesel::sql_query(
        "SELECT current_user::text AS role_name, \
         (rolsuper OR rolbypassrls) AS bypasses_rls \
         FROM pg_roles WHERE rolname = current_user",
    )
    .get_result(&mut conn)
    .map_err(|e| format!("role inspection query: {e}"))?;

    Ok(RoleRlsPosture {
        role_name: row.role_name,
        bypasses_rls: row.bypasses_rls,
    })
}

/// Resolve `(max_size, min_idle)` for the pool from the env strings,
/// applying the same defaults and clamps as the live pool. Split out as a
/// pure fn so the bounds are unit-testable without touching process env or
/// a database: `max_size` defaults to 10 and is clamped to `2..=100`;
/// `min_idle` defaults to 1 and can never exceed `max_size`.
fn resolve_pool_sizing(max_size_env: Option<String>, min_idle_env: Option<String>) -> (u32, u32) {
    let max_size = max_size_env
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(10)
        .clamp(2, 100);
    let min_idle = min_idle_env
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(1)
        .min(max_size);
    (max_size, min_idle)
}

/// Render a connection URL safe for logging: scheme, host, port and
/// database name only. The userinfo (`user:password@`) and any query
/// string are dropped so credentials never reach the logs — the prior
/// "first 30 chars" log leaked the password for
/// `postgres://user:password@host/db`. Falls back to a placeholder when
/// the URL doesn't parse.
fn redact_db_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => {
            let host = u.host_str().unwrap_or("?");
            let db = u.path().trim_start_matches('/');
            match u.port() {
                Some(port) => format!("{}://{host}:{port}/{db}", u.scheme()),
                None => format!("{}://{host}/{db}", u.scheme()),
            }
        }
        Err(_) => "<unparseable DATABASE_URL>".to_string(),
    }
}

pub fn establish_connection_pool() -> Pool {
    dotenv().ok();

    let database_url = match env::var("DATABASE_URL") {
        Ok(url) => {
            info!(database = %redact_db_url(&url), "DATABASE_URL found");
            url
        }
        Err(e) => {
            error!(error = %e, "DATABASE_URL environment variable must be set");
            std::process::exit(1);
        }
    };

    // Pool sizing. `DB_MAX_CONNECTIONS` is the per-machine connection cap to
    // budget against Postgres `max_connections`: across N machines the peak
    // is `N × (max_size + dedicated LISTEN connections)` plus headroom for
    // migrations / psql / monitoring / the control plane.
    //
    // `DB_MIN_CONNECTIONS` (min_idle) is kept low on purpose. r2d2 treats an
    // unset `min_idle` as `= max_size`, so the default behaviour is to
    // eagerly open and then pin the full cap on every machine, even when
    // idle. A low min_idle makes the footprint elastic: an idle machine
    // holds only a couple of connections and grows to `max_size` under load,
    // releasing the slack again via `idle_timeout`.
    let (max_size, min_idle) = resolve_pool_sizing(
        env::var("DB_MAX_CONNECTIONS").ok(),
        env::var("DB_MIN_CONNECTIONS").ok(),
    );
    // How long a checkout waits for a free connection before erroring (pool
    // exhausted). Defaults to r2d2's 30s; clamped to a sane band.
    let connection_timeout = env::var("DB_CONNECTION_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(30)
        .clamp(1, 120);

    // Dedicated (non-pool) LISTEN connections each machine also holds, so
    // the logged budget reflects the true peak. Keep in sync with the
    // listener spawns in `main.rs`.
    let listen_conns = 2 // sync_outbox + email_queue (always spawned)
        + u32::from(
            env::var("NOSDESK_SEARCH_REPLICATION")
                .map(|v| v.trim().eq_ignore_ascii_case("true"))
                .unwrap_or(false), // search_replicator (opt-in)
        );
    info!(
        max_size,
        min_idle,
        connection_timeout_secs = connection_timeout,
        dedicated_listen = listen_conns,
        peak_per_machine = max_size + listen_conns,
        "DB pool sizing — budget: N_machines × peak_per_machine + headroom ≤ Postgres max_connections"
    );

    info!("Attempting to create database connection pool");
    let manager = ConnectionManager::<PgConnection>::new(database_url);

    match r2d2::Pool::builder()
        .max_size(max_size)
        .min_idle(Some(min_idle))
        .connection_timeout(Duration::from_secs(connection_timeout))
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

#[cfg(test)]
mod role_posture_tests {
    use super::*;
    use crate::test_helpers::setup_test_pool;

    /// The test pool drops to `nosdesk_app` (NOBYPASSRLS) on acquire,
    /// mirroring production, so the posture query should report it as NOT
    /// bypassing RLS — the case where the hosted-mode startup guard allows
    /// boot. This exercises the query end-to-end (a `QueryableByName` /
    /// column-alias mistake fails here, not just at runtime in prod) and
    /// proves the guard won't false-block a correctly-configured role. The
    /// inverse (a superuser → `bypasses_rls = true`, which the guard refuses
    /// in production) is verified out-of-band via psql against the same DB.
    #[test]
    fn inspect_role_rls_posture_clears_the_app_role() {
        let pool = setup_test_pool();
        let posture = inspect_role_rls_posture(&pool).expect("role posture query");
        assert_eq!(
            posture.role_name, "nosdesk_app",
            "test pool should authenticate as nosdesk_app"
        );
        assert!(
            !posture.bypasses_rls,
            "nosdesk_app is NOBYPASSRLS and must not be flagged as bypassing"
        );
    }
}

#[cfg(test)]
mod pool_sizing_tests {
    use super::resolve_pool_sizing;

    fn sz(max: Option<&str>, min: Option<&str>) -> (u32, u32) {
        resolve_pool_sizing(max.map(String::from), min.map(String::from))
    }

    #[test]
    fn defaults_when_unset() {
        assert_eq!(sz(None, None), (10, 1));
    }

    #[test]
    fn max_size_clamped_and_garbage_falls_back() {
        assert_eq!(sz(Some("1"), None).0, 2, "floor");
        assert_eq!(sz(Some("500"), None).0, 100, "ceiling");
        assert_eq!(sz(Some("not-a-number"), None).0, 10, "garbage -> default");
        assert_eq!(sz(Some(" 25 "), None).0, 25, "trimmed");
    }

    #[test]
    fn min_idle_never_exceeds_max_size() {
        assert_eq!(
            sz(Some("3"), Some("5")),
            (3, 3),
            "min_idle clamped to max_size"
        );
        assert_eq!(
            sz(Some("10"), Some("0")),
            (10, 0),
            "zero allowed (fully lazy)"
        );
        assert_eq!(sz(Some("10"), Some("4")), (10, 4));
    }
}

#[cfg(test)]
mod redact_db_url_tests {
    use super::redact_db_url;

    #[test]
    fn strips_credentials_and_query() {
        let out =
            redact_db_url("postgres://nosdesk:s3cr3t@db.internal:5432/helpdesk?sslmode=require");
        assert_eq!(out, "postgres://db.internal:5432/helpdesk");
        assert!(!out.contains("s3cr3t"), "password must not appear: {out}");
        assert!(!out.contains("nosdesk"), "username must not appear: {out}");
    }

    #[test]
    fn handles_no_port_and_no_credentials() {
        assert_eq!(
            redact_db_url("postgres://localhost/nosdesk"),
            "postgres://localhost/nosdesk"
        );
    }

    #[test]
    fn unparseable_falls_back_without_leaking() {
        assert_eq!(redact_db_url("not a url"), "<unparseable DATABASE_URL>");
    }
}
