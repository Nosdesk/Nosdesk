//! The Map step's memory and mode: suggestions from the type's last mapped
//! import (fields and Don't import, with who and when), the rows each mode
//! skips, and the legacy `match_by: none`.

use std::collections::BTreeMap;

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, import_workspaces, json, mapping_request,
    template_mapping, upload_request, upload_request_for,
};

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

async fn upload(pool: &TestPool, ws: &WorkspaceSeed, file: &str) -> serde_json::Value {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request_for("assets", "data.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    job
}

async fn map(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    job: &serde_json::Value,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let id = job["id"].as_str().expect("job id");
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, mapping_request(id, body)).await;
    (status, json(&body))
}

/// Each suggestion as (field, reason), by column.
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

fn user_name(pool: &TestPool, uuid: uuid::Uuid) -> String {
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    diesel::sql_query("SELECT name FROM users WHERE uuid = $1")
        .bind::<diesel::sql_types::Uuid, _>(uuid)
        .get_result::<Name>(&mut pool.get().expect("conn"))
        .expect("read user name")
        .name
}

const FILE: &str = "Device,Type,Comments,Owner,Extra\nCam 1,generic,dusty,Ana,x\n";

/// The next upload of the type takes each header's choice from the latest
/// mapped import, a field or Don't import, with who made it and when. A
/// column left without a choice is guessed afresh, and a saved Don't import
/// beats the synonym that would guess Comments as notes.
#[actix_web::test]
async fn the_latest_mapped_import_fills_the_next_one() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let first = upload(&pool, &ws, FILE).await;
    assert_eq!(
        suggestions(&first).get(&2),
        Some(&pick(Some("notes"), "synonym"))
    );
    // Comments and Extra stay out on purpose; Owner gets no choice.
    let (status, first) = map(
        &pool,
        &ws,
        &first,
        serde_json::json!({ "mapping": { "name": 0, "kind": 1 }, "ignored": [4, 2] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["ignored"], serde_json::json!([2, 4]), "{first}");
    assert!(first["source"].get("ignored").is_none(), "{first}");

    let second = upload(&pool, &ws, FILE).await;
    assert_eq!(
        suggestions(&second),
        BTreeMap::from([
            (0, pick(Some("name"), "previous")),
            (1, pick(Some("kind"), "previous")),
            (2, pick(None, "previous")),
            (4, pick(None, "previous")),
        ])
    );
    let provenance = &second["suggested_mapping"][0]["provenance"];
    assert_eq!(provenance["job_id"], first["id"], "{provenance}");
    assert_eq!(provenance["by"], ws.admin_uuid.to_string(), "{provenance}");
    assert_eq!(
        provenance["by_name"],
        user_name(&pool, ws.admin_uuid),
        "{provenance}"
    );
    assert_eq!(provenance["at"], first["updated_at"], "{provenance}");

    // The second import, mapped later, changes its mind about Comments.
    let (status, second) = map(
        &pool,
        &ws,
        &second,
        serde_json::json!({ "mapping": { "name": 0, "kind": 1, "notes": 2 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let third = upload(&pool, &ws, FILE).await;
    assert_eq!(
        suggestions(&third).get(&2),
        Some(&pick(Some("notes"), "previous"))
    );
    // It set nothing to Don't import, so Extra is no longer remembered.
    assert_eq!(suggestions(&third).get(&4), None);
    assert_eq!(
        third["suggested_mapping"][0]["provenance"]["job_id"],
        second["id"]
    );

    // Checking the first import again makes it the latest.
    let (status, first) = map(
        &pool,
        &ws,
        &first,
        serde_json::json!({ "mapping": { "name": 0, "kind": 1 }, "ignored": [2] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let fourth = upload(&pool, &ws, FILE).await;
    assert_eq!(suggestions(&fourth).get(&2), Some(&pick(None, "previous")));
}

/// Memory is the workspace's own: another workspace's later import of the
/// same type suggests nothing here.
#[actix_web::test]
async fn header_memory_stays_in_its_workspace() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let (a, b) = (&seeded.a, &seeded.b);

    let in_a = upload(&pool, a, FILE).await;
    let (status, body) = map(
        &pool,
        a,
        &in_a,
        serde_json::json!({ "mapping": { "name": 0, "kind": 1 }, "ignored": [2] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let in_b = upload(&pool, b, FILE).await;
    let (status, body) = map(
        &pool,
        b,
        &in_b,
        serde_json::json!({ "mapping": { "name": 3, "kind": 1, "notes": 2 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let next_a = upload(&pool, a, FILE).await;
    assert_eq!(
        suggestions(&next_a),
        BTreeMap::from([
            (0, pick(Some("name"), "previous")),
            (1, pick(Some("kind"), "previous")),
            (2, pick(None, "previous")),
        ])
    );
    assert_eq!(
        next_a["suggested_mapping"][0]["provenance"]["job_id"],
        in_a["id"]
    );
}

/// The Map step refuses a Don't import it can't apply.
#[actix_web::test]
async fn ignored_columns_are_validated() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    let job = upload(&pool, &ws, FILE).await;
    for (ignored, expected) in [
        (serde_json::json!([9]), "file has 5 columns"),
        (serde_json::json!([0]), "also set to Don't import"),
        (serde_json::json!([2, 2]), "twice"),
    ] {
        let (status, body) = map(
            &pool,
            &ws,
            &job,
            serde_json::json!({ "mapping": { "name": 0, "kind": 1 }, "ignored": ignored }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{ignored}: {body}");
        assert!(
            body["error"].as_str().unwrap_or("").contains(expected),
            "{expected}: {body}"
        );
    }
}

fn seed_asset(pool: &TestPool, ws: &WorkspaceSeed, tag: &str) {
    diesel::sql_query(
        "INSERT INTO assets (workspace_id, name, kind, asset_tag, location)
         VALUES ($1, $2, 'generic', $2, 'Store')",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .bind::<Text, _>(tag)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed asset");
}

fn location_of(pool: &TestPool, tag: &str) -> Option<String> {
    #[derive(QueryableByName)]
    struct Location {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        location: Option<String>,
    }
    diesel::sql_query("SELECT location FROM assets WHERE asset_tag = $1")
        .bind::<Text, _>(tag)
        .get_result::<Location>(&mut pool.get().expect("conn"))
        .optional()
        .expect("read location")
        .and_then(|l| l.location)
}

/// Upload a template file and map it with `options`.
async fn check_with(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    file: &str,
    options: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(
        pool,
        ws,
        ws.admin_uuid,
        upload_request("assets.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let mut body = template_mapping();
    body["options"] = options;
    map(pool, ws, &job, body).await
}

async fn get(pool: &TestPool, ws: &WorkspaceSeed, uri: &str) -> (StatusCode, String) {
    let (status, body) =
        call_import_route(pool, ws, ws.admin_uuid, TestRequest::get().uri(uri)).await;
    (status, String::from_utf8(body.to_vec()).expect("utf-8"))
}

/// Skipped rows as (row, key, column, message), from `GET rows`.
async fn skipped(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
) -> Vec<(u64, String, String, String)> {
    let (status, body) = get(
        pool,
        ws,
        &format!("/api/admin/import/{id}/rows?filter=skipped"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let page: serde_json::Value = serde_json::from_str(&body).expect("json");
    page["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| {
            (
                r["row"].as_u64().expect("row"),
                r["key"].as_str().unwrap_or("").to_string(),
                r["errors"][0]["column"].as_str().unwrap_or("").to_string(),
                r["errors"][0]["message"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn commit_request(id: &str) -> TestRequest {
    TestRequest::post().uri(&format!("/api/admin/import/{id}/commit"))
}

/// Only create new skips a row whose tag exists as "already exists", in
/// the row results and the error report, and leaves that asset alone;
/// the other rows create, constants included.
#[actix_web::test]
async fn only_create_new_skips_existing_assets() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_asset(&pool, &ws, "MODE-1");

    let mut file_mapping = template_mapping();
    file_mapping["mapping"]
        .as_object_mut()
        .expect("mapping")
        .remove("location");
    file_mapping["options"] = serde_json::json!({
        "mode": "create_only",
        "constants": [{ "field": "location", "value": "Studio B", "apply_to": "all" }],
    });
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request(
            "assets.csv",
            format!("{HEADER}\nAgain,generic,MODE-1,,,,,,,,\nFresh,generic,MODE-2,,,,,,,,\n")
                .as_bytes(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, job) = map(&pool, &ws, &json(&body), file_mapping).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["options"]["mode"], "create_only", "{job}");
    assert_eq!(
        (
            &job["summary"]["would_create"],
            &job["summary"]["would_update"],
            &job["summary"]["rows_with_errors"]
        ),
        (
            &serde_json::json!(1),
            &serde_json::json!(0),
            &serde_json::json!(1)
        ),
        "{job}"
    );
    let id = job["id"].as_str().expect("job id").to_string();
    assert_eq!(
        skipped(&pool, &ws, &id).await,
        vec![(
            2,
            "MODE-1".to_string(),
            "asset_tag".to_string(),
            "already exists".to_string()
        )]
    );
    let (status, report) = get(&pool, &ws, &format!("/api/admin/import/{id}/error-report")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        report.contains("2,asset_tag: already exists,Again,generic,MODE-1"),
        "{report}"
    );

    let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&id)).await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
    assert_eq!(location_of(&pool, "MODE-1").as_deref(), Some("Store"));
    assert_eq!(location_of(&pool, "MODE-2").as_deref(), Some("Studio B"));
}

/// Only update existing skips a row whose tag matches nothing as "not
/// found", and one with no tag; matched rows update.
#[actix_web::test]
async fn only_update_existing_skips_unmatched_rows() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_asset(&pool, &ws, "UPD-1");

    let (status, job) = check_with(
        &pool,
        &ws,
        &format!(
            "{HEADER}\nUPD-1,generic,UPD-1,,,,Studio C,,,,\nNew,generic,UPD-9,,,,Studio C,,,,\nNo tag,generic,,,,,Studio C,,,,\n"
        ),
        serde_json::json!({ "mode": "update_only" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(
        (
            &job["summary"]["would_create"],
            &job["summary"]["would_update"]
        ),
        (&serde_json::json!(0), &serde_json::json!(1)),
        "{job}"
    );
    let id = job["id"].as_str().expect("job id").to_string();
    assert_eq!(
        skipped(&pool, &ws, &id).await,
        vec![
            (
                3,
                "UPD-9".to_string(),
                "asset_tag".to_string(),
                "not found: no asset has this tag".to_string()
            ),
            (
                4,
                String::new(),
                "asset_tag".to_string(),
                "not found: no asset tag to match an asset by".to_string()
            ),
        ]
    );
    let (status, report) = get(&pool, &ws, &format!("/api/admin/import/{id}/error-report")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        report.contains("3,asset_tag: not found: no asset has this tag,New,generic,UPD-9"),
        "{report}"
    );

    let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, commit_request(&id)).await;
    assert_eq!(status, StatusCode::OK, "{}", json(&body));
    assert_eq!(location_of(&pool, "UPD-1").as_deref(), Some("Studio C"));
    assert_eq!(location_of(&pool, "UPD-9"), None);
}

/// The legacy `match_by: none` is stored as what it meant, create only by
/// tag, and refused beside a mode that updates. A mode other than create
/// and update is for assets.
#[actix_web::test]
async fn match_by_none_reads_as_only_create_new() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);
    seed_asset(&pool, &ws, "LEG-1");
    let file = format!("{HEADER}\nAgain,generic,LEG-1,,,,,,,,\nFresh,generic,LEG-2,,,,,,,,\n");

    let (status, job) =
        check_with(&pool, &ws, &file, serde_json::json!({ "match_by": "none" })).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["options"]["match_by"], "asset_tag", "{job}");
    assert_eq!(job["options"]["mode"], "create_only", "{job}");
    let id = job["id"].as_str().expect("job id").to_string();
    assert_eq!(
        skipped(&pool, &ws, &id).await,
        vec![(
            2,
            "LEG-1".to_string(),
            "asset_tag".to_string(),
            "already exists".to_string()
        )]
    );

    let (status, body) = check_with(
        &pool,
        &ws,
        &file,
        serde_json::json!({ "match_by": "none", "mode": "update_only" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("match_by 'none'"),
        "{body}"
    );

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request_for(
            "users",
            "people.csv",
            b"email,name,role\na@example.com,A,user\n",
        ),
    )
    .await;
    let users = json(&body);
    assert_eq!(status, StatusCode::OK, "{users}");
    let (status, body) = map(
        &pool,
        &ws,
        &users,
        serde_json::json!({
            "mapping": { "email": 0, "name": 1, "role": 2 },
            "options": { "mode": "create_only" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("asset imports only"),
        "{body}"
    );
}
