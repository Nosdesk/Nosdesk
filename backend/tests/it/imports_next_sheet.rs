//! Import the next sheet (`POST /admin/import/{id}/next-sheet`): Done
//! offers the next visible sheet with rows that nothing from the upload has
//! imported; the new draft reads that sheet from its own copy of the upload
//! with nothing else carried, and the refusals.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use backend::services::imports::files;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{Storage, WorkspaceScopedStorage};
use diesel::prelude::*;
use rust_xlsxwriter::Workbook;
use uuid::Uuid;

use crate::common::{test_storage, TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{
    call_import_route, import_workspace, import_workspaces, job_files, json, mapping_request,
    upload_request, upload_request_for,
};

const REF: &str = "test:imports_next_sheet";

/// Cameras, Vehicles and Misc share the columns Tag, Name and Type;
/// Lookups is hidden and Empty has a header and no rows.
fn workbook() -> Vec<u8> {
    let mut wb = Workbook::new();
    let sheet = |wb: &mut Workbook, name: &str, rows: &[[&str; 3]]| {
        let ws = wb.add_worksheet();
        ws.set_name(name).expect("name");
        for (c, h) in ["Tag", "Name", "Type"].iter().enumerate() {
            ws.write_string(0, c as u16, *h).expect("write");
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                ws.write_string(r as u32 + 1, c as u16, *v).expect("write");
            }
        }
    };
    sheet(
        &mut wb,
        "Cameras",
        &[
            ["NS-C1", "Camera 1", "generic"],
            ["NS-C2", "Camera 2", "generic"],
        ],
    );
    sheet(&mut wb, "Vehicles", &[["NS-V1", "Van 1", "generic"]]);
    {
        let ws = wb.add_worksheet();
        ws.set_name("Lookups").expect("name");
        ws.set_hidden(true);
        ws.write_string(0, 0, "Type").expect("write");
        ws.write_string(1, 0, "generic").expect("write");
    }
    sheet(&mut wb, "Empty", &[]);
    sheet(&mut wb, "Misc", &[["NS-M1", "Tripod", "generic"]]);
    wb.save_to_buffer().expect("save workbook")
}

fn mapping() -> serde_json::Value {
    serde_json::json!({
        "mapping": { "asset_tag": 0, "name": 1, "kind": 2 },
        "options": {
            "constants": [{ "field": "location", "value": "Studio B", "apply_to": "new" }]
        }
    })
}

async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(pool, ws, ws.admin_uuid, req).await;
    (status, json(&body))
}

fn next_sheet(id: &str, sheet: &str) -> TestRequest {
    TestRequest::post()
        .uri(&format!("/api/admin/import/{id}/next-sheet"))
        .set_json(serde_json::json!({ "sheet": sheet }))
}

fn id_of(job: &serde_json::Value) -> String {
    job["id"].as_str().expect("job id").to_string()
}

/// Map `id` with [`mapping`] and commit it: the commit's answer.
async fn map_and_commit(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, job) = call(pool, ws, mapping_request(id, mapping())).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["summary"]["rows_with_errors"], 0, "{job}");
    let (status, job) = call(
        pool,
        ws,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "done", "{job}");
    job
}

/// The workbook uploaded, on Cameras, and committed.
async fn committed_cameras(pool: &TestPool, ws: &WorkspaceSeed) -> serde_json::Value {
    let (status, job) = call(pool, ws, upload_request("gear.xlsx", &workbook())).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["source"]["sheet"], "Cameras", "{job}");
    map_and_commit(pool, ws, &id_of(&job)).await
}

fn upload_of(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(id).expect("uuid");
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        use backend::schema::import_jobs::dsl as j;
        j::import_jobs
            .find(id)
            .select(j::upload_of)
            .first::<Option<Uuid>>(c)
    })
    .expect("read upload_of")
}

async fn get_job(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, job) = call(
        pool,
        ws,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    job
}

