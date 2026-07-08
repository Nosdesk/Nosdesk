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
use diesel::r2d2;
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

pub type TestPool = backend::db::Pool;
pub type TestPooledConn = backend::db::DbConnection;

/// Init the at-rest Keyring + JWT secret once per process.
/// `backend::test_helpers::ensure_test_keyring` is `#[cfg(test)]`-
/// gated inside the crate so it's invisible to integration tests
/// (separate crate). This helper is the integration-tests-side twin.
///
/// Idempotent under `Once`, safe to call from every test fn at the
/// top, even those that don't touch encryption (defensive against
/// future encrypted-column additions that would silently panic).
pub fn ensure_test_keyring() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        if std::env::var("MFA_KEK_V1").is_err() {
            std::env::set_var(
                "MFA_KEK_V1",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-32-characters-min-for-tests");
        }
        if let Err(e) = backend::utils::encryption::init_keyring() {
            panic!("ensure_test_keyring: init_keyring failed: {e}");
        }
    });
}

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
        self.pool_with_size(1)
    }

    /// Pool variant for tests that need to hold more than one
    /// connection at a time (e.g. an actix-test server handling
    /// concurrent requests). M5 integration tests use size 4 so
    /// the middleware stack can grab a conn for the idempotency
    /// cache check while the handler still owns its own.
    pub fn pool_with_size(&self, max_size: u32) -> TestPool {
        let manager = backend::db::ResettingManager::new(&self.url);
        r2d2::Pool::builder()
            .max_size(max_size)
            .connection_customizer(Box::new(WorkspaceGucCustomizer))
            // Integration tests seed app.workspace_id=1 per connection and
            // read it ambiently (they run outside the request middleware), so
            // keep the production per-checkout GUC scrub off this pool.
            .test_on_check_out(false)
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
    use backend::models::NewUser;
    use backend::schema::users;
    let new_user = NewUser {
        uuid: Uuid::new_v4(),
        name: name.to_string(),
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_secret_kek_id: None,
        mfa_enabled: false,
        // Bootstrap admin: `users.role` column was dropped in the
        // W2 cleanup; platform_admin + the workspace_members row
        // (seeded by the migration backfill) drive the legacy
        // projection.
        platform_role: Some("platform_admin".to_string()),
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .get_result(conn)
        .expect("insert user")
}

/// Mint a user-bound `api_tokens` row directly. Returns the raw token
/// string for use in the Authorization header.
pub fn mint_api_token(conn: &mut PgConnection, user: &backend::models::User, name: &str) -> String {
    use backend::models::NewApiToken;
    use backend::repository::api_tokens::{get_token_prefix, hash_token};
    use backend::schema::api_tokens;
    let raw = format!("nsk_test_{}", Uuid::new_v4().simple());
    let new_token = NewApiToken {
        token_hash: hash_token(&raw),
        token_prefix: get_token_prefix(&raw),
        user_uuid: user.uuid,
        name: name.to_string(),
        scopes: Some(vec![Some("full".to_string())]),
        created_by: user.uuid,
        expires_at: None,
    };
    diesel::insert_into(api_tokens::table)
        .values(&new_token)
        .execute(conn)
        .expect("insert api_token");
    raw
}

// --- Platform provisioning (EdDSA JWT) test helpers --------------------
//
// The `/api/internal/v1/*` surface authenticates with an EdDSA JWT
// verified by `extractors::PlatformAuth`. These helpers configure the
// process env to trust a throwaway keypair and mint signed tokens, so
// the provisioning integration tests don't need the control plane.

/// Throwaway Ed25519 keypair, for tests only. NOT a production key.
pub const PLATFORM_TEST_PRIV: &str = "-----BEGIN PRIVATE KEY-----\n\
    MC4CAQAwBQYDK2VwBCIEIO6Su/YmjzEi0murpwXB/YjsQHnYIjRqJDJaxagBTQ88\n\
    -----END PRIVATE KEY-----\n";
pub const PLATFORM_TEST_PUB: &str = "-----BEGIN PUBLIC KEY-----\n\
    MCowBQYDK2VwAyEAbQxmQHWB+LZXvtyh54SrZM41ptz/WroW9djdAx1HPZQ=\n\
    -----END PUBLIC KEY-----\n";
pub const PLATFORM_TEST_ISS: &str = "https://control.test";

/// Put the process in hosted mode and configure platform verification
/// against the test keypair. Idempotent (every caller sets the same
/// values), so concurrent tests in one binary don't race on the value.
pub fn enable_platform_auth() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("PLATFORM_PUBLIC_KEY", PLATFORM_TEST_PUB);
    std::env::set_var("PLATFORM_ISSUER", PLATFORM_TEST_ISS);
}

