//! CSV asset importer integration tests.
//!
//! Runs against per-test sandbox DBs cloned from the template
//! (see `crate::common::TestDb`), so each test starts from the
//! migration baseline.

use diesel::prelude::*;

use backend::services::imports::{self, csv_parser, ImportType};

use crate::common::{count_table, fixture_path, import_ctx, TestDb};

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

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 4);
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
    let first =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit 1");
    assert_eq!(first.len(), 4);

    // Re-run the same CSV: every tag matches → 4 updates, 0 creates.
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run 2");
    assert_eq!(summary.would_create, 0);
    assert_eq!(summary.would_update, 4);
    assert!(summary.errors.is_empty());

    let second =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit 2");
    assert_eq!(second.len(), 4);

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

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

/// Parse CSV text the way an upload is parsed.
fn parse_text(body: &str) -> csv_parser::ParsedCsv {
    let file = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(file.path(), format!("{HEADER}\n{body}")).expect("write csv");
    csv_parser::parse_file(file.path()).expect("parse")
}

fn kind_of(conn: &mut diesel::PgConnection, tag: &str) -> Option<String> {
    #[derive(diesel::QueryableByName)]
    struct KindRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        kind: String,
    }
    diesel::sql_query("SELECT kind FROM assets WHERE asset_tag = $1")
        .bind::<diesel::sql_types::Text, _>(tag)
        .get_result::<KindRow>(conn)
        .optional()
        .expect("read kind")
        .map(|r| r.kind)
}

/// Values the database would refuse are row errors in the dry-run, and
/// the commit writes exactly the rows the dry-run counted.
#[test]
fn dry_run_catches_what_the_database_refuses_and_commit_agrees() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "INSERT INTO assets (name, kind, serial_number) VALUES ('Existing', 'generic', 'SN-TAKEN')",
    )
    .execute(&mut *conn)
    .expect("seed asset");

    let long_name = "x".repeat(256);
    let parsed = parse_text(&format!(
        "{long_name},generic,LONG-001,,,,,,,,\n\
         Negative Threshold,generic,NEG-001,,,,,,5,pcs,-1\n\
         Huge Quantity,generic,HUGE-001,,,,,,1000000000,pcs,\n\
         Serial One,generic,SER-001,SN-DUP,,,,,,,\n\
         Serial Two,generic,SER-002,SN-DUP,,,,,,,\n\
         Taken Serial,generic,SER-003,SN-TAKEN,,,,,,,\n"
    ));

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    let errors: Vec<(usize, Option<&str>)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref()))
        .collect();
    assert_eq!(
        errors,
        vec![
            (2, Some("name")),
            (3, Some("low_stock_threshold")),
            (4, Some("quantity")),
            (6, Some("serial_number")),
            (7, Some("serial_number")),
        ],
        "{:?}",
        summary.errors
    );
    assert_eq!(summary.would_create, 1);

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), summary.would_create + summary.would_update);
    assert_eq!(kind_of(&mut conn, "SER-001").as_deref(), Some("generic"));
}

/// An invalid row doesn't claim its tag, so a later valid row with the
/// same tag imports, in the dry-run and the commit alike.
#[test]
fn an_invalid_row_does_not_block_a_later_row_with_its_tag() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let parsed = parse_text(
        ",generic,REPEAT-001,,,,,,,,\n\
         Repeat Valid,generic,REPEAT-001,,,,,,,,\n",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!(summary.would_create, 1, "{:?}", summary.errors);
    assert!(
        summary.errors.iter().all(|e| e.row == 2),
        "{:?}",
        summary.errors
    );

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 1);
    assert_eq!(kind_of(&mut conn, "REPEAT-001").as_deref(), Some("generic"));
}

/// A kind matches its label or slug in any case and is stored as the slug.
#[test]
fn kind_matches_label_or_slug_ignoring_case() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let parsed = parse_text(
        "Switch,network device,KIND-001,,,,,,,,\n\
         Notebook,LAPTOP,KIND-002,,,,,,,,\n",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);

    imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(
        kind_of(&mut conn, "KIND-001").as_deref(),
        Some("network_device")
    );
    assert_eq!(kind_of(&mut conn, "KIND-002").as_deref(), Some("laptop"));
}

