//! Importing service history (`?type=asset_records`): rows find their asset
//! by tag, serial number or tag then serial, and a row whose asset isn't
//! here is an error grouped with the others like it; outcomes are answered
//! per record type; dates with and without a time; costs under a decimal
//! comma; ticket numbers linked, unknown ones left off; statuses never
//! change; rows that look already recorded are skipped unless imported
//! anyway; `source_ref` updates, re-runs and Restore; Remove keeps an edited
//! record; a catalogue type the workspace hasn't added is added on commit.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable, Text};

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, json, mapping_request, upload_request_for,
};
use crate::imports_attributes::{commit, errors, id_of, rows};

const REF: &str = "test:imports_asset_records";

/// The fixture workspace, its currency AUD and its timezone `zone`.
fn workspace(pool: &TestPool, zone: &str) -> WorkspaceSeed {
    let ws = import_workspace(pool);
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        backend::repository::site_settings::get_site_settings(c)?;
        diesel::sql_query(
            "UPDATE site_settings SET currency_code = 'AUD', default_timezone = $1 WHERE workspace_id = $2",
        )
        .bind::<Text, _>(zone)
        .bind::<Integer, _>(ws.workspace_id)
        .execute(c)
    })
    .expect("settings");
    ws
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// An asset in service, by tag and serial: its id.
fn seed_asset(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    name: &str,
    tag: Option<&str>,
    serial: Option<&str>,
) -> i32 {
    diesel::sql_query(
        "INSERT INTO assets (name, kind, asset_tag, serial_number, location, status, workspace_id)
         VALUES ($1, 'generic', $2, $3, 'Studio A', 'in_service', $4) RETURNING id",
    )
    .bind::<Text, _>(name)
    .bind::<Nullable<Text>, _>(tag)
    .bind::<Nullable<Text>, _>(serial)
    .bind::<Integer, _>(ws.workspace_id)
    .get_result::<Id>(&mut pool.get().expect("conn"))
    .expect("seed asset")
    .id
}

#[derive(QueryableByName, Debug, Clone)]
struct Record {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Integer)]
    asset_id: i32,
    #[diesel(sql_type = Text)]
    record_type: String,
    #[diesel(sql_type = Nullable<Text>)]
    summary: Option<String>,
    #[diesel(sql_type = Text)]
    occurred_on: String,
    #[diesel(sql_type = Nullable<Text>)]
    occurred_at: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    completed_on: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    outcome: Option<String>,
    #[diesel(sql_type = Nullable<BigInt>)]
    cost_minor: Option<i64>,
    #[diesel(sql_type = Nullable<Text>)]
    cost_currency: Option<String>,
    #[diesel(sql_type = Nullable<Integer>)]
    ticket_id: Option<i32>,
    #[diesel(sql_type = Text)]
    source: String,
    #[diesel(sql_type = Nullable<Text>)]
    source_ref: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    import_job_id: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    recorded_by: Option<String>,
    #[diesel(sql_type = Integer)]
    version: i32,
}

