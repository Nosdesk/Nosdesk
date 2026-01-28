//! Test helpers — DB connection setup and fixture factories.
//!
//! Every connection returned by [`setup_test_connection`] is wrapped in a
//! transaction that is **never committed**, so tests are fully isolated and
//! leave no residue in the database.

use diesel::prelude::*;
use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager};
use diesel::Connection;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

/// Obtain a pooled connection wrapped in a test transaction.
///
/// Uses `DATABASE_URL` (same DB the dev container already has) — safe because
/// `begin_test_transaction` ensures everything is rolled back on drop.
pub fn setup_test_connection() -> DbConnection {
    dotenv::dotenv().ok();

    let database_url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("DATABASE_URL or TEST_DATABASE_URL must be set for tests");

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
            passkey_credentials: None,
        };

        diesel::insert_into(users::table)
            .values(&new_user)
            .get_result(conn)
            .expect("Failed to create test user")
    }

    /// Insert a group and return it.
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
        let new_ticket = NewTicket {
            title: title.to_string(),
            status: TicketStatus::Open,
            priority: TicketPriority::Medium,
            requester_uuid: requester,
            assignee_uuid: None,
            category_id,
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
/// Unlike `setup_test_connection`, this returns a Pool that can be used with `web::Data`.
/// Note: Tests using this pool share the same database state.
pub fn setup_test_pool() -> crate::db::Pool {
    dotenv::dotenv().ok();

    let database_url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("DATABASE_URL or TEST_DATABASE_URL must be set for tests");

    let manager = ConnectionManager::<PgConnection>::new(database_url);
    r2d2::Pool::builder()
        .max_size(2)
        .build(manager)
        .expect("Failed to create test pool")
}

/// Create a JWT token for a test user.
/// Requires JWT_SECRET to be set.
pub fn create_test_token(user: &User) -> String {
    // Ensure JWT_SECRET is set for tests
    if std::env::var("JWT_SECRET").is_err() {
        std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only-32chars");
    }
    crate::utils::jwt::JwtUtils::create_token(user).expect("Failed to create test token")
}

/// Create test Claims for injecting into request extensions.
pub fn create_test_claims(user: &User) -> crate::models::Claims {
    crate::models::Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        role: format!("{:?}", user.role).to_lowercase(),
        scope: "full".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}
