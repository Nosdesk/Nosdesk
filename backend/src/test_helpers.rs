//! Test helpers — DB connection setup and fixture factories.
//!
//! Every connection from [`setup_test_pool`] is wrapped in a test transaction
//! via `r2d2::CustomizeConnection`, so tests are fully isolated and leave no
//! residue in the database.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2;
use diesel::Connection;
use diesel_migrations::MigrationHarness;
use once_cell::sync::OnceCell;
use std::sync::Mutex;
use std::time::Duration;
use uuid::Uuid;

use crate::db::{DbConnection, MIGRATIONS};
use crate::models::*;
use crate::schema::*;

/// The server's base test database: `TEST_DATABASE_URL`, else `DATABASE_URL`.
/// Unit tests run in their own database next to it (`test_database_url`).
fn base_database_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests")
}

/// `url` with its database name swapped for `db`.
fn with_database(url: &str, db: &str) -> String {
    let q = url.find('?').unwrap_or(url.len());
    let path_start = url[..q].rfind('/').expect("database URL has a path");
    format!("{}/{}{}", &url[..path_start], db, &url[q..])
}

/// This build's unit-test database. `build.rs` hashes `migrations/` into
/// `NOSDESK_SCHEMA_HASH`, so each migration set gets its own database and one
/// branch's migrations never land in another branch's.
fn unit_database_name() -> String {
    format!("nosdesk_test_unit_{}", env!("NOSDESK_SCHEMA_HASH"))
}

/// The database `setup_test_connection` and `setup_test_pool` connect to.
fn test_database_url() -> String {
    with_database(&base_database_url(), &unit_database_name())
}

/// Advisory lock serialising unit-test database setup across processes.
/// Taken on the `postgres` database (advisory locks are per database).
const UNIT_DB_LOCK: i64 = 0x6e6f_7364_756e_6974; // "nosdunit"

/// Run `f` on a fresh connection to the `postgres` database holding the
/// unit-test database lock. The lock is session-level, so a panic in `f`
/// releases it with the connection.
fn with_unit_db_lock<T>(
    f: impl FnOnce(&mut PgConnection) -> Result<T, String>,
) -> Result<T, String> {
    let mut admin = PgConnection::establish(&with_database(&base_database_url(), "postgres"))
        .map_err(|e| format!("connect to the postgres database: {e}"))?;
    diesel::sql_query(format!("SELECT pg_advisory_lock({UNIT_DB_LOCK})"))
        .execute(&mut admin)
        .map_err(|e| format!("take the unit-test database lock: {e}"))?;
    let out = f(&mut admin);
    let _ =
        diesel::sql_query(format!("SELECT pg_advisory_unlock({UNIT_DB_LOCK})")).execute(&mut admin);
    out
}

#[derive(QueryableByName)]
struct DatabaseName {
    #[diesel(sql_type = diesel::sql_types::Text)]
    datname: String,
}

/// Create database `name` if it's missing, then run `populate` on it; true
/// when this call created it. Needs the unit-test database lock.
fn ensure_database_locked(
    admin: &mut PgConnection,
    name: &str,
    populate: impl FnOnce(&mut PgConnection) -> Result<(), String>,
) -> Result<bool, String> {
    let exists = diesel::sql_query("SELECT datname FROM pg_database WHERE datname = $1")
        .bind::<diesel::sql_types::Text, _>(name)
        .get_result::<DatabaseName>(admin)
        .optional()
        .map_err(|e| format!("look up {name}: {e}"))?
        .is_some();
    if !exists {
        diesel::sql_query(format!("CREATE DATABASE \"{name}\""))
            .execute(admin)
            .map_err(|e| format!("CREATE DATABASE {name}: {e}"))?;
    }
    let mut conn = PgConnection::establish(&with_database(&base_database_url(), name))
        .map_err(|e| format!("connect to {name}: {e}"))?;
    populate(&mut conn)?;
    Ok(!exists)
}

/// Create database `name` once, however many processes ask at the same
/// time, and run `populate` on it for each. True when this call created it.
fn ensure_database(
    name: &str,
    populate: impl FnOnce(&mut PgConnection) -> Result<(), String>,
) -> Result<bool, String> {
    with_unit_db_lock(|admin| ensure_database_locked(admin, name, populate))
}