/// The workspace's records in id order.
fn records(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<Record> {
    diesel::sql_query(
        "SELECT r.id, r.asset_id, t.slug AS record_type, r.summary, r.occurred_on::text AS occurred_on,
                to_char(r.occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS') AS occurred_at,
                r.completed_on::text AS completed_on, r.outcome, r.cost_minor, r.cost_currency::text AS cost_currency,
                r.ticket_id, r.source, r.source_ref, r.import_job_id::text AS import_job_id,
                r.recorded_by::text AS recorded_by, r.version
         FROM asset_records r JOIN asset_record_types t ON t.id = r.record_type_id
         WHERE r.workspace_id = $1 ORDER BY r.id",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .load(&mut pool.get().expect("conn"))
    .expect("read records")
}

/// Upload `file` as service history and check it with `mapping` (header to
/// field id) and `options`: the job.
async fn checked(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    file: &str,
    mapping: &[(&str, &str)],
    options: serde_json::Value,
) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request_for("asset_records", "history.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    remap(pool, ws, &id_of(&job), file, mapping, options).await
}

/// Check job `id` again with `mapping` and `options`.
async fn remap(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    file: &str,
    mapping: &[(&str, &str)],
    options: serde_json::Value,
) -> serde_json::Value {
    let headers: Vec<&str> = file.lines().next().expect("header").split(',').collect();
    let body_mapping: serde_json::Map<String, serde_json::Value> = mapping
        .iter()
        .map(|(header, field)| {
            let index = headers
                .iter()
                .position(|h| h == header)
                .unwrap_or_else(|| panic!("no {header} column"));
            (field.to_string(), index.into())
        })
        .collect();
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        mapping_request(
            id,
            serde_json::json!({ "mapping": body_mapping, "options": options }),
        ),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    job
}

async fn get(pool: &TestPool, ws: &WorkspaceSeed, uri: &str) -> (StatusCode, Vec<u8>) {
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, TestRequest::get().uri(uri)).await;
    (status, body.to_vec())
}

async fn post(pool: &TestPool, ws: &WorkspaceSeed, uri: &str) -> (StatusCode, serde_json::Value) {
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, TestRequest::post().uri(uri)).await;
    (status, json(&body))
}

fn notice(job: &serde_json::Value, code: &str) -> Option<u64> {
    job["summary"]["notices"]
        .as_array()?
        .iter()
        .find(|n| n["code"] == code)
        .and_then(|n| n["count"].as_u64())
}

fn summary(job: &serde_json::Value, key: &str) -> u64 {
    job["summary"][key].as_u64().unwrap_or(0)
}

const BASIC: &[(&str, &str)] = &[
    ("Tag", "asset_tag"),
    ("Serial", "serial_number"),
    ("Type", "record_type"),
    ("Date", "occurred_on"),
];

#[actix_web::test]
async fn rows_find_their_asset_by_tag_by_serial_or_by_tag_then_serial() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    let cam1 = seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), Some("S-1"));
    let cam2 = seed_asset(&pool, &ws, "Camera 2", Some("CAM-002"), Some("S-2"));
    let untagged = seed_asset(&pool, &ws, "Tripod", None, Some("S-3"));
    let file = "Tag,Serial,Type,Date\n\
                CAM-001,,Repair,2020-01-02\n\
                ,S-3,Repair,2020-01-03\n\
                CAM-404,S-2,Repair,2020-01-04\n\
                CAM-999,,Repair,2020-01-05\n\
                cam-002,,Repair,2020-01-06\n\
                CAM-001,S-2,Repair,2020-01-07\n";

    let by_tag = checked(&pool, &ws, file, BASIC, serde_json::json!({})).await;
    assert_eq!(summary(&by_tag, "would_create"), 3, "{by_tag}");
    let errs = errors(&by_tag);
    assert!(errs.contains(&(
        3,
        Some("asset_tag".into()),
        "no asset tag to match an asset by".into()
    )));
    assert!(errs.contains(&(
        4,
        Some("asset_tag".into()),
        "no asset with tag 'CAM-404'".into()
    )));
    assert_eq!(notice(&by_tag, "unmatched_assets"), Some(2), "{by_tag}");

    let by_serial = remap(
        &pool,
        &ws,
        &id_of(&by_tag),
        file,
        BASIC,
        serde_json::json!({ "match_by": "serial_number" }),
    )
    .await;
    // S-3 and S-2 (twice) match; the rest have no serial.
    assert_eq!(summary(&by_serial, "would_create"), 3, "{by_serial}");

    let fallback = remap(
        &pool,
        &ws,
        &id_of(&by_tag),
        file,
        BASIC,
        serde_json::json!({ "match_by": "tag_then_serial" }),
    )
    .await;
    // CAM-001, S-3, CAM-404 by S-2, cam-002 by its tag in another case.
    assert_eq!(summary(&fallback, "would_create"), 4, "{fallback}");
    let errs = errors(&fallback);
    assert!(
        errs.iter()
            .any(|(row, _, m)| *row == 5 && m == "no asset with tag 'CAM-999'"),
        "{errs:?}"
    );
    assert!(
        errs.iter().any(|(row, _, m)| *row == 7
            && m == "tag 'CAM-001' and serial number 'S-2' name different assets"),
        "{errs:?}"
    );
    assert_eq!(notice(&fallback, "unmatched_assets"), Some(1), "{fallback}");

    // Asset imports don't take the fallback.
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request_for("assets", "assets.csv", b"Name,Tag\nA,T-1\n"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(
            &id_of(&json(&body)),
            serde_json::json!({ "mapping": { "name": 0, "asset_tag": 1 }, "options": { "match_by": "tag_then_serial" } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    commit(&pool, &ws, &id_of(&fallback)).await;
    let written: Vec<i32> = records(&pool, &ws).iter().map(|r| r.asset_id).collect();
    assert_eq!(written, vec![cam1, untagged, cam2, cam2]);
}

#[actix_web::test]
async fn unmatched_rows_are_one_problem_with_one_download() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let file = "Tag,Type,Date\n\
                CAM-010,Repair,2020-01-02\n\
                CAM-001,Repair,2020-01-03\n\
                CAM-011,Repair,2020-01-04\n\
                CAM-012,Inspection,2020-01-05\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
        ],
        serde_json::json!({}),
    )
    .await;
    let groups = job["summary"]["error_groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1, "{job}");
    assert_eq!(groups[0]["message"], "no asset with this tag");
    assert_eq!(groups[0]["count"], 3);

    let id = id_of(&job);
    let (status, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/error-report?group=0"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let report = String::from_utf8(body).expect("utf-8");
    let lines: Vec<&str> = report.trim_start_matches('\u{FEFF}').lines().collect();
    assert_eq!(lines.len(), 4, "{report}");
    for tag in ["CAM-010", "CAM-011", "CAM-012"] {
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&format!("no asset with tag '{tag}'"))),
            "{report}"
        );
    }
}

#[actix_web::test]
async fn outcomes_are_answered_per_record_type() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let file = "Tag,Type,Date,Result\n\
                CAM-001,Repair,2020-01-02,Repaired\n\
                CAM-001,Inspection,2020-01-03,Passed with notes\n\
                CAM-001,Inspection,2020-01-04,Failed\n\
                CAM-001,Repair,2020-01-05,Passed\n";
    let job = checked(
        &pool,
        &ws,
        file,
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Result", "outcome"),
        ],
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&job);
    let (status, body) = get(&pool, &ws, &format!("/api/admin/import/{id}/values")).await;
    assert_eq!(status, StatusCode::OK);
    let view: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let outcome_contexts: Vec<&str> = view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .filter(|c| c["field"] == "outcome")
        .map(|c| c["context"].as_str().unwrap_or_default())
        .collect();
    assert!(outcome_contexts.contains(&"repair"), "{view}");
    assert!(outcome_contexts.contains(&"inspection"), "{view}");
    let inspection = view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["field"] == "outcome" && c["context"] == "inspection")
        .expect("inspection outcomes");
    assert_eq!(inspection["context_label"], "Inspection");
    // "Passed" is on a repair row, and no repair outcome has that name: as
    // any optional choice nothing matches, it is left blank.
    let repair = view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["field"] == "outcome" && c["context"] == "repair")
        .expect("repair outcomes");
    let passed = repair["values"]
        .as_array()
        .expect("values")
        .iter()
        .find(|v| v["text"] == "Passed")
        .expect("Passed under repair");
    assert_ne!(passed["match"], "exact", "{passed}");

    commit(&pool, &ws, &id).await;
    let outcomes: Vec<(String, Option<String>)> = records(&pool, &ws)
        .into_iter()
        .map(|r| (r.record_type, r.outcome))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("repair".to_string(), Some("repaired".to_string())),
            (
                "inspection".to_string(),
                Some("passed_with_notes".to_string())
            ),
            ("inspection".to_string(), Some("failed".to_string())),
            ("repair".to_string(), None),
        ]
    );
}

