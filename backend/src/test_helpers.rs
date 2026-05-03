//! Test helpers — DB connection setup and fixture factories.
//!
//! Every connection from [`setup_test_pool`] is wrapped in a test transaction
//! via `r2d2::CustomizeConnection`, so tests are fully isolated and leave no
//! residue in the database.

use diesel::prelude::*;
use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel::Connection;
use diesel_migrations::MigrationHarness;
use std::sync::Once;
use std::time::Duration;
use uuid::Uuid;

use crate::db::{DbConnection, MIGRATIONS};
use crate::models::*;
use crate::schema::*;

/// Resolve the test database URL from env. Both `setup_test_connection`
/// and `setup_test_pool` use the same precedence: dedicated test DB
/// preferred, fall back to the dev DB only when explicitly configured.
fn test_database_url() -> String {
    dotenv::dotenv().ok();
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests")
}

/// Ensure the test database has all migrations applied. Runs once
/// per process via `Once`. Without this, the first fixture insert
/// fails with `FailedToLookupTypeError(... "user_role" ...)` because
/// Diesel can't find the OID for custom Postgres enum types that
/// the migrations would have created.
fn ensure_test_db_migrated() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let url = test_database_url();
        let mut conn = PgConnection::establish(&url)
            .expect("Failed to connect to test DB for migration bootstrap");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to apply migrations to test DB");
    });
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
            theme: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
            signature: None,
            dashboard_layout: None,
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
        let open_state =
            crate::repository::workflow_states::state_for_legacy_status(conn, "open")
                .expect("workflow_states must be seeded for tests");
        let new_ticket = NewTicket {
            title: title.to_string(),
            workflow_state_id: open_state.id,
            priority: TicketPriority::Medium,
            requester_uuid: requester,
            assignee_uuid: None,
            category_id,
            submitted_via: None,
            guest_lookup_token: None,
            verification_state: None,
            origin_channel_id: None,
            triage_state: None,
        };

        diesel::insert_into(tickets::table)
            .values(&new_ticket)
            .get_result(conn)
            .expect("Failed to create test ticket")
    }

    /// Insert a comment on a ticket and return it.
    pub fn create_comment(conn: &mut DbConnection, ticket_id: i32, user_uuid: Uuid, content: &str) -> Comment {
        let new_comment = NewComment {
            content: content.to_string(),
            ticket_id,
            user_uuid,
            channel_metadata: None,
            is_internal: false,
            content_format: Default::default(),
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
    pub fn create_user_email(conn: &mut DbConnection, user_uuid: Uuid, email: &str, is_primary: bool) -> UserEmail {
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
    crate::utils::jwt::JwtUtils::create_token(user, session_id).expect("Failed to create test token")
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
