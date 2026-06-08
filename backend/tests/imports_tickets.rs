//! CSV ticket importer integration tests.
//!
//! Tickets reference users by primary email, so each test
//! seeds the user prerequisites via the user importer before
//! running the ticket flow. The cross-importer setup doubles
//! as a smoke test of the multi-step "import users, then
//! tickets that mention them" workflow.

mod common;

use diesel::prelude::*;

use backend::services::imports::{self, csv_parser, ImportType};

use common::{count_table, fixture_path, TestDb};

fn seed_user_prerequisites(conn: &mut backend::db::DbConnection) {
    let parsed =
        csv_parser::parse_file(&fixture_path("imports/users_valid.csv")).expect("parse users");
    imports::commit(conn, ImportType::Users, &parsed).expect("commit users");
}

#[test]
fn happy_path_creates_tickets_with_resolved_refs() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed_user_prerequisites(&mut conn);

    let tickets_before = count_table(&mut conn, "tickets");
    let parsed = csv_parser::parse_file(&fixture_path("imports/tickets_valid.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Tickets, &parsed).expect("dry-run");
    assert_eq!(summary.row_count, 3);
    assert_eq!(summary.would_create, 3);
    assert!(summary.errors.is_empty(), "errors: {:?}", summary.errors);

    let records = imports::commit(&mut conn, ImportType::Tickets, &parsed).expect("commit");
    assert_eq!(records.len(), 3);
    assert_eq!(count_table(&mut conn, "tickets"), tickets_before + 3);

    // Sentinel: the onboarding ticket has the right priority,
    // requester, and workflow state.
    #[derive(diesel::QueryableByName)]
    struct TicketRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        title: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        priority: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        workflow_state_name: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        requester_email: Option<String>,
    }
    let row: TicketRow = diesel::sql_query(
        "SELECT t.title, t.priority::text AS priority, ws.name AS workflow_state_name, \
                ue.email AS requester_email \
         FROM tickets t \
         JOIN workflow_states ws ON ws.id = t.workflow_state_id \
         LEFT JOIN user_emails ue ON ue.user_uuid = t.requester_uuid AND ue.is_primary \
         WHERE t.title = 'Onboarding kit for new hire'",
    )
    .get_result(&mut *conn)
    .expect("re-read ticket");
    assert_eq!(row.title, "Onboarding kit for new hire");
    assert_eq!(row.priority, "high");
    assert_eq!(row.workflow_state_name, "Backlog");
    assert_eq!(row.requester_email.as_deref(), Some("alex.kim@example.com"));
}

#[test]
fn errors_csv_classifies_each_failure() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed_user_prerequisites(&mut conn);

    let parsed =
        csv_parser::parse_file(&fixture_path("imports/tickets_errors.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Tickets, &parsed).expect("dry-run");

    assert_eq!(summary.row_count, 8);
    // Only "Valid Ticket" survives validation.
    assert_eq!(summary.would_create, 1);

    let expected: &[(&str, &str)] = &[
        ("title", "required"),
        ("workflow_state", "required"),
        ("workflow_state", "unknown workflow state"),
        ("priority", "not a valid priority"),
        ("requester_email", "no user has primary email"),
        ("category", "unknown category"),
        ("due_date", "not a valid date"),
    ];
    for (col, snippet) in expected {
        let hit = summary
            .errors
            .iter()
            .any(|e| e.column.as_deref() == Some(col) && e.message.contains(snippet));
        assert!(
            hit,
            "expected an error on column '{col}' containing '{snippet}'; got {:?}",
            summary.errors
        );
    }
}

#[test]
fn tickets_are_insert_only_running_twice_doubles_the_count() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed_user_prerequisites(&mut conn);

    let tickets_before = count_table(&mut conn, "tickets");
    let parsed = csv_parser::parse_file(&fixture_path("imports/tickets_valid.csv")).expect("parse");

    imports::commit(&mut conn, ImportType::Tickets, &parsed).expect("commit 1");
    imports::commit(&mut conn, ImportType::Tickets, &parsed).expect("commit 2");

    // Insert-only: no natural key, so the same file twice
    // means twice the rows. This is the documented behaviour.
    assert_eq!(count_table(&mut conn, "tickets"), tickets_before + 6);
}
