//! A workspace export carries the workspace's content only: no earlier
//! exports, CSV import uploads and grids, staged uploads or workspace
//! archives waiting to import, and no CSV import history. An export from a
//! workspace that ran a CSV import imports, and so does an older archive that
//! still carries the import tables.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::workspace_context::DeploymentMode;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewImportValueAnswer};
use backend::repository::imports as import_repo;
use backend::services::workspace_archive::plan::Runner;
use backend::services::workspace_import::{
    import_workspace, read_archive, ArchiveContents, ImportOptions,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};
use backend::utils::storage::{Storage, WorkspaceScopedStorage};

use crate::common::{test_storage, TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{import_workspaces, upload_and_check};

const REF: &str = "test:workspace_export_hygiene";

const HEADER: &str =
    "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold";

/// Sealed, so the export keeps the fixture webhook's secret.
const PASSWORD: &str = "hygiene-password";

const LEFT_OUT: &[&str] = &["imports/", "exports/", "temp/", "workspace-imports/"];

/// One request to the platform-admin workspace export and import routes.
pub(crate) async fn call_admin_route(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: http_test::TestRequest,
) -> (StatusCode, web::Bytes) {
    call_admin_route_with_search(pool, ws, req, None).await
}

/// [`call_admin_route`], with the search index the import reindexes.
pub(crate) async fn call_admin_route_with_search(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: http_test::TestRequest,
    search: Option<std::sync::Arc<backend::services::search::SearchService>>,
) -> (StatusCode, web::Bytes) {
    let admin = Uuid::now_v7();
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: admin.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(admin, Some(corr)).with_workspace(ws.workspace_id);
    let mut app = App::new().app_data(web::Data::new(pool.clone()));
    if let Some(search) = search {
        app = app.app_data(web::Data::new(search));
    }
    let app = http_test::init_service(
        app.wrap_fn(move |req, srv| {
            req.extensions_mut().insert(workspace.clone());
            req.extensions_mut().insert(claims.clone());
            req.extensions_mut()
                .insert(RequestContext::new(corr, actor.clone()));
            srv.call(req)
        })
        .service(web::scope("/api").configure(backend::handlers::workspace_export::config)),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    (status, http_test::read_body(resp).await)
}

async fn export(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<u8> {
    let (status, body) = call_admin_route(
        pool,
        ws,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workspaces/{}/export", ws.workspace_id))
            .set_json(serde_json::json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    body.to_vec()
}

fn file_names(contents: &ArchiveContents) -> Vec<String> {
    contents
        .files
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count_in(pool: &TestPool, table: &str, workspace_id: i32) -> i64 {
    diesel::sql_query(format!(
        "SELECT COUNT(*) AS n FROM {table} WHERE workspace_id = $1"
    ))
    .bind::<Integer, _>(workspace_id)
    .get_result::<Count>(&mut pool.get().expect("conn"))
    .expect("count")
    .n
}

/// An export after a CSV import and an earlier export leaves out the import's
/// files and rows, the earlier export, staged uploads and waiting workspace
/// archives, keeps the workspace's own files, and imports as a new workspace.
#[actix_web::test]
async fn an_export_carries_the_workspace_and_leaves_out_imports_and_exports() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspaces(&pool).a;
    let storage = WorkspaceScopedStorage::new(test_storage(), ws.workspace_id);

    // A CSV import: a job row with its upload and grid, and a saved match.
    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nLaptop,generic,HYG-001,,,,,,,,\n"),
    )
    .await;
    let job_id = Uuid::parse_str(job["id"].as_str().expect("job id")).expect("uuid");
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        import_repo::upsert_value_answers(
            c,
            &[NewImportValueAnswer {
                job_type: "assets".to_string(),
                field: "kind".to_string(),
                context: String::new(),
                normalized_value: "laptop".to_string(),
                answer: serde_json::json!({ "action": "match", "target": "generic" }),
                answered_by: Some(ws.admin_uuid),
                job_id: Some(job_id),
                source_text: Some("Laptop".to_string()),
                created: false,
            }],
        )
    })
    .expect("save a match");
    assert!(count_in(&pool, "import_jobs", ws.workspace_id) > 0);
    assert!(count_in(&pool, "import_value_answers", ws.workspace_id) > 0);

    let kept = format!("tickets/1/{}.txt", Uuid::now_v7());
    let staged = format!("temp/{}.png", Uuid::now_v7());
    let waiting = format!("workspace-imports/{}.nosdesk", Uuid::now_v7());
    for key in [&kept, &staged, &waiting] {
        storage
            .put_file(b"bytes", key, "application/octet-stream")
            .await
            .expect("store file");
    }

    let first = export(&pool, &ws).await;
    let contents = read_archive(&first, Some(PASSWORD)).expect("read archive");
    let names = file_names(&contents);
    assert!(
        names.contains(&format!("files/{kept}")),
        "the workspace's own file is exported: {names:?}"
    );
    for prefix in LEFT_OUT {
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with(&format!("files/{prefix}"))),
            "nothing under {prefix} is exported: {names:?}"
        );
    }
    for table in ["import_jobs", "import_value_answers"] {
        assert!(
            !contents.tables.contains_key(table) && !contents.manifest.tables.contains_key(table),
            "{table} is not exported"
        );
    }

    // An export after an export: the earlier artifact stays out.
    let earlier = format!("exports/{}.nosdesk", Uuid::now_v7());
    storage
        .put_file(&first, &earlier, "application/octet-stream")
        .await
        .expect("store the earlier export");
    let second = export(&pool, &ws).await;
    let names = file_names(&read_archive(&second, Some(PASSWORD)).expect("read archive"));
    assert!(
        !names.iter().any(|n| n.starts_with("files/exports/")),
        "an earlier export is not exported: {names:?}"
    );
    assert!(names.contains(&format!("files/{kept}")));

    // The archive imports as a new workspace.
    let mut payload = b"--x\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"ws.nosdesk\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    payload.extend_from_slice(&second);
    payload.extend_from_slice(
        format!(
            "\r\n--x\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\n{PASSWORD}\
             \r\n--x\r\nContent-Disposition: form-data; name=\"regenerate_uuids\"\r\n\r\ntrue\
             \r\n--x\r\nContent-Disposition: form-data; name=\"slug\"\r\n\r\nhygiene-{}\
             \r\n--x--\r\n",
            Uuid::now_v7().simple()
        )
        .as_bytes(),
    );
    let (status, body) = call_admin_route(
        &pool,
        &ws,
        http_test::TestRequest::post()
            .uri("/api/admin/workspaces/import")
            .insert_header(("content-type", "multipart/form-data; boundary=x"))
            .set_payload(payload),
    )
    .await;
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let imported = result["workspace_id"].as_i64().expect("workspace id") as i32;
    assert_ne!(imported, ws.workspace_id);
    assert_eq!(count_in(&pool, "import_jobs", imported), 0);
}

