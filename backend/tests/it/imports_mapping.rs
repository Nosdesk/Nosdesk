//! Column mapping: suggestions, the mapping endpoint's refusals, the
//! mapped grid the importers read, and the Map step's options.

use std::collections::BTreeMap;

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;

use backend::services::imports::mapping::{self, ColumnMapping};
use backend::services::imports::{
    self, csv_parser, DecimalMark, ImportContext, ImportOptions, ImportType, MatchBy,
};

use crate::common::TestDb;
use crate::imports_assets::{
    call_import_route, import_workspace, json, mapping_request, template_mapping,
    upload_request_for,
};

// ---- service level ----

fn parse(text: &str) -> csv_parser::ParsedCsv {
    csv_parser::parse_bytes(text.as_bytes()).expect("parse")
}

/// Map a file by header name: `pairs` is (field id, header).
fn mapped(
    conn: &mut backend::db::DbConnection,
    grid: &csv_parser::ParsedCsv,
    pairs: &[(&str, &str)],
) -> csv_parser::ParsedCsv {
    let fields = imports::importer_for(ImportType::Assets)
        .fields(conn)
        .expect("fields");
    let mapping: ColumnMapping = pairs
        .iter()
        .map(|(field, header)| {
            let index = grid
                .headers
                .iter()
                .position(|h| h == header)
                .expect("header");
            (field.to_string(), index)
        })
        .collect();
    mapping::apply_mapping(grid, &mapping, &fields)
}

fn ctx(options: ImportOptions) -> ImportContext {
    ImportContext {
        actor: None,
        job_id: uuid::Uuid::new_v4(),
        options,
    }
}

#[derive(diesel::QueryableByName, Debug, PartialEq)]
struct Stored {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    kind: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    location: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    notes: Option<String>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    quantity: Option<String>,
}

fn stored(conn: &mut diesel::PgConnection, filter: &str) -> Stored {
    diesel::sql_query(format!(
        "SELECT name, kind, location, notes, quantity::text AS quantity FROM assets WHERE {filter}"
    ))
    .get_result(conn)
    .expect("read asset")
}

fn seed(conn: &mut diesel::PgConnection, name: &str, tag: Option<&str>, serial: Option<&str>) {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, serial_number, location) VALUES ($1, 'generic', $2, $3, 'Store')",
    )
    .bind::<diesel::sql_types::Text, _>(name)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(tag)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(serial)
    .execute(conn)
    .expect("seed asset");
}

/// A field no column maps to reaches the importer blank, and a blank
/// never clears: the asset keeps its location.
#[test]
fn an_unmapped_field_leaves_the_stored_value() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed(&mut conn, "Camera 1", Some("MAP-001"), None);

    let grid = parse(
        "Tag,Title,Type,Remarks,Location\nMAP-001,Camera One,generic,Lens cap lost,Studio B\n",
    );
    let parsed = mapped(
        &mut conn,
        &grid,
        &[
            ("asset_tag", "Tag"),
            ("name", "Title"),
            ("kind", "Type"),
            ("notes", "Remarks"),
        ],
    );
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!(
        (summary.would_update, summary.errors_total),
        (1, 0),
        "{summary:?}"
    );
    imports::commit(
        &mut conn,
        ImportType::Assets,
        &parsed,
        &ctx(ImportOptions::default()),
    )
    .expect("commit");

    let asset = stored(&mut conn, "asset_tag = 'MAP-001'");
    assert_eq!(asset.name, "Camera One");
    assert_eq!(asset.notes.as_deref(), Some("Lens cap lost"));
    assert_eq!(asset.location.as_deref(), Some("Store"));
}

/// Matching by serial number updates the asset holding the serial, even
/// with no tag in the row; matching by tag reads the same row as a new
/// asset whose serial is taken.
#[test]
fn match_by_serial_number_updates_the_serial_holder() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed(&mut conn, "Old name", Some("SER-TAG"), Some("SN-9"));
    let grid = parse("name,kind,serial_number\nNew name,generic,SN-9\n");
    let parsed = mapped(
        &mut conn,
        &grid,
        &[
            ("name", "name"),
            ("kind", "kind"),
            ("serial_number", "serial_number"),
        ],
    );

    let by_tag = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!(by_tag.would_create + by_tag.would_update, 0);
    assert!(by_tag.errors[0]
        .message
        .contains("already used by another asset"));

    let options = ImportOptions {
        match_by: MatchBy::SerialNumber,
        ..Default::default()
    };
    let by_serial =
        imports::dry_run_with(&mut conn, ImportType::Assets, &parsed, &options).expect("dry-run");
    assert_eq!(
        (
            by_serial.would_create,
            by_serial.would_update,
            by_serial.errors_total
        ),
        (0, 1, 0),
        "{by_serial:?}"
    );
    imports::commit(&mut conn, ImportType::Assets, &parsed, &ctx(options)).expect("commit");
    let asset = stored(&mut conn, "serial_number = 'SN-9'");
    assert_eq!(asset.name, "New name");
    let tag: Option<String> = assets_tag(&mut conn, "SN-9");
    assert_eq!(tag.as_deref(), Some("SER-TAG"));
}