#[actix_web::test]
async fn dates_read_as_days_or_moments_in_the_workspace_zone() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "Australia/Sydney");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let file = "Tag,Type,Date,Done\n\
                CAM-001,Repair,14/03/2019,\n\
                CAM-001,Repair,14/03/2019 09:30,15/03/2019\n";
    let mapping = &[
        ("Tag", "asset_tag"),
        ("Type", "record_type"),
        ("Date", "occurred_on"),
        ("Done", "completed_on"),
    ];
    let job = checked(
        &pool,
        &ws,
        file,
        mapping,
        serde_json::json!({ "date_order": "dmy" }),
    )
    .await;
    assert_eq!(summary(&job, "would_create"), 2, "{job}");
    commit(&pool, &ws, &id_of(&job)).await;
    let written = records(&pool, &ws);
    assert_eq!(written[0].occurred_on, "2019-03-14");
    assert_eq!(written[0].occurred_at, None);
    // A blank completion means the same day by default.
    assert_eq!(written[0].completed_on.as_deref(), Some("2019-03-14"));
    // 09:30 in Sydney is the evening before in UTC; the day is Sydney's.
    assert_eq!(written[1].occurred_on, "2019-03-14");
    assert_eq!(
        written[1].occurred_at.as_deref(),
        Some("2019-03-13T22:30:00")
    );
    assert_eq!(written[1].completed_on.as_deref(), Some("2019-03-15"));
    // Imported, by the job, with no author of their own.
    assert!(written.iter().all(|r| r.source == "import"
        && r.recorded_by.is_none()
        && r.import_job_id.as_deref() == Some(id_of(&job).as_str())));

    // With blank completion as open, the work stays open.
    let open = checked(
        &pool,
        &ws,
        "Tag,Type,Date,Done\nCAM-001,Inspection,2019-04-01,\n",
        mapping,
        serde_json::json!({ "blank_completion": "open" }),
    )
    .await;
    commit(&pool, &ws, &id_of(&open)).await;
    assert_eq!(records(&pool, &ws)[2].completed_on, None);
}

#[actix_web::test]
async fn costs_read_under_a_decimal_comma_in_their_currency() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let file = "Tag;Type;Date;Cost;Currency\n\
                CAM-001;Repair;2020-01-02;1.250,50;\n\
                CAM-001;Repair;2020-01-03;€ 12,5;EUR\n\
                CAM-001;Repair;2020-01-04;1.250;JPY\n\
                CAM-001;Repair;2020-01-05;12,345;JPY\n";
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request_for("asset_records", "costs.csv", file.as_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = id_of(&json(&body));
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(
            &id,
            serde_json::json!({
                "mapping": { "asset_tag": 0, "record_type": 1, "occurred_on": 2, "cost": 3, "currency": 4 },
                "options": { "decimal": "," }
            }),
        ),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    // Yen take no decimals.
    assert!(
        errors(&job)
            .iter()
            .any(|(row, col, _)| *row == 5 && col.as_deref() == Some("cost")),
        "{job}"
    );
    let costs = &job["summary"]["history"]["costs"];
    assert_eq!(
        costs,
        &serde_json::json!([
            { "currency": "AUD", "amount": "1250.50", "rows": 1 },
            { "currency": "EUR", "amount": "12.50", "rows": 1 },
            { "currency": "JPY", "amount": "1250", "rows": 1 }
        ]),
        "{job}"
    );
    commit(&pool, &ws, &id).await;
    let written: Vec<(Option<i64>, Option<String>)> = records(&pool, &ws)
        .into_iter()
        .map(|r| (r.cost_minor, r.cost_currency))
        .collect();
    assert_eq!(
        written,
        vec![
            (Some(125_050), Some("AUD".to_string())),
            (Some(1_250), Some("EUR".to_string())),
            (Some(1_250), Some("JPY".to_string())),
        ]
    );
}

#[derive(QueryableByName)]
struct TicketRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Integer)]
    number: i32,
}

#[actix_web::test]
async fn ticket_numbers_link_and_unknown_ones_are_left_off() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let ticket: TicketRow = diesel::sql_query(
        "INSERT INTO tickets (workspace_id, title, workflow_state_id)
         VALUES ($1, 'Lens cap', (SELECT id FROM workflow_states WHERE workspace_id = $1
                                  ORDER BY is_default DESC, id LIMIT 1))
         RETURNING id, number",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("ticket");
    let file = format!(
        "Tag,Type,Date,Ticket\n\
         CAM-001,Repair,2020-01-02,#{}\n\
         CAM-001,Repair,2020-01-03,99999\n",
        ticket.number
    );
    let job = checked(
        &pool,
        &ws,
        &file,
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Ticket", "ticket"),
        ],
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&job, "would_create"), 2, "{job}");
    let dropped = job["summary"]["dropped"].as_array().expect("dropped");
    assert_eq!(dropped.len(), 1, "{job}");
    assert_eq!(
        (
            &dropped[0]["column"],
            &dropped[0]["value"],
            &dropped[0]["count"]
        ),
        (
            &serde_json::json!("ticket"),
            &serde_json::json!("99999"),
            &serde_json::json!(1)
        ),
        "{job}"
    );
    commit(&pool, &ws, &id_of(&job)).await;
    let links: Vec<Option<i32>> = records(&pool, &ws).iter().map(|r| r.ticket_id).collect();
    assert_eq!(links, vec![Some(ticket.id), None]);
}

