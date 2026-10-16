//! Match values: the distinct values of each mapped column with how they
//! matched (`GET values`), the admin's answers (`PUT values`), and the dry
//! run and commit reading those answers.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};

use backend::models::NewImportValueAnswer;
use backend::repository::imports as repo;
use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, import_workspaces, json, mapping_request,
    template_mapping, upload_request, upload_request_for,
};

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";
const REF: &str = "test:imports_values";

fn seed_kinds(pool: &TestPool, ws: &WorkspaceSeed) {
    for (slug, label) in [("camera", "Camera"), ("camera_support", "Camera support")] {
        diesel::sql_query(
            "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
             VALUES ($1, $2, '{\"type\":\"object\",\"properties\":{}}', 20, false, 'physical', $3)
             ON CONFLICT (workspace_id, slug) DO NOTHING",
        )
        .bind::<Text, _>(slug)
        .bind::<Text, _>(label)
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut pool.get().expect("conn"))
        .expect("seed kind");
    }
}

/// One asset row of the template layout.
fn row(name: &str, kind: &str, tag: &str, quantity: &str) -> String {
    format!("{name},{kind},{tag},,,,,,{quantity},,")
}

fn file(rows: &[String]) -> String {
    format!("{HEADER}\n{}\n", rows.join("\n"))
}

/// Upload `file` as assets and map it as a template: the job, checked.
async fn checked(pool: &TestPool, ws: &WorkspaceSeed, file: &str) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request("assets.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
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
    job
}

fn id_of(job: &serde_json::Value) -> String {
    job["id"].as_str().expect("job id").to_string()
}

async fn get_values(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
    )
    .await;
    (status, json(&body))
}

fn values_request(id: &str, answers: serde_json::Value) -> TestRequest {
    TestRequest::put()
        .uri(&format!("/api/admin/import/{id}/values"))
        .set_json(serde_json::json!({ "answers": answers }))
}

async fn put_values(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    answers: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, values_request(id, answers)).await;
    (status, json(&body))
}

async fn commit(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    (status, json(&body))
}

fn column<'a>(view: &'a serde_json::Value, field: &str) -> &'a serde_json::Value {
    view["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["field"] == field)
        .unwrap_or_else(|| panic!("no {field} column in {view}"))
}

/// Each value as (text, rows, match), in the order the view lists them.
fn listed(column: &serde_json::Value) -> Vec<(String, usize, String)> {
    column["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(|v| {
            (
                v["text"].as_str().expect("text").to_string(),
                v["rows"].as_array().expect("rows").len(),
                v["match"].as_str().expect("match").to_string(),
            )
        })
        .collect()
}

fn value<'a>(column: &'a serde_json::Value, text: &str) -> &'a serde_json::Value {
    column["values"]
        .as_array()
        .expect("values")
        .iter()
        .find(|v| v["text"] == text)
        .unwrap_or_else(|| panic!("no value {text} in {column}"))
}

#[derive(QueryableByName, Debug, PartialEq)]
struct AssetRow {
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
    quantity: Option<String>,
}

