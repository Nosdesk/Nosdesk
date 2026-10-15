//! Row results: every row's outcome kept on the job, paged through
//! `GET rows`, the error report built from them, and the match key each
//! row carries.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;

use backend::services::imports::{
    self, csv_parser, ImportOptions, ImportType, MatchBy, RecordId, RowOutcome,
};

use crate::common::{import_ctx, TestDb};
use crate::imports_assets::{
    call_import_route, import_workspace, import_workspaces, json, mapping_request,
    upload_and_check, upload_request,
};

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

fn asset_id(conn: &mut diesel::PgConnection, where_clause: &str) -> i32 {
    #[derive(diesel::QueryableByName)]
    struct Id {
        #[diesel(sql_type = diesel::sql_types::Integer)]
        id: i32,
    }
    diesel::sql_query(format!("SELECT id FROM assets WHERE {where_clause}"))
        .get_result::<Id>(conn)
        .expect("read asset id")
        .id
}

/// An asset in workspace 1, the workspace service-level tests run in.
fn seed_asset(conn: &mut diesel::PgConnection, name: &str, tag: &str, serial: &str) -> i32 {
    seed_asset_in(conn, 1, name, tag, serial)
}

fn seed_asset_in(
    conn: &mut diesel::PgConnection,
    workspace_id: i32,
    name: &str,
    tag: &str,
    serial: &str,
) -> i32 {
    diesel::sql_query(
        "INSERT INTO assets (workspace_id, name, kind, asset_tag, serial_number)
         VALUES ($1, $2, 'generic', $3, $4)",
    )
    .bind::<diesel::sql_types::Integer, _>(workspace_id)
    .bind::<diesel::sql_types::Text, _>(name)
    .bind::<diesel::sql_types::Text, _>(tag)
    .bind::<diesel::sql_types::Text, _>(serial)
    .execute(conn)
    .expect("seed asset");
    asset_id(conn, &format!("asset_tag = '{tag}'"))
}

/// Matched by serial number, each row's key is its serial; a row whose
/// tag another asset holds is skipped rather than sharing the tag; and
/// rows with neither tag nor serial are counted in a notice, since
/// importing the file again would create them again.
#[test]
fn match_by_serial_keys_rows_by_serial() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let first = seed_asset(&mut conn, "First", "KEY-1", "SN-1");
    seed_asset(&mut conn, "Second", "KEY-2", "SN-2");

    let parsed = csv_parser::parse_bytes(
        format!(
            "{HEADER}\n\
             First renamed,generic,,SN-1,,,,,,,\n\
             Takes a tag,generic,KEY-1,SN-2,,,,,,,\n\
             New,generic,,SN-NEW,,,,,,,\n\
             No key one,generic,,,,,,,,,\n\
             No key two,generic,,,,,,,,,\n"
        )
        .as_bytes(),
    )
    .expect("parse");
    let mut ctx = import_ctx();
    ctx.options = ImportOptions {
        match_by: MatchBy::SerialNumber,
        ..Default::default()
    };

    let (summary, rows) =
        imports::check(&mut conn, ImportType::Assets, &parsed, &ctx).expect("dry-run");
    let got: Vec<(usize, RowOutcome, Option<&str>)> = rows
        .iter()
        .map(|r| (r.row, r.outcome, r.key.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            (2, RowOutcome::Update, Some("SN-1")),
            (3, RowOutcome::Skipped, Some("SN-2")),
            (4, RowOutcome::Create, Some("SN-NEW")),
            (5, RowOutcome::Create, None),
            (6, RowOutcome::Create, None),
        ]
    );
    assert_eq!(rows[0].record_id, Some(RecordId::Int(first)));
    assert_eq!(rows[0].label.as_deref(), Some("First renamed"));
    assert_eq!(
        rows[1].errors[0].message,
        "tag 'KEY-1' is already used by First"
    );
    assert_eq!(summary.notices.len(), 1, "{:?}", summary.notices);
    assert_eq!(
        (summary.notices[0].code.as_str(), summary.notices[0].count),
        ("no_key", 2)
    );

    let committed = imports::apply(&mut conn, ImportType::Assets, &parsed, &ctx).expect("commit");
    let new_id = asset_id(&mut conn, "serial_number = 'SN-NEW'");
    assert_eq!(committed.rows[2].record_id, Some(RecordId::Int(new_id)));
    assert_eq!(committed.rows[0].record_id, Some(RecordId::Int(first)));
    assert_eq!(committed.rows[1].outcome, RowOutcome::Skipped);
}