/// The CSV carries no attributes, so a kind's required attributes don't
/// stop its rows importing.
#[test]
fn a_kind_with_required_attributes_imports() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        r#"INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category)
           VALUES ('badge', 'Badge',
                   '{"type":"object","properties":{"badge_no":{"type":"string"}},"required":["badge_no"]}',
                   200, false, 'physical')"#,
    )
    .execute(&mut *conn)
    .expect("seed kind");
    let parsed = parse_text("Front door badge,badge,BADGE-001,,,,,,,,\n");

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(kind_of(&mut conn, "BADGE-001").as_deref(), Some("badge"));
}

/// Seed an asset with a tag, kind and attributes for the kind-change tests.
fn seed_tagged_asset(conn: &mut diesel::PgConnection, tag: &str, kind: &str, attributes: &str) {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, attributes) VALUES ('Seeded', $1, $2, $3::jsonb)",
    )
    .bind::<diesel::sql_types::Text, _>(kind)
    .bind::<diesel::sql_types::Text, _>(tag)
    .bind::<diesel::sql_types::Text, _>(attributes)
    .execute(conn)
    .expect("seed asset");
}

/// An update that changes the kind while the asset holds attributes the
/// new kind doesn't define is a row error in the dry run, and the commit
/// leaves that asset alone while importing the other rows.
#[test]
fn a_kind_change_that_would_drop_attributes_is_a_row_error() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed_tagged_asset(&mut conn, "KC-001", "laptop", r#"{"hostname":"pc-1"}"#);
    let parsed = parse_text(
        "Renamed,generic,KC-001,,,,,,,,
         Other,generic,KC-002,,,,,,,,
",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!(summary.errors.len(), 1, "{:?}", summary.errors);
    let err = &summary.errors[0];
    assert_eq!((err.row, err.column.as_deref()), (2, Some("kind")));
    assert_eq!(
        err.message,
        "changing kind from Laptop to Generic asset would drop attributes: hostname; \
         change the kind on the asset page instead"
    );
    assert_eq!(summary.would_update, 0);
    assert_eq!(summary.would_create, 1);

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 1);
    assert_eq!(kind_of(&mut conn, "KC-001").as_deref(), Some("laptop"));
    assert_eq!(kind_of(&mut conn, "KC-002").as_deref(), Some("generic"));
    let untouched: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM assets \
         WHERE asset_tag = 'KC-001' AND name = 'Seeded' AND attributes = '{\"hostname\":\"pc-1\"}'::jsonb",
    )
    .get_result::<CountRow>(&mut *conn)
    .map(|r| r.count)
    .expect("re-read KC-001");
    assert_eq!(untouched, 1);
}

