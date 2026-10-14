//! CSV asset importer integration tests.
//!
//! Runs against per-test sandbox DBs cloned from the template
//! (see `crate::common::TestDb`), so each test starts from the
//! migration baseline.

use diesel::prelude::*;

use backend::services::imports::{self, csv_parser, ImportType};

use crate::common::{count_table, fixture_path, TestDb};

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

    let records = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit");
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
    let first = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit 1");
    assert_eq!(first.len(), 4);

    // Re-run the same CSV: every tag matches → 4 updates, 0 creates.
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run 2");
    assert_eq!(summary.would_create, 0);
    assert_eq!(summary.would_update, 4);
    assert!(summary.errors.is_empty());

    let second = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit 2");
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

    let records = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit");
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

    let records = imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit");
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

    imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit");
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
    imports::commit(&mut conn, ImportType::Assets, &parsed).expect("commit");
    assert_eq!(kind_of(&mut conn, "BADGE-001").as_deref(), Some("badge"));
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

    match imports::commit(&mut conn, ImportType::Assets, &parsed) {
        Err(imports::CommitError::Row { row, message }) => {
            assert_eq!(row, 3);
            assert!(message.contains("test_refuse_name"), "{message}");
        }
        Ok(_) => panic!("commit should stop at the refused row"),
        Err(e) => panic!("expected a row error, got {e:?}"),
    }
}

/// Over HTTP: a refused row rolls the whole commit back, answers 422
/// naming the row, and leaves the job marked failed with that message.
#[actix_web::test]
async fn a_failed_commit_is_recorded_on_the_job() {
    use actix_web::dev::Service;
    use actix_web::http::StatusCode;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};
    use std::sync::Arc;

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::services::search::SearchService;
    use backend::sync::actor::ActorContext;

    crate::common::ensure_test_keyring();
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let csv = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(
        csv.path(),
        format!("{HEADER}\nFine,generic,JOB-001,,,,,,,,\nRefused,generic,JOB-002,,,,,,,,\n"),
    )
    .expect("write csv");
    let job_id = uuid::Uuid::new_v4();
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query(
            "ALTER TABLE assets ADD CONSTRAINT test_refuse_name CHECK (name <> 'Refused')",
        )
        .execute(&mut conn)
        .expect("add constraint");
        diesel::sql_query(
            "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
             VALUES ('generic', 'Generic asset', '{\"type\":\"object\",\"properties\":{}}', 5, true, 'generic', $1)
             ON CONFLICT (workspace_id, slug) DO NOTHING",
        )
        .bind::<diesel::sql_types::Integer, _>(ws.workspace_id)
        .execute(&mut conn)
        .expect("seed kind");
        diesel::sql_query(
            "INSERT INTO import_jobs (id, job_type, status, filename, file_path, workspace_id)
             VALUES ($1, 'assets', 'dry_run_done', 'assets.csv', $2, $3)",
        )
        .bind::<diesel::sql_types::Uuid, _>(job_id)
        .bind::<diesel::sql_types::Text, _>(csv.path().to_string_lossy().to_string())
        .bind::<diesel::sql_types::Integer, _>(ws.workspace_id)
        .execute(&mut conn)
        .expect("seed job");
    }

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
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
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
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
            .service(web::scope("/api").route(
                "/admin/import/{id}/commit",
                web::post().to(backend::handlers::imports::commit),
            )),
    )
    .await;

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/import/{job_id}/commit"))
            .to_request(),
    )
    .await;
    let status = resp.status();
    let body: serde_json::Value =
        serde_json::from_slice(&http_test::read_body(resp).await).unwrap_or_default();
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

#[derive(diesel::QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}