/// Advisory-lock key for the processes using a unit-test database: its
/// name's hash suffix, the key the integration tests hold for that schema's
/// template too. `None` for a name without one.
fn unit_database_key(name: &str) -> Option<i64> {
    let hash = name.strip_prefix("nosdesk_test_unit_")?;
    if hash.len() != 16 {
        return None;
    }
    // The hash is 64 bits; reinterpreting it as a signed key is lossless.
    u64::from_str_radix(hash, 16).ok().map(|key| key as i64)
}

/// Hold unit-test database `name`'s key shared for the rest of the process,
/// on a connection kept open for it, so a stale pass in another process
/// leaves the database alone between this process's connections.
fn hold_unit_database(name: &str) -> Result<(), String> {
    static HOLD: OnceCell<Mutex<PgConnection>> = OnceCell::new();
    let key = unit_database_key(name).ok_or_else(|| format!("{name} carries no hash"))?;
    let mut conn = PgConnection::establish(&with_database(&base_database_url(), "postgres"))
        .map_err(|e| format!("connect to the postgres database: {e}"))?;
    diesel::sql_query(format!("SELECT pg_advisory_lock_shared({key})"))
        .execute(&mut conn)
        .map_err(|e| format!("hold {name}: {e}"))?;
    let _ = HOLD.set(Mutex::new(conn));
    Ok(())
}

#[derive(QueryableByName)]
struct Locked {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    locked: bool,
}

/// Drop the unit-test databases of other migration sets that no process is
/// using. Switching branches then costs one migration replay. A process
/// using one holds its key shared (`hold_unit_database`), so a database whose
/// key can't be taken here is in use and stays.
fn drop_stale_unit_databases(admin: &mut PgConnection, keep: &str) {
    let stale = diesel::sql_query(
        "SELECT datname FROM pg_database \
         WHERE datname ~ '^nosdesk_test_unit_[0-9a-f]{16}$' AND datname <> $1",
    )
    .bind::<diesel::sql_types::Text, _>(keep)
    .load::<DatabaseName>(admin)
    .unwrap_or_default();
    for DatabaseName { datname } in stale {
        let Some(key) = unit_database_key(&datname) else {
            continue;
        };
        let free = diesel::sql_query(format!("SELECT pg_try_advisory_lock({key}) AS locked"))
            .get_result::<Locked>(admin)
            .is_ok_and(|row| row.locked);
        if !free {
            continue;
        }
        let _ = diesel::sql_query(format!("DROP DATABASE \"{datname}\"")).execute(admin);
        let _ = diesel::sql_query(format!("SELECT pg_advisory_unlock({key})")).execute(admin);
    }
}