fn assets_tag(conn: &mut diesel::PgConnection, serial: &str) -> Option<String> {
    #[derive(diesel::QueryableByName)]
    struct Tag {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        asset_tag: Option<String>,
    }
    diesel::sql_query("SELECT asset_tag FROM assets WHERE serial_number = $1")
        .bind::<diesel::sql_types::Text, _>(serial)
        .get_result::<Tag>(conn)
        .expect("read tag")
        .asset_tag
}

/// With matching off every row creates, and a row whose tag or serial an
/// asset already has is an error naming that asset.
#[test]
fn match_by_none_refuses_rows_that_already_exist() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed(
        &mut conn,
        "Existing camera",
        Some("NONE-001"),
        Some("SN-NONE"),
    );
    let grid = parse(
        "name,kind,asset_tag,serial_number\n\
         Again,generic,NONE-001,\n\
         Serial again,generic,,SN-NONE\n\
         Fresh,generic,NONE-002,\n",
    );
    let parsed = mapped(
        &mut conn,
        &grid,
        &[
            ("name", "name"),
            ("kind", "kind"),
            ("asset_tag", "asset_tag"),
            ("serial_number", "serial_number"),
        ],
    );
    let options = ImportOptions {
        match_by: MatchBy::None,
        ..Default::default()
    };
    let summary =
        imports::dry_run_with(&mut conn, ImportType::Assets, &parsed, &options).expect("dry-run");
    assert_eq!((summary.would_create, summary.would_update), (1, 0));
    let errors: Vec<(usize, Option<&str>, &str)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref(), e.message.as_str()))
        .collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!((errors[0].0, errors[0].1), (2, Some("asset_tag")));
    assert!(
        errors[0].2.contains("already exists as Existing camera"),
        "{errors:?}"
    );
    assert_eq!((errors[1].0, errors[1].1), (3, Some("serial_number")));
    assert!(
        errors[1].2.contains("already exists as Existing camera"),
        "{errors:?}"
    );
}

/// With `,` as the decimal mark, `12,5` is twelve and a half and a `.`
/// (a thousands separator, perhaps) is an error; with `.` a lone comma
/// points at the option.
#[test]
fn the_decimal_mark_reads_number_cells() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let grid = parse(
        "name;kind;asset_tag;quantity\n\
         Toner;generic;DEC-001;12,5\n\
         Paper;generic;DEC-002;1.234,5\n",
    );
    let parsed = mapped(
        &mut conn,
        &grid,
        &[
            ("name", "name"),
            ("kind", "kind"),
            ("asset_tag", "asset_tag"),
            ("quantity", "quantity"),
        ],
    );
    let comma = ImportOptions {
        decimal: Some(DecimalMark::Comma),
        ..Default::default()
    };
    let summary =
        imports::dry_run_with(&mut conn, ImportType::Assets, &parsed, &comma).expect("dry-run");
    assert_eq!(summary.would_create, 1);
    assert_eq!(summary.errors.len(), 1, "{:?}", summary.errors);
    assert_eq!(summary.errors[0].row, 3);
    assert!(summary.errors[0].message.contains("decimal mark is ','"));
    imports::commit(&mut conn, ImportType::Assets, &parsed, &ctx(comma)).expect("commit");
    assert_eq!(
        stored(&mut conn, "asset_tag = 'DEC-001'")
            .quantity
            .as_deref(),
        Some("12.500")
    );

    let dot = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert!(
        dot.errors[0].message.contains("choose ',' at the Map step"),
        "{:?}",
        dot.errors
    );
}

