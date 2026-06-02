//! CSV asset importer integration tests.
//!
//! Runs against per-test sandbox DBs cloned from the template
//! (see `common::TestDb`), so each test starts from the
//! migration baseline.

mod common;

use diesel::prelude::*;

use backend::services::imports::{self, csv_parser, ImportType};

use common::{count_table, fixture_path, TestDb};

#[derive(diesel::QueryableByName)]
struct AssetRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    kind: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    asset_tag: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Numeric>)]
    quantity: Option<bigdecimal::BigDecimal>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    unit: Option<String>,
}

#[test]
fn happy_path_creates_every_row() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let assets_before = count_table(&mut conn, "assets");

    let parsed = csv_parser::parse_file(&fixture_path("imports/assets_valid.csv")).expect("parse");

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!(summary.row_count, 4);
    assert_eq!(summary.would_create, 4);
    assert_eq!(summary.would_update, 0);
    assert!(
        summary.errors.is_empty(),
        "expected no errors, got {:?}",
        summary.errors
    );

    let count = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit");
    assert_eq!(count, 4);
    assert_eq!(count_table(&mut conn, "assets"), assets_before + 4);

    // Sentinel: STOCK-001 landed with the right decimal quantity.
    let row: AssetRow = diesel::sql_query(
        "SELECT name, kind, asset_tag, quantity, unit FROM assets WHERE asset_tag = 'STOCK-001'",
    )
    .get_result(&mut *conn)
    .expect("re-read STOCK-001");
    assert_eq!(row.name, "Black Toner Cartridge");
    assert_eq!(row.kind, "generic");
    assert_eq!(row.asset_tag.as_deref(), Some("STOCK-001"));
    assert_eq!(
        row.quantity.map(|q| q.to_string()).as_deref(),
        Some("50.000")
    );
    assert_eq!(row.unit.as_deref(), Some("pcs"));
}

#[test]
fn errors_csv_classifies_each_failure() {
    let db = TestDb::new();
    let mut conn = db.conn();

    let parsed = csv_parser::parse_file(&fixture_path("imports/assets_errors.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");

    assert_eq!(summary.row_count, 7);
    // VALID-001 and the first DUP-001 are creatable; the second
    // DUP-001 fails on the in-file duplicate check.
    assert_eq!(summary.would_create, 2);
    assert_eq!(summary.would_update, 0);

    // Errors we expect (column, partial message substring):
    let expected: &[(&str, &str)] = &[
        ("name", "required"),
        ("kind", "required"),
        ("kind", "unknown kind"),
        ("low_stock_threshold", "valid decimal"),
        ("asset_tag", "appears more than once"),
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
fn second_commit_upserts_by_tag() {
    let db = TestDb::new();
    let mut conn = db.conn();

    let parsed = csv_parser::parse_file(&fixture_path("imports/assets_valid.csv")).expect("parse");

    // First commit: 4 creates.
    let first = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit 1");
    assert_eq!(first, 4);

    // Re-run the same CSV: every tag matches → 4 updates, 0 creates.
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run 2");
    assert_eq!(summary.would_create, 0);
    assert_eq!(summary.would_update, 4);
    assert!(summary.errors.is_empty());

    let second = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit 2");
    assert_eq!(second, 4);

    // Row count unchanged: upserts, not inserts.
    #[derive(diesel::QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let by_tag: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM assets WHERE asset_tag IN ('STOCK-001','STOCK-002','STOCK-003','STOCK-004')",
    )
    .get_result::<CountRow>(&mut *conn)
    .map(|r| r.count)
    .expect("count by tag");
    assert_eq!(by_tag, 4);
}