/// Ensure this build's unit-test database exists with every migration
/// applied. Runs once per process. Without this, the first fixture insert
/// fails with `FailedToLookupTypeError(... "user_role" ...)` because Diesel
/// can't find the OID for custom Postgres enum types that the migrations
/// would have created.
///
/// Uses `OnceCell::get_or_try_init` rather than `std::sync::Once`
/// so an early connection failure (e.g. the dev compose stack
/// isn't running, or the test DB is briefly unreachable) errors
/// out just the test that triggered it. `std::sync::Once` would
/// poison the cell on any panic in the init closure, cascading
/// the failure into every subsequent test in the same process
/// with an opaque "instance has been poisoned" error.
fn ensure_test_db_migrated() {
    static INIT: OnceCell<()> = OnceCell::new();
    let init = INIT.get_or_try_init(|| -> Result<(), String> {
        let name = unit_database_name();
        with_unit_db_lock(|admin| {
            let created = ensure_database_locked(admin, &name, |conn| {
                conn.run_pending_migrations(MIGRATIONS)
                    .map(|_| ())
                    .map_err(|e| format!("Failed to apply migrations to test DB: {e}"))
            })?;
            // Still under the lock, so no stale pass can run between
            // setting the database up and holding it.
            hold_unit_database(&name)?;
            if created {
                drop_stale_unit_databases(admin, &name);
            }
            Ok(())
        })?;

        // Provision the partitions the current calendar month needs, once,
        // COMMITTED. Every pool test connection runs inside an uncommitted
        // test transaction (see `TestTransaction`), so if a data test were
        // the first to touch a not-yet-provisioned month it would run the
        // partition ATTACH inside that transaction and hold
        // `SHARE UPDATE EXCLUSIVE` on the parent for the whole test, which
        // deadlocks parallel tests (the `sync::partitions::tests`
        // PARTITION_TEST_LOCK only serialises its own module). Provisioning
        // here from `Utc::now` keeps `ensure_one_partition`'s `is_attached`
        // fast path a no-op in every other test, and survives month
        // rollover instead of relying on the migration's fixed seed months.
        //
        // A fresh single-connection pool with no `TestTransaction`
        // customizer so it commits; `ResettingManager` acquires with
        // `RESET ROLE`, i.e. the privileged login role the partition DDL
        // (`ALTER TABLE ... OWNER TO`/`ATTACH`) needs.
        let provisioning_pool = r2d2::Pool::builder()
            .max_size(1)
            .connection_timeout(Duration::from_secs(5))
            .build(crate::db::ResettingManager::new(test_database_url()))
            .map_err(|e| format!("Failed to build partition-provisioning pool: {e}"))?;
        let mut provisioning_conn = provisioning_pool
            .get()
            .map_err(|e| format!("Failed to check out partition-provisioning conn: {e}"))?;
        crate::sync::partitions::ensure_partitions(&mut provisioning_conn, 35)
            .map_err(|e| format!("Failed to provision test partitions: {e}"))?;
        Ok(())
    });
    // `get_or_try_init` doesn't memoise an error, so the next test retries.
    if let Err(e) = init {
        panic!("Test DB migration bootstrap failed: {e}");
    }
}

/// Connection customizer that begins a test transaction on every new
/// connection. Combined with `max_size(1)`, all code shares the same
/// connection and transaction. When the pool is dropped at test end the
/// transaction rolls back — zero residue.
#[derive(Debug)]
struct TestTransaction;

impl r2d2::CustomizeConnection<PgConnection, r2d2::Error> for TestTransaction {
    fn on_acquire(&self, conn: &mut PgConnection) -> Result<(), r2d2::Error> {
        conn.begin_test_transaction()
            .map_err(r2d2::Error::QueryError)?;
        // Mirror setup_test_connection: drop to nosdesk_app so RLS
        // policies apply in handler-level tests too (the connection
        // auths as nosdesk superuser which bypasses RLS), and
        // default the workspace GUC to the bootstrap workspace so
        // every tenant query and insert sees a populated value.
        // Without this, post-3h.4 handler tests fail because
        // set_actor's baseline `SET LOCAL ROLE nosdesk_app` drops
        // privileges on a connection that has no workspace pin,
        // and every tenant insert trips the strict WITH CHECK.
        diesel::sql_query("SET LOCAL ROLE nosdesk_app")
            .execute(conn)
            .map_err(r2d2::Error::QueryError)?;
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(r2d2::Error::QueryError)?;
        Ok(())
    }
}

/// Initialise the global at-rest encryption Keyring exactly once per
/// test process. Any test that touches a code path which calls
/// `utils::encryption::keyring()` (channel credentials, MFA secrets,
/// plugin secret settings, plugin local signing key) needs this; in
/// production `main.rs::init_keyring` runs at boot, but unit tests
/// don't run main.
///
/// Sets `MFA_KEK_V1` to a fixed 64-hex-char test key before delegating
/// to `init_keyring`. `std::sync::Once` guards against the
/// "init_keyring called twice" panic when many test fixtures call into
/// here from the same process.
fn ensure_test_keyring() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Stable test key — distinct, non-constant, passes the
        // validate_key_material checks. Reused across every test in
        // the process so generated frames decrypt back to the
        // same value if a downstream test reads what an upstream
        // test wrote.
        if std::env::var("MFA_KEK_V1").is_err() {
            std::env::set_var(
                "MFA_KEK_V1",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }
        // If `main` already initialised the keyring (e.g. an
        // integration test bringing the server up), respect that.
        if let Err(e) = crate::utils::encryption::init_keyring() {
            panic!("ensure_test_keyring: init_keyring failed: {e}");
        }
    });
}

