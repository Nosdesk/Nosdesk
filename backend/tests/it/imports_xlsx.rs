//! .xlsx uploads over HTTP: sheets, title rows above the header, typed
//! cells read as text, cell errors, true sheet row numbers through the
//! check, the commit and the error report, choosing another sheet or
//! header row, and the refusals for unsafe or unreadable files.

use std::io::{Cursor, Write};

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook};

use crate::common::TestDb;
use crate::imports_assets::{
    call_import_route, import_workspace, json, mapping_request, upload_request,
};

/// The workbook the tests import:
///
/// - Cameras: a title on row 1, row 2 blank, headers on row 3. Row 4 is a
///   whole camera (a date, a whole number stored as a float, a formula's
///   cached text), row 5 is blank, row 6 has `#N/A` in Notes, row 7 has no
///   name, row 8 is whole.
/// - Vehicles: headers on row 1.
/// - Lookups: hidden.
fn workbook() -> Vec<u8> {
    let mut wb = Workbook::new();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    {
        let ws = wb.add_worksheet();
        ws.set_name("Cameras").expect("name");
        ws.write_string(0, 0, "TVS Assets 2026").expect("write");
        for (c, h) in [
            "Asset #",
            "Device Name",
            "Type",
            "Qty",
            "Purchased",
            "Notes",
        ]
        .iter()
        .enumerate()
        {
            ws.write_string(2, c as u16, *h).expect("write");
        }
        let camera = |ws: &mut rust_xlsxwriter::Worksheet, row: u32, tag: &str, name: &str| {
            ws.write_string(row, 0, tag).expect("write");
            if !name.is_empty() {
                ws.write_string(row, 1, name).expect("write");
            }
            ws.write_string(row, 2, "generic").expect("write");
        };
        camera(ws, 3, "XL-001", "Camera A");
        ws.write_number(3, 3, 2.0).expect("write");
        ws.write_datetime_with_format(
            3,
            4,
            ExcelDateTime::from_ymd(2024, 3, 5).expect("date"),
            &date,
        )
        .expect("write");
        ws.write_formula(
            3,
            5,
            Formula::new("=\"Lens \"&\"cap\"").set_result("Lens cap"),
        )
        .expect("write");
        camera(ws, 5, "XL-002", "Camera B");
        ws.write_formula(5, 5, Formula::new("=NA()").set_result("#N/A"))
            .expect("write");
        camera(ws, 6, "XL-003", "");
        camera(ws, 7, "XL-004", "Camera D");
        ws.write_number(7, 3, 3.0).expect("write");
    }
    {
        let ws = wb.add_worksheet();
        ws.set_name("Vehicles").expect("name");
        ws.write_string(0, 0, "Rego").expect("write");
        ws.write_string(0, 1, "Make").expect("write");
        ws.write_string(1, 0, "ABC-123").expect("write");
        ws.write_string(1, 1, "Toyota").expect("write");
    }
    {
        let ws = wb.add_worksheet();
        ws.set_name("Lookups").expect("name");
        ws.set_hidden(true);
        ws.write_string(0, 0, "Kind").expect("write");
        ws.write_string(1, 0, "Camera").expect("write");
    }
    wb.save_to_buffer().expect("save workbook")
}

async fn upload(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    bytes: &[u8],
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request("assets.xlsx", bytes),
    )
    .await;
    (status, json(&body))
}

fn source_request(job_id: &str, body: serde_json::Value) -> TestRequest {
    TestRequest::put()
        .uri(&format!("/api/admin/import/{job_id}/source"))
        .set_json(body)
}

fn header_names(job: &serde_json::Value) -> Vec<String> {
    job["source"]["headers"]
        .as_array()
        .expect("headers")
        .iter()
        .map(|h| h["name"].as_str().unwrap_or("").to_string())
        .collect()
}

/// The upload opens on the first visible sheet, finds the header under the
/// title, lists every sheet with its rows (the hidden one marked), and
/// reads typed cells as text.
#[actix_web::test]
async fn upload_lists_sheets_and_finds_the_header_under_the_title() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let (status, job) = upload(&pool, &ws, &workbook()).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "parsed");
    let source = &job["source"];
    assert_eq!(source["format"], "xlsx");
    assert_eq!(source["sheet"], "Cameras");
    assert_eq!(source["header_row"], 3);
    assert_eq!(source["row_count"], 4);
    assert_eq!(
        source["sheets"],
        serde_json::json!([
            {"name": "Cameras", "visible": true, "rows": 4},
            {"name": "Vehicles", "visible": true, "rows": 1},
            {"name": "Lookups", "visible": false, "rows": 1},
        ])
    );
    assert_eq!(
        header_names(&job),
        vec![
            "Asset #",
            "Device Name",
            "Type",
            "Qty",
            "Purchased",
            "Notes"
        ]
    );
    let samples = |i: usize| source["headers"][i]["samples"].clone();
    assert_eq!(samples(3), serde_json::json!(["2", "3"]));
    assert_eq!(samples(4), serde_json::json!(["2024-03-05"]));
    // The formula's cached value; the #N/A cell is no sample.
    assert_eq!(samples(5), serde_json::json!(["Lens cap"]));
    // The preview starts at the title so the header row can be confirmed.
    assert_eq!(source["preview"][0]["row"], 1);
    assert_eq!(source["preview"][0]["cells"][0], "TVS Assets 2026");
    assert_eq!(source["preview"][2]["cells"][1], "Device Name");
    // Labels and synonyms still suggest a mapping.
    let suggested: Vec<(u64, String)> = job["suggested_mapping"]
        .as_array()
        .expect("suggestions")
        .iter()
        .filter_map(|s| Some((s["index"].as_u64()?, s["field"].as_str()?.to_string())))
        .collect();
    assert!(
        suggested.contains(&(0, "asset_tag".to_string())),
        "{suggested:?}"
    );
}

