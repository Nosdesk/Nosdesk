//! Test helpers — DB connection setup and fixture factories.
//!
//! Every connection from [`setup_test_pool`] is wrapped in a test transaction
//! via `r2d2::CustomizeConnection`, so tests are fully isolated and leave no
//! residue in the database.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager};
use diesel::Connection;
use diesel_migrations::MigrationHarness;
use once_cell::sync::OnceCell;
use std::time::Duration;
use uuid::Uuid;

use crate::db::{DbConnection, MIGRATIONS};
use crate::models::*;
use crate::schema::*;

/// Resolve the test database URL from env. Both `setup_test_connection`
/// and `setup_test_pool` use the same precedence: dedicated test DB
/// preferred, fall back to the dev DB only when explicitly configured.
fn test_database_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests")
}

/// Ensure the test database has all migrations applied. Runs once
/// per process. Without this, the first fixture insert fails with
/// `FailedToLookupTypeError(... "user_role" ...)` because Diesel
/// can't find the OID for custom Postgres enum types that the
/// migrations would have created.
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
    if INIT
        .get_or_try_init(|| -> Result<(), Box<dyn std::error::Error>> {
            let url = test_database_url();
            let mut conn = PgConnection::establish(&url).map_err(|e| {
                format!("Failed to connect to test DB for migration bootstrap: {e}")
            })?;
            conn.run_pending_migrations(MIGRATIONS)
                .map_err(|e| format!("Failed to apply migrations to test DB: {e}"))?;
            Ok(())
        })
        .is_err()
    {
        // Re-derive a panic so the test's failure message reads
        // the same as before for any tooling that parses the
        // panic line. The retry-on-next-call semantic comes from
        // `get_or_try_init` not memoising errors.
        panic!("Test DB migration bootstrap failed; see the previous error");
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
        Ok(())
    }
}

/// Obtain a single pooled connection wrapped in a test transaction.
///
/// Requires `TEST_DATABASE_URL` to point at a dedicated test database.
/// We deliberately do *not* fall back to `DATABASE_URL`: PostgreSQL
/// sequences are non-transactional, so every fixture insert would burn
/// an id out of the dev database's `tickets_id_seq` etc., pushing
/// ticket numbers into the thousands after a handful of `cargo test`
/// runs. The dev compose file provisions `helpdesk_test` and wires
/// this env var; see `init-db.sql`.
pub fn setup_test_connection() -> DbConnection {
    ensure_test_db_migrated();

    let database_url = std::env::var("TEST_DATABASE_URL").expect(
        "TEST_DATABASE_URL must be set (use a dedicated DB, not DATABASE_URL — \
         sequences advance even on rolled-back transactions and would trash \
         dev ticket/user ids)",
    );

    let manager = ConnectionManager::<PgConnection>::new(database_url);
    let pool = r2d2::Pool::builder()
        .max_size(1)
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
    pub fn create_user(conn: &mut DbConnection, name: &str, role: UserRole) -> User {
        let new_user = NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            role,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
        };

        diesel::insert_into(users::table)
            .values(&new_user)
            .get_result(conn)
            .expect("Failed to create test user")
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
        // Resolve the legacy "open" bucket to a concrete workflow state.
        // Using the legacy helper so the fixture continues to mean "the
        // open-equivalent state" regardless of workspace customisation.
        let open_state = crate::repository::workflow_states::state_for_legacy_status(conn, "open")
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

    let manager = ConnectionManager::<PgConnection>::new(test_database_url());
    r2d2::Pool::builder()
        .max_size(1)
        .connection_customizer(Box::new(TestTransaction))
        .connection_timeout(Duration::from_secs(5))
        .build(manager)
        .expect("Failed to create test pool")
}

/// Create a JWT token for a test user.
/// Requires JWT_SECRET to be set.
pub fn create_test_token(user: &User, session_id: &uuid::Uuid) -> String {
    // Ensure JWT_SECRET is set for tests
    if std::env::var("JWT_SECRET").is_err() {
        std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only-32chars");
    }
    crate::utils::jwt::JwtUtils::create_token(user, session_id)
        .expect("Failed to create test token")
}

/// Create test Claims for injecting into request extensions.
pub fn create_test_claims(user: &User) -> crate::models::Claims {
    crate::models::Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        role: user.role.as_str().to_string(),
        scope: "full".to_string(),
        sid: None,
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
pub fn claims_for(pool: &crate::db::Pool, role: UserRole) -> crate::models::Claims {
    let mut conn = pool.get().expect("test pool connection");
    let user = TestFixtures::create_user(
        &mut conn,
        &format!("permtest-{}", uuid::Uuid::now_v7()),
        role,
    );
    create_test_claims(&user)
}