/// Obtain a single pooled connection wrapped in a test transaction.
///
/// Connects to this build's unit-test database (`test_database_url`), a
/// database of its own next to `TEST_DATABASE_URL`'s, so fixture inserts
/// never advance another database's sequences.
pub fn setup_test_connection() -> DbConnection {
    ensure_test_db_migrated();
    ensure_test_keyring();

    let database_url = test_database_url();

    // test_on_check_out(false): keep the production GUC scrub off this
    // single held fixture connection so the role + workspace GUCs this
    // helper sets below survive for the test's lifetime.
    let manager = crate::db::ResettingManager::new(database_url);
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .test_on_check_out(false)
        .build(manager)
        .expect("Failed to create test connection pool");

    let mut conn = pool.get().expect("Failed to get test connection");
    conn.begin_test_transaction()
        .expect("Failed to begin test transaction");
    // Drop down to the non-superuser app role so RLS policies apply
    // to the test connection the same way they do in production.
    // Superusers (which the migration role is in dev) bypass RLS
    // unconditionally, even with FORCE RLS on the table; without
    // this SET ROLE every RLS test would silently pass nothing. The
    // `nosdesk_app` role is provisioned in the Phase 3a migration.
    diesel::sql_query("SET LOCAL ROLE nosdesk_app")
        .execute(&mut conn)
        .expect("Failed to drop to nosdesk_app role");
    // Default the workspace GUC to the bootstrap workspace so every
    // existing test that touches an RLS-enabled tenant table (Phase
    // 3a onwards) sees rows. Tests exercising cross-workspace
    // isolation override the GUC explicitly via with_actor_context.
    diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
        .execute(&mut conn)
        .expect("Failed to set default workspace GUC");
    conn
}

/// Convenience factories for common test fixtures.
pub struct TestFixtures;

impl TestFixtures {
    /// Insert a minimal user and return it.
    ///
    /// Also seeds a `workspace_members` row in the bootstrap workspace
    /// (id=1) so post-W2 gates (`require_workspace_role`) resolve
    /// against the role the test wants. Mapping: Admin → admin,
    /// Technician → agent, User → member, AuditReviewer → member.
    /// Handler-level unit tests' cfg(test) fallback in
    /// `require_workspace_role` pins workspace_id to 1.
    pub fn create_user(conn: &mut DbConnection, name: &str, role: &str) -> User {
        // Map the legacy test role string onto the W2 split, preserving
        // the pre-W2 fixture semantics: admin → platform_admin + ws
        // admin; technician → ws agent; audit_reviewer → platform
        // audit_reviewer; everything else → plain ws member. Without
        // this the DB default ('user' / no membership) wins and
        // admin-gated handlers reject the test caller.
        let (platform_role, workspace_role) = match role {
            "admin" => ("platform_admin", "admin"),
            "technician" => ("user", "agent"),
            "audit_reviewer" => ("audit_reviewer", "member"),
            _ => ("user", "member"),
        };
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
            platform_role: Some(platform_role.to_string()),
        };

        let user: User = diesel::insert_into(users::table)
            .values(&new_user)
            .get_result(conn)
            .expect("Failed to create test user");

        diesel::insert_into(crate::schema::workspace_members::table)
            .values((
                crate::schema::workspace_members::workspace_id.eq(1),
                crate::schema::workspace_members::user_uuid.eq(user.uuid),
                crate::schema::workspace_members::role.eq(workspace_role),
                crate::schema::workspace_members::accepted_at.eq(Some(chrono::Utc::now())),
            ))
            .on_conflict_do_nothing()
            .execute(conn)
            .expect("seed workspace_members for test user");