fn asset(pool: &TestPool, tag: &str) -> Option<AssetRow> {
    diesel::sql_query("SELECT kind, quantity::text AS quantity FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<AssetRow>(&mut pool.get().expect("conn"))
        .optional()
        .expect("read asset")
}

fn saved(field: &str, value: &str, target: &str, by: uuid::Uuid) -> NewImportValueAnswer {
    NewImportValueAnswer {
        job_type: "assets".to_string(),
        field: field.to_string(),
        context: String::new(),
        normalized_value: value.to_string(),
        answer: serde_json::json!({ "action": "match", "target": target }),
        answered_by: Some(by),
        job_id: None,
        source_text: None,
        created: false,
    }
}

/// `GET values` lists each mapped kind and number column's distinct values
/// with their row counts and how they matched: the kind's key or label
/// exactly, a saved answer, a known other name or a close spelling
/// (Guessed), or nothing. Values to read come first, most rows first; a
/// number that reads is only counted. The first check already applies what
/// matching suggests.
#[actix_web::test]
async fn values_are_listed_with_counts_and_how_they_matched() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);
    run_in_workspace(&db.runtime_pool(2), REF, ws.workspace_id, |c| {
        repo::upsert_value_answers(
            c,
            &[saved("kind", "b/cast", "camera_support", ws.admin_uuid)],
        )
    })
    .expect("save an answer");

    let job = checked(
        &pool,
        &ws,
        &file(&[
            row("Cam 1", "camera", "V-1", ""),
            row("Cam 2", "Camera", "V-2", ""),
            row("Cam 3", "Cam", "V-3", ""),
            row("Cam 4", "Camra", "V-4", ""),
            row("Tripod 1", "tripod", "V-5", "two"),
            row("Tripod 2", "TRIPOD", "V-6", "two"),
            row("Tripod 3", "tripod ", "V-7", "3"),
            row("Kit 1", "B/cast", "V-8", ""),
        ]),
    )
    .await;
    // The three tripods have no kind; the two "two" rows are among them.
    assert_eq!(job["summary"]["rows_with_errors"], 3, "{job}");
    assert_eq!(job["summary"]["would_create"], 5, "{job}");

    let (status, view) = get_values(&pool, &ws, &id_of(&job)).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["rows_checked"], 8, "{view}");
    assert_eq!(view["rows_skipped"], 3, "{view}");
    assert_eq!(view["skipped_other"], serde_json::json!([]), "{view}");

    let kind = column(&view, "kind");
    assert_eq!(kind["header"], "kind");
    assert_eq!(kind["distinct"], 5, "{kind}");
    assert_eq!(kind["matched"], 1, "{kind}");
    assert_eq!(
        listed(kind),
        vec![
            ("tripod".to_string(), 3, "no_match".to_string()),
            ("Cam".to_string(), 1, "guessed".to_string()),
            ("Camra".to_string(), 1, "guessed".to_string()),
            ("B/cast".to_string(), 1, "saved".to_string()),
            ("camera".to_string(), 2, "exact".to_string()),
        ]
    );
    let tripod = value(kind, "tripod");
    assert_eq!(tripod["key"], "tripod");
    assert_eq!(tripod["skipped_rows"], serde_json::json!([6, 7, 8]));
    assert_eq!(tripod["suggested"], serde_json::json!({ "action": "skip" }));
    assert_eq!(
        value(kind, "Cam")["reason"],
        serde_json::json!({ "code": "synonym" })
    );
    assert_eq!(
        value(kind, "Camra")["reason"],
        serde_json::json!({ "code": "close_to", "label": "Camera" })
    );
    let bcast = value(kind, "B/cast");
    assert_eq!(bcast["saved"]["by"], ws.admin_uuid.to_string(), "{bcast}");
    assert_eq!(
        bcast["suggested"],
        serde_json::json!({ "action": "match", "target": "camera_support" })
    );
    let choices: Vec<&str> = kind["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .map(|c| c["label"].as_str().expect("label"))
        .collect();
    assert!(choices.contains(&"Camera support"), "{choices:?}");

    let quantity = column(&view, "quantity");
    assert_eq!(quantity["value_type"], "number");
    assert_eq!(
        listed(quantity),
        vec![("two".to_string(), 2, "no_match".to_string())]
    );
    assert_eq!(
        value(quantity, "two")["reason"],
        serde_json::json!({ "code": "unreadable" })
    );
    assert_eq!(quantity["matched"], 1);
    assert!(
        view["columns"].as_array().expect("columns").len() == 2,
        "only kind and quantity have values to match: {view}"
    );
}