/// An email two rows of a user file hold skips both rows, each naming
/// the other.
#[test]
fn an_email_twice_in_the_file_skips_both_rows() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let admin = crate::imports_users::new_member(&mut conn, 1, "admin");
    let parsed = csv_parser::parse_bytes(
        b"email,name,role,pronouns\n\
          twice@example.com,First,user,\n\
          once@example.com,Once,user,\n\
          TWICE@example.com,Second,user,\n",
    )
    .expect("parse");
    let mut ctx = import_ctx();
    ctx.actor = Some(admin);

    let (summary, rows) =
        imports::check(&mut conn, ImportType::Users, &parsed, &ctx).expect("dry-run");
    assert_eq!(summary.would_create, 1);
    let outcomes: Vec<RowOutcome> = rows.iter().map(|r| r.outcome).collect();
    assert_eq!(
        outcomes,
        vec![RowOutcome::Skipped, RowOutcome::Create, RowOutcome::Skipped]
    );
    assert!(rows[0].errors[0].message.contains("(rows 2 and 4)"));
    assert!(rows[2].errors[0].message.contains("(rows 2 and 4)"));
}

async fn get(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    uri: &str,
) -> (StatusCode, actix_web::web::Bytes) {
    call_import_route(pool, ws, ws.admin_uuid, TestRequest::get().uri(uri)).await
}

/// Every row's result is stored on the job at the dry run, paged and
/// filtered by `GET rows`, and rewritten by the commit with the ids it
/// wrote.
#[actix_web::test]
async fn rows_are_stored_and_paged() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let existing = seed_asset_in(
        &mut pool.get().expect("conn"),
        ws.workspace_id,
        "Existing",
        "EX-1",
        "EX-SN",
    );

    let job = upload_and_check(
        &pool,
        &ws,
        &format!(
            "{HEADER}\n\
             Camera 1,generic,C-1,,,,,,,,\n\
             Existing renamed,generic,EX-1,,,,,,,,\n\
             ,generic,NO-NAME,,,,,,,,\n\
             Camera 2,generic,C-2,,,,,,,,\n\
             Dup A,generic,D-1,,,,,,,,\n\
             Dup B,generic,D-1,,,,,,,,\n\
             Camera 3,generic,C-3,,,,,,,,\n"
        ),
    )
    .await;
    let id = job["id"].as_str().expect("job id").to_string();
    assert!(job.get("row_results").is_none(), "{job}");
    assert_eq!(job["summary"]["would_create"], 3, "{job}");

    let (status, body) = get(&pool, &ws, &format!("/api/admin/import/{id}/rows")).await;
    let page = json(&body);
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["phase"], "checked");
    assert_eq!(
        page["counts"],
        serde_json::json!({"all": 7, "create": 3, "update": 1, "unchanged": 0, "skipped": 3})
    );
    assert_eq!(page["total"], 7);
    assert_eq!(page["limit"], 50);
    let rows = page["rows"].as_array().expect("rows");
    let numbers: Vec<u64> = rows.iter().map(|r| r["row"].as_u64().unwrap()).collect();
    assert_eq!(numbers, vec![2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(rows[1]["outcome"], "update");
    assert_eq!(rows[1]["key"], "EX-1");
    assert_eq!(rows[1]["label"], "Existing renamed");
    assert_eq!(rows[1]["record_id"], existing);
    assert!(rows.iter().all(|r| r.get("cells").is_none()), "{page}");

    // Skipped rows, one at a time: the second is the first duplicate.
    let (status, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/rows?filter=skipped&offset=1&limit=1"),
    )
    .await;
    let page = json(&body);
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["total"], 3);
    let rows = page["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["row"], 6);
    assert_eq!(rows[0]["errors"][0]["column"], "asset_tag");
    assert_eq!(rows[0]["errors"][0]["source_column"], "asset_tag");
    assert!(rows[0]["errors"][0]["message"]
        .as_str()
        .unwrap()
        .contains("(rows 6 and 7)"));

    // `error` reads as `skipped`; an unknown filter is refused.
    let (_, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/rows?filter=error"),
    )
    .await;
    assert_eq!(json(&body)["total"], 3);
    let (status, _) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/rows?filter=nope"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
    assert_eq!(json(&body)["records_committed"], 4);

    let (_, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/rows?filter=create"),
    )
    .await;
    let page = json(&body);
    assert_eq!(page["phase"], "committed");
    assert_eq!(page["total"], 3);
    let mut conn = pool.get().expect("conn");
    for row in page["rows"].as_array().expect("rows") {
        let tag = row["key"].as_str().expect("key");
        let written = asset_id(&mut conn, &format!("asset_tag = '{tag}'"));
        assert_eq!(row["record_id"], written, "{row}");
    }
}

/// The error report gives back every skipped row in the file's own
/// layout and delimiter: its line number, the reasons under the file's
/// headers, then its cells, each through the formula guard. Uploaded
/// again, `Row` and `Errors` map to nothing and the fixed rows import.
#[actix_web::test]
async fn the_error_report_round_trips() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let file = "Asset #;Title;Type;Remarks;Qty\n\
                CAM-1;Camera 1;generic;;2,5\n\
                CAM-2;Camera 2;nonsense;=HYPERLINK(\"x\");1\n\
                CAM-3;;generic;-tripod;\n\
                CAM-4;Camera 4;generic;;\n\
                CAM-4;Camera 4 again;generic;;\n";
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request("Studio kit.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("job id").to_string();

    // Before the dry run there are no rows to report.
    let (status, _) = get(&pool, &ws, &format!("/api/admin/import/{id}/error-report")).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let mapping = serde_json::json!({
        "mapping": {"asset_tag": 0, "name": 1, "kind": 2, "notes": 3, "quantity": 4}
    });
    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, mapping_request(&id, mapping)).await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["summary"]["would_create"], 1, "{job}");

    let req = TestRequest::get().uri(&format!("/api/admin/import/{id}/error-report"));
    let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with("\u{FEFF}".as_bytes()), "BOM");
    let text = String::from_utf8(body.to_vec()).expect("utf-8");
    let lines: Vec<&str> = text.trim_start_matches('\u{FEFF}').lines().collect();
    assert_eq!(lines[0], "Row;Errors;Asset #;Title;Type;Remarks;Qty");
    assert!(
        lines[1].starts_with("3;Type: unknown kind 'nonsense'."),
        "{}",
        lines[1]
    );
    assert!(
        lines[1].ends_with(";CAM-2;Camera 2;nonsense;\"'=HYPERLINK(\"\"x\"\")\";1"),
        "{}",
        lines[1]
    );
    assert_eq!(
        lines[2],
        "4;Title: name is required;CAM-3;;generic;'-tripod;"
    );
    let duplicate = "Asset #: tag 'CAM-4' appears twice in this file (rows 5 and 6)";
    assert_eq!(lines[3], format!("5;{duplicate};CAM-4;Camera 4;generic;;"));
    assert_eq!(
        lines[4],
        format!("6;{duplicate};CAM-4;Camera 4 again;generic;;")
    );
    assert_eq!(lines.len(), 5);

    // Fix the rows in the report and upload it again.
    let fixed = text
        .replace(";nonsense;", ";generic;")
        .replace(";CAM-3;;", ";CAM-3;Camera 3;")
        .replace(";CAM-4;Camera 4 again;", ";CAM-5;Camera 5;");
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request("Studio kit-errors.csv", fixed.as_bytes()),
    )
    .await;
    let again = json(&body);
    assert_eq!(status, StatusCode::OK, "{again}");
    let suggested = again["suggested_mapping"].as_array().expect("suggestions");
    assert_eq!(suggested[0]["reason"], "report");
    assert_eq!(suggested[1]["reason"], "report");
    let again_id = again["id"].as_str().expect("job id").to_string();
    let mapping = serde_json::json!({
        "mapping": {"asset_tag": 2, "name": 3, "kind": 4, "notes": 5, "quantity": 6}
    });
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&again_id, mapping),
    )
    .await;
    let checked = json(&body);
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["summary"]["would_create"], 4, "{checked}");
    assert_eq!(checked["summary"]["rows_with_errors"], 0, "{checked}");
}

