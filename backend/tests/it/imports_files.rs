//! An import's files in the storage backend: a missing file answers 409
//! `import_file_missing`, reads stay under the job's own prefix in its own
//! workspace, and the sweep removes files no job keeps.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use backend::services::imports::files;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{
    create_storage, Storage, StorageConfig, StorageError, WorkspaceScopedStorage,
};
use diesel::RunQueryDsl;
use uuid::Uuid;

use crate::common::{test_storage, TestDb};
use crate::imports_assets::{
    call_import_route, import_workspaces, job_files, json, mapping_request, template_mapping,
    upload_and_check,
};

const REF: &str = "test:imports_files";

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

fn scoped(workspace_id: i32) -> WorkspaceScopedStorage {
    WorkspaceScopedStorage::new(test_storage(), workspace_id)
}

fn assert_missing(status: StatusCode, body: &serde_json::Value) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], files::MISSING_CODE, "{body}");
    assert_eq!(body["error"], files::MISSING, "{body}");
}

/// A checked draft whose grid is gone answers 409 `import_file_missing` on
/// every step that reads it, never a 500.
#[actix_web::test]
async fn a_missing_grid_asks_for_the_file_again() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspaces(&pool).a;

    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nGone,generic,GONE-001,,,,,,,,\n"),
    )
    .await;
    let id = job["id"].as_str().expect("job id").to_string();
    let storage = scoped(ws.workspace_id);
    for name in job_files(ws.workspace_id, &id).await {
        if name.starts_with("grid.") {
            storage
                .delete_file(&format!("imports/{id}/{name}"))
                .await
                .expect("delete grid");
        }
    }

    for req in [
        mapping_request(&id, template_mapping()),
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/values"))
            .set_json(serde_json::json!({ "answers": {} })),
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    ] {
        let (status, body) = call_import_route(&pool, &ws, ws.admin_uuid, req).await;
        assert_missing(status, &json(&body));
    }
}

/// A job's grid is read under its own prefix only: pointing its source at
/// another job's grid version finds nothing, and another workspace's
/// storage can't reach it.
#[actix_web::test]
async fn reads_stay_under_the_job_and_its_workspace() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let (a, b) = (&seeded.a, &seeded.b);

    let first = upload_and_check(
        &pool,
        a,
        &format!("{HEADER}\nFirst,generic,PFX-001,,,,,,,,\n"),
    )
    .await;
    let second = upload_and_check(
        &pool,
        a,
        &format!("{HEADER}\nSecond,generic,PFX-002,,,,,,,,\n"),
    )
    .await;
    let first_id = first["id"].as_str().expect("id").to_string();
    let second_id = second["id"].as_str().expect("id").to_string();

    // The first job's source names the second job's grid version.
    let second_grid = job_files(a.workspace_id, &second_id)
        .await
        .into_iter()
        .find(|n| n.starts_with("grid."))
        .expect("second grid");
    let version = second_grid
        .trim_start_matches("grid.")
        .trim_end_matches(".json")
        .to_string();
    run_in_workspace(&pool, REF, a.workspace_id, |c| {
        diesel::sql_query(
            "UPDATE import_jobs SET source = jsonb_set(source, '{grid}', to_jsonb($1::text)) WHERE id = $2",
        )
        .bind::<diesel::sql_types::Text, _>(&version)
        .bind::<diesel::sql_types::Uuid, _>(Uuid::parse_str(&first_id).expect("uuid"))
        .execute(c)
    })
    .expect("point at the other grid");
    let (status, body) = call_import_route(
        &pool,
        a,
        a.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{first_id}/commit")),
    )
    .await;
    assert_missing(status, &json(&body));

    // Workspace B's storage doesn't reach workspace A's files.
    let key = format!("imports/{second_id}/{second_grid}");
    assert!(scoped(a.workspace_id).get_file(&key).await.is_ok());
    assert!(matches!(
        scoped(b.workspace_id).get_file(&key).await,
        Err(StorageError::NotFound(_))
    ));
    // Nor does a path that climbs out of a folder, which every file route
    // builds its path under.
    assert!(matches!(
        scoped(b.workspace_id)
            .get_file(&format!("temp/../../{}/{key}", a.workspace_id))
            .await,
        Err(StorageError::InvalidPath(_))
    ));
}

