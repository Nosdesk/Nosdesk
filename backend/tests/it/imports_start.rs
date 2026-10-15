//! The Start and Map steps' job changes: switching a draft's type and
//! discarding a draft with its files.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;

use backend::services::imports::mapping;

use crate::common::TestDb;
use crate::imports_assets::{
    call_import_route, import_workspace, import_workspaces, json, mapping_request,
    upload_and_check, upload_request,
};

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

fn type_request(id: &str, job_type: &str) -> TestRequest {
    TestRequest::put()
        .uri(&format!("/api/admin/import/{id}/type"))
        .set_json(serde_json::json!({ "type": job_type }))
}

fn delete_request(id: &str) -> TestRequest {
    TestRequest::delete().uri(&format!("/api/admin/import/{id}"))
}

fn get_request(id: &str) -> TestRequest {
    TestRequest::get().uri(&format!("/api/admin/import/{id}"))
}

/// The field each suggestion names, by column.
fn suggested(job: &serde_json::Value) -> Vec<(u64, String)> {
    job["suggested_mapping"]
        .as_array()
        .expect("suggested_mapping")
        .iter()
        .map(|s| {
            (
                s["index"].as_u64().expect("index"),
                s["field"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

/// Every file kept for a job: the upload and its grid.
fn job_files(id: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(mapping::imports_dir())
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with(&format!("{id}.")))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A checked draft switched to another type comes back to `parsed` with
/// the same columns, suggestions made for the new type, and no mapping,
/// options or check; asking for the type it has changes nothing.
#[actix_web::test]
async fn a_draft_switches_type_and_loses_its_mapping() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let file = "Name,Email,Role\nAlex Smith,alex@example.com,user\n";
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        upload_request("new starters.csv", file.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["job_type"], "assets");
    let id = job["id"].as_str().expect("job id").to_string();
    assert!(!suggested(&job).iter().any(|(_, f)| f == "email"), "{job}");

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(
            &id,
            serde_json::json!({ "mapping": { "name": 0 }, "options": { "default_kind": "generic" } }),
        ),
    )
    .await;
    let checked = json(&body);
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["status"], "dry_run_done");

    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, type_request(&id, "users")).await;
    let switched = json(&body);
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["job_type"], "users");
    assert_eq!(switched["status"], "parsed");
    for column in ["mapping", "options", "summary", "answers"] {
        assert!(switched[column].is_null(), "{column}: {switched}");
    }
    assert_eq!(switched["source"]["headers"], checked["source"]["headers"]);
    assert_eq!(
        suggested(&switched),
        vec![
            (0, "name".to_string()),
            (1, "email".to_string()),
            (2, "role".to_string())
        ],
        "{switched}"
    );
    // The check went with the mapping.
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows")),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, type_request(&id, "users")).await;
    let same = json(&body);
    assert_eq!(status, StatusCode::OK, "{same}");
    assert_eq!(same["updated_at"], switched["updated_at"]);

    let (status, _) =
        call_import_route(&pool, &ws, ws.admin_uuid, type_request(&id, "printers")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The new type maps and checks like an upload of it.
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        mapping_request(
            &id,
            serde_json::json!({ "mapping": { "name": 0, "email": 1, "role": 2 } }),
        ),
    )
    .await;
    let checked = json(&body);
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["summary"]["would_create"], 1, "{checked}");
}

/// Once committed, a job's type is fixed.
#[actix_web::test]
async fn the_type_cannot_change_after_commit() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nTyped,generic,TYPE-001,,,,,,,,\n"),
    )
    .await;
    let id = job["id"].as_str().expect("job id").to_string();
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) =
        call_import_route(&pool, &ws, ws.admin_uuid, type_request(&id, "users")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{}", json(&body));
    let (_, body) = call_import_route(&pool, &ws, ws.admin_uuid, get_request(&id)).await;
    let kept = json(&body);
    assert_eq!(kept["job_type"], "assets");
    assert_eq!(kept["status"], "done");
}

/// Deleting a draft removes the job and every file kept for it; a second
/// delete finds nothing.
#[actix_web::test]
async fn a_draft_is_deleted_with_its_files() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nDiscarded,generic,DEL-001,,,,,,,,\n"),
    )
    .await;
    let id = job["id"].as_str().expect("job id").to_string();
    assert_eq!(
        job_files(&id),
        vec![format!("{id}.csv"), format!("{id}.grid.json")]
    );

    let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, delete_request(&id)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{}", json(&body));
    assert!(job_files(&id).is_empty(), "{:?}", job_files(&id));
    let (status, _) = call_import_route(&pool, &ws, ws.admin_uuid, get_request(&id)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call_import_route(&pool, &ws, ws.admin_uuid, delete_request(&id)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A committed job is the record of what it wrote: deleting it is refused
/// and its files stay.
#[actix_web::test]
async fn a_committed_import_cannot_be_deleted() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nKept,generic,KEEP-DEL-001,,,,,,,,\n"),
    )
    .await;
    let id = job["id"].as_str().expect("job id").to_string();
    let (status, _) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, delete_request(&id)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{}", json(&body));
    let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, get_request(&id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["status"], "done");
    assert_eq!(job_files(&id).len(), 2, "{:?}", job_files(&id));
}

/// Another workspace's job is not found, for a type change and a delete
/// alike, and is left as it was.
#[actix_web::test]
async fn start_routes_stay_in_their_workspace() {
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
            format!("{HEADER}\nMine,generic,WS-1,,,,,,,,\n").as_bytes(),
        ),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("job id").to_string();

    let (status, _) = call_import_route(&pool, b, b.admin_uuid, type_request(&id, "users")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call_import_route(&pool, b, b.admin_uuid, delete_request(&id)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = call_import_route(&pool, a, a.admin_uuid, get_request(&id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["job_type"], "assets");
    assert_eq!(job_files(&id).len(), 2, "{:?}", job_files(&id));
}