/// A blank kind takes the default kind on a new asset and keeps an
/// existing asset's kind.
#[test]
fn the_default_kind_fills_blank_kinds_on_new_assets() {
    let db = TestDb::new();
    let mut conn = db.conn();
    seed(&mut conn, "Kept", Some("DK-001"), None);
    let grid = parse("name,asset_tag\nKept,DK-001\nNew,DK-002\n");
    let parsed = mapped(
        &mut conn,
        &grid,
        &[("name", "name"), ("asset_tag", "asset_tag")],
    );

    let without = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!((without.would_update, without.errors_total), (1, 1));
    assert_eq!(without.errors[0].column.as_deref(), Some("kind"));

    let options = ImportOptions {
        default_kind: Some("laptop".into()),
        ..Default::default()
    };
    imports::commit(&mut conn, ImportType::Assets, &parsed, &ctx(options)).expect("commit");
    assert_eq!(stored(&mut conn, "asset_tag = 'DK-001'").kind, "generic");
    assert_eq!(stored(&mut conn, "asset_tag = 'DK-002'").kind, "laptop");
}

/// The summary counts every error and groups identical ones per column,
/// even past the stored list's cap.
#[test]
fn errors_are_counted_and_grouped_past_the_cap() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let mut text = String::from("name,kind,quantity\n");
    for i in 0..120 {
        text.push_str(&format!("Item {i},generic,lots\n"));
    }
    text.push_str(",generic,1\n");
    let grid = parse(&text);
    let parsed = mapped(
        &mut conn,
        &grid,
        &[("name", "name"), ("kind", "kind"), ("quantity", "quantity")],
    );
    let summary = imports::dry_run(&mut conn, ImportType::Assets, &parsed).expect("dry-run");
    assert_eq!(summary.errors.len(), 100);
    assert!(summary.errors_truncated);
    assert_eq!(summary.errors_total, 121);
    assert_eq!(summary.rows_with_errors, 121);
    let groups: Vec<(Option<&str>, &str, usize)> = summary
        .error_groups
        .iter()
        .map(|g| (g.column.as_deref(), g.message.as_str(), g.count))
        .collect();
    assert_eq!(
        groups,
        vec![
            (Some("quantity"), "'lots' is not a valid decimal", 120),
            (Some("name"), "name is required", 1),
        ]
    );
}

// ---- over HTTP ----

async fn upload(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    job_type: &str,
    file: &str,
) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request_for(job_type, "data.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "parsed", "{job}");
    job
}

fn suggestions(job: &serde_json::Value) -> BTreeMap<usize, (Option<String>, String)> {
    job["suggested_mapping"]
        .as_array()
        .expect("suggested_mapping")
        .iter()
        .map(|s| {
            (
                s["index"].as_u64().expect("index") as usize,
                (
                    s["field"].as_str().map(String::from),
                    s["reason"].as_str().expect("reason").to_string(),
                ),
            )
        })
        .collect()
}

fn pick(field: Option<&str>, reason: &str) -> (Option<String>, String) {
    (field.map(String::from), reason.to_string())
}