fn camera_mapping() -> serde_json::Value {
    serde_json::json!({
        "mapping": {"asset_tag": 0, "name": 1, "kind": 2, "quantity": 3, "notes": 5},
        "options": {}
    })
}

/// Skipped rows carry their sheet row numbers through the check, the
/// commit and the error report; a `#N/A` in a mapped column skips its row
/// and nothing is written for it.
#[actix_web::test]
async fn sheet_row_numbers_reach_the_rows_and_the_report() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let (status, job) = upload(&pool, &ws, &workbook()).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("id").to_string();

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, camera_mapping()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["summary"]["row_count"], 4, "{job}");
    assert_eq!(job["summary"]["would_create"], 2, "{job}");
    assert_eq!(job["summary"]["rows_with_errors"], 2, "{job}");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows")),
    )
    .await;
    let page = json(&body);
    assert_eq!(status, StatusCode::OK, "{page}");
    let rows: Vec<(u64, String)> = page["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| {
            (
                r["row"].as_u64().unwrap_or(0),
                r["outcome"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (4, "create".to_string()),
            (6, "skipped".to_string()),
            (7, "skipped".to_string()),
            (8, "create".to_string()),
        ]
    );
    let na = &page["rows"][1]["errors"][0];
    assert_eq!(na["source_column"], "Notes", "{na}");
    assert!(
        na["message"].as_str().unwrap_or("").contains("#N/A"),
        "{na}"
    );

    let report = report(&pool, &ws, &id).await;
    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(
        lines[0],
        "\u{feff}Row,Errors,Asset #,Device Name,Type,Qty,Purchased,Notes"
    );
    assert!(lines[1].starts_with("6,"), "{report}");
    assert!(
        lines[1].ends_with(",XL-002,Camera B,generic,,,#N/A"),
        "{report}"
    );
    assert!(lines[2].starts_with("7,"), "{report}");
    assert_eq!(lines.len(), 3, "{report}");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
    assert_eq!(json(&body)["records_committed"], 2);

    #[derive(diesel::QueryableByName, Debug, PartialEq)]
    struct Stored {
        #[diesel(sql_type = diesel::sql_types::Text)]
        asset_tag: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        quantity: Option<String>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        notes: Option<String>,
    }
    let stored: Vec<Stored> = diesel::sql_query(
        "SELECT asset_tag, quantity::text AS quantity, notes FROM assets
         WHERE asset_tag LIKE 'XL-%' ORDER BY asset_tag",
    )
    .load(&mut pool.get().expect("conn"))
    .expect("read assets");
    assert_eq!(
        stored,
        vec![
            Stored {
                asset_tag: "XL-001".into(),
                quantity: Some("2.000".into()),
                notes: Some("Lens cap".into()),
            },
            Stored {
                asset_tag: "XL-004".into(),
                quantity: Some("3.000".into()),
                notes: None,
            },
        ]
    );

    // After the commit the report keeps the same rows and numbers.
    let after = self::report(&pool, &ws, &id).await;
    assert_eq!(after, report);
}

async fn report(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    id: &str,
) -> String {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/error-report")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    String::from_utf8(body.to_vec()).expect("utf-8 report")
}