/// An archive taken before the import tables were left out still imports:
/// their rows are skipped, not refused.
#[actix_web::test]
async fn an_older_archive_with_import_history_imports_without_it() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = import_workspaces(&pool).a;
    let job = upload_and_check(
        &pool,
        &ws,
        &format!("{HEADER}\nDesk,generic,HYG-002,,,,,,,,\n"),
    )
    .await;
    assert!(job["id"].is_string(), "{job}");

    let archive = export(&pool, &ws).await;
    let mut contents = read_archive(&archive, Some(PASSWORD)).expect("read archive");

    // Splice in the rows a 1.1.2 export carried.
    #[derive(QueryableByName)]
    struct RowText {
        #[diesel(sql_type = Text)]
        row: String,
    }
    let rows: Vec<serde_json::Value> = diesel::sql_query(
        "SELECT row_to_json(t)::text AS row FROM import_jobs t WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .load::<RowText>(&mut pool.get().expect("conn"))
    .expect("import_jobs rows")
    .into_iter()
    .map(|r| serde_json::from_str(&r.row).expect("row json"))
    .collect();
    assert!(!rows.is_empty());
    contents.tables.insert("import_jobs".to_string(), rows);
    contents
        .tables
        .insert("import_value_answers".to_string(), Vec::new());

    let opts = ImportOptions {
        slug_override: Some(format!("older-{}", Uuid::now_v7().simple())),
        uuid_override: None,
        regenerate_uuids: true,
        mode: DeploymentMode::SelfHosted,
        runner: Runner::Operator { user: None },
    };
    let actor = ActorContext::system(REF);
    let mut conn = pool.get().expect("conn");
    let result = with_actor_bypass_context(&mut conn, &actor, |c| {
        import_workspace(c, &contents, &opts)
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    })
    .expect("the archive imports");
    drop(conn);
    assert_eq!(count_in(&pool, "import_jobs", result.workspace_id), 0);
}