/// Answers are stored, the dry run runs again with them, and the commit
/// writes what it showed: a kind linked to another kind, a number replaced,
/// an unreadable optional number left blank, and rows the admin skipped
/// held back with the reason. Changing the mapping keeps the answers.
#[actix_web::test]
async fn answers_are_read_by_the_dry_run_and_the_commit() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);

    let job = checked(
        &pool,
        &ws,
        &file(&[
            row("Tripod 1", "tripod", "A-1", "two"),
            row("Tripod 2", "Tripod", "A-2", "lots"),
            row("Dolly", "dolly", "A-3", "1"),
            row("Cam 1", "Cam", "A-4", "1"),
        ]),
    )
    .await;
    let id = id_of(&job);
    assert_eq!(job["summary"]["rows_with_errors"], 3, "{job}");

    let (status, job) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({
            "kind": { "": {
                "Tripod": { "action": "match", "target": "camera_support" },
                "dolly": { "action": "skip" },
            }},
            "quantity": { "": {
                "two": { "action": "replace", "value": "2" },
                "lots": { "action": "leave_blank" },
            }},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "dry_run_done");
    assert_eq!(job["summary"]["would_create"], 3, "{job}");
    assert_eq!(job["summary"]["rows_with_errors"], 1, "{job}");
    // Stored by value as matching keys it.
    assert_eq!(
        job["answers"]["kind"][""]["tripod"],
        serde_json::json!({ "action": "match", "target": "camera_support" })
    );
    let group = &job["summary"]["error_groups"][0];
    assert_eq!(group["column"], "kind", "{group}");
    assert_eq!(
        group["message"], "'dolly' is set to Skip these rows",
        "{group}"
    );

    // The view shows the answers.
    let (_, view) = get_values(&pool, &ws, &id).await;
    let kind = column(&view, "kind");
    assert_eq!(value(kind, "tripod")["answer"]["target"], "camera_support");
    assert_eq!(view["rows_skipped"], 1);

    // Mapping again keeps the answers.
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, template_mapping()),
    )
    .await;
    let again = json(&body);
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["summary"]["rows_with_errors"], 1, "{again}");

    let (status, done) = commit(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["records_committed"], 3, "{done}");
    assert_eq!(
        asset(&pool, "A-1"),
        Some(AssetRow {
            kind: "camera_support".into(),
            quantity: Some("2.000".into()),
        })
    );
    assert_eq!(
        asset(&pool, "A-2"),
        Some(AssetRow {
            kind: "camera_support".into(),
            quantity: None,
        })
    );
    assert_eq!(asset(&pool, "A-3"), None);
    // Guessed from a known other name, without an answer.
    assert_eq!(
        asset(&pool, "A-4").map(|a| a.kind),
        Some("camera".to_string())
    );
}