#[derive(QueryableByName, Debug, PartialEq)]
struct AssetState {
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = Text)]
    updated_at: String,
    #[diesel(sql_type = Nullable<Text>)]
    location: Option<String>,
    #[diesel(sql_type = BigInt)]
    events: i64,
}

fn asset_state(pool: &TestPool, id: i32) -> AssetState {
    diesel::sql_query(
        "SELECT status::text AS status, updated_at::text AS updated_at, location,
                (SELECT COUNT(*) FROM asset_lifecycle_events e WHERE e.asset_id = a.id) AS events
         FROM assets a WHERE id = $1",
    )
    .bind::<Integer, _>(id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("asset")
}

#[actix_web::test]
async fn an_import_never_changes_a_status() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    let cam = seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let before = asset_state(&pool, cam);
    let today = chrono::Utc::now().date_naive();
    let file = format!(
        "Tag,Type,Date,Done,Result,Where\n\
         CAM-001,Repair,2023-05-01,2023-05-09,Beyond repair,Workshop\n\
         CAM-001,Repair,{today},,,Field kit\n"
    );
    let job = checked(
        &pool,
        &ws,
        &file,
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Done", "completed_on"),
            ("Result", "outcome"),
            ("Where", "location"),
        ],
        serde_json::json!({ "blank_completion": "open" }),
    )
    .await;
    assert_eq!(summary(&job, "would_create"), 2, "{job}");
    commit(&pool, &ws, &id_of(&job)).await;
    assert_eq!(records(&pool, &ws).len(), 2);
    assert_eq!(asset_state(&pool, cam), before);
}

#[actix_web::test]
async fn rows_that_look_recorded_are_skipped_unless_imported_anyway() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let file = "Tag,Type,Date,Ref,Cost,Summary\n\
                CAM-001,Repair,2020-01-02,RMA-1,,\n\
                CAM-001,Repair,2020-01-02,,80,\n\
                CAM-001,Repair,2020-01-02,,,Lens\n\
                CAM-001,Inspection,2020-01-02,,,\n";
    let mapping = &[
        ("Tag", "asset_tag"),
        ("Type", "record_type"),
        ("Date", "occurred_on"),
        ("Ref", "reference"),
        ("Cost", "cost"),
        ("Summary", "summary"),
    ];
    let first = checked(&pool, &ws, file, mapping, serde_json::json!({})).await;
    assert_eq!(summary(&first, "would_create"), 4, "{first}");
    commit(&pool, &ws, &id_of(&first)).await;

    // The same file again: every row is already there.
    let again = checked(&pool, &ws, file, mapping, serde_json::json!({})).await;
    assert_eq!(summary(&again, "would_create"), 0, "{again}");
    assert_eq!(summary(&again, "unchanged"), 4, "{again}");
    assert_eq!(notice(&again, "likely_duplicate"), Some(4), "{again}");

    // A row whose reference and cost differ from every record's is new.
    let changed = checked(
        &pool,
        &ws,
        "Tag,Type,Date,Ref,Cost,Summary\nCAM-001,Repair,2020-01-02,RMA-2,95,\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&changed, "would_create"), 1, "{changed}");

    // Import these anyway.
    let anyway = remap(
        &pool,
        &ws,
        &id_of(&again),
        file,
        mapping,
        serde_json::json!({ "import_likely_duplicates": true }),
    )
    .await;
    assert_eq!(summary(&anyway, "would_create"), 4, "{anyway}");
    assert_eq!(notice(&anyway, "likely_duplicate"), None, "{anyway}");
    commit(&pool, &ws, &id_of(&anyway)).await;
    assert_eq!(records(&pool, &ws).len(), 8);
}

const BY_REF: &[(&str, &str)] = &[
    ("Id", "source_ref"),
    ("Tag", "asset_tag"),
    ("Type", "record_type"),
    ("Date", "occurred_on"),
    ("Summary", "summary"),
    ("Cost", "cost"),
];