/// A kind change goes through when the new kind defines every attribute
/// the asset holds, or the asset holds none. Keeping the kind is never
/// checked.
#[test]
fn a_kind_change_that_keeps_every_attribute_imports() {
    let db = TestDb::new();
    let mut conn = db.conn();
    // Desktop defines the same keys as Laptop.
    seed_tagged_asset(&mut conn, "KC-010", "laptop", r#"{"hostname":"pc-1"}"#);
    seed_tagged_asset(&mut conn, "KC-011", "laptop", "{}");
    seed_tagged_asset(&mut conn, "KC-012", "laptop", r#"{"hostname":"pc-2"}"#);
    let parsed = parse_text(
        "To Desktop,desktop,KC-010,,,,,,,,
         To Generic,generic,KC-011,,,,,,,,
         Same Kind,Laptop,KC-012,,,,,,,,
",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.would_update, 3);

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 3);
    assert_eq!(kind_of(&mut conn, "KC-010").as_deref(), Some("desktop"));
    assert_eq!(kind_of(&mut conn, "KC-011").as_deref(), Some("generic"));
    assert_eq!(kind_of(&mut conn, "KC-012").as_deref(), Some("laptop"));
}

#[derive(diesel::QueryableByName)]
struct NameKindRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    kind: String,
}

fn name_and_kind(conn: &mut diesel::PgConnection, tag: &str) -> (String, String) {
    let row: NameKindRow = diesel::sql_query("SELECT name, kind FROM assets WHERE asset_tag = $1")
        .bind::<diesel::sql_types::Text, _>(tag)
        .get_result(conn)
        .expect("read asset");
    (row.name, row.kind)
}

/// The asset page doesn't offer a kind change on an asset linked to a
/// model, since the model sets the kind, so the import refuses one too. A
/// row that keeps the kind still updates the asset.
#[test]
fn a_kind_change_on_a_model_linked_asset_is_a_row_error() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "WITH m AS (INSERT INTO manufacturers (name) VALUES ('Lenovo') RETURNING id),
              am AS (INSERT INTO asset_models (manufacturer_id, name, kind)
                     SELECT id, 'ThinkPad T14', 'laptop' FROM m RETURNING id)
         INSERT INTO assets (name, kind, asset_tag, model_id)
         SELECT v.name, 'laptop', v.tag, am.id
         FROM am, (VALUES ('Laptop-01', 'KC-020'), ('Laptop-03', 'KC-022')) AS v(name, tag)",
    )
    .execute(&mut *conn)
    .expect("seed model-linked assets");

    let parsed = parse_text(
        "Model Change,desktop,KC-020,,,,,,,,\n\
         Model Same Kind,laptop,KC-022,,,,,,,,\n",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    let errors: Vec<(usize, Option<&str>, &str)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref(), e.message.as_str()))
        .collect();
    assert_eq!(
        errors,
        vec![(
            2,
            Some("kind"),
            "Laptop-01 takes its kind from its model ThinkPad T14; \
             change the model on the asset page instead"
        )]
    );
    assert_eq!(summary.would_update, 1);

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 1);
    assert_eq!(
        name_and_kind(&mut conn, "KC-020"),
        ("Laptop-01".to_string(), "laptop".to_string())
    );
    assert_eq!(
        name_and_kind(&mut conn, "KC-022"),
        ("Model Same Kind".to_string(), "laptop".to_string())
    );
}

fn updated_at(conn: &mut diesel::PgConnection, tag: &str) -> chrono::NaiveDateTime {
    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Timestamp)]
        updated_at: chrono::NaiveDateTime,
    }
    diesel::sql_query("SELECT updated_at FROM assets WHERE asset_tag = $1")
        .bind::<diesel::sql_types::Text, _>(tag)
        .get_result::<Row>(conn)
        .expect("read updated_at")
        .updated_at
}

/// PUT takes no imported column on an asset synced from Microsoft, so the
/// import doesn't either. A row that would change one is an error on the
/// first such column; a row that matches (blank cells leave a column as it
/// is) writes nothing. Re-importing an export is a no-op.
#[test]
fn a_synced_asset_takes_no_column_from_the_file() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, serial_number, manufacturer, model,
                             location, notes, external_sync_source, updated_at)
         SELECT 'PC-' || n, 'device', 'KC-0' || n, 'SN-' || n, 'Dell', 'OptiPlex',
                'Lab', 'Imaged in March', 'entra', TIMESTAMP '2026-01-01 00:00:00'
         FROM generate_series(31, 35) AS n",
    )
    .execute(&mut *conn)
    .expect("seed synced assets");

    let parsed = parse_text(
        "PC-31,device,KC-031,,,,,,,,\n\
         PC-32,laptop,KC-032,,,,,,,,\n\
         Renamed,device,KC-033,,,,,,,,\n\
         PC-34,device,KC-034,,,,,Returned,,,\n\
         PC-35,device,KC-035,,,,,,,pcs,\n",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    let errors: Vec<(usize, Option<&str>, &str)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref(), e.message.as_str()))
        .collect();
    let synced = |name: &str, column: &str| {
        format!(
            "{name} is synced from Microsoft Entra, which sets its {column}; \
             unmanage it on the asset page to edit it here"
        )
    };
    assert_eq!(
        errors,
        vec![
            (3, Some("kind"), synced("PC-32", "kind").as_str()),
            (4, Some("name"), synced("PC-33", "name").as_str()),
            (5, Some("notes"), synced("PC-34", "notes").as_str()),
            (6, Some("unit"), synced("PC-35", "unit").as_str()),
        ]
    );
    assert_eq!(summary.would_update, 1);
    assert_eq!(summary.would_create, 0);

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 1);
    let unchanged = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .expect("date");
    for n in 31..=35 {
        let tag = format!("KC-0{n}");
        assert_eq!(
            name_and_kind(&mut conn, &tag),
            (format!("PC-{n}"), "device".to_string())
        );
        assert_eq!(updated_at(&mut conn, &tag), unchanged, "{tag}");
    }

    // An export of the synced assets imports back without an error.
    let exported: Vec<backend::models::Asset> = {
        use backend::schema::assets;
        assets::table
            .filter(assets::asset_tag.like("KC-03%"))
            .load(&mut *conn)
            .expect("load assets")
    };
    let csv = backend::services::imports::assets::write_assets_csv(&exported).expect("export");
    let file = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(file.path(), csv).expect("write csv");
    let reimport = csv_parser::parse_file(file.path()).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &reimport).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.would_update, 5);
}

