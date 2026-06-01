//! Shared helpers for backup/restore integration tests.
//!
//! Strategy: per-test database cloned from a once-per-binary
//! template. The template is created on first call, migrated
//! once, and reused across all tests in the binary. Each
//! `TestDb::new()` runs `CREATE DATABASE ... TEMPLATE`, which
//! Postgres implements as a filesystem copy (~100-300ms on a
//! warm cluster, vs ~1-2s for a full migration replay). Drop
//! terminates open connections and drops the per-test DB.
//!
//! Pattern follows `#[sqlx::test]` ([sqlx docs][1]). We hand-roll
//! it for Diesel since Diesel ships no equivalent.
//!
//! [1]: https://docs.rs/sqlx/latest/sqlx/attr.test.html

#![allow(dead_code)]

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager, PooledConnection};
use diesel_migrations::MigrationHarness;
use std::sync::OnceLock;
use uuid::Uuid;

use backend::db::MIGRATIONS;

/// Customizer that seeds `app.workspace_id` on every fresh
/// connection. After the Phase 3d NOT-NULL flip, the
/// `workspace_id` column on every tenant table defaults to
/// `NULLIF(current_setting('app.workspace_id', true), '')::int`.
/// Integration tests connect outside the request middleware
/// chain, so without this seed every INSERT against a tenant
/// table fails the NOT-NULL check before the test body even
/// runs. The customizer sets the GUC to the bootstrap workspace
/// (id=1, present in every fresh template) so existing
/// fixtures keep working without per-test ceremony. Tests that
/// need to exercise multi-workspace behaviour can override the
/// GUC explicitly inside their own with_actor_context wrap.
#[derive(Debug)]
struct WorkspaceGucCustomizer;

impl r2d2::CustomizeConnection<PgConnection, r2d2::Error> for WorkspaceGucCustomizer {
    fn on_acquire(&self, conn: &mut PgConnection) -> Result<(), r2d2::Error> {
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(r2d2::Error::QueryError)?;
        Ok(())
    }
}

pub type TestPool = r2d2::Pool<ConnectionManager<PgConnection>>;
pub type TestPooledConn = PooledConnection<ConnectionManager<PgConnection>>;

const TEMPLATE_NAME: &str = "nosdesk_test_template";

/// Resolve the base test database URL from env. Same precedence
/// as `backend::test_helpers`: dedicated test DB preferred.
fn base_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests")
}

/// Swap the database name on a Postgres connection URL.
/// Format assumed: `postgres://user:pw@host:port/dbname[?params]`.
fn with_database(url: &str, db: &str) -> String {
    let q = url.find('?').unwrap_or(url.len());
    let path_end = q;
    let path_start = url[..path_end].rfind('/').expect("URL must have a path");
    format!("{}/{}{}", &url[..path_start], db, &url[path_end..])
}

fn admin_url() -> String {
    with_database(&base_url(), "postgres")
}

fn template_url() -> String {
    with_database(&base_url(), TEMPLATE_NAME)
}

/// Ensure the template DB exists with every migration applied.
/// Idempotent; runs at most once per process.
fn ensure_template_ready() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let mut admin =
            PgConnection::establish(&admin_url()).expect("connect to admin DB (postgres)");

        // CREATE DATABASE IF NOT EXISTS isn't a thing in PG; do it
        // through a probe.
        let exists = diesel::sql_query(format!(
            "SELECT 1 AS one FROM pg_database WHERE datname = '{TEMPLATE_NAME}'"
        ))
        .execute(&mut admin)
        .map(|n| n > 0)
        .unwrap_or(false);

        if !exists {
            diesel::sql_query(format!("CREATE DATABASE \"{TEMPLATE_NAME}\""))
                .execute(&mut admin)
                .expect("CREATE template DB");
        }

        // Run all migrations against the template. Embedded
        // migrations are idempotent: re-running on an
        // already-migrated DB is a no-op.
        let mut template =
            PgConnection::establish(&template_url()).expect("connect to template DB");
        template
            .run_pending_migrations(MIGRATIONS)
            .expect("migrate template");

        // Mark the template so `CREATE DATABASE ... TEMPLATE`
        // doesn't refuse it. Idempotent.
        let _ = diesel::sql_query(format!(
            "ALTER DATABASE \"{TEMPLATE_NAME}\" IS_TEMPLATE TRUE"
        ))
        .execute(&mut admin);
    });
}