        user
    }

    /// Insert a group and return it.
    /// Insert a channel fixture and return it. Used by repository and
    /// service tests that need a channel to scope their messages against.
    /// Defaults are sensible for phase-1 email testing; override via
    /// `channels::update` if a test needs something different.
    pub fn create_channel(conn: &mut DbConnection, provider: &str) -> Channel {
        let new_channel = NewChannel {
            provider: provider.to_string(),
            name: format!("test-{provider}"),
            enabled: true,
            config: serde_json::json!({}),
        };

        diesel::insert_into(channels::table)
            .values(&new_channel)
            .get_result(conn)
            .expect("Failed to create test channel")
    }

    pub fn create_group(conn: &mut DbConnection, name: &str) -> Group {
        let new_group = NewGroup {
            name: name.to_string(),
            description: None,
            color: None,
            created_by: None,
        };

        diesel::insert_into(groups::table)
            .values(&new_group)
            .get_result(conn)
            .expect("Failed to create test group")
    }

    /// Add a user to a group.
    pub fn add_user_to_group(conn: &mut DbConnection, user_uuid: Uuid, group_id: i32) {
        let entry = NewUserGroup {
            user_uuid,
            group_id,
            created_by: None,
        };

        diesel::insert_into(user_groups::table)
            .values(&entry)
            .execute(conn)
            .expect("Failed to add user to group");
    }

    /// Insert a ticket category and return it.
    pub fn create_category(conn: &mut DbConnection, name: &str) -> TicketCategory {
        let new_cat = NewTicketCategory {
            name: name.to_string(),
            description: None,
            color: None,
            icon: None,
            display_order: 0,
            is_active: true,
            created_by: None,
            requester_visible: false,
            approval_required: false,
            approval_rule: "any".to_string(),
            approval_by_manager: false,
        };

        diesel::insert_into(ticket_categories::table)
            .values(&new_cat)
            .get_result(conn)
            .expect("Failed to create test category")
    }

    /// Restrict a category so only the given groups can see it.
    pub fn set_category_visibility(conn: &mut DbConnection, category_id: i32, group_ids: &[i32]) {
        for &gid in group_ids {
            let entry = NewCategoryGroupVisibility {
                category_id,
                group_id: gid,
                created_by: None,
            };

            diesel::insert_into(category_group_visibility::table)
                .values(&entry)
                .execute(conn)
                .expect("Failed to set category visibility");
        }
    }

    /// Insert a ticket and return it.
    pub fn create_ticket(
        conn: &mut DbConnection,
        title: &str,
        requester: Option<Uuid>,
        category_id: Option<i32>,
    ) -> Ticket {
        // Put the fixture in the workspace-default state (the non-terminal
        // entry point), regardless of workspace customisation.
        let open_state = crate::repository::workflow_states::default_state(conn)
            .expect("workflow_states must be seeded for tests");
        let new_ticket = NewTicket {
            title: title.to_string(),
            workflow_state_id: open_state.id,
            requester_uuid: requester,
            category_id,
            ..Default::default()
        };

        diesel::insert_into(tickets::table)
            .values(&new_ticket)
            .get_result(conn)
            .expect("Failed to create test ticket")
    }

    /// Record `source` as merged into `destination`, as a merge does: a
    /// `ticket_merges` row.
    pub fn mark_merged(conn: &mut DbConnection, source: &Ticket, destination: &Ticket, by: Uuid) {
        diesel::insert_into(ticket_merges::table)
            .values((
                ticket_merges::ticket_id.eq(source.id),
                ticket_merges::merged_into_ticket_id.eq(destination.id),
                ticket_merges::merged_at.eq(chrono::Utc::now()),
                ticket_merges::merged_by_user_uuid.eq(by),
            ))
            .execute(conn)
            .expect("Failed to record the merge");
    }

    /// Give `ticket` a number unlike its id. In the shared test database a
    /// workspace's numbers and ids advance together, so a test that must tell
    /// them apart needs this.
    pub fn renumber_ticket(conn: &mut DbConnection, ticket: Ticket) -> Ticket {
        diesel::update(tickets::table.find(ticket.id))
            .set(tickets::number.eq(ticket.id + 1_000_000))
            .get_result(conn)
            .expect("Failed to renumber test ticket")
    }

    /// Insert a comment on a ticket and return it.
    pub fn create_comment(
        conn: &mut DbConnection,
        ticket_id: i32,
        user_uuid: Uuid,
        content: &str,
    ) -> Comment {
        let new_comment = NewComment {
            content: content.to_string(),
            ticket_id,
            user_uuid,
            ..Default::default()
        };

        diesel::insert_into(comments::table)
            .values(&new_comment)
            .get_result(conn)
            .expect("Failed to create test comment")
    }

    /// Insert an attachment on a comment and return it.
    pub fn create_attachment(conn: &mut DbConnection, comment_id: i32, name: &str) -> Attachment {
        let new_att = NewAttachment {
            url: format!("/uploads/tickets/{name}"),
            name: name.to_string(),
            file_size: Some(1024),
            mime_type: Some("application/pdf".to_string()),
            checksum: None,
            comment_id: Some(comment_id),
            uploaded_by: None,
            transcription: None,
        };

        diesel::insert_into(attachments::table)
            .values(&new_att)
            .get_result(conn)
            .expect("Failed to create test attachment")
    }

    /// Insert a user email and return it.
    pub fn create_user_email(
        conn: &mut DbConnection,
        user_uuid: Uuid,
        email: &str,
        is_primary: bool,
    ) -> UserEmail {
        let new_email = NewUserEmail {
            user_uuid,
            email: email.to_string(),
            email_type: "personal".to_string(),
            is_primary,
            is_verified: true,
            source: None,
        };

        diesel::insert_into(user_emails::table)
            .values(&new_email)
            .get_result(conn)
            .expect("Failed to create test user email")
    }

    /// Insert a project and return it.
    pub fn create_project(conn: &mut DbConnection, name: &str) -> Project {
        let new_project = NewProject {
            name: name.to_string(),
            description: None,
            status: ProjectStatus::Active,
            start_date: None,
            end_date: None,
        };

        diesel::insert_into(projects::table)
            .values(&new_project)
            .get_result(conn)
            .expect("Failed to create test project")
    }
}