/// A v7 id dated `days` ago.
fn id_from(days: u64) -> Uuid {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        - days * 86_400;
    Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, secs, 0))
}

/// The sweep removes the files of a job with no row, once the job is old
/// enough that its upload can't still be creating it; it keeps a job's
/// files while the job keeps them, a recent upload's, and those under an
/// id it can't date.
#[actix_web::test]
async fn the_sweep_removes_files_no_job_keeps() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspaces(&pool).a;
    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nKept,generic,SWEEP-001,,,,,,,,\n"),
    )
    .await;
    let kept = Uuid::parse_str(job["id"].as_str().expect("id")).expect("uuid");

    // Its own storage: the shared one holds other tests' fresh files.
    let root = tempfile::tempdir().expect("tempdir");
    let base = create_storage(StorageConfig::Local {
        base_path: root.path().to_string_lossy().into_owned(),
    });
    let storage = WorkspaceScopedStorage::new(base.clone(), ws.workspace_id);
    let orphan = id_from(3);
    let recent = id_from(0);
    let undated = Uuid::new_v4();
    for id in [kept, orphan, recent, undated] {
        files::put_upload(&storage, id, "csv", b"a,b\n")
            .await
            .expect("put upload");
        files::put_grid(&storage, id, b"{}")
            .await
            .expect("put grid");
    }

    backend::services::scheduled_jobs::sweep_import_files_in(
        &db.job_pool(),
        base.clone(),
        chrono::Utc::now() - chrono::Duration::days(1),
    )
    .await
    .expect("sweep");
    let left = files::stored_jobs(&storage).await.expect("list");
    assert!(!left.contains(&orphan), "{left:?}");
    for id in [kept, recent, undated] {
        assert!(left.contains(&id), "{id} in {left:?}");
    }

    // Once the prune has reached the job, its files go too.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("UPDATE import_jobs SET pruned_at = now() WHERE id = $1")
            .bind::<diesel::sql_types::Uuid, _>(kept)
            .execute(c)
    })
    .expect("mark pruned");
    backend::services::scheduled_jobs::sweep_import_files_in(
        &db.job_pool(),
        base,
        chrono::Utc::now() + chrono::Duration::days(1),
    )
    .await
    .expect("sweep");
    let left = files::stored_jobs(&storage).await.expect("list");
    assert_eq!(left, vec![undated], "{left:?}");
}

/// A workbook whose upload is gone can't be read from another sheet: 409
/// `import_file_missing`, and the job keeps the grid it had.
#[actix_web::test]
async fn a_missing_workbook_asks_for_the_file_again() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspaces(&pool).a;

    let mut wb = rust_xlsxwriter::Workbook::new();
    for name in ["Cameras", "Vehicles"] {
        let sheet = wb.add_worksheet();
        sheet.set_name(name).expect("name");
        sheet.write_string(0, 0, "name").expect("write");
        sheet.write_string(1, 0, "Thing").expect("write");
    }
    let bytes = wb.save_to_buffer().expect("workbook");
    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        crate::imports_assets::upload_request("assets.xlsx", &bytes),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_str().expect("id").to_string();
    scoped(ws.workspace_id)
        .delete_file(&format!("imports/{id}/upload.xlsx"))
        .await
        .expect("delete upload");
    let before = job_files(ws.workspace_id, &id).await;

    let (status, body) = call_import_route(
        &pool,
        &ws,
        ws.admin_uuid,
        TestRequest::put()
            .uri(&format!("/api/admin/import/{id}/source"))
            .set_json(serde_json::json!({ "sheet": "Vehicles" })),
    )
    .await;
    assert_missing(status, &json(&body));
    assert_eq!(job_files(ws.workspace_id, &id).await, before);
}