/// A row the database refuses at commit, for a reason the dry-run can't
/// foresee (a test-only constraint stands in), stops the commit with
/// that row's number.
#[test]
fn a_refused_row_names_its_row() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "ALTER TABLE assets ADD CONSTRAINT test_refuse_name CHECK (name <> 'Refused')",
    )
    .execute(&mut *conn)
    .expect("add constraint");
    let parsed = parse_text(
        "Fine,generic,REFUSE-001,,,,,,,,\n\
         Refused,generic,REFUSE-002,,,,,,,,\n",
    );

    match imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()) {
        Err(imports::CommitError::Row { row, message }) => {
            assert_eq!(row, 3);
            assert!(message.contains("test_refuse_name"), "{message}");
        }
        Ok(_) => panic!("commit should stop at the refused row"),
        Err(e) => panic!("expected a row error, got {e:?}"),
    }
}

/// Parse a whole CSV file's text, header included.
fn parse_file_text(text: &str) -> csv_parser::ParsedCsv {
    let file = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(file.path(), text).expect("write csv");
    csv_parser::parse_file(file.path()).expect("parse")
}

/// Errors name the line a row starts on, so a quoted field spanning lines
/// doesn't shift the rows after it.
#[test]
fn row_errors_name_the_line_the_row_starts_on() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let parsed = parse_text(
        "Notes,generic,ML-001,,,,,\"first line\nsecond line\nthird line\",,,\n\
         ,generic,ML-002,,,,,,,,\n\
         \n\
         Bad Threshold,generic,ML-003,,,,,,,,x\n",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    let errors: Vec<(usize, Option<&str>)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref()))
        .collect();
    assert_eq!(
        errors,
        vec![(5, Some("name")), (7, Some("low_stock_threshold"))],
        "{:?}",
        summary.errors
    );
    assert_eq!(summary.would_create, 1);
}

/// A file saved with `;` between fields, as European Excel does, imports.
#[test]
fn a_semicolon_separated_file_imports() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let parsed = parse_file_text(&format!(
        "{}\nSemi Colon;generic;SEMI-001;;;;\"Room 1; shelf 2\";;4;pcs;\n",
        HEADER.replace(',', ";")
    ));
    assert_eq!(parsed.delimiter, b';');

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");

    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        location: Option<String>,
    }
    let row: Row = diesel::sql_query("SELECT location FROM assets WHERE asset_tag = 'SEMI-001'")
        .get_result(&mut *conn)
        .expect("read asset");
    assert_eq!(row.location.as_deref(), Some("Room 1; shelf 2"));
}

/// The export's formula guard comes off on import, so an exported asset
/// whose values start with `=` or `-` imports back unchanged.
#[test]
fn an_export_imports_back_through_the_formula_guard() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, notes) \
         VALUES ('=SUM(A1)', 'generic', 'GUARD-001', '-fragile')",
    )
    .execute(&mut *conn)
    .expect("seed asset");

    let exported: Vec<backend::models::Asset> = {
        use backend::schema::assets;
        assets::table
            .filter(assets::asset_tag.eq("GUARD-001"))
            .load(&mut *conn)
            .expect("load asset")
    };
    let csv = backend::services::imports::assets::write_assets_csv(&exported).expect("export");
    let text = String::from_utf8(csv).expect("utf-8");
    assert!(text.contains("'=SUM(A1)"), "{text}");

    let parsed = parse_file_text(&text);
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.would_update, 1);
    imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");

    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        name: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        notes: Option<String>,
    }
    let row: Row =
        diesel::sql_query("SELECT name, notes FROM assets WHERE asset_tag = 'GUARD-001'")
            .get_result(&mut *conn)
            .expect("read asset");
    assert_eq!(row.name, "=SUM(A1)");
    assert_eq!(row.notes.as_deref(), Some("-fragile"));
}