// ============================================================================
// Handler Test Utilities
// ============================================================================

/// Create a test database pool for handler tests.
///
/// Every connection is wrapped in a test transaction that rolls back on drop.
/// `max_size(1)` ensures all code shares the same connection and transaction,
/// so fixture data created by the test is visible to handlers.
///
/// **Important**: Tests must drop their fixture connection before making HTTP
/// calls, otherwise the single-connection pool will deadlock.
pub fn setup_test_pool() -> crate::db::Pool {
    ensure_test_db_migrated();
    ensure_test_keyring();

    // test_on_check_out(false): the TestTransaction customizer sets the
    // role + ambient workspace GUC once on acquire; the production checkout
    // scrub would clear them between the handler's pool checkouts, so it
    // stays off for the test pool.
    let manager = crate::db::ResettingManager::new(test_database_url());
    r2d2::Pool::builder()
        .max_size(1)
        .connection_customizer(Box::new(TestTransaction))
        .connection_timeout(Duration::from_secs(5))
        .test_on_check_out(false)
        .build(manager)
        .expect("Failed to create test pool")
}

/// Assert that a route `configure` fn registers every `(method, path)` in
/// `cases`. This is the regression net for the per-domain route-probe tests
/// that guard the `main.rs` -> `config` extraction.
///
/// Each `(method, path)` is sent with its real method, and the app carries a
/// sentinel `default_service`: a *registered* route dispatches to its handler
/// (any status — even a handler-level 404 from a `web::Path<Uuid>` rejecting a
/// placeholder, a `WorkspaceContext` with no middleware, or a handler that
/// returns `NotFound`), while an *unregistered* method/path falls through to
/// the sentinel. So "response is not the sentinel" means the exact route is
/// registered — immune to handler-level 404s. `{param}` segments accept any
/// value, so the cases use placeholders.
pub async fn assert_config_registers(
    configure: fn(&mut actix_web::web::ServiceConfig),
    cases: &[(&str, &str)],
) {
    use actix_web::{http::Method, http::StatusCode, test, web, App, HttpResponse};
    let sentinel = StatusCode::from_u16(599).unwrap();
    let pool = setup_test_pool();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure)
            .default_service(
                web::route().to(move || async move { HttpResponse::build(sentinel).finish() }),
            ),
    )
    .await;
    for (method, path) in cases {
        let req = test::TestRequest::default()
            .method(Method::from_bytes(method.as_bytes()).expect("valid HTTP method"))
            .uri(path)
            .to_request();
        // `try_call_service` (not `call_service`) so a route whose auth
        // middleware short-circuits with an `Err` (e.g. cookie_auth's
        // "Authentication required") doesn't panic: the middleware running at
        // all means the route matched, i.e. it is registered. Only a
        // sentinel-status success means the request fell through to the
        // default service (unregistered).
        match test::try_call_service(&app, req).await {
            Ok(resp) => assert_ne!(
                resp.status(),
                sentinel,
                "route not registered by config(): {method} {path}"
            ),
            Err(_) => { /* matched a route; its middleware errored — registered */ }
        }
    }
}

