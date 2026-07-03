use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel::{Connection, RunQueryDsl};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use dotenvy::dotenv;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{error, info, warn};

pub type Pool = r2d2::Pool<ResettingManager>;
pub type DbConnection = r2d2::PooledConnection<ResettingManager>;

/// Clear every per-request `app.*` GUC and reset any leaked `SET ROLE` on
/// checkout.
///
/// Empty-string is equivalent to unset everywhere these GUCs are read
/// (every reader goes through `NULLIF(current_setting(...), '')`), matching
/// `sync::session::reset_session_role`. Executing the statement also proves
/// the backend is live, which is why this doubles as the pool's checkout
/// validity check (see [`ResettingManager::is_valid`]).
///
/// `RESET ROLE` fails the pool CLOSED regardless of caller discipline: a
/// background job that elevates to the BYPASSRLS role (`elevate_session_role`)
/// and then panics before its `reset_session_role` runs would otherwise return
/// an RLS-bypassing, unscoped session to the pool, and the next consumer would
/// inherit it. Resetting on every checkout makes that leak unreachable instead
/// of relying on every caller's unwind path.
///
/// `app.bypass_workspace_check` is a retired isolation switch (no RLS policy
/// reads it since the 2026-06-12 migration) but stays in the scrub list so a
/// future change can't accidentally inherit a stale `true` across requests.
fn clear_app_gucs(conn: &mut PgConnection) -> diesel::QueryResult<()> {
    diesel::sql_query(
        "SELECT set_config('app.workspace_id', '', false), \
                set_config('app.actor_uuid', '', false), \
                set_config('app.actor_kind', '', false), \
                set_config('app.actor_ref', '', false), \
                set_config('app.correlation_id', '', false), \
                set_config('app.client_tx_id', '', false), \
                set_config('app.bypass_workspace_check', '', false)",
    )
    .execute(conn)?;
    diesel::sql_query("RESET ROLE").execute(conn).map(|_| ())
}

/// Pool connection manager that scrubs per-request GUCs on every checkout.
///
/// r2d2's `CustomizeConnection::on_acquire` only fires when a backend is
/// first created, not per checkout, so a session-scoped `SET` (e.g. the
/// `pin_workspace` helpers) survives on a pooled connection across requests
/// and would leak one request's workspace into the next. r2d2 DOES call
/// `is_valid` on every checkout when `test_on_check_out` is set (it is, on
/// the production pool), so that is where we scrub: every request starts
/// from a clean slate, and a handler that forgets to pin its workspace fails
/// closed (sees no rows under RLS) instead of inheriting the previous
/// request's tenant. `with_actor_context` callers are unaffected — their
/// `SET LOCAL` GUCs already die at commit.
#[derive(Debug)]
pub struct ResettingManager(ConnectionManager<PgConnection>);

impl ResettingManager {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self(ConnectionManager::new(database_url))
    }
}

impl r2d2::ManageConnection for ResettingManager {
    type Connection = PgConnection;
    type Error = <ConnectionManager<PgConnection> as r2d2::ManageConnection>::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        self.0.connect()
    }

    fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        // The scrub statement also proves liveness, so it subsumes the
        // standard validity ping rather than running in addition to it.
        clear_app_gucs(conn).map_err(r2d2::Error::QueryError)
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        self.0.has_broken(conn)
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
            // Same (migration-capable) connection, so the applied-migrations read
            // has the privileges the runtime pool role lacks.
            assert_no_migration_drift(conn)?;
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

/// Build a short-lived single-connection pool over a *privileged* role for
/// one-off schema DDL that the runtime `nosdesk_app` role can't perform:
/// partition `CREATE` / `ATTACH` / `DETACH` / `DROP`, which need ownership of
/// the parent table and `CREATE` on the schema (PG15+ revokes `CREATE ON
/// SCHEMA public` from `PUBLIC`, so `nosdesk_app` can't create the monthly
/// child tables).
///
/// Mirrors [`run_migrations`]: uses `MIGRATION_DATABASE_URL` when set and
/// returns `None` otherwise, so single-role dev / self-hosted setups (where
/// `DATABASE_URL` already owns the schema) fall back to the runtime pool
/// unchanged. The pool is built per call: the partition jobs run at startup
/// and on daily ticks, so the extra connect is negligible, and not holding an
/// elevated pool open for the process lifetime keeps the privileged
/// credentials in use only while DDL is actually running.
pub fn privileged_ddl_pool() -> Option<Pool> {
    let url = env::var("MIGRATION_DATABASE_URL").ok()?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }
    match r2d2::Pool::builder()
        .max_size(1)
        .build(ResettingManager::new(url))
    {
        Ok(pool) => Some(pool),
        Err(e) => {
            warn!(
                error = %e,
                "MIGRATION_DATABASE_URL privileged pool build failed; \
                 falling back to runtime pool for partition DDL"
            );
            None
        }
    }
}