/// A tag two existing assets share can't say which to update, so the row
/// is an error naming both, in the dry run and the commit alike.
#[test]
fn a_tag_more_than_one_asset_holds_is_a_row_error() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag) \
         VALUES ('Camera A', 'generic', 'SHARED-1'), ('Camera B', 'generic', 'SHARED-1')",
    )
    .execute(&mut *conn)
    .expect("seed assets");
    let parsed = parse_text(
        "Camera C,generic,SHARED-1,,,,,,,,\n\
         Tripod,generic,TRIPOD-1,,,,,,,,\n",
    );

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    let errors: Vec<(usize, Option<&str>, &str)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref(), e.message.as_str()))
        .collect();
    assert_eq!(
        errors,
        vec![(
            2,
            Some("asset_tag"),
            "tag 'SHARED-1' is held by 2 assets (Camera A, Camera B); \
             give each its own tag on the asset page, then import again"
        )]
    );
    assert_eq!((summary.would_create, summary.would_update), (1, 0));

    let records =
        imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");
    assert_eq!(records.len(), 1);
    let names: Vec<String> = {
        use backend::schema::assets;
        assets::table
            .filter(assets::asset_tag.eq("SHARED-1"))
            .select(assets::name)
            .order(assets::name)
            .load(&mut *conn)
            .expect("load assets")
    };
    assert_eq!(names, vec!["Camera A", "Camera B"]);
}

/// On update a blank cell leaves every column as it is, quantity, unit and
/// low-stock threshold included.
#[test]
fn a_blank_cell_never_clears_on_update() {
    let db = TestDb::new();
    let mut conn = db.conn();
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, location, quantity, unit, low_stock_threshold) \
         VALUES ('Toner', 'generic', 'KEEP-001', 'Store', 12, 'pcs', 3)",
    )
    .execute(&mut *conn)
    .expect("seed asset");
    let parsed = parse_text("Toner Black,generic,KEEP-001,,,,,,,,\n");

    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.would_update, 1);
    imports::commit(&mut conn, ImportType::Assets, &parsed, &import_ctx()).expect("commit");

    let row: AssetRow = diesel::sql_query(
        "SELECT name, kind, asset_tag, quantity, unit FROM assets WHERE asset_tag = 'KEEP-001'",
    )
    .get_result(&mut *conn)
    .expect("re-read KEEP-001");
    assert_eq!(row.name, "Toner Black");
    assert_eq!(
        row.quantity.map(|q| q.to_string()).as_deref(),
        Some("12.000")
    );
    assert_eq!(row.unit.as_deref(), Some("pcs"));

    #[derive(diesel::QueryableByName)]
    struct Kept {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        location: Option<String>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        threshold: Option<String>,
    }
    let kept: Kept = diesel::sql_query(
        "SELECT location, low_stock_threshold::text AS threshold FROM assets WHERE asset_tag = 'KEEP-001'",
    )
    .get_result(&mut *conn)
    .expect("re-read KEEP-001");
    assert_eq!(kept.location.as_deref(), Some("Store"));
    assert_eq!(kept.threshold.as_deref(), Some("3.000"));
}

/// One request to the import routes, signed in as `user` in `ws`.
pub(crate) async fn call_import_route(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    user: uuid::Uuid,
    req: actix_web::test::TestRequest,
) -> (actix_web::http::StatusCode, actix_web::web::Bytes) {
    use actix_web::dev::Service;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};
    use std::sync::Arc;

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::services::search::SearchService;
    use backend::sync::actor::ActorContext;

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Importer".to_string(),
        email: "importer@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = uuid::Uuid::now_v7();
    let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), pool).expect("init search"));
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::imports::config)),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    (status, http_test::read_body(resp).await)
}