/// The row routes answer a non-admin with 403 (`import_routes_refuse_a_non_admin`),
/// another workspace's job with 404, and a job not yet checked with 409.
#[actix_web::test]
async fn row_routes_stay_in_their_workspace() {
    let db = TestDb::new();
    let seeded = import_workspaces(&db.pool_with_size(2));
    let pool = db.runtime_pool(4);
    let (a, b) = (&seeded.a, &seeded.b);

    let (status, body) = call_import_route(
        &pool,
        a,
        a.admin_uuid,
        upload_request(
            "assets.csv",
            format!("{HEADER}\nOne,generic,W-1,,,,,,,,\n").as_bytes(),
        ),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("job id").to_string();

    for path in ["rows", "error-report"] {
        let uri = format!("/api/admin/import/{id}/{path}");
        assert_eq!(get(&pool, a, &uri).await.0, StatusCode::CONFLICT, "{path}");
        assert_eq!(get(&pool, b, &uri).await.0, StatusCode::NOT_FOUND, "{path}");
    }

    let (status, _) = call_import_route(
        &pool,
        a,
        a.admin_uuid,
        mapping_request(&id, crate::imports_assets::template_mapping()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for path in ["rows", "error-report"] {
        let uri = format!("/api/admin/import/{id}/{path}");
        assert_eq!(get(&pool, a, &uri).await.0, StatusCode::OK, "{path}");
        assert_eq!(get(&pool, b, &uri).await.0, StatusCode::NOT_FOUND, "{path}");
    }
}