#[actix_web::test]
async fn a_source_reference_updates_its_record_and_restore_puts_it_back() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let original = "Id,Tag,Type,Date,Summary,Cost\n\
                    R-1,CAM-001,Repair,2020-01-02,Lens,100\n\
                    R-2,CAM-001,Inspection,2020-02-02,Yearly,\n";
    let first = checked(&pool, &ws, original, BY_REF, serde_json::json!({})).await;
    assert_eq!(summary(&first, "would_create"), 2, "{first}");
    commit(&pool, &ws, &id_of(&first)).await;

    // Run again: nothing changes, and nothing is written.
    let rerun = checked(&pool, &ws, original, BY_REF, serde_json::json!({})).await;
    assert_eq!(summary(&rerun, "unchanged"), 2, "{rerun}");
    commit(&pool, &ws, &id_of(&rerun)).await;
    assert!(records(&pool, &ws).iter().all(|r| r.version == 1));

    // A changed row updates its record; a blank cell leaves a value.
    let edit = "Id,Tag,Type,Date,Summary,Cost\n\
                R-1,,,,Lens mount,150\n\
                R-2,,,,,\n";
    let update = checked(&pool, &ws, edit, BY_REF, serde_json::json!({})).await;
    assert_eq!(summary(&update, "would_update"), 1, "{update}");
    assert_eq!(summary(&update, "unchanged"), 1, "{update}");
    let update_id = id_of(&update);
    commit(&pool, &ws, &update_id).await;
    let r1 = records(&pool, &ws)
        .into_iter()
        .find(|r| r.source_ref.as_deref() == Some("R-1"))
        .expect("R-1");
    assert_eq!(
        (r1.summary.as_deref(), r1.cost_minor, r1.version),
        (Some("Lens mount"), Some(15_000), 2)
    );

    // The previous values file, keyed by the source reference.
    let (status, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{update_id}/previous-values"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let file = String::from_utf8(body).expect("utf-8");
    let lines: Vec<&str> = file.trim_start_matches('\u{FEFF}').lines().collect();
    // A cost goes with its currency.
    assert_eq!(
        lines,
        vec!["source_ref,summary,cost,currency", "R-1,Lens,100.00,AUD"]
    );

    // Restore puts them back through a records draft.
    let (status, draft) = post(
        &pool,
        &ws,
        &format!("/api/admin/import/{update_id}/restore"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert_eq!(draft["job_type"], "asset_records", "{draft}");
    assert_eq!(draft["status"], "dry_run_done", "{draft}");
    assert_eq!(summary(&draft, "would_update"), 1, "{draft}");
    commit(&pool, &ws, &id_of(&draft)).await;
    let r1 = records(&pool, &ws)
        .into_iter()
        .find(|r| r.source_ref.as_deref() == Some("R-1"))
        .expect("R-1");
    assert_eq!(
        (r1.summary.as_deref(), r1.cost_minor, r1.version),
        (Some("Lens"), Some(10_000), 3)
    );

    // Only new records: a reference that exists is skipped.
    let only_new = checked(
        &pool,
        &ws,
        original,
        BY_REF,
        serde_json::json!({ "mode": "create_only" }),
    )
    .await;
    assert_eq!(summary(&only_new, "rows_with_errors"), 2, "{only_new}");

    // A reference twice in a file is an error on both rows.
    let twice = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date,Summary,Cost\nR-9,CAM-001,Repair,2020-01-02,,\nR-9,CAM-001,Repair,2020-01-03,,\n",
        BY_REF,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&twice, "rows_with_errors"), 2, "{twice}");

    // A history import with no updates keeps no previous values.
    let first_id = id_of(&first);
    let (status, _) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{first_id}/previous-values"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[actix_web::test]
async fn remove_deletes_the_records_untouched_since_and_keeps_an_edited_one() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let job = checked(
        &pool,
        &ws,
        "Tag,Type,Date,Summary\n\
         CAM-001,Repair,2020-01-02,Lens\n\
         CAM-001,Repair,2020-01-03,Mount\n\
         CAM-001,Inspection,2020-01-04,Yearly\n",
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Summary", "summary"),
        ],
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&job);
    commit(&pool, &ws, &id).await;
    let edited = records(&pool, &ws)[1].id;
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "UPDATE asset_records SET summary = 'Mount, again', version = version + 1, updated_at = now()
             WHERE id = $1",
        )
        .bind::<Integer, _>(edited)
        .execute(c)
    })
    .expect("edit");

    let (status, body) = get(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/remove-created"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let plan: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(plan["created"], 3, "{plan}");
    assert_eq!(plan["deletable"], 2, "{plan}");
    assert_eq!(plan["kept"][0]["id"], edited, "{plan}");
    assert_eq!(plan["kept"][0]["reasons"][0]["code"], "edited", "{plan}");
    assert_eq!(plan["kept"][0]["name"], "Camera 1: Mount, again", "{plan}");

    let (status, done) = post(
        &pool,
        &ws,
        &format!("/api/admin/import/{id}/remove-created"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["remove_result"]["deleted"], 2, "{done}");
    let left: Vec<i32> = records(&pool, &ws).iter().map(|r| r.id).collect();
    assert_eq!(left, vec![edited]);
    // The type the import added from the catalogue stays.
    let types: i64 =
        diesel::sql_query("SELECT COUNT(*) AS n FROM asset_record_types WHERE workspace_id = $1")
            .bind::<Integer, _>(ws.workspace_id)
            .get_result::<N>(&mut pool.get().expect("conn"))
            .expect("types")
            .n;
    assert_eq!(types, 2);
}

#[derive(QueryableByName)]
struct N {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct TypeRow {
    #[diesel(sql_type = Text)]
    slug: String,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    is_builtin: bool,
    #[diesel(sql_type = Nullable<Text>)]
    label: Option<String>,
}

#[actix_web::test]
async fn a_built_in_type_is_added_on_commit_and_an_unknown_one_is_an_error() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let job = checked(
        &pool,
        &ws,
        "Tag,Type,Date\n\
         CAM-001,Preventative maintenance,2020-01-02\n\
         CAM-001,Calibration,2020-01-03\n\
         CAM-001,Gizmo,2020-01-04\n",
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
        ],
        serde_json::json!({}),
    )
    .await;
    // Nothing is in the workspace until the commit.
    let types = || -> Vec<TypeRow> {
        diesel::sql_query(
            "SELECT slug, is_builtin, label FROM asset_record_types WHERE workspace_id = $1 ORDER BY slug",
        )
        .bind::<Integer, _>(ws.workspace_id)
        .load(&mut pool.get().expect("conn"))
        .expect("types")
    };
    assert!(types().is_empty());
    // Calibration is a known other name for an inspection, which Match
    // values guesses; Gizmo names nothing.
    assert_eq!(
        job["summary"]["catalog"]["record_types"],
        serde_json::json!(["preventative_maintenance", "inspection"]),
        "{job}"
    );
    assert!(
        errors(&job).iter().any(|(row, col, m)| *row == 4
            && col.as_deref() == Some("record_type")
            && m.starts_with("unknown record type 'Gizmo'")),
        "{job}"
    );
    commit(&pool, &ws, &id_of(&job)).await;
    let added: Vec<(String, bool, Option<String>)> = types()
        .into_iter()
        .map(|t| (t.slug, t.is_builtin, t.label))
        .collect();
    assert_eq!(
        added,
        vec![
            ("inspection".to_string(), true, None),
            ("preventative_maintenance".to_string(), true, None),
        ]
    );
    assert_eq!(records(&pool, &ws).len(), 2);
    let listed = rows(&pool, &ws, &id_of(&job)).await;
    assert_eq!(listed.len(), 3);
}