/// The defaults decided for 1.1.2: a number that doesn't read skips its rows
/// until answered, and a blank kind keeps an existing asset's kind while a
/// new row takes the kind its answer names.
#[actix_web::test]
async fn unreadable_values_skip_and_a_blank_kind_answers_for_new_rows() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);
    diesel::sql_query(
        "INSERT INTO assets (workspace_id, name, kind, asset_tag) VALUES ($1, 'Old', 'generic', 'B-1')",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed asset");

    let job = checked(
        &pool,
        &ws,
        &file(&[
            row("Old", "", "B-1", ""),
            row("New", "", "B-2", ""),
            row("Counted", "camera", "B-3", "two"),
        ]),
    )
    .await;
    let id = id_of(&job);
    // The new blank-kind row has no kind; the "two" row doesn't read.
    assert_eq!(job["summary"]["rows_with_errors"], 2, "{job}");
    assert_eq!(job["summary"]["unchanged"], 1, "{job}");

    let (_, view) = get_values(&pool, &ws, &id).await;
    let blank = value(column(&view, "kind"), "");
    assert_eq!(blank["blank"], true, "{blank}");
    assert_eq!(blank["match"], "no_match", "{blank}");
    assert_eq!(blank["skipped_rows"], serde_json::json!([3]), "{blank}");
    assert_eq!(
        blank["suggested"],
        serde_json::json!({ "action": "leave_blank" })
    );
    let two = value(column(&view, "quantity"), "two");
    assert_eq!(two["suggested"], serde_json::json!({ "action": "skip" }));

    let (status, job) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({ "kind": { "": { "": { "action": "match", "target": "camera" } } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["summary"]["rows_with_errors"], 1, "{job}");
    let (status, done) = commit(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(
        asset(&pool, "B-1").map(|a| a.kind),
        Some("generic".to_string())
    );
    assert_eq!(
        asset(&pool, "B-2").map(|a| a.kind),
        Some("camera".to_string())
    );
    assert_eq!(asset(&pool, "B-3"), None);
}

/// An optional choice with no match is left blank: a ticket whose priority
/// reads as nothing takes the default priority. A workflow state spelled
/// close to one is guessed, and a date that doesn't read is replaced.
#[actix_web::test]
async fn an_optional_choice_with_no_match_is_left_blank() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let tickets =
        "title,workflow_state,priority,requester_email,assignee_email,category,due_date\n\
                   Printer jam,backlog,asap,,,,\n\
                   Desk move,Backlg,normal,,,,2026-13-40\n";
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request_for("tickets", "tickets.csv", tickets.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = id_of(&job);
    let mapping: serde_json::Map<String, serde_json::Value> = tickets
        .lines()
        .next()
        .expect("header")
        .split(',')
        .enumerate()
        .map(|(i, h)| (h.to_string(), i.into()))
        .collect();
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(&id, serde_json::json!({ "mapping": mapping })),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    // Only the date that doesn't read skips its row.
    assert_eq!(job["summary"]["rows_with_errors"], 1, "{job}");
    assert_eq!(job["summary"]["errors"][0]["column"], "due_date", "{job}");

    let (_, view) = get_values(&pool, &ws, &id).await;
    let priority = column(&view, "priority");
    assert_eq!(value(priority, "asap")["match"], "no_match");
    assert_eq!(
        value(priority, "asap")["suggested"],
        serde_json::json!({ "action": "leave_blank" })
    );
    assert_eq!(value(priority, "normal")["match"], "guessed");
    let state = column(&view, "workflow_state");
    assert_eq!(value(state, "backlog")["match"], "exact");
    assert_eq!(value(state, "Backlg")["match"], "guessed");
    let due = column(&view, "due_date");
    assert_eq!(
        listed(due),
        vec![("2026-13-40".to_string(), 1, "no_match".to_string())]
    );

    let (status, job) = put_values(
        &pool,
        &ws,
        &id,
        serde_json::json!({ "due_date": { "": { "2026-13-40": { "action": "replace", "value": "2026-12-04" } } } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["summary"]["would_create"], 2, "{job}");
    let (status, done) = commit(&pool, &ws, &id).await;
    assert_eq!(status, StatusCode::OK, "{done}");

    #[derive(QueryableByName)]
    struct Ticket {
        #[diesel(sql_type = Text)]
        title: String,
        #[diesel(sql_type = Text)]
        priority: String,
    }
    let mut got: Vec<(String, String)> = diesel::sql_query(
        "SELECT title, priority::text AS priority FROM tickets WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .load::<Ticket>(&mut pool.get().expect("conn"))
    .expect("read tickets")
    .into_iter()
    .map(|t| (t.title, t.priority))
    .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            ("Desk move".to_string(), "medium".to_string()),
            ("Printer jam".to_string(), "medium".to_string()),
        ]
    );
}

/// Answers that don't fit their column are refused, naming the value.
#[actix_web::test]
async fn answers_that_dont_fit_are_refused() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_kinds(&pool, &ws);
    let job = checked(&pool, &ws, &file(&[row("T", "tripod", "C-1", "two")])).await;
    let id = id_of(&job);

    for (answers, expected) in [
        (
            serde_json::json!({ "name": { "": { "t": { "action": "skip" } } } }),
            "isn't a mapped column",
        ),
        (
            serde_json::json!({ "kind": { "": { "tripod": { "action": "match", "target": "boat" } } } }),
            "isn't one of the choices",
        ),
        (
            serde_json::json!({ "kind": { "": { "tripod": { "action": "leave_blank" } } } }),
            "can't be left blank",
        ),
        (
            serde_json::json!({ "kind": { "": { "tripod": { "action": "create" } } } }),
            "can't create",
        ),
        (
            serde_json::json!({ "quantity": { "": { "two": { "action": "replace", "value": "three" } } } }),
            "'three' isn't a number",
        ),
        (
            serde_json::json!({ "kind": { "Camera": { "tripod": { "action": "skip" } } } }),
            "take no context",
        ),
    ] {
        let (status, body) = put_values(&pool, &ws, &id, answers.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answers}: {body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains(expected),
            "{expected}: {body}"
        );
    }
}

/// Above 200 values to answer the view still lists every one, so the step
/// can offer counts, bulk answers and the download.
#[actix_web::test]
async fn every_value_to_answer_is_listed_past_the_scale_cap() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let rows: Vec<String> = (0..250)
        .map(|i| {
            row(
                &format!("Thing {i}"),
                &format!("odd-{i}"),
                &format!("S-{i}"),
                "",
            )
        })
        .collect();
    let job = checked(&pool, &ws, &file(&rows)).await;
    let (status, view) = get_values(&pool, &ws, &id_of(&job)).await;
    assert_eq!(status, StatusCode::OK);
    let kind = column(&view, "kind");
    assert_eq!(kind["distinct"], 250);
    assert_eq!(kind["values"].as_array().expect("values").len(), 250);
    assert_eq!(view["rows_skipped"], 250);
}

/// A kind added after the check turns a value with no match into an exact
/// one, so the commit's plan differs from the one reviewed and nothing is
/// written.
#[actix_web::test]
async fn the_commit_matches_values_against_the_workspace_as_it_is() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = checked(&pool, &ws, &file(&[row("T", "tripod", "D-1", "")])).await;
    assert_eq!(job["summary"]["rows_with_errors"], 1, "{job}");
    diesel::sql_query(
        "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
         VALUES ('tripod', 'Tripod', '{\"type\":\"object\",\"properties\":{}}', 30, false, 'physical', $1)",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("add kind");
    let (status, body) = commit(&pool, &ws, &id_of(&job)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_plan_changed", "{body}");
    assert_eq!(body["job"]["summary"]["would_create"], 1, "{body}");
    assert_eq!(asset(&pool, "D-1"), None);
}

/// The values routes answer a non-admin with 403, another workspace's job
/// with 404, and a job not yet checked with 409.
#[actix_web::test]
async fn values_routes_stay_with_admins_in_their_workspace() {
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
            file(&[row("One", "generic", "W-1", "")]).as_bytes(),
        ),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = id_of(&job);
    let uri = format!("/api/admin/import/{id}/values");
    let empty = serde_json::json!({});

    assert_eq!(get_values(&pool, a, &id).await.0, StatusCode::CONFLICT);
    assert_eq!(
        put_values(&pool, a, &id, empty.clone()).await.0,
        StatusCode::CONFLICT
    );

    let (status, _) = call_import_route(
        &pool,
        a,
        a.admin_uuid,
        mapping_request(&id, template_mapping()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(get_values(&pool, a, &id).await.0, StatusCode::OK);
    assert_eq!(get_values(&pool, b, &id).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        put_values(&pool, b, &id, empty.clone()).await.0,
        StatusCode::NOT_FOUND
    );

    for req in [
        TestRequest::get().uri(&uri),
        values_request(&id, empty.clone()),
    ] {
        let (status, _) = call_import_route(&pool, a, a.member_uuid, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (status, job) = put_values(&pool, a, &id, empty).await;
    assert_eq!(status, StatusCode::OK, "{job}");
}