/// Refuse to start if the database has applied migrations this binary does not
/// embed — i.e. the DB schema is *ahead* of the code. That happens on a rollback
/// to an older release, or (in a shared dev DB) when another branch's migration
/// gets applied and then you switch away from it. Serving old code against a
/// newer schema silently corrupts writes: a stale-column read inside a write
/// transaction aborts the transaction, and the subsequent `COMMIT` becomes a
/// `ROLLBACK` that Diesel reports as success — the write is lost with a 200.
///
/// Fail-closed by default. `NOSDESK_ALLOW_MIGRATION_DRIFT=1` overrides it (e.g.
/// an intentional staged rollback where you accept the risk).
fn assert_no_migration_drift<C>(
    conn: &mut C,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    C: MigrationHarness<diesel::pg::Pg>,
{
    use diesel::migration::MigrationSource;

    let embedded: std::collections::HashSet<String> =
        MigrationSource::<diesel::pg::Pg>::migrations(&MIGRATIONS)
            .map_err(|e| format!("migration-drift check: listing embedded migrations: {e}"))?
            .iter()
            .map(|m| m.name().version().to_string())
            .collect();

    let unknown: Vec<String> = conn
        .applied_migrations()
        .map_err(|e| format!("migration-drift check: reading applied migrations: {e}"))?
        .into_iter()
        .map(|v| v.to_string())
        .filter(|v| !embedded.contains(v))
        .collect();

    if unknown.is_empty() {
        return Ok(());
    }

    let allow = env::var("NOSDESK_ALLOW_MIGRATION_DRIFT")
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    if allow {
        warn!(
            unknown_migrations = ?unknown,
            "Migration drift: the database has migrations this binary does not embed. \
             Continuing because NOSDESK_ALLOW_MIGRATION_DRIFT is set."
        );
        return Ok(());
    }

    Err(format!(
        "Migration drift: the database has {} applied migration(s) this binary does not embed: {unknown:?}. \
         The DB is AHEAD of this build (a rollback to an older release, or a foreign migration applied to a \
         shared dev database). Serving this code against a newer schema risks silent write loss, so refusing \
         to start. Resync the database to this build, or set NOSDESK_ALLOW_MIGRATION_DRIFT=1 to override.",
        unknown.len()
    )
    .into())
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
    // Runs pending migrations and asserts the DB isn't ahead of this binary
    // (the drift guard, on the same privileged connection).
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
    let manager = ResettingManager::new(database_url);

    match r2d2::Pool::builder()
        .max_size(max_size)
        .min_idle(Some(min_idle))
        .connection_timeout(Duration::from_secs(connection_timeout))
        // Scrub per-request GUCs on every checkout (ResettingManager::is_valid).
        // test_on_check_out is on by default; set explicitly because the GUC
        // scrub — and the tenant isolation that depends on it — relies on it.
        .test_on_check_out(true)
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
mod checkout_reset_tests {
    use super::*;
    use diesel::dsl::sql;
    use diesel::select;
    use diesel::sql_types::{Nullable, Text};

    // The production pool scrubs per-request app.* GUCs on every checkout, so
    // a session-scoped SET left behind by one request can't leak into the
    // next request that reuses the same pooled backend.
    #[test]
    fn checkout_scrubs_leaked_session_gucs() {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for the checkout-scrub test");
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .test_on_check_out(true)
            .build(ResettingManager::new(url))
            .expect("build reset-test pool");

        // One request leaves a workspace pinned on the connection...
        {
            let mut conn = pool.get().expect("first checkout");
            diesel::sql_query("SELECT set_config('app.workspace_id', '5', false)")
                .execute(&mut conn)
                .expect("pin workspace");
        }

        // ...and the next checkout of that same backend must start clean.
        let mut conn = pool.get().expect("second checkout");
        let leaked: Option<String> = select(sql::<Nullable<Text>>(
            "current_setting('app.workspace_id', true)",
        ))
        .get_result(&mut conn)
        .expect("read workspace guc");
        assert_eq!(
            leaked.unwrap_or_default(),
            "",
            "a leaked session GUC must be scrubbed on checkout"
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
