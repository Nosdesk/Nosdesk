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

    let before = crate::imports_assets::job_files(ws.workspace_id, &id).await;
    let (status, job) = put(serde_json::json!({"sheet": "Vehicles"})).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    // The new sheet's grid takes the old one's place in storage.
    let after = crate::imports_assets::job_files(ws.workspace_id, &id).await;
    assert_eq!(after.len(), 2, "{after:?}");
    assert_ne!(after[0], before[0], "a new grid version");
    assert_eq!(after[1], "upload.xlsx");
    assert!(job["source"].get("grid").is_none(), "{job}");
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
    assert_eq!(body["code"], "import_sheet_unknown", "{body}");
    let (status, body) = put(serde_json::json!({"sheet": "Cameras", "header_row": 2})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("Row 2"),
        "{body}"
    );
    assert_eq!(body["code"], "import_header_row_empty", "{body}");
    assert_eq!(
        body["args"],
        serde_json::json!({"row": "2", "sheet": "Cameras"}),
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
    assert!(
        body["code"]
            .as_str()
            .is_some_and(|c| c.starts_with("import_")),
        "{body}"
    );
    body["error"].as_str().unwrap_or("").to_string()
}

/// A workbook whose rows are all on hidden sheets is refused with its own
/// code rather than opened on a hidden sheet, and makes no job.
#[actix_web::test]
async fn rows_on_hidden_sheets_alone_are_refused() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let mut wb = Workbook::new();
    {
        let sheet = wb.add_worksheet();
        sheet.set_name("Cover").expect("name");
    }
    {
        let sheet = wb.add_worksheet();
        sheet.set_name("Assets").expect("name");
        sheet.set_hidden(true);
        sheet.write_string(0, 0, "name").expect("write");
        sheet.write_string(0, 1, "kind").expect("write");
        sheet.write_string(1, 0, "Camera A").expect("write");
        sheet.write_string(1, 1, "generic").expect("write");
    }
    let bytes = wb.save_to_buffer().expect("save workbook");

    let (status, body) = upload(&pool, &ws, &bytes).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "import_sheets_hidden", "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("hidden sheets"),
        "{body}"
    );
    let jobs: i64 = diesel::sql_query("SELECT COUNT(*)::bigint AS count FROM import_jobs")
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .map(|r| r.count)
        .expect("count");
    assert_eq!(jobs, 0);
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

    // None of the refused uploads left a job.
    let jobs: i64 = diesel::sql_query("SELECT COUNT(*)::bigint AS count FROM import_jobs")
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .map(|r| r.count)
        .expect("count");
    assert_eq!(jobs, 0);
}