/// A multipart upload of `body` as `filename` to `POST /admin/import`.
fn upload_request(filename: &str, body: &[u8]) -> actix_web::test::TestRequest {
    upload_request_for("assets", filename, body)
}

/// A multipart upload of a `job_type` file.
pub(crate) fn upload_request_for(
    job_type: &str,
    filename: &str,
    body: &[u8],
) -> actix_web::test::TestRequest {
    let mut payload = format!(
        "--x\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: text/csv\r\n\r\n"
    )
    .into_bytes();
    payload.extend_from_slice(body);
    payload.extend_from_slice(b"\r\n--x--\r\n");
    actix_web::test::TestRequest::post()
        .uri(&format!("/api/admin/import?type={job_type}"))
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .set_payload(payload)
}

pub(crate) fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).unwrap_or_default()
}

/// The fixture workspace, with the generic kind the asset tests import.
pub(crate) fn import_workspace(pool: &crate::common::TestPool) -> crate::common::WorkspaceSeed {
    crate::common::ensure_test_keyring();
    crate::common::with_upload_dir();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    diesel::sql_query(
        "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
         VALUES ('generic', 'Generic asset', '{\"type\":\"object\",\"properties\":{}}', 5, true, 'generic', $1)
         ON CONFLICT (workspace_id, slug) DO NOTHING",
    )
    .bind::<diesel::sql_types::Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed kind");
    ws
}

/// Over HTTP: a refused row rolls the whole commit back, answers 422
/// naming the row, and leaves the job marked failed with that message.
#[actix_web::test]
async fn a_failed_commit_is_recorded_on_the_job() {
    use actix_web::http::StatusCode;

    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nFine,generic,JOB-001,,,,,,,,\nRefused,generic,JOB-002,,,,,,,,\n"),
    )
    .await;
    let job_id = uuid::Uuid::parse_str(job["id"].as_str().expect("job id")).expect("uuid");
    diesel::sql_query(
        "ALTER TABLE assets ADD CONSTRAINT test_refuse_name CHECK (name <> 'Refused')",
    )
    .execute(&mut pool.get().expect("conn"))
    .expect("add constraint");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        actix_web::test::TestRequest::post().uri(&format!("/api/admin/import/{job_id}/commit")),
    )
    .await;
    let body = json(&body);
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("Row 3"),
        "{body}"
    );

    #[derive(diesel::QueryableByName)]
    struct JobRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        status: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        error_message: Option<String>,
    }
    let mut conn = pool.get().expect("conn");
    let job: JobRow =
        diesel::sql_query("SELECT status, error_message FROM import_jobs WHERE id = $1")
            .bind::<diesel::sql_types::Uuid, _>(job_id)
            .get_result(&mut conn)
            .expect("read job");
    assert_eq!(job.status, "failed");
    assert!(
        job.error_message.as_deref().unwrap_or("").contains("Row 3"),
        "{:?}",
        job.error_message
    );
    // The row before the refused one was rolled back with it.
    let written: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM assets WHERE asset_tag IN ('JOB-001', 'JOB-002')",
    )
    .get_result::<CountRow>(&mut conn)
    .map(|r| r.count)
    .expect("count");
    assert_eq!(written, 0);
}