/// A sheet of one kind of work, with no type column: the type is set for
/// every row, and outcomes are answered as that type's.
#[actix_web::test]
async fn a_record_type_set_for_every_row_reads_outcomes_as_its_own() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let long_ref = "R".repeat(256);
    let file = format!(
        "Tag,Date,Result,Id\n\
         CAM-001,2020-01-02,Repaired,R-1\n\
         CAM-001,2020-01-03,No fault found,R-2\n\
         CAM-001,2020-01-04,Repaired,{long_ref}\n"
    );
    let job = checked(
        &pool,
        &ws,
        &file,
        &[
            ("Tag", "asset_tag"),
            ("Date", "occurred_on"),
            ("Result", "outcome"),
            ("Id", "source_ref"),
        ],
        serde_json::json!({ "constants": [{ "field": "record_type", "value": "repair" }] }),
    )
    .await;
    assert_eq!(summary(&job, "would_create"), 2, "{job}");
    // A reference longer than a record holds is a row error, not a failed
    // commit.
    assert!(
        errors(&job)
            .iter()
            .any(|(row, col, _)| *row == 4 && col.as_deref() == Some("source_ref")),
        "{job}"
    );
    commit(&pool, &ws, &id_of(&job)).await;
    let written: Vec<(String, Option<String>)> = records(&pool, &ws)
        .into_iter()
        .map(|r| (r.record_type, r.outcome))
        .collect();
    assert_eq!(
        written,
        vec![
            ("repair".to_string(), Some("repaired".to_string())),
            ("repair".to_string(), Some("no_fault_found".to_string())),
        ]
    );
}

#[derive(QueryableByName)]
struct Stored {
    #[diesel(sql_type = Text)]
    record_type: String,
    #[diesel(sql_type = Nullable<Text>)]
    outcome: Option<String>,
    #[diesel(sql_type = Text)]
    attributes: String,
    #[diesel(sql_type = Nullable<BigInt>)]
    cost_minor: Option<i64>,
    #[diesel(sql_type = Nullable<Text>)]
    cost_currency: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    holder: Option<String>,
    #[diesel(sql_type = Text)]
    snapshot_source: String,
}

fn stored(pool: &TestPool, source_ref: &str) -> Stored {
    diesel::sql_query(
        "SELECT t.slug AS record_type, r.outcome, r.attributes::text AS attributes, r.cost_minor,
                r.cost_currency::text AS cost_currency, r.holder_uuid::text AS holder, r.snapshot_source
         FROM asset_records r JOIN asset_record_types t ON t.id = r.record_type_id
         WHERE r.source_ref = $1",
    )
    .bind::<Text, _>(source_ref)
    .get_result(&mut pool.get().expect("conn"))
    .expect("record")
}

fn attributes_of(s: &Stored) -> serde_json::Value {
    serde_json::from_str(&s.attributes).expect("attributes")
}

/// The previous values file of committed job `id`, line by line.
async fn previous_lines(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> Vec<String> {
    let (status, body) = get(pool, ws, &format!("/api/admin/import/{id}/previous-values")).await;
    assert_eq!(status, StatusCode::OK);
    String::from_utf8(body)
        .expect("utf-8")
        .trim_start_matches('\u{FEFF}')
        .lines()
        .map(str::to_string)
        .collect()
}

/// Restore committed job `id` and commit the draft.
async fn restore_and_commit(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, draft) = post(pool, ws, &format!("/api/admin/import/{id}/restore")).await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    commit(pool, ws, &id_of(&draft)).await;
    draft
}

#[actix_web::test]
async fn a_currency_change_restores_with_its_amount() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let mapping = &[
        ("Id", "source_ref"),
        ("Tag", "asset_tag"),
        ("Type", "record_type"),
        ("Date", "occurred_on"),
        ("Cost", "cost"),
        ("Currency", "currency"),
    ];
    let first = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date,Cost,Currency\nR-1,CAM-001,Repair,2020-01-02,100,\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    commit(&pool, &ws, &id_of(&first)).await;

    // The amount stays; it was paid in dollars.
    let update = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date,Cost,Currency\nR-1,,,,100,USD\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&update, "would_update"), 1, "{update}");
    // Review says only the currency changes.
    let changed: Vec<&str> = update["summary"]["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|c| c["field"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(changed, vec!["currency"], "{update}");
    let update_id = id_of(&update);
    commit(&pool, &ws, &update_id).await;
    let r1 = stored(&pool, "R-1");
    assert_eq!(
        (r1.cost_minor, r1.cost_currency.as_deref()),
        (Some(10_000), Some("USD"))
    );

    // The previous values carry the amount with its currency.
    assert_eq!(
        previous_lines(&pool, &ws, &update_id).await,
        vec!["source_ref,cost,currency", "R-1,100.00,AUD"]
    );
    let draft = restore_and_commit(&pool, &ws, &update_id).await;
    assert_eq!(summary(&draft, "would_update"), 1, "{draft}");
    let r1 = stored(&pool, "R-1");
    assert_eq!(
        (r1.cost_minor, r1.cost_currency.as_deref()),
        (Some(10_000), Some("AUD"))
    );
}