#[derive(diesel::QueryableByName)]
struct Count {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// A template download's content type, filename and bytes, asserting 200.
async fn template_download(
    pool: &crate::common::TestPool,
    ws: &crate::common::WorkspaceSeed,
    uri: &str,
) -> (String, String, Vec<u8>) {
    let (status, headers, body) = crate::imports_assets::call_import_route_with_headers(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(uri),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{uri}");
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    (
        header("content-type"),
        header("content-disposition"),
        body.to_vec(),
    )
}

/// Each sheet's name and whether it's visible.
fn sheets(bytes: &[u8]) -> Vec<(String, bool)> {
    use calamine::Reader;
    let wb: calamine::Xlsx<_> = calamine::Xlsx::new(Cursor::new(bytes.to_vec())).expect("xlsx");
    wb.sheets_metadata()
        .iter()
        .map(|s| (s.name.clone(), s.visible == calamine::SheetVisible::Visible))
        .collect()
}

/// A sheet's cells, row by row, as calamine reads them.
fn cells(bytes: &[u8], sheet: &str) -> Vec<Vec<calamine::Data>> {
    use calamine::Reader;
    let mut wb: calamine::Xlsx<_> = calamine::Xlsx::new(Cursor::new(bytes.to_vec())).expect("xlsx");
    let range = wb.worksheet_range(sheet).expect("sheet");
    range.rows().map(<[calamine::Data]>::to_vec).collect()
}

fn text(cell: &calamine::Data) -> String {
    match cell {
        calamine::Data::String(s) => s.clone(),
        calamine::Data::Empty => String::new(),
        other => panic!("{other:?} is not a string cell"),
    }
}

/// The Lists sheet's columns in order, as field id and entries, every
/// cell a string.
fn lists(bytes: &[u8]) -> Vec<(String, Vec<String>)> {
    let rows = cells(bytes, "Lists");
    let Some(head) = rows.first() else {
        return Default::default();
    };
    head.iter()
        .enumerate()
        .map(|(col, id)| {
            let entries = rows[1..]
                .iter()
                .map(|row| text(&row[col]))
                .filter(|v| !v.is_empty())
                .collect();
            (text(id), entries)
        })
        .collect()
}

fn part(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut xml = String::new();
    archive
        .by_name(name)
        .expect("part")
        .read_to_string(&mut xml)
        .expect("read part");
    xml
}

/// `?format=xlsx` gives a workbook for every type and kind: the Template
/// sheet's header row is the CSV template's, bold and frozen, with
/// Warning-style dropdowns on rows 2 to 5,001 pointing at the hidden Lists
/// sheet; Fields describes each column. Dropdowns list labels, a kind's
/// attributes list that kind's options, and a workspace-named value that
/// looks like a formula is a string cell.
#[actix_web::test]
async fn the_xlsx_template_has_dropdowns_and_a_fields_sheet() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = crate::imports_attributes::workspace(&pool);
    crate::imports_attributes::seed_kind(
        &pool,
        &ws,
        "hostile",
        "=1+1",
        320,
        serde_json::json!({ "type": "object", "properties": {} }),
    );

    for (path, kind, stem) in [
        ("assets", None, "nosdesk-assets-template"),
        ("assets", Some("vehicle"), "nosdesk-assets-vehicle-template"),
        ("assets", Some("camera"), "nosdesk-assets-camera-template"),
        ("users", None, "nosdesk-users-template"),
        ("tickets", None, "nosdesk-tickets-template"),
    ] {
        let csv_uri = match kind {
            Some(kind) => format!("/api/admin/import/template/{path}?kind={kind}"),
            None => format!("/api/admin/import/template/{path}"),
        };
        let xlsx_uri = match kind {
            Some(_) => format!("{csv_uri}&format=xlsx"),
            None => format!("{csv_uri}?format=xlsx"),
        };
        let (content_type, disposition, bytes) = template_download(&pool, &ws, &xlsx_uri).await;
        assert_eq!(
            content_type,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(disposition, format!("attachment; filename=\"{stem}.xlsx\""));
        assert_eq!(
            sheets(&bytes),
            vec![
                ("Template".to_string(), true),
                ("Fields".to_string(), true),
                ("Lists".to_string(), false),
            ],
            "{xlsx_uri}"
        );

        // The header row is the CSV template's, unguarded.
        let (_, _, csv) = template_download(&pool, &ws, &csv_uri).await;
        let csv = String::from_utf8(csv).expect("utf8");
        let csv_headers: Vec<String> = csv
            .trim_start_matches('\u{FEFF}')
            .trim_end()
            .split(',')
            .map(|h| h.strip_prefix('\'').unwrap_or(h).to_string())
            .collect();
        let template = cells(&bytes, "Template");
        assert_eq!(template.len(), 1, "{xlsx_uri}: header row only");
        let headers: Vec<String> = template[0].iter().map(text).collect();
        assert_eq!(headers, csv_headers, "{xlsx_uri}");

        // Fields: a heading row, then one row per column in header order.
        let fields = cells(&bytes, "Fields");
        assert_eq!(
            fields[0].iter().map(text).collect::<Vec<_>>(),
            [
                "Column",
                "Label",
                "Type",
                "Required",
                "Allowed values",
                "Format"
            ]
        );
        assert_eq!(
            fields[1..].iter().map(|r| text(&r[0])).collect::<Vec<_>>(),
            headers,
            "{xlsx_uri}"
        );

        // Every dropdown on the Template sheet points at its Lists column,
        // as a warning, over the data rows.
        let lists = lists(&bytes);
        let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains(r#"<pane ySplit="1""#),
            "{xlsx_uri}: frozen header"
        );
        for (i, (id, entries)) in lists.iter().enumerate() {
            let col = headers.iter().position(|h| h == id).expect("list column");
            let letter = backend::services::imports::xlsx::column_letter(col as u32);
            let list = backend::services::imports::xlsx::column_letter(i as u32);
            let formula = format!(
                "<formula1>Lists!${list}$2:${list}${}</formula1>",
                entries.len() + 1
            );
            assert!(sheet.contains(&formula), "{xlsx_uri}: {id} {formula}");
            assert!(
                sheet.contains(&format!(r#"sqref="{letter}2:{letter}5001""#)),
                "{xlsx_uri}: {id}"
            );
        }
        assert!(!sheet.contains(r#"errorStyle="stop""#));
        assert_eq!(
            sheet.matches("<dataValidation ").count(),
            sheet.matches(r#"errorStyle="warning""#).count(),
            "{xlsx_uri}"
        );
        for name in [
            "xl/worksheets/sheet1.xml",
            "xl/worksheets/sheet2.xml",
            "xl/worksheets/sheet3.xml",
        ] {
            assert!(!part(&bytes, name).contains("<f>"), "{xlsx_uri}: {name}");
        }

        let list = |id: &str| {
            lists
                .iter()
                .find(|(list, _)| list == id)
                .map(|(_, entries)| entries.clone())
                .unwrap_or_default()
        };
        let field_row = |id: &str| {
            fields
                .iter()
                .find(|r| text(&r[0]) == id)
                .map(|r| r.iter().map(text).collect::<Vec<_>>())
                .unwrap_or_else(|| panic!("{xlsx_uri}: no {id} on Fields"))
        };
        match (path, kind) {
            ("assets", kind) => {
                let kinds = list("kind");
                assert!(kinds.contains(&"Vehicle".to_string()), "{kinds:?}");
                assert!(kinds.contains(&"=1+1".to_string()), "{kinds:?}");
                assert_eq!(
                    list("status"),
                    [
                        "In service",
                        "In stock",
                        "On order",
                        "In transit",
                        "In repair",
                        "Retired",
                        "Lost"
                    ]
                );
                assert_eq!(
                    field_row("kind")[..4],
                    ["kind", "Kind", "Choice", "Yes"].map(String::from)
                );
                assert!(field_row("kind")[4].contains("=1+1"));
                assert_eq!(field_row("purchase_date")[2], "Date");
                assert_eq!(field_row("purchase_date")[5], "YYYY-MM-DD");
                assert!(sheet.contains(r#"type="date""#));
                assert!(sheet.contains(r#"type="decimal""#));
                match kind {
                    Some("vehicle") => {
                        assert_eq!(list("fuel"), ["Diesel", "Petrol", "Electric"]);
                        assert_eq!(list("condition"), ["Roadworthy", "Unregistered"]);
                        assert_eq!(list("insured"), ["Yes", "No"]);
                        assert_eq!(field_row("fuel")[3], "For Vehicle");
                        assert_eq!(field_row("serviced")[5], "YYYY-MM-DD");
                        assert!(headers.contains(&"@tare".to_string()));
                    }
                    Some("camera") => {
                        assert_eq!(list("condition"), ["Good", "Damaged"]);
                        assert_eq!(list("mount"), ["EF", "PL"]);
                        assert!(list("fuel").is_empty());
                    }
                    _ => assert!(list("fuel").is_empty()),
                }
            }
            ("users", _) => {
                assert_eq!(list("role"), ["Admin", "Technician", "User"]);
                assert_eq!(field_row("email")[3], "Yes");
            }
            _ => {
                assert_eq!(
                    list("priority"),
                    ["None", "Low", "Medium", "High", "Urgent"]
                );
                assert!(!list("workflow_state").is_empty());
                assert_eq!(field_row("due_date")[5], "YYYY-MM-DD");
            }
        }
    }

    // CSV stays the default; an unknown format is refused.
    let (content_type, _, default) =
        template_download(&pool, &ws, "/api/admin/import/template/users").await;
    assert_eq!(content_type, "text/csv; charset=utf-8");
    let (_, _, csv) =
        template_download(&pool, &ws, "/api/admin/import/template/users?format=csv").await;
    assert_eq!(default, csv);
    assert_eq!(String::from_utf8_lossy(&csv), "email,name,role,pronouns\n");
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import/template/assets?format=pdf"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.member_uuid,
        TestRequest::get().uri("/api/admin/import/template/assets?format=xlsx"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// A template filled in with the dropdowns' labels imports as it stands:
/// every header maps by its exact id, every value matches exactly, and
/// the rows are written.
#[actix_web::test]
async fn a_filled_xlsx_template_imports_with_exact_matches() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = crate::imports_attributes::workspace(&pool);
    let (_, _, bytes) = template_download(
        &pool,
        &ws,
        "/api/admin/import/template/assets?kind=vehicle&format=xlsx",
    )
    .await;
    let headers: Vec<String> = cells(&bytes, "Template")[0].iter().map(text).collect();
    let lists = lists(&bytes);
    let entries = |id: &str| {
        lists
            .iter()
            .find(|(list, _)| list == id)
            .map(|(_, entries)| entries.clone())
            .unwrap_or_else(|| panic!("no {id} list"))
    };
    let pick = |id: &str, i: usize| entries(id)[i].clone();
    let vehicle = entries("kind")
        .into_iter()
        .find(|k| k == "Vehicle")
        .expect("Vehicle on the kind list");

    // The admin's copy: the template's header row and three rows, choice
    // cells typed from the dropdowns, a date and a number as Excel stores
    // them.
    let mut wb = Workbook::new();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    {
        let sheet = wb.add_worksheet();
        sheet.set_name("Template").expect("name");
        for (c, h) in headers.iter().enumerate() {
            sheet.write_string(0, c as u16, h).expect("write");
        }
        let col = |id: &str| headers.iter().position(|h| h == id).expect(id) as u16;
        for r in 1..=3u32 {
            let i = r as usize - 1;
            sheet
                .write_string(r, col("name"), format!("QA van {r}"))
                .expect("write");
            sheet.write_string(r, col("kind"), &vehicle).expect("write");
            sheet
                .write_string(r, col("asset_tag"), format!("QA-X-{r}"))
                .expect("write");
            sheet
                .write_string(r, col("status"), pick("status", 4))
                .expect("write");
            sheet
                .write_string(r, col("location"), "Studio A")
                .expect("write");
            sheet
                .write_string(r, col("fuel"), pick("fuel", i))
                .expect("write");
            sheet
                .write_string(r, col("condition"), pick("condition", 0))
                .expect("write");
            sheet
                .write_string(r, col("insured"), pick("insured", i % 2))
                .expect("write");
            sheet
                .write_number(r, col("odometer"), 1000.0 * f64::from(r))
                .expect("write");
            sheet
                .write_datetime_with_format(
                    r,
                    col("serviced"),
                    ExcelDateTime::from_ymd(2026, 3, r as u8).expect("date"),
                    &date,
                )
                .expect("write");
        }
    }
    let filled = wb.save_to_buffer().expect("save");

    let (status, job) = upload(&pool, &ws, &filled).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["source"]["sheet"], "Template", "{job}");
    let suggested = job["suggested_mapping"].as_array().expect("suggestions");
    assert_eq!(suggested.len(), headers.len(), "{job}");
    let mut mapping = serde_json::Map::new();
    for s in suggested {
        let index = s["index"].as_u64().expect("index") as usize;
        assert_eq!(s["reason"], "exact", "{s}");
        assert_eq!(s["field"], headers[index].as_str(), "{s}");
        mapping.insert(headers[index].clone(), index.into());
    }
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, serde_json::json!({ "mapping": mapping })),
    )
    .await;
    let checked = json(&body);
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["summary"]["would_create"], 3, "{checked}");
    assert_eq!(checked["summary"]["rows_with_errors"], 0, "{checked}");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
    )
    .await;
    let view = json(&body);
    assert_eq!(status, StatusCode::OK, "{view}");
    let columns = view["columns"].as_array().expect("columns");
    for field in ["kind", "status", "fuel", "insured", "condition"] {
        assert!(
            columns.iter().any(|c| c["field"] == field),
            "no {field} column in {view}"
        );
    }
    for column in columns {
        for value in column["values"].as_array().expect("values") {
            assert_eq!(value["match"], "exact", "{} {value}", column["field"]);
        }
    }

    crate::imports_attributes::commit(&pool, &ws, &id).await;
    #[derive(QueryableByName)]
    struct Stored {
        #[diesel(sql_type = diesel::sql_types::Text)]
        kind: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        status: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        attributes: String,
    }
    let stored: Stored = diesel::sql_query(
        "SELECT kind, status, attributes::text AS attributes FROM assets WHERE asset_tag = 'QA-X-2'",
    )
    .get_result(&mut pool.get().expect("conn"))
    .expect("asset");
    assert_eq!(stored.kind, "vehicle");
    assert_eq!(stored.status, "in_repair");
    let attributes: serde_json::Value = serde_json::from_str(&stored.attributes).expect("json");
    assert_eq!(attributes["fuel"], "Petrol");
    assert_eq!(attributes["insured"], false);
    assert_eq!(attributes["odometer"], 2000);
    assert_eq!(attributes["serviced"], "2026-03-02");
}