/// Upload describes the file and suggests a mapping; the next upload of
/// the type reuses the admin's choices for the same headers, whether or
/// not that import was committed.
#[actix_web::test]
async fn upload_suggests_a_mapping_and_remembers_the_last_one() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let job = upload(
        &pool,
        &ws,
        "assets",
        "Device,Type,Asset #,S/N,Brand,Room,Comments,Qty,Row,Errors,Owner\n\
         Cam 1,generic,T-1,SN-1,Sony,Studio A,,2,,,Ana\n\
         Cam 2,generic,T-2,SN-2,Sony,,,,,,\n",
    )
    .await;
    let source = &job["source"];
    assert_eq!(source["format"], "csv");
    assert_eq!(source["header_row"], 1);
    assert_eq!(source["row_count"], 2);
    assert_eq!(source["headers"][0]["name"], "Device");
    assert_eq!(
        source["headers"][0]["samples"],
        serde_json::json!(["Cam 1", "Cam 2"])
    );
    assert_eq!(
        source["headers"][5]["samples"],
        serde_json::json!(["Studio A"])
    );
    assert!(source.get("suggested_mapping").is_none(), "{source}");
    assert_eq!(
        suggestions(&job),
        BTreeMap::from([
            (1, pick(Some("kind"), "synonym")),
            (2, pick(Some("asset_tag"), "synonym")),
            (3, pick(Some("serial_number"), "synonym")),
            (4, pick(Some("manufacturer"), "synonym")),
            (5, pick(Some("location"), "synonym")),
            (6, pick(Some("notes"), "synonym")),
            (7, pick(Some("quantity"), "synonym")),
            (8, pick(None, "report")),
            (9, pick(None, "report")),
        ])
    );

    // The admin maps Device to name and Owner (not Comments) to notes,
    // and checks without committing.
    let id = job["id"].as_str().expect("id");
    let body = serde_json::json!({
        "mapping": { "name": 0, "kind": 1, "asset_tag": 2, "notes": 10 },
    });
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, mapping_request(id, body)).await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));

    let next = upload(
        &pool,
        &ws,
        "assets",
        "device,Comments,OWNER,Asset tag\nCam 3,,Bo,T-3\n",
    )
    .await;
    assert_eq!(
        suggestions(&next),
        BTreeMap::from([
            (0, pick(Some("name"), "previous")),
            (2, pick(Some("notes"), "previous")),
            (3, pick(Some("asset_tag"), "label")),
        ])
    );

    // Users suggest from their own fields.
    let users = upload(
        &pool,
        &ws,
        "users",
        "Email Address,Full Name,role\na@example.com,A,user\n",
    )
    .await;
    assert_eq!(
        suggestions(&users),
        BTreeMap::from([
            (0, pick(Some("email"), "synonym")),
            (1, pick(Some("name"), "synonym")),
            (2, pick(Some("role"), "exact")),
        ])
    );
    let id = users["id"].as_str().expect("id");
    let body = serde_json::json!({ "mapping": { "email": 0, "name": 1, "role": 2 } });
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, mapping_request(id, body)).await;
    let users = json(&body);
    assert_eq!(status, StatusCode::OK, "{users}");
    assert_eq!(users["summary"]["would_create"], 1, "{users}");
}

/// The fields endpoint lists what a column can map to, grouped.
#[actix_web::test]
async fn the_fields_endpoint_lists_the_targets() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/fields?type=assets"),
    )
    .await;
    let body = json(&body);
    assert_eq!(status, StatusCode::OK, "{body}");
    let fields = body["fields"].as_array().expect("fields");
    let ids: Vec<&str> = fields.iter().filter_map(|f| f["id"].as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "name",
            "kind",
            "asset_tag",
            "serial_number",
            "location",
            "notes",
            "quantity",
            "unit",
            "low_stock_threshold",
            "manufacturer",
            "model"
        ]
    );
    assert_eq!(fields[0]["required"], true);
    assert_eq!(fields[0]["group"], "core");
    assert_eq!(fields[0]["label_key"], "import-field-assets-name");
    assert!(fields[1]["enum"]
        .as_array()
        .is_some_and(|kinds| kinds.iter().any(|k| k == "generic")));
    assert_eq!(fields[6]["value_type"], "number");
    assert_eq!(fields[9]["group"], "links");

    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/fields?type=widgets"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The mapping endpoint refuses a mapping it can't run, naming the
/// problem, and accepts the default kind in place of a Kind column.
#[actix_web::test]
async fn the_mapping_endpoint_refuses_what_it_cannot_run() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = upload(
        &pool,
        &ws,
        "assets",
        "Name,Kind,Serial,S/N\nCam,generic,A,B\n",
    )
    .await;
    let id = job["id"].as_str().expect("id").to_string();

    let cases = [
        (
            serde_json::json!({ "mapping": { "name": 0, "kind": 1, "serial_number": 0 } }),
            "column 'Name' is mapped to both",
        ),
        (
            serde_json::json!({ "mapping": { "name": 0, "kind": 1, "colour": 2 } }),
            "unknown field 'colour'",
        ),
        (
            serde_json::json!({ "mapping": { "kind": 1 } }),
            "no column maps to Name",
        ),
        (
            serde_json::json!({ "mapping": { "name": 0 } }),
            "no column maps to Kind",
        ),
        (
            serde_json::json!({ "mapping": { "name": 0, "kind": 9 } }),
            "the file has 4 columns",
        ),
        (
            serde_json::json!({ "mapping": { "name": 0 }, "options": { "default_kind": "spaceship" } }),
            "unknown default kind 'spaceship'",
        ),
    ];
    for (body, message) in cases {
        let (status, resp) =
            call_import_route(&pool, &ws, ws.admin_uuid, mapping_request(&id, body)).await;
        let resp = json(&resp);
        assert_eq!(status, StatusCode::BAD_REQUEST, "{message}: {resp}");
        assert!(
            resp["error"].as_str().unwrap_or("").contains(message),
            "{message}: {resp}"
        );
    }

    // Malformed bodies: a field named twice, an unknown option, a bad value.
    for raw in [
        r#"{"mapping":{"name":0,"name":2}}"#,
        r#"{"mapping":{"name":0,"kind":1},"options":{"colour":"red"}}"#,
        r#"{"mapping":{"name":0,"kind":1},"options":{"match_by":"barcode"}}"#,
    ] {
        let req = TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/mapping"))
            .insert_header(("content-type", "application/json"))
            .set_payload(raw);
        let (status, _) = call_import_route(&pool, &ws, ws.admin_uuid, req).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{raw}");
    }

    // The kind's label resolves to its slug and covers the Kind column.
    let body = serde_json::json!({
        "mapping": { "name": 0, "serial_number": 3 },
        "options": { "default_kind": "Generic asset", "match_by": "serial_number" },
    });
    let (status, resp) =
        call_import_route(&pool, &ws, ws.admin_uuid, mapping_request(&id, body)).await;
    let resp = json(&resp);
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["status"], "dry_run_done");
    assert_eq!(resp["options"]["default_kind"], "generic");
    assert_eq!(resp["options"]["match_by"], "serial_number");
    assert_eq!(resp["options"]["decimal"], ".");
    assert_eq!(
        resp["mapping"],
        serde_json::json!({ "name": 0, "serial_number": 3 })
    );
    assert_eq!(resp["summary"]["would_create"], 1, "{resp}");
}