const TYPE_CHANGE: &[(&str, &str)] = &[
    ("Id", "source_ref"),
    ("Tag", "asset_tag"),
    ("Type", "record_type"),
    ("Date", "occurred_on"),
    ("Done", "completed_on"),
    ("Result", "outcome"),
    ("Fault", "fault"),
    ("Certificate", "inspector_certificate"),
];

#[actix_web::test]
async fn a_type_change_says_what_it_would_drop_and_restores_exactly() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let header = "Id,Tag,Type,Date,Done,Result,Fault,Certificate";
    let first = checked(
        &pool,
        &ws,
        &format!("{header}\nR-1,CAM-001,Repair,2020-01-02,2020-01-03,Repaired,x,\n"),
        TYPE_CHANGE,
        serde_json::json!({}),
    )
    .await;
    commit(&pool, &ws, &id_of(&first)).await;
    let before = stored(&pool, "R-1");

    // Only the type: the repair's fault and outcome have nowhere to go.
    let bare = checked(
        &pool,
        &ws,
        &format!("{header}\nR-1,,Inspection,,,,,\n"),
        TYPE_CHANGE,
        serde_json::json!({}),
    )
    .await;
    let errs = errors(&bare);
    assert!(
        errs.iter()
            .any(|(_, col, m)| col.as_deref() == Some("record_type") && m.contains("has no Fault")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|(_, col, m)| col.as_deref() == Some("outcome") && m.contains("'Repaired'")),
        "{errs:?}"
    );

    // With the inspection's own values, the type changes.
    let change = checked(
        &pool,
        &ws,
        &format!("{header}\nR-1,,Inspection,,,Passed,,C-1\n"),
        TYPE_CHANGE,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&change, "would_update"), 1, "{change}");
    let change_id = id_of(&change);
    commit(&pool, &ws, &change_id).await;
    let inspected = stored(&pool, "R-1");
    assert_eq!(inspected.record_type, "inspection");
    assert_eq!(
        attributes_of(&inspected),
        serde_json::json!({ "inspector_certificate": "C-1" })
    );

    // The previous values hold everything the change displaced.
    assert_eq!(
        previous_lines(&pool, &ws, &change_id).await,
        vec![
            "source_ref,record_type,outcome,fault,inspector_certificate",
            "R-1,repair,repaired,x,"
        ]
    );
    restore_and_commit(&pool, &ws, &change_id).await;
    let restored = stored(&pool, "R-1");
    assert_eq!(restored.record_type, before.record_type);
    assert_eq!(restored.outcome, before.outcome);
    assert_eq!(attributes_of(&restored), attributes_of(&before));
    assert_eq!(
        attributes_of(&restored),
        serde_json::json!({ "fault": "x" })
    );
}

#[actix_web::test]
async fn asset_references_read_by_tag_or_id_and_restore_by_tag() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let spare_a = seed_asset(&pool, &ws, "Spare A", Some("SPARE-A"), None);
    let spare_b = seed_asset(&pool, &ws, "Spare B", None, Some("SB-1"));
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO asset_record_types (slug, label, field_schema, field_order, core_fields, outcomes)
             VALUES ('swap', 'Swap',
                     '{\"type\":\"object\",\"properties\":{\"spare\":{\"type\":\"string\",\"format\":\"asset-ref\"}}}',
                     '{spare}', '{}', '[]')",
        )
        .execute(c)
    })
    .expect("swap type");
    let mapping = &[
        ("Id", "source_ref"),
        ("Tag", "asset_tag"),
        ("Type", "record_type"),
        ("Date", "occurred_on"),
        ("Spare", "spare"),
    ];
    let first = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date,Spare\nS-1,CAM-001,Swap,2020-01-02,SPARE-A\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    commit(&pool, &ws, &id_of(&first)).await;
    assert_eq!(
        attributes_of(&stored(&pool, "S-1")),
        serde_json::json!({ "spare": spare_a.to_string() })
    );

    // An asset without a tag, by its id.
    let update = checked(
        &pool,
        &ws,
        &format!("Id,Tag,Type,Date,Spare\nS-1,,,,{spare_b}\n"),
        mapping,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&update, "would_update"), 1, "{update}");
    let update_id = id_of(&update);
    commit(&pool, &ws, &update_id).await;
    assert_eq!(
        attributes_of(&stored(&pool, "S-1")),
        serde_json::json!({ "spare": spare_b.to_string() })
    );
    // The value before is written as the tag, and restores.
    assert_eq!(
        previous_lines(&pool, &ws, &update_id).await,
        vec!["source_ref,spare", "S-1,SPARE-A"]
    );
    restore_and_commit(&pool, &ws, &update_id).await;
    assert_eq!(
        attributes_of(&stored(&pool, "S-1")),
        serde_json::json!({ "spare": spare_a.to_string() })
    );
}

#[actix_web::test]
async fn a_time_a_clock_change_skips_imports_and_times_keep_their_seconds() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "Australia/Sydney");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let mapping = &[
        ("Id", "source_ref"),
        ("Tag", "asset_tag"),
        ("Type", "record_type"),
        ("Date", "occurred_on"),
    ];
    let first = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date\nT-1,CAM-001,Repair,2023-10-01 02:30\nT-2,CAM-001,Repair,2019-03-14 09:30:15\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&first, "would_create"), 2, "{first}");
    commit(&pool, &ws, &id_of(&first)).await;
    let written = records(&pool, &ws);
    assert_eq!(written[0].occurred_on, "2023-10-01");
    assert_eq!(
        written[0].occurred_at.as_deref(),
        Some("2023-09-30T16:30:00")
    );
    assert_eq!(
        written[1].occurred_at.as_deref(),
        Some("2019-03-13T22:30:15")
    );

    // The same file again changes nothing, to the second.
    let again = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date\nT-2,CAM-001,Repair,2019-03-14 09:30:15\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&again, "unchanged"), 1, "{again}");
    let moved = checked(
        &pool,
        &ws,
        "Id,Tag,Type,Date\nT-2,CAM-001,Repair,2019-03-14 09:31\n",
        mapping,
        serde_json::json!({}),
    )
    .await;
    let moved_id = id_of(&moved);
    commit(&pool, &ws, &moved_id).await;
    assert_eq!(
        previous_lines(&pool, &ws, &moved_id).await,
        vec!["source_ref,occurred_on", "T-2,2019-03-14T09:30:15+11:00"]
    );
}