/// Done offers Vehicles; pressing it makes a draft on Vehicles with its own
/// copy of the upload and nothing carried: no mapping, options, constants
/// or answers, with the columns suggested from the Cameras import's memory.
#[actix_web::test]
async fn the_next_sheet_starts_a_draft_with_only_the_file() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let cameras = committed_cameras(&pool, &ws).await;
    let cameras_id = id_of(&cameras);
    assert_eq!(cameras["next_sheet"], "Vehicles", "{cameras}");
    assert_eq!(
        get_job(&pool, &ws, &cameras_id).await["next_sheet"],
        "Vehicles"
    );
    let source_files = job_files(ws.workspace_id, &cameras_id).await;

    let (status, vehicles) = call(&pool, &ws, next_sheet(&cameras_id, "Vehicles")).await;
    assert_eq!(status, StatusCode::CREATED, "{vehicles}");
    let vehicles_id = id_of(&vehicles);
    assert_ne!(vehicles_id, cameras_id);
    assert_eq!(vehicles["status"], "parsed");
    assert_eq!(vehicles["state"], "draft");
    assert_eq!(vehicles["job_type"], "assets");
    assert_eq!(vehicles["filename"], "gear.xlsx");
    assert_eq!(vehicles["source"]["sheet"], "Vehicles");
    assert_eq!(vehicles["source"]["header_row"], 1);
    assert_eq!(vehicles["source"]["row_count"], 1);
    assert!(vehicles["mapping"].is_null(), "{vehicles}");
    assert!(vehicles["options"].is_null(), "{vehicles}");
    assert!(vehicles["answers"].is_null(), "{vehicles}");
    assert!(vehicles["summary"].is_null(), "{vehicles}");
    // Map suggests from the Cameras import's memory, as any new import.
    let suggested = vehicles["suggested_mapping"].as_array().expect("suggested");
    assert_eq!(suggested.len(), 3, "{vehicles}");
    for s in suggested {
        assert_eq!(s["reason"], "previous", "{s}");
        assert_eq!(s["provenance"]["job_id"], cameras_id.as_str(), "{s}");
    }
    assert_eq!(
        upload_of(&pool, &ws, &vehicles_id),
        Some(Uuid::parse_str(&cameras_id).unwrap())
    );

    // Its own upload and grid; the source keeps its files as they were.
    let names = job_files(ws.workspace_id, &vehicles_id).await;
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.iter().any(|n| n == "upload.xlsx"), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("grid.")), "{names:?}");
    assert_eq!(job_files(ws.workspace_id, &cameras_id).await, source_files);

    // Pressing it again goes on with the same draft.
    let (status, again) = call(&pool, &ws, next_sheet(&cameras_id, "Vehicles")).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(id_of(&again), vehicles_id);

    // The source's files going (its prune) leaves the draft working.
    let scoped = WorkspaceScopedStorage::new(test_storage(), ws.workspace_id);
    files::remove_job_files(&scoped, Uuid::parse_str(&cameras_id).unwrap())
        .await
        .expect("remove source files");
    let (status, checked) = call(
        &pool,
        &ws,
        mapping_request(
            &vehicles_id,
            serde_json::json!({ "mapping": { "asset_tag": 0, "name": 1, "kind": 2 } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["summary"]["would_create"], 1, "{checked}");
    // No constant came across: the van has no location.
    let (status, done) = call(
        &pool,
        &ws,
        TestRequest::post().uri(&format!("/api/admin/import/{vehicles_id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let location: Option<String> = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        use backend::schema::assets::dsl as a;
        a::assets
            .filter(a::asset_tag.eq("NS-V1"))
            .select(a::location)
            .first::<Option<String>>(c)
    })
    .expect("read van");
    assert_eq!(location, None);
}

/// A chain: Cameras, then Vehicles, then Misc. Each Done offers the next
/// sheet nothing has imported; a sheet imported once is refused; after
/// the last one nothing is offered (the hidden and the empty sheet aren't).
#[actix_web::test]
async fn each_sheet_is_offered_once_across_the_upload() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspace(&pool);

    let cameras = committed_cameras(&pool, &ws).await;
    let cameras_id = id_of(&cameras);
    let (status, vehicles) = call(&pool, &ws, next_sheet(&cameras_id, "Vehicles")).await;
    assert_eq!(status, StatusCode::CREATED, "{vehicles}");
    let vehicles = map_and_commit(&pool, &ws, &id_of(&vehicles)).await;
    let vehicles_id = id_of(&vehicles);
    assert_eq!(vehicles["next_sheet"], "Misc", "{vehicles}");
    assert_eq!(get_job(&pool, &ws, &cameras_id).await["next_sheet"], "Misc");

    for (from, sheet) in [(&cameras_id, "Vehicles"), (&vehicles_id, "Cameras")] {
        let (status, body) = call(&pool, &ws, next_sheet(from, sheet)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], "import_sheet_already_imported", "{body}");
    }

    // From the second job, the new one still names the first upload.
    let (status, misc) = call(&pool, &ws, next_sheet(&vehicles_id, "Misc")).await;
    assert_eq!(status, StatusCode::CREATED, "{misc}");
    let misc_id = id_of(&misc);
    assert_eq!(
        upload_of(&pool, &ws, &misc_id),
        Some(Uuid::parse_str(&cameras_id).unwrap())
    );
    let misc = map_and_commit(&pool, &ws, &misc_id).await;
    assert!(misc.get("next_sheet").is_none(), "{misc}");
    assert!(
        get_job(&pool, &ws, &cameras_id)
            .await
            .get("next_sheet")
            .is_none(),
        "nothing left to offer"
    );
}

/// Each refusal, by code.
#[actix_web::test]
async fn refusals() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    // A draft: nothing imported yet.
    let (status, draft) = call(&pool, ws, upload_request("gear.xlsx", &workbook())).await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    let (status, body) = call(&pool, ws, next_sheet(&id_of(&draft), "Vehicles")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_not_committed", "{body}");

    let cameras = committed_cameras(&pool, ws).await;
    let id = id_of(&cameras);
    for (sheet, code) in [
        ("Nope", "import_sheet_unknown"),
        ("Lookups", "import_sheet_hidden"),
    ] {
        let (status, body) = call(&pool, ws, next_sheet(&id, sheet)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], code, "{body}");
    }
    // A sheet with no rows is refused as the sheet panel refuses it.
    let (status, body) = call(&pool, ws, next_sheet(&id, "Empty")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Another workspace's job.
    let (status, _) = call_import_route(
        &pool,
        &seeded.b,
        seeded.b.admin_uuid,
        next_sheet(&id, "Vehicles"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A CSV has one sheet.
    let csv = "name,kind,asset_tag\nCam,generic,NS-CSV-1\n";
    let (status, job) = call(
        &pool,
        ws,
        upload_request_for("assets", "gear.csv", csv.as_bytes()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let csv_id = id_of(&job);
    let (status, done) = {
        let (s, j) = call(
            &pool,
            ws,
            mapping_request(
                &csv_id,
                serde_json::json!({ "mapping": { "name": 0, "kind": 1, "asset_tag": 2 } }),
            ),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{j}");
        call(
            &pool,
            ws,
            TestRequest::post().uri(&format!("/api/admin/import/{csv_id}/commit")),
        )
        .await
    };
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done.get("next_sheet").is_none(), "{done}");
    let (status, body) = call(&pool, ws, next_sheet(&csv_id, "Cameras")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "import_single_sheet", "{body}");

    // The file gone: upload it again.
    let scoped = WorkspaceScopedStorage::new(test_storage(), ws.workspace_id);
    let upload = files::upload_key(Uuid::parse_str(&id).unwrap(), "xlsx");
    scoped.delete_file(&upload).await.expect("remove upload");
    let (status, body) = call(&pool, ws, next_sheet(&id, "Vehicles")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], files::MISSING_CODE, "{body}");
    // Nothing was left behind for a job that wasn't made.
    let jobs: i64 = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        use backend::schema::import_jobs::dsl as j;
        j::import_jobs
            .filter(j::upload_of.is_not_null())
            .count()
            .get_result(c)
    })
    .expect("count");
    assert_eq!(jobs, 0);

    // Pruned: the details, and the file, are gone.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        use backend::schema::import_jobs::dsl as j;
        diesel::update(j::import_jobs.find(Uuid::parse_str(&id).unwrap()))
            .set(j::pruned_at.eq(Some(chrono::Utc::now())))
            .execute(c)
    })
    .expect("prune");
    let (status, body) = call(&pool, ws, next_sheet(&id, "Vehicles")).await;
    assert_eq!(status, StatusCode::GONE, "{body}");
    assert_eq!(body["code"], "import_details_pruned", "{body}");
    assert!(get_job(&pool, ws, &id).await.get("next_sheet").is_none());
}