/// `PUT source` reads another sheet or header row, re-derives headers and
/// suggestions, and takes a checked job back to `parsed` with its mapping
/// cleared. It refuses an unknown sheet, an empty header row, a CSV job
/// and a committed one.
#[actix_web::test]
async fn put_source_switches_sheet_and_header_row() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let (_, job) = upload(&pool, &ws, &workbook()).await;
    let id = job["id"].as_str().expect("id").to_string();
    let put = |body: serde_json::Value| {
        let id = id.clone();
        let pool = pool.clone();
        let ws = ws.clone();
        async move {
            let (status, body) =
                call_import_route(&pool, &ws, ws.admin_uuid, source_request(&id, body)).await;
            (status, json(&body))
        }
    };

    let (status, job) = put(serde_json::json!({"sheet": "Vehicles"})).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["source"]["sheet"], "Vehicles");
    assert_eq!(job["source"]["header_row"], 1);
    assert_eq!(header_names(&job), vec!["Rego", "Make"]);
    assert_eq!(job["source"]["row_count"], 1);

    // The title row as header: the old header row becomes data.
    let (status, job) = put(serde_json::json!({"sheet": "Cameras", "header_row": 1})).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["source"]["header_row"], 1);
    assert_eq!(job["source"]["row_count"], 5);
    assert_eq!(header_names(&job)[0], "TVS Assets 2026");

    // Back to the guessed header, then check, then change the sheet: the
    // job is parsed again with no mapping or summary.
    let (status, job) = put(serde_json::json!({"sheet": "Cameras"})).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["source"]["header_row"], 3);
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, camera_mapping()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
    let (status, job) = put(serde_json::json!({"sheet": "Cameras", "header_row": 3})).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "parsed");
    assert!(job["mapping"].is_null(), "{job}");
    assert!(job["summary"].is_null(), "{job}");
    // The checked mapping is remembered by header, so it comes back as
    // the suggestion.
    assert!(
        job["suggested_mapping"]
            .as_array()
            .expect("suggestions")
            .iter()
            .any(|s| s["index"] == 5 && s["field"] == "notes" && s["reason"] == "previous"),
        "{job}"
    );
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows")),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) = put(serde_json::json!({"sheet": "Nope"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = put(serde_json::json!({"sheet": "Cameras", "header_row": 2})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("Row 2"),
        "{body}"
    );

    // Checked again and committed: the sheet can't change any more.
    call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, camera_mapping()),
    )
    .await;
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = put(serde_json::json!({"sheet": "Vehicles"})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // A CSV has no sheets.
    let (status, csv_job) = {
        let (status, body) = call_import_route(
            &pool,
            &ws,
            ws.admin_uuid,
            upload_request("assets.csv", b"name,kind\nA,generic\n"),
        )
        .await;
        (status, json(&body))
    };
    assert_eq!(status, StatusCode::OK, "{csv_job}");
    let csv_id = csv_job["id"].as_str().expect("id");
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        source_request(csv_id, serde_json::json!({"sheet": "Sheet1"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{}", json(&body));

    // Workspace admins only.
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.member_uuid,
        source_request(&id, serde_json::json!({"sheet": "Cameras"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// A zip with entries named like a workbook's parts.
fn zip_of(entries: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(method)
        .large_file(true);
    for (name, data) in entries {
        zip.start_file(*name, options).expect("start");
        zip.write_all(data).expect("write");
    }
    zip.finish().expect("finish").into_inner()
}

async fn refusal(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    bytes: &[u8],
) -> String {
    let (status, body) = upload(pool, ws, bytes).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    body["error"].as_str().unwrap_or("").to_string()
}

/// A part that inflates past 64 MB, parts that together pass 128 MB, more
/// than 10,000 entries and an encrypted entry are refused before the
/// reader sees them; so is an OLE file (an .xls or a password-protected
/// workbook).
#[actix_web::test]
async fn unsafe_and_unreadable_workbooks_are_refused() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let big = vec![0u8; 64 * 1024 * 1024 + 1];
    let bomb = zip_of(
        &[("xl/worksheets/sheet1.xml", &big)],
        zip::CompressionMethod::Deflated,
    );
    assert!(bomb.len() < 10 * 1024 * 1024, "fixture fits the upload cap");
    assert!(refusal(&pool, &ws, &bomb)
        .await
        .contains("too large once unpacked"));

    let fifty = vec![0u8; 50 * 1024 * 1024];
    let total = zip_of(
        &[
            ("xl/worksheets/sheet1.xml", &fifty),
            ("xl/worksheets/sheet2.xml", &fifty),
            ("xl/worksheets/sheet3.xml", &fifty),
        ],
        zip::CompressionMethod::Deflated,
    );
    assert!(refusal(&pool, &ws, &total)
        .await
        .contains("too large once unpacked"));

    let names: Vec<String> = (0..=10_000).map(|i| format!("xl/p{i}.xml")).collect();
    let many: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
    let many = zip_of(&many, zip::CompressionMethod::Stored);
    assert!(refusal(&pool, &ws, &many)
        .await
        .contains("more than 10,000 parts"));

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .with_aes_encryption(zip::AesMode::Aes256, "secret");
    zip.start_file("xl/workbook.xml", options).expect("start");
    zip.write_all(b"<workbook/>").expect("write");
    let encrypted = zip.finish().expect("finish").into_inner();
    assert!(refusal(&pool, &ws, &encrypted)
        .await
        .contains("password-protected or an older .xls"));

    let mut cfb = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    cfb.extend_from_slice(&[0u8; 512]);
    assert!(refusal(&pool, &ws, &cfb)
        .await
        .contains("password-protected or an older .xls"));

    // A zip that isn't a workbook.
    let not_a_workbook = zip_of(&[("hello.txt", b"hi")], zip::CompressionMethod::Deflated);
    assert!(refusal(&pool, &ws, &not_a_workbook)
        .await
        .contains("can't be read"));

    // None of the refused uploads left a job that could be checked.
    let checkable: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM import_jobs WHERE status <> 'failed'",
    )
    .get_result::<Count>(&mut pool.get().expect("conn"))
    .map(|r| r.count)
    .expect("count");
    assert_eq!(checkable, 0);
}

#[derive(diesel::QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}