#[actix_web::test]
async fn a_holder_who_isnt_a_member_is_recorded_as_nobody() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    let cam = seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    // Today's work would otherwise take the asset's own primary user.
    diesel::sql_query("UPDATE assets SET primary_user_uuid = $1 WHERE id = $2")
        .bind::<diesel::sql_types::Uuid, _>(ws.member_uuid)
        .bind::<Integer, _>(cam)
        .execute(&mut pool.get().expect("conn"))
        .expect("primary user");
    let today = chrono::Utc::now().date_naive();
    let job = checked(
        &pool,
        &ws,
        &format!("Id,Tag,Type,Date,Holder\nH-1,CAM-001,Repair,{today},nobody@example.com\n"),
        &[
            ("Id", "source_ref"),
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Holder", "holder"),
        ],
        serde_json::json!({}),
    )
    .await;
    let dropped = job["summary"]["dropped"].as_array().expect("dropped");
    assert_eq!(dropped.len(), 1, "{job}");
    assert_eq!(dropped[0]["column"], "holder", "{job}");
    commit(&pool, &ws, &id_of(&job)).await;
    let h1 = stored(&pool, "H-1");
    assert_eq!((h1.holder, h1.snapshot_source.as_str()), (None, "entered"));
}

#[actix_web::test]
async fn a_second_sheet_without_references_finds_the_work_by_cost() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let first = checked(
        &pool,
        &ws,
        "Tag,Type,Date,Ref,Cost\nCAM-001,Repair,2020-01-02,RMA-1,80\nCAM-001,Repair,2020-01-02,RMA-2,\n",
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Ref", "reference"),
            ("Cost", "cost"),
        ],
        serde_json::json!({}),
    )
    .await;
    commit(&pool, &ws, &id_of(&first)).await;
    // The other sheet has no reference column: the costs, then the blank
    // summaries, tell the work apart.
    let second = checked(
        &pool,
        &ws,
        "Tag,Type,Date,Cost\nCAM-001,Repair,2020-01-02,80\nCAM-001,Repair,2020-01-02,95\n",
        &[
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("Cost", "cost"),
        ],
        serde_json::json!({}),
    )
    .await;
    assert_eq!(notice(&second, "likely_duplicate"), Some(2), "{second}");
    assert_eq!(summary(&second, "would_create"), 0, "{second}");
}

#[derive(QueryableByName)]
struct Email {
    #[diesel(sql_type = Text)]
    email: String,
}

/// Give `user` a primary email (the fixture's people have none): it.
fn primary_email(pool: &TestPool, user: uuid::Uuid, name: &str) -> String {
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified)
         VALUES ($1, $2, true, true) RETURNING email",
    )
    .bind::<diesel::sql_types::Uuid, _>(user)
    .bind::<Text, _>(format!("{name}-{user}@example.com"))
    .get_result::<Email>(&mut pool.get().expect("conn"))
    .expect("email")
    .email
}

/// Whoever did the work is named under the record API's rule: someone who
/// can work tickets here. A member who can't is left blank and reported,
/// as the API would refuse them.
#[actix_web::test]
async fn the_technician_is_someone_who_can_work_tickets() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = workspace(&pool, "UTC");
    seed_asset(&pool, &ws, "Camera 1", Some("CAM-001"), None);
    let admin = primary_email(&pool, ws.admin_uuid, "boss");
    let member = primary_email(&pool, ws.member_uuid, "sam");
    let job = checked(
        &pool,
        &ws,
        &format!(
            "Id,Tag,Type,Date,By\nP-1,CAM-001,Repair,2020-01-02,{admin}\nP-2,CAM-001,Repair,2020-01-03,{member}\n"
        ),
        &[
            ("Id", "source_ref"),
            ("Tag", "asset_tag"),
            ("Type", "record_type"),
            ("Date", "occurred_on"),
            ("By", "performed_by"),
        ],
        serde_json::json!({}),
    )
    .await;
    assert_eq!(summary(&job, "would_create"), 2, "{job}");
    let dropped = job["summary"]["dropped"].as_array().expect("dropped");
    assert_eq!(dropped.len(), 1, "{job}");
    assert_eq!(
        (&dropped[0]["column"], &dropped[0]["value"]),
        (
            &serde_json::json!("performed_by"),
            &serde_json::json!(member)
        ),
        "{job}"
    );
    commit(&pool, &ws, &id_of(&job)).await;
    #[derive(QueryableByName)]
    struct By {
        #[diesel(sql_type = Nullable<Text>)]
        by: Option<String>,
    }
    let by: Vec<Option<String>> = diesel::sql_query(
        "SELECT performed_by_uuid::text AS by FROM asset_records WHERE workspace_id = $1 ORDER BY source_ref",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .load::<By>(&mut pool.get().expect("conn"))
    .expect("records")
    .into_iter()
    .map(|b| b.by)
    .collect();
    assert_eq!(by, vec![Some(ws.admin_uuid.to_string()), None]);
}