/// Per-test sandbox DB. Cloned from the template on construct,
/// dropped on Drop. Holds the URL so callers can establish their
/// own connections inside the test body.
pub struct TestDb {
    name: String,
    url: String,
}

impl TestDb {
    pub fn new() -> Self {
        ensure_template_ready();

        // 24 hex chars from a v4 UUID. v7's first 48 bits are a
        // timestamp, so tests starting in the same millisecond
        // collide if we truncate; v4 is fully random.
        let suffix: String = Uuid::new_v4().simple().to_string()[..24].to_string();
        let name = format!("nosdesk_test_{suffix}");
        let url = with_database(&base_url(), &name);

        let mut admin =
            PgConnection::establish(&admin_url()).expect("connect to admin DB for sandbox CREATE");
        diesel::sql_query(format!(
            "CREATE DATABASE \"{name}\" TEMPLATE \"{TEMPLATE_NAME}\""
        ))
        .execute(&mut admin)
        .expect("CREATE sandbox DB from template");

        Self { name, url }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Single-connection r2d2 pool over the sandbox DB. Matches
    /// the shape `backend::db::DbConnection` (a
    /// `PooledConnection`), which is what backup_service and
    /// every repository fn expect.
    pub fn pool(&self) -> TestPool {
        let manager = ConnectionManager::<PgConnection>::new(&self.url);
        r2d2::Pool::builder()
            .max_size(1)
            .connection_customizer(Box::new(WorkspaceGucCustomizer))
            .build(manager)
            .expect("build sandbox pool")
    }

    pub fn conn(&self) -> TestPooledConn {
        self.pool().get().expect("pool.get")
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // PG refuses DROP DATABASE while sessions are connected;
        // terminate first. If admin connect fails (shutdown race),
        // we silently leak — better than panicking out of Drop.
        if let Ok(mut admin) = PgConnection::establish(&admin_url()) {
            let _ = diesel::sql_query(format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                 WHERE datname = '{}' AND pid <> pg_backend_pid()",
                self.name
            ))
            .execute(&mut admin);
            let _ = diesel::sql_query(format!("DROP DATABASE IF EXISTS \"{}\"", self.name))
                .execute(&mut admin);
        }
    }
}

/// Hash a table's row content for round-trip equality. Uses
/// `string_agg(t::text, ',' ORDER BY ctid)` so equality is
/// row-set-level: same rows, any order. md5 over the
/// concatenated text representation. NULL-safe via COALESCE on
/// the aggregate.
pub fn hash_table(conn: &mut PgConnection, table: &str) -> String {
    use diesel::deserialize::QueryableByName;
    use diesel::sql_types::Text;

    #[derive(QueryableByName)]
    struct HashRow {
        #[diesel(sql_type = Text)]
        hash: String,
    }

    // Two-step: hash inside PG to keep the wire payload tiny,
    // even for tables with thousands of rows.
    let q = format!(
        "SELECT COALESCE(md5(string_agg(t::text, ',' ORDER BY t::text)), 'empty') AS hash \
         FROM \"{table}\" t"
    );
    diesel::sql_query(q)
        .get_result::<HashRow>(conn)
        .unwrap_or_else(|e| panic!("hash_table {table} failed: {e}"))
        .hash
}

/// Count rows in `table`. Convenience wrapper used in basic
/// row-level assertions.
pub fn count_table(conn: &mut PgConnection, table: &str) -> i64 {
    use diesel::deserialize::QueryableByName;
    use diesel::sql_types::BigInt;

    #[derive(QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }

    let q = format!("SELECT COUNT(*) AS count FROM \"{table}\"");
    diesel::sql_query(q)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("count_table {table} failed: {e}"))
        .count
}

// ---- Fixture helpers ---------------------------------------