/// Over HTTP: upload and commit answer with the job but not the server's
/// file path, the committing admin is recorded on the assets it created,
/// and a long filename is cut to fit rather than failing the upload.
#[actix_web::test]
async fn upload_and_commit_answer_without_the_file_path() {
    use actix_web::http::StatusCode;

    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let filename = format!("{}.csv", "a".repeat(300));
    let file = format!("{HEADER}\nUploaded,generic,UP-001,,,,,,,,\n");
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request(&filename, file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert!(job.get("file_path").is_none(), "{job}");
    assert_eq!(job["status"], "parsed");
    assert_eq!(
        job["filename"].as_str().map(|f| f.chars().count()),
        Some(255)
    );
    let job_id = job["id"].as_str().expect("job id").to_string();

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&job_id, template_mapping()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert!(job.get("file_path").is_none(), "{job}");
    assert_eq!(job["status"], "dry_run_done");
    assert_eq!(job["summary"]["would_create"], 1, "{job}");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        actix_web::test::TestRequest::get().uri(&format!("/api/admin/import/{job_id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json(&body).get("file_path").is_none());

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        actix_web::test::TestRequest::post().uri(&format!("/api/admin/import/{job_id}/commit")),
    )
    .await;
    let done = json(&body);
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done.get("file_path").is_none(), "{done}");
    assert_eq!(done["records_committed"], 1);

    #[derive(diesel::QueryableByName)]
    struct CreatedBy {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
        created_by: Option<uuid::Uuid>,
    }
    let row: CreatedBy =
        diesel::sql_query("SELECT created_by FROM assets WHERE asset_tag = 'UP-001'")
            .get_result(&mut pool.get().expect("conn"))
            .expect("read asset");
    assert_eq!(row.created_by, Some(ws.admin_uuid));
}

/// Writes to an import job land in the audit log, with the summary and
/// the file's columns, mapping and options (cell values can carry other
/// people's emails) reduced to whether they changed.
#[actix_web::test]
async fn an_import_job_is_audited_without_its_contents() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nAudited,generic,AUD-001,,,,,,,,\n"),
    )
    .await;
    let job_id = job["id"].as_str().expect("job id").to_string();

    #[derive(diesel::QueryableByName)]
    struct AuditRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        op: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
        after_jsonb: Option<serde_json::Value>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
        actor_uuid: Option<uuid::Uuid>,
    }
    let rows: Vec<AuditRow> = diesel::sql_query(
        "SELECT op::text AS op, after_jsonb, actor_uuid FROM audit_log
         WHERE table_name = 'import_jobs' AND pk_text = $1 ORDER BY id",
    )
    .bind::<diesel::sql_types::Text, _>(&job_id)
    .load(&mut pool.get().expect("conn"))
    .expect("read audit log");
    let ops: Vec<&str> = rows.iter().map(|r| r.op.as_str()).collect();
    assert_eq!(ops, vec!["I", "U", "U"]);
    for row in &rows {
        let after = row.after_jsonb.as_ref().expect("after_jsonb");
        for column in ["summary", "source", "mapping", "options", "row_results"] {
            assert!(after.get(column).is_none(), "{column}: {after}");
        }
    }
    let parsed = rows[1].after_jsonb.as_ref().expect("after_jsonb");
    assert_eq!(parsed["status"], "parsed");
    assert_eq!(parsed["source_changed"], true, "{parsed}");
    let checked = rows[2].after_jsonb.as_ref().expect("after_jsonb");
    assert_eq!(checked["status"], "dry_run_done");
    for column in ["summary", "mapping", "options"] {
        assert_eq!(checked[format!("{column}_changed")], true, "{checked}");
    }
    assert!(rows.iter().all(|r| r.actor_uuid == Some(ws.admin_uuid)));
}

