//! CSV user importer integration tests.

mod common;

use diesel::prelude::*;

use backend::services::imports::{self, csv_parser, ImportType};

use common::{count_table, fixture_path, TestDb};

#[test]
fn happy_path_creates_users_and_primary_emails() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let users_before = count_table(&mut conn, "users");

    let parsed = csv_parser::parse_file(&fixture_path("imports/users_valid.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed).expect("dry-run");
    assert_eq!(summary.row_count, 3);
    assert_eq!(summary.would_create, 3);
    assert_eq!(summary.would_update, 0);
    assert!(summary.errors.is_empty(), "errors: {:?}", summary.errors);

    let count = imports::commit(&mut conn, ImportType::Users, &parsed).expect("commit");
    assert_eq!(count, 3);
    assert_eq!(count_table(&mut conn, "users"), users_before + 3);

    // Sentinel: Alex's primary email row landed with the
    // right address and source='csv_import'.
    #[derive(diesel::QueryableByName)]
    struct EmailRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        email: String,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        is_primary: bool,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        source: Option<String>,
    }
    let row: EmailRow = diesel::sql_query(
        "SELECT email, is_primary, source FROM user_emails \
         WHERE email = 'alex.kim@example.com'",
    )
    .get_result(&mut *conn)
    .expect("re-read Alex's email");
    assert!(row.is_primary);
    assert_eq!(row.source.as_deref(), Some("csv_import"));
    assert_eq!(row.email, "alex.kim@example.com");
}

#[test]
fn errors_csv_classifies_each_failure() {
    let db = TestDb::new();
    let mut conn = db.conn();

    let parsed = csv_parser::parse_file(&fixture_path("imports/users_errors.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed).expect("dry-run");

    assert_eq!(summary.row_count, 9);
    // valid + the first occurrence of duplicate@example.com →
    // 2 would-creates. nopronouns@example.com fails because
    // name is required.
    assert_eq!(summary.would_create, 2);
    assert_eq!(summary.would_update, 0);

    let expected: &[(&str, &str)] = &[
        ("email", "required"),
        ("email", "not a valid email"),
        ("email", "appears more than once"),
        ("role", "not a valid role"),
        ("name", "required"),
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
fn second_commit_upserts_by_email() {
    let db = TestDb::new();
    let mut conn = db.conn();

    let parsed = csv_parser::parse_file(&fixture_path("imports/users_valid.csv")).expect("parse");

    let first = imports::commit(&mut conn, ImportType::Users, &parsed).expect("commit 1");
    assert_eq!(first, 3);

    // Second pass: every primary email matches → 3 updates.
    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed).expect("dry-run 2");
    assert_eq!(summary.would_create, 0);
    assert_eq!(summary.would_update, 3);
    assert!(summary.errors.is_empty());

    let second = imports::commit(&mut conn, ImportType::Users, &parsed).expect("commit 2");
    assert_eq!(second, 3);

    // No duplicate user_emails rows.
    #[derive(diesel::QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let n: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM user_emails \
         WHERE email = 'alex.kim@example.com'",
    )
    .get_result::<CountRow>(&mut *conn)
    .map(|r| r.count)
    .expect("count emails");
    assert_eq!(n, 1);
}