/// Create a JWT token for a test user with the given role.
/// Requires JWT_SECRET to be set.
pub fn create_test_token(user: &User, session_id: &uuid::Uuid) -> String {
    // Ensure JWT_SECRET is set for tests
    if std::env::var("JWT_SECRET").is_err() {
        std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only-32chars");
    }
    crate::utils::jwt::JwtUtils::create_token(user, session_id)
        .expect("Failed to create test token")
}

/// Create test Claims for injecting into request extensions. The
/// platform role is read off the user row (seeded by
/// `create_user`); the per-workspace role is resolved per-request
/// from `workspace_members`, so it isn't carried here.
pub fn create_test_claims(user: &User) -> crate::models::Claims {
    crate::models::Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: user.platform_role.clone(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// Build claims for a fresh user with the given role. Compresses the
/// "create user, drop conn, build claims" three-step that every
/// permission-matrix test was open-coding.
///
/// The connection is acquired and dropped synchronously inside the
/// helper, so handler tests are free to call into `test::call_service`
/// immediately after — the single-connection test pool won't deadlock.
pub fn claims_for(pool: &crate::db::Pool, role: &str) -> crate::models::Claims {
    let mut conn = pool.get().expect("test pool connection");
    let user = TestFixtures::create_user(
        &mut conn,
        &format!("permtest-{}", uuid::Uuid::now_v7()),
        role,
    );
    create_test_claims(&user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::sql_types::{Bool, Text};
    use std::sync::{Arc, Barrier};

    #[derive(QueryableByName)]
    struct Seen {
        #[diesel(sql_type = Text)]
        db: String,
        #[diesel(sql_type = Bool)]
        foreign: bool,
    }

    /// A schema standing in for a migration only another branch has, applied
    /// to the shared base database the way every branch's unit tests used to
    /// apply theirs. Dropped however the test ends.
    struct ForeignObject {
        url: String,
        schema: String,
    }

    impl Drop for ForeignObject {
        fn drop(&mut self) {
            if let Ok(mut conn) = PgConnection::establish(&self.url) {
                let _ = diesel::sql_query(format!("DROP SCHEMA IF EXISTS \"{}\"", self.schema))
                    .execute(&mut conn);
            }
        }
    }

    #[test]
    fn another_branchs_migration_stays_out_of_unit_tests() {
        let base = base_database_url();
        let schema = format!("g7_foreign_{}", &Uuid::new_v4().simple().to_string()[..12]);
        let mut base_conn = PgConnection::establish(&base).expect("connect to the base test DB");
        diesel::sql_query(format!("CREATE SCHEMA \"{schema}\""))
            .execute(&mut base_conn)
            .expect("plant the foreign schema");
        let _foreign = ForeignObject {
            url: base,
            schema: schema.clone(),
        };

        let mut conn = setup_test_connection();
        let seen = diesel::sql_query(
            "SELECT current_database()::text AS db, \
                    EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1) AS foreign",
        )
        .bind::<Text, _>(&schema)
        .get_result::<Seen>(&mut conn)
        .expect("look at the unit-test DB");
        assert_eq!(
            seen.db,
            format!("nosdesk_test_unit_{}", env!("NOSDESK_SCHEMA_HASH")),
            "unit tests run in the database for this build's migrations"
        );
        assert!(
            !seen.foreign,
            "the unit-test DB has `{schema}`, which only the shared base DB has"
        );
    }

    /// Drops a throwaway database however the test ends.
    struct Throwaway(String);

    impl Drop for Throwaway {
        fn drop(&mut self) {
            let admin_url = with_database(&base_database_url(), "postgres");
            if let Ok(mut admin) = PgConnection::establish(&admin_url) {
                let _ = diesel::sql_query(format!(
                    "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                    self.0
                ))
                .execute(&mut admin);
            }
        }
    }

    /// Separate sessions racing for a database that doesn't exist yet: one
    /// creates it and every caller gets it.
    #[test]
    fn concurrent_first_use_creates_the_database_once() {
        const CALLERS: usize = 4;
        let name = format!(
            "nosdesk_unit_race_{}",
            &Uuid::new_v4().simple().to_string()[..16]
        );
        let _cleanup = Throwaway(name.clone());
        let start = Arc::new(Barrier::new(CALLERS));
        let callers: Vec<_> = (0..CALLERS)
            .map(|_| {
                let (name, start) = (name.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    ensure_database(&name, |conn| {
                        diesel::sql_query("CREATE TABLE IF NOT EXISTS populated (id int)")
                            .execute(conn)
                            .map_err(|e| format!("populate: {e}"))?;
                        // Long enough for every other caller to arrive mid-setup.
                        std::thread::sleep(Duration::from_millis(300));
                        Ok(())
                    })
                })
            })
            .collect();
        let results: Vec<Result<bool, String>> = callers
            .into_iter()
            .map(|caller| caller.join().expect("caller thread"))
            .collect();

        assert!(
            results.iter().all(|r| r.is_ok()),
            "every concurrent caller gets the database: {results:?}"
        );
        assert_eq!(
            results.iter().filter(|r| matches!(r, Ok(true))).count(),
            1,
            "exactly one caller creates it: {results:?}"
        );
    }

    fn database_exists(conn: &mut PgConnection, name: &str) -> bool {
        diesel::sql_query("SELECT datname FROM pg_database WHERE datname = $1")
            .bind::<Text, _>(name)
            .get_result::<DatabaseName>(conn)
            .optional()
            .expect("look up pg_database")
            .is_some()
    }

    /// A stale pass drops a unit-test database nobody holds and keeps one
    /// another process holds, even while it has no connection open to it.
    #[test]
    fn a_unit_database_in_use_survives_a_stale_pass() {
        let fake = || {
            let hash = Uuid::new_v4().simple().to_string()[..16].to_string();
            let key = u64::from_str_radix(&hash, 16).expect("hex") as i64;
            (format!("nosdesk_test_unit_{hash}"), key)
        };
        let (held, held_key) = fake();
        let (unheld, _) = fake();
        let _held_cleanup = Throwaway(held.clone());
        let _unheld_cleanup = Throwaway(unheld.clone());

        let admin_url = with_database(&base_database_url(), "postgres");
        let mut admin = PgConnection::establish(&admin_url).expect("connect to admin DB");
        for name in [&held, &unheld] {
            diesel::sql_query(format!("CREATE DATABASE \"{name}\""))
                .execute(&mut admin)
                .expect("create database");
        }
        // Another process using `held` holds its key shared.
        let mut other_process =
            PgConnection::establish(&admin_url).expect("connect as another process");
        diesel::sql_query(format!("SELECT pg_advisory_lock_shared({held_key})"))
            .execute(&mut other_process)
            .expect("hold the database");

        with_unit_db_lock(|lock| {
            drop_stale_unit_databases(lock, &unit_database_name());
            Ok(())
        })
        .expect("stale pass");

        assert_eq!(
            (
                database_exists(&mut admin, &held),
                database_exists(&mut admin, &unheld)
            ),
            (true, false),
            "(held, unheld): a held database is kept and an unheld one is dropped"
        );
    }
}