/// Every import route, the template included, is workspace-admin only.
#[actix_web::test]
async fn import_routes_refuse_a_non_admin() {
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;

    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = uuid::Uuid::new_v4();

    let requests = [
        TestRequest::get().uri("/api/admin/import/template/assets"),
        TestRequest::get().uri("/api/admin/import/fields?type=assets"),
        upload_request("assets.csv", format!("{HEADER}\n").as_bytes()),
        TestRequest::get().uri(&format!("/api/admin/import/{job}")),
        mapping_request(&job.to_string(), template_mapping()),
        TestRequest::post().uri(&format!("/api/admin/import/{job}/commit")),
    ];
    for req in requests {
        let (status, _) = call_import_route(&pool, &ws, ws.member_uuid, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/template/assets"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(String::from_utf8_lossy(&body), format!("{HEADER}\n"));
}

/// A file over a cap is refused whole, naming the cap, and the job is
/// recorded as failed.
#[actix_web::test]
async fn a_file_over_a_cap_is_refused() {
    use actix_web::http::StatusCode;

    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let too_many_rows = format!("{HEADER}\n{}", "Row,generic,,,,,,,,,\n".repeat(5_001));
    let wide_header = (0..257)
        .map(|i| format!("c{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let long_cell = format!("{HEADER}\nLong,generic,,,,,,{},,,\n", "x".repeat(32_768));
    let long_header = format!("{HEADER},{}\n", "h".repeat(256));
    let duplicate = format!("{HEADER},name\n");
    let cases = [
        (too_many_rows, "more than 5,000 data rows"),
        (format!("{wide_header}\n"), "257 columns"),
        (
            long_cell,
            "row 2, column 'notes' is longer than 32,767 characters",
        ),
        (long_header, "column 12 is longer than 255 characters"),
        (duplicate, "column 'name' appears more than once"),
    ];
    for (file, message) in cases {
        let (status, body) = call_import_route(
            &pool,
            &ws,
            ws.admin_uuid,
            upload_request("assets.csv", file.as_bytes()),
        )
        .await;
        let body = json(&body);
        assert_eq!(status, StatusCode::BAD_REQUEST, "{message}: {body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains(message),
            "{message}: {body}"
        );
    }

    #[derive(diesel::QueryableByName)]
    struct Failed {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let failed: Failed = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM import_jobs WHERE status = 'failed'",
    )
    .get_result(&mut pool.get().expect("conn"))
    .expect("count failed jobs");
    assert_eq!(failed.count, 5);
}

#[derive(diesel::QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// `PUT /admin/import/{id}/mapping` with `body`.
pub(crate) fn mapping_request(
    job_id: &str,
    body: serde_json::Value,
) -> actix_web::test::TestRequest {
    actix_web::test::TestRequest::put()
        .uri(&format!("/api/admin/import/{job_id}/mapping"))
        .set_json(body)
}

/// The mapping a template-headed file takes: each field from the column
/// of the same name.
pub(crate) fn template_mapping() -> serde_json::Value {
    let mapping: serde_json::Map<String, serde_json::Value> = HEADER
        .split(',')
        .enumerate()
        .map(|(i, h)| (h.to_string(), i.into()))
        .collect();
    serde_json::json!({ "mapping": mapping })
}

/// Upload `file` and map it as a template: the job, checked.
async fn upload_and_check(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    file: &str,
) -> serde_json::Value {
    use actix_web::http::StatusCode;
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request("assets.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "parsed", "{job}");
    let id = job["id"].as_str().expect("job id").to_string();
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        mapping_request(&id, template_mapping()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "dry_run_done", "{job}");
    job
}

/// Purging a user who ran an import clears them from the job and the
/// assets it created; the audit triggers on both run in the purge's
/// workspace context.
#[test]
fn purging_an_importer_clears_them_from_the_job_and_its_assets() {
    use backend::models::NewImportJob;
    use backend::sync::actor::ActorContext;
    use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

    const REF: &str = "test:imports_purge_importer";
    let db = TestDb::new();
    let pool = db.pool_with_size(2);
    let ws = import_workspace(&pool);
    let importer = ws.member_uuid;
    let parsed = parse_text("Purged Import,generic,PURGE-001,,,,,,,,\n");

    let job_id = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        let job = backend::repository::imports::create(
            c,
            NewImportJob {
                id: uuid::Uuid::new_v4(),
                job_type: "assets".to_string(),
                filename: "assets.csv".to_string(),
                file_path: "/dev/null".to_string(),
                created_by: Some(importer),
            },
        )?;
        let ctx = imports::ImportContext {
            actor: Some(importer),
            job_id: job.id,
            options: Default::default(),
        };
        imports::commit(c, ImportType::Assets, &parsed, &ctx)
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e}").into()))?;
        Ok(job.id)
    })
    .expect("import as the member");

    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, ws.workspace_id)?;
        backend::repository::users::purge_user(&importer, c, None)
    })
    .expect("purge the importer");

    #[derive(diesel::QueryableByName)]
    struct CreatedBy {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
        created_by: Option<uuid::Uuid>,
    }
    let job: CreatedBy = diesel::sql_query("SELECT created_by FROM import_jobs WHERE id = $1")
        .bind::<diesel::sql_types::Uuid, _>(job_id)
        .get_result(&mut conn)
        .expect("read job");
    let asset: CreatedBy =
        diesel::sql_query("SELECT created_by FROM assets WHERE asset_tag = 'PURGE-001'")
            .get_result(&mut conn)
            .expect("read asset");
    assert_eq!((job.created_by, asset.created_by), (None, None));
}