/// Seed a user with a UUID PK. Returns the inserted row.
pub fn insert_user(conn: &mut PgConnection, name: &str) -> backend::models::User {
    use backend::models::{NewUser, UserRole};
    use backend::schema::users;
    let new_user = NewUser {
        uuid: Uuid::new_v4(),
        name: name.to_string(),
        role: UserRole::Admin,
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_secret_kek_id: None,
        mfa_enabled: false,
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .get_result(conn)
        .expect("insert user")
}

/// Seed a stock-tracked asset with the Phase E columns set.
/// Exercises NUMERIC(12,3), jsonb attributes, and the generic
/// kind path. Returns the new row's id.
pub fn insert_stock_asset(conn: &mut PgConnection, name: &str) -> i32 {
    use backend::models::NewAsset;
    use backend::schema::assets;
    use bigdecimal::BigDecimal;
    use std::str::FromStr;
    let new = NewAsset {
        name: name.to_string(),
        serial_number: None,
        manufacturer: Some("Acme".to_string()),
        model: Some("Pipe".to_string()),
        location: Some("Warehouse A".to_string()),
        notes: None,
        primary_user_uuid: None,
        purchase_date: None,
        asset_tag: None,
        kind: "generic".to_string(),
        attributes: serde_json::json!({"warranty_status": "Active", "color": "blue"}),
        quantity: Some(BigDecimal::from_str("123.456").unwrap()),
        unit: Some("m".to_string()),
        external_sync_source: None,
        low_stock_threshold: Some(BigDecimal::from_str("10.000").unwrap()),
    };
    diesel::insert_into(assets::table)
        .values(&new)
        .returning(assets::id)
        .get_result(conn)
        .expect("insert asset")
}

/// Seed a backup_jobs row so `create_backup` has something to
/// update on completion. Mirrors what `handlers::backup` does.
pub fn seed_backup_job(conn: &mut backend::db::DbConnection) -> Uuid {
    use backend::models::NewBackupJob;
    use backend::repository::backup as backup_repo;
    backup_repo::create_backup_job(
        conn,
        NewBackupJob {
            job_type: "export".to_string(),
            status: "processing".to_string(),
            include_sensitive: true,
            created_by: None,
        },
    )
    .expect("seed backup_jobs row")
    .id
}

/// Process-wide UPLOAD_DIR. Tests share one root because the
/// env var is global; per-test temp dirs would race each other
/// under `cargo test`'s parallel execution (test A's TempDir
/// Drop would wipe test B's backup file). Each `create_backup`
/// call generates a timestamped filename so backups don't
/// collide. The shared tempdir lives in a `OnceLock` so it
/// cleans up when the test process exits.
pub fn with_upload_dir() {
    static UPLOAD_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let dir = UPLOAD_DIR.get_or_init(|| {
        let d = tempfile::tempdir().expect("tempdir");
        std::env::set_var("UPLOAD_DIR", d.path());
        d
    });
    // Defensive re-set: another test's overwrite would have
    // pointed UPLOAD_DIR somewhere else, leaving the OnceLock
    // path stale. Stamping again on every call is cheap and
    // makes the helper robust to call ordering.
    std::env::set_var("UPLOAD_DIR", dir.path());
}

/// Resolve a path inside `tests/fixtures/`. Tests use this so
/// `cargo test` invocations don't depend on the current working
/// directory.
pub fn fixture_path(rel: &str) -> std::path::PathBuf {
    let manifest =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set under `cargo test`");
    std::path::PathBuf::from(manifest)
        .join("tests")
        .join("fixtures")
        .join(rel)
}

/// Names of every user table (ordinary + partitioned parent,
/// excluding partition children and Diesel's migration ledger).
/// Match the writer's view from `backup_service::create_backup`.
pub fn user_tables(conn: &mut PgConnection) -> Vec<String> {
    use diesel::deserialize::QueryableByName;
    use diesel::sql_types::Text;

    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        table_name: String,
    }

    diesel::sql_query(
        "SELECT c.relname AS table_name \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' \
           AND c.relkind IN ('r','p') \
           AND c.relispartition = false \
           AND c.relname <> '__diesel_schema_migrations' \
         ORDER BY c.relname",
    )
    .load::<Row>(conn)
    .expect("list user tables")
    .into_iter()
    .map(|r| r.table_name)
    .collect()
}