/// Sign a platform JWT with the test private key. `scope` /
/// `exp_offset` (seconds from now; negative = expired) are explicit so
/// tests can exercise the reject paths.
pub fn mint_platform_jwt(scope: &str, exp_offset: i64) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    #[derive(serde::Serialize)]
    struct Claims<'a> {
        iss: &'a str,
        scope: &'a str,
        exp: usize,
    }
    let exp = (chrono::Utc::now().timestamp() + exp_offset).max(0) as usize;
    encode(
        &Header::new(Algorithm::EdDSA),
        &Claims {
            iss: PLATFORM_TEST_ISS,
            scope,
            exp,
        },
        &EncodingKey::from_ed_pem(PLATFORM_TEST_PRIV.as_bytes()).expect("encode key"),
    )
    .expect("mint platform jwt")
}

/// Insert a workspaces row with a freshly-generated UUID. Returns
/// the inserted row's id.
pub fn mint_workspace(conn: &mut PgConnection, slug: &str, name: &str) -> i32 {
    use backend::models::NewWorkspace;
    use backend::schema::workspaces;
    let new_ws = NewWorkspace {
        uuid: Uuid::now_v7(),
        slug: slug.to_string(),
        name: name.to_string(),
        seat_limit: None,
    };
    diesel::insert_into(workspaces::table)
        .values(&new_ws)
        .returning(workspaces::id)
        .get_result::<i32>(conn)
        .expect("insert workspace")
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

// ---- Two-workspace isolation fixture (Border C / C0) -------
//
// Seeds two independent workspaces (A and B), each with its own
// admin + plain member, a webhook subscribed to a shared event
// type, and one installed plugin. The webhook event types
// deliberately OVERLAP across A and B so cross-tenant fan-out
// tests are meaningful: an event raised in A must never reach B's
// subscriber (C1), and vice versa. This is the verification
// substrate for C1 (cross-tenant webhook delivery), C2 (plugin
// event injection), and C3 (API-token role escalation).

/// The event type both workspaces' webhooks subscribe to. Shared
/// on purpose: cross-tenant isolation tests need an A-event that a
/// B-only subscriber would (wrongly) match on event type alone, so
/// the only thing keeping them apart is the workspace predicate.
pub const FIXTURE_WEBHOOK_EVENT: &str = "ticket.created";

/// The event type the seeded plugins declare in their manifest.
/// C2 asserts an emit for a manifest-declared event is accepted
/// and a non-declared one is rejected.
pub const FIXTURE_PLUGIN_EVENT: &str = "ticket.created";

/// One workspace's seeded contents. All ids/uuids are captured so
/// later tests can pin a connection to this workspace and assert
/// what is (and isn't) visible from the other.
#[derive(Debug, Clone)]
pub struct WorkspaceSeed {
    pub workspace_id: i32,
    pub workspace_uuid: Uuid,
    pub slug: String,
    /// Workspace-role `admin` member (NOT a platform super-admin;
    /// `platform_role` is NULL, matching a real workspace admin).
    pub admin_uuid: Uuid,
    /// Workspace-role `member` (plain, non-privileged).
    pub member_uuid: Uuid,
    /// A webhook subscribed to [`FIXTURE_WEBHOOK_EVENT`], enabled.
    pub webhook_id: i32,
    pub webhook_uuid: Uuid,
    /// An installed (active) plugin whose manifest declares
    /// [`FIXTURE_PLUGIN_EVENT`].
    pub plugin_id: i32,
    pub plugin_uuid: Uuid,
    pub plugin_name: String,
}

/// A+B, seeded and independent. Pass to isolation assertions.
#[derive(Debug, Clone)]
pub struct TwoWorkspaces {
    pub a: WorkspaceSeed,
    pub b: WorkspaceSeed,
}

/// Insert a plain (non-platform-admin) user. Fixture members are
/// workspace-scoped actors, so `platform_role` stays NULL — the
/// "admin" distinction is the workspace_members.role, not a
/// platform super-admin bit (unlike [`insert_user`], which mints a
/// bootstrap platform admin).
fn insert_plain_user(conn: &mut PgConnection, name: &str) -> Uuid {
    use backend::models::NewUser;
    use backend::schema::users;
    let u: backend::models::User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert plain user");
    u.uuid
}

/// Seed one workspace end-to-end and return its handle. `label` is
/// woven into the slug/name/plugin so A and B never collide.
fn seed_one_workspace(conn: &mut TestPooledConn, label: &str) -> WorkspaceSeed {
    use backend::models::{NewPlugin, NewWorkspace, PluginState};
    use backend::repository::webhooks::create_webhook;
    use backend::repository::workspaces::{add_membership, create_workspace};
    use backend::schema::plugins;
    use backend::services::seed::seed_workspace_defaults;
    use backend::sync::actor::ActorContext;
    use backend::sync::session::with_actor_context;

    // Unique suffix so repeated fixture calls in one binary (each on
    // its own sandbox DB, but belt-and-braces) never trip the
    // workspaces.slug UNIQUE / retired-slug guards.
    let suffix = Uuid::new_v4().simple().to_string()[..8].to_string();
    let slug = format!("fixture-{label}-{suffix}");

    // Workspaces + users are global tables (no RLS workspace scope),
    // so they seed on the ambient GUC=1 connection.
    let ws = create_workspace(
        conn,
        &NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: slug.clone(),
            name: format!("Fixture Workspace {label}"),
            seat_limit: None,
        },
    )
    .expect("create workspace");
    assert_ne!(ws.id, 1, "fixture must use a non-bootstrap workspace");

    let admin_uuid = insert_plain_user(conn, &format!("{label} Admin"));
    let member_uuid = insert_plain_user(conn, &format!("{label} Member"));

    // Everything below is workspace-scoped: run pinned to `ws.id` as
    // the workspace admin so the tenant-table `workspace_id` column
    // defaults (driven by the `app.workspace_id` GUC) stamp the right
    // workspace, RLS WITH CHECK passes, and audit triggers attribute
    // the writes. One transaction per workspace.
    let actor = ActorContext::user(admin_uuid, None).with_workspace(ws.id);
    let (webhook_id, webhook_uuid, plugin_id, plugin_uuid, plugin_name) =
        with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
            // Usable defaults (workflow states / SLA / categories).
            seed_workspace_defaults(c, Some(admin_uuid))?;

            add_membership(c, ws.id, admin_uuid, "admin")?;
            add_membership(c, ws.id, member_uuid, "member")?;

            let webhook = create_webhook(
                c,
                format!("{label} webhook"),
                format!("https://sink.invalid/{label}"),
                format!("secret-{label}"),
                vec![FIXTURE_WEBHOOK_EVENT.to_string()],
                None,
                Some(admin_uuid),
            )?;

            // Insert the plugin row directly: the repository's
            // `create_plugin` demands an `InstallToken`, whose only
            // test constructor is `#[cfg(test)]`-gated inside the
            // crate and thus invisible to this integration-test crate.
            // A direct Insertable insert is the sanctioned test path;
            // `workspace_id` falls to the GUC-driven column default.
            let plugin_name = format!("fixture-plugin-{label}");
            let manifest = serde_json::json!({
                "name": plugin_name,
                "version": "1.0.0",
                // Manifest-declared events C2 will validate emits against.
                "events": [FIXTURE_PLUGIN_EVENT],
            });
            let new_plugin = NewPlugin {
                name: plugin_name.clone(),
                display_name: format!("{label} Fixture Plugin"),
                version: "1.0.0".to_string(),
                description: Some("Two-workspace isolation fixture plugin".to_string()),
                manifest,
                state: PluginState::Installed,
                trust_level: "verified".to_string(),
                installed_by: Some(admin_uuid),
                source: "provisioned".to_string(),
                signer_pubkey: None,
                signer_source: None,
                signature_metadata: None,
                icon_svg: None,
            };
            let plugin: backend::models::Plugin = diesel::insert_into(plugins::table)
                .values(&new_plugin)
                .get_result(c)?;

            Ok((
                webhook.id,
                webhook.uuid,
                plugin.id,
                plugin.uuid,
                plugin_name,
            ))
        })
        .expect("seed workspace-scoped fixture rows");

    WorkspaceSeed {
        workspace_id: ws.id,
        workspace_uuid: ws.uuid,
        slug,
        admin_uuid,
        member_uuid,
        webhook_id,
        webhook_uuid,
        plugin_id,
        plugin_uuid,
        plugin_name,
    }
}

/// Seed two independent workspaces (A and B) with overlapping
/// webhook/plugin event subscriptions and distinct members. See the
/// module-level fixture comment for intent. Call on a connection
/// from a [`TestDb`] pool.
pub fn seed_two_workspaces(conn: &mut TestPooledConn) -> TwoWorkspaces {
    TwoWorkspaces {
        a: seed_one_workspace(conn, "a"),
        b: seed_one_workspace(conn, "b"),
    }
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