/// Errors name the file's header next to the field, and the summary groups
/// them; a `;`-separated file defaults to the `,` decimal mark.
#[actix_web::test]
async fn errors_name_the_source_column_and_group() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = upload(
        &pool,
        &ws,
        "assets",
        "Title;Kind;Qty\nA;generic;lots\nB;generic;lots\n;generic;1,5\nD;generic;2,5\n",
    )
    .await;
    assert_eq!(job["source"]["delimiter"], ";");
    let id = job["id"].as_str().expect("id").to_string();
    let body = serde_json::json!({ "mapping": { "name": 0, "kind": 1, "quantity": 2 } });
    let (status, resp) =
        call_import_route(&pool, &ws, ws.admin_uuid, mapping_request(&id, body)).await;
    let job = json(&resp);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["options"]["decimal"], ",");

    let summary = &job["summary"];
    assert_eq!(summary["would_create"], 1, "{summary}");
    assert_eq!(summary["errors_total"], 3);
    assert_eq!(summary["rows_with_errors"], 3);
    assert_eq!(summary["errors"][0]["column"], "quantity");
    assert_eq!(summary["errors"][0]["source_column"], "Qty");
    assert_eq!(summary["errors"][2]["source_column"], "Title");
    assert_eq!(
        summary["error_groups"][0],
        serde_json::json!({
            "column": "quantity",
            "source_column": "Qty",
            "message": "'lots' is not a valid decimal",
            "count": 2,
        })
    );

    // Committing uses the stored options: 2,5 lands as 2.5.
    let (status, resp) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", json(&resp));
    #[derive(diesel::QueryableByName)]
    struct Quantity {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        quantity: Option<String>,
    }
    let row: Quantity = diesel::sql_query(
        "SELECT quantity::text AS quantity FROM assets WHERE name = 'D' AND workspace_id = $1",
    )
    .bind::<diesel::sql_types::Integer, _>(ws.workspace_id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("read asset");
    assert_eq!(row.quantity.as_deref(), Some("2.500"));

    // A committed job's mapping is settled.
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, template_mapping()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// A job checked before mapping existed has no mapping to commit with: the
/// admin is told to upload again.
#[actix_web::test]
async fn a_job_without_a_mapping_cannot_be_committed() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job_id = uuid::Uuid::new_v4();
    diesel::sql_query(
        "INSERT INTO import_jobs (id, job_type, status, filename, file_path, workspace_id)
         VALUES ($1, 'assets', 'dry_run_done', 'assets.csv', '/dev/null', $2)",
    )
    .bind::<diesel::sql_types::Uuid, _>(job_id)
    .bind::<diesel::sql_types::Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed job");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{job_id}/commit")),
    )
    .await;
    let body = json(&body);
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("Upload the file again"),
        "{body}"
    );
}

/// A header-only file is refused at upload.
#[actix_web::test]
async fn a_file_without_data_rows_is_refused() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request_for("assets", "assets.csv", b"name,kind\n"),
    )
    .await;
    let body = json(&body);
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("header row and no data"),
        "{body}"
    );
}
