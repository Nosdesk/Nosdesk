//! Import history: Recent imports (`GET /admin/import`), the derived
//! state (Draft, Imported, Expired), the 30-day prune that keeps counts,
//! options, mapping and record ids and drops the file and per-row detail,
//! the 410 for a pruned job's rows and error report, and the asset list
//! filtered to an import's records (`?import=`).

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Uuid as SqlUuid};
use uuid::Uuid;

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{call_import_route, import_workspaces, json, upload_request};
use crate::imports_attributes::{checked, commit, id_of};

const REF: &str = "test:imports_history";

const MAPPING: &[(&str, &str)] = &[
    ("name", "name"),
    ("kind", "kind"),
    ("asset_tag", "asset_tag"),
];

async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = call_import_route(pool, ws, user, req).await;
    (status, json(&body))
}

async fn list(pool: &TestPool, ws: &WorkspaceSeed, query: &str) -> Vec<serde_json::Value> {
    let (status, body) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import{query}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["jobs"].as_array().expect("jobs").clone()
}

async fn get(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> serde_json::Value {
    let (status, body) = call(
        pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn upload(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    name: &str,
    file: &str,
) -> (StatusCode, serde_json::Value) {
    call(
        pool,
        ws,
        ws.admin_uuid,
        upload_request(name, file.as_bytes()),
    )
    .await
}

/// Run `sql` with `$1` bound to the job, pinned to its workspace so the
/// audit trigger has one.
fn on_job(pool: &TestPool, ws: &WorkspaceSeed, sql: &str, id: &str) {
    let id = Uuid::parse_str(id).expect("uuid");
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(sql).bind::<SqlUuid, _>(id).execute(c)
    })
    .expect(sql);
}

#[derive(QueryableByName)]
struct JobRow {
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    row_results: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    answers: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    summary: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    source: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    mapping: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    options: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
    record_ids: Option<serde_json::Value>,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>)]
    pruned_at: Option<chrono::DateTime<chrono::Utc>>,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn job_row(pool: &TestPool, id: &str) -> Option<JobRow> {
    diesel::sql_query(
        "SELECT row_results, answers, summary, source, mapping, options, record_ids, pruned_at, updated_at \
         FROM import_jobs WHERE id = $1",
    )
    .bind::<SqlUuid, _>(Uuid::parse_str(id).expect("uuid"))
    .get_result(&mut pool.get().expect("conn"))
    .optional()
    .expect("read job")
}

async fn files_of(ws: &crate::common::WorkspaceSeed, id: &str) -> usize {
    crate::imports_assets::job_files(ws.workspace_id, id)
        .await
        .len()
}

async fn prune(db: &TestDb) {
    let cutoff = chrono::Utc::now() - backend::services::imports::history::details_window();
    backend::services::scheduled_jobs::prune_import_history_before(&db.job_pool(), cutoff)
        .await
        .expect("prune");
}

/// Recent imports lists the jobs that reached Map, newest first, with
/// their state, who uploaded them and the outcome counts; a refused upload
/// isn't listed, `limit` caps it, another workspace's jobs don't show, and
/// only a workspace admin may read it.
#[actix_web::test]
async fn recent_imports_list_the_jobs_that_reached_map() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let (ws, other) = (&seeded.a, &seeded.b);

    let (status, refused) = upload(&pool, ws, "empty.csv", "name,kind,asset_tag\n").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let committed = checked(
        &pool,
        ws,
        "name,kind,asset_tag\nCam A,generic,H-1\nCam B,generic,H-2\nNo tag kind,nope,H-3\n",
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let committed = commit(&pool, ws, &id_of(&committed)).await;
    let checked_draft = checked(
        &pool,
        ws,
        "name,kind,asset_tag\nCam C,generic,H-4\n",
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let (status, parsed) = upload(
        &pool,
        ws,
        "Cameras.csv",
        "name,kind,asset_tag\nCam D,generic,H-5\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{parsed}");
    let (status, theirs) = upload(
        &pool,
        other,
        "theirs.csv",
        "name,kind,asset_tag\nX,generic,T-1\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{theirs}");

    let jobs = list(&pool, ws, "").await;
    let ids: Vec<&str> = jobs.iter().map(|j| j["id"].as_str().expect("id")).collect();
    assert_eq!(
        ids,
        vec![
            parsed["id"].as_str().unwrap(),
            checked_draft["id"].as_str().unwrap(),
            committed["id"].as_str().unwrap(),
        ],
        "{jobs:#?}"
    );
    assert_eq!(jobs[0]["state"], "draft");
    assert_eq!(jobs[0]["status"], "parsed");
    assert_eq!(jobs[0]["filename"], "Cameras.csv");
    assert!(jobs[0]["counts"].is_null(), "{}", jobs[0]);
    assert_eq!(jobs[1]["state"], "draft");
    assert_eq!(jobs[1]["counts"]["new"], 1);
    assert_eq!(jobs[2]["state"], "imported");
    assert_eq!(jobs[2]["counts"]["new"], 2);
    assert_eq!(jobs[2]["counts"]["updated"], 0);
    assert_eq!(jobs[2]["counts"]["skipped"], 1);
    assert_eq!(jobs[2]["created_by"], ws.admin_uuid.to_string());
    assert!(jobs[2]["created_by_name"].is_string(), "{}", jobs[2]);
    // Light: no file columns, no rows.
    assert!(jobs[2].get("source").is_none());
    assert!(jobs[2].get("summary").is_none());

    let two = list(&pool, ws, "?limit=2").await;
    assert_eq!(two.len(), 2);
    assert_eq!(two[0]["id"], parsed["id"]);

    let theirs_listed = list(&pool, other, "").await;
    assert_eq!(theirs_listed.len(), 1);
    assert_eq!(theirs_listed[0]["id"], theirs["id"]);

    let (status, body) = call(
        &pool,
        ws,
        ws.member_uuid,
        TestRequest::get().uri("/api/admin/import"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

/// Thirty days after the commit the prune drops the file, the grid, the
/// row results, the answers and the summary's detail, and keeps the
/// counts, options, mapping, header names and record ids, without moving
/// the job's last change. The job reads Imported still; its rows and error
/// report answer 410 instead of a broken file.
#[actix_web::test]
async fn the_prune_keeps_counts_ids_and_options_and_drops_the_detail() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    let job = checked(
        &pool,
        ws,
        "name,kind,asset_tag\nCam A,generic,P-1\nBad,nope,P-2\n",
        MAPPING,
        serde_json::json!({ "match_by": "asset_tag" }),
    )
    .await;
    let id = id_of(&job);
    commit(&pool, ws, &id).await;

    // Before the prune: the detail and the report are there.
    let (status, _) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/error-report")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, page) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/rows")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(files_of(ws, &id).await > 0);
    let before = job_row(&pool, &id).expect("job");
    assert!(before.row_results.is_some());

    // Not due yet: nothing changes.
    prune(&db).await;
    assert!(job_row(&pool, &id).expect("job").pruned_at.is_none());

    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET completed_at = now() - interval '31 days', \
         updated_at = now() - interval '31 days' WHERE id = $1",
        &id,
    );
    let aged = job_row(&pool, &id).expect("job");
    prune(&db).await;

    let after = job_row(&pool, &id).expect("job");
    assert!(after.pruned_at.is_some());
    assert!(after.row_results.is_none());
    assert!(after.answers.is_none());
    assert_eq!(
        after.updated_at, aged.updated_at,
        "the prune left updated_at alone"
    );
    let summary = after.summary.expect("summary");
    assert_eq!(summary["row_count"], 2);
    assert_eq!(summary["would_create"], 1);
    assert_eq!(summary["rows_with_errors"], 1);
    assert_eq!(summary["errors"], serde_json::json!([]));
    assert_eq!(summary["error_groups"], serde_json::json!([]));
    assert_eq!(after.mapping, before.mapping);
    assert_eq!(after.options, before.options);
    assert_eq!(after.record_ids, before.record_ids);
    assert_eq!(
        after.record_ids.expect("ids")["created"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let source = after.source.expect("source");
    let headers = source["headers"].as_array().expect("headers");
    assert_eq!(headers.len(), 3);
    assert_eq!(headers[0]["name"], "name");
    assert!(headers
        .iter()
        .all(|h| h["samples"].as_array().is_some_and(Vec::is_empty)));
    assert_eq!(files_of(ws, &id).await, 0, "the file and grid are gone");

    let read = get(&pool, ws, &id).await;
    assert_eq!(read["state"], "imported");
    assert!(read["pruned_at"].is_string(), "{read}");
    assert_eq!(read["summary"]["would_create"], 1);

    for path in ["rows", "error-report"] {
        let (status, body) = call(
            &pool,
            ws,
            ws.admin_uuid,
            TestRequest::get().uri(&format!("/api/admin/import/{id}/{path}")),
        )
        .await;
        assert_eq!(status, StatusCode::GONE, "{path}: {body}");
        assert_eq!(body["code"], "import_details_pruned", "{path}: {body}");
    }

    // Still listed, as Imported, with its counts.
    let jobs = list(&pool, ws, "").await;
    assert_eq!(jobs[0]["id"], id.as_str());
    assert_eq!(jobs[0]["state"], "imported");
    assert_eq!(jobs[0]["counts"]["new"], 1);
    assert_eq!(jobs[0]["counts"]["skipped"], 1);

    // The next upload still remembers this import's headers.
    let (status, next) = upload(
        &pool,
        ws,
        "again.csv",
        "name,kind,asset_tag\nCam Z,generic,P-9\n",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{next}");
    let previous = next["suggested_mapping"]
        .as_array()
        .expect("suggestions")
        .iter()
        .filter(|s| s["reason"] == "previous")
        .count();
    assert_eq!(previous, 3, "{next}");
}

/// A draft untouched for 30 days reads Expired before the prune reaches
/// it and can't be changed or committed; the prune then drops its file and
/// rows and keeps it listed as Expired. A recent draft is left alone, and a
/// refused upload leaves no job to prune.
#[actix_web::test]
async fn drafts_expire_after_thirty_days() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    let draft = checked(
        &pool,
        ws,
        "name,kind,asset_tag\nCam A,generic,E-1\n",
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&draft);
    let fresh = checked(
        &pool,
        ws,
        "name,kind,asset_tag\nCam B,generic,E-2\n",
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let fresh_id = id_of(&fresh);
    let (status, _) = upload(&pool, ws, "empty.csv", "name,kind,asset_tag\n").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    #[derive(QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let refused = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM import_jobs WHERE status = 'failed' AND workspace_id = $1",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .get_result::<CountRow>(&mut pool.get().expect("conn"))
    .expect("count refused jobs")
    .count;
    assert_eq!(refused, 0, "a refused upload makes no job");

    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET updated_at = now() - interval '31 days' WHERE id = $1",
        &id,
    );

    // Expired by age, before any prune.
    let read = get(&pool, ws, &id).await;
    assert_eq!(read["state"], "expired");
    assert!(read["pruned_at"].is_null());
    assert_eq!(get(&pool, ws, &fresh_id).await["state"], "draft");
    let (status, body) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/admin/import/{id}/commit")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap_or("").contains("expired"),
        "{body}"
    );
    let (status, body) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/values")),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    prune(&db).await;

    let after = job_row(&pool, &id).expect("draft kept");
    assert!(after.pruned_at.is_some());
    assert!(after.row_results.is_none());
    assert!(after.mapping.is_some());
    assert_eq!(files_of(ws, &id).await, 0);
    let fresh_after = job_row(&pool, &fresh_id).expect("fresh draft");
    assert!(fresh_after.pruned_at.is_none());
    assert!(fresh_after.row_results.is_some());
    assert!(files_of(ws, &fresh_id).await > 0);

    let jobs = list(&pool, ws, "").await;
    let states: Vec<(&str, &str)> = jobs
        .iter()
        .map(|j| (j["id"].as_str().unwrap(), j["state"].as_str().unwrap()))
        .collect();
    assert_eq!(
        states,
        vec![(fresh_id.as_str(), "draft"), (id.as_str(), "expired")]
    );
}

/// One request to the paginated asset list, signed in as `user` in `ws`.
async fn assets_page(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    query: &str,
) -> (StatusCode, serde_json::Value) {
    use actix_web::dev::Service;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::sync::actor::ActorContext;

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Viewer".to_string(),
        email: "viewer@example.com".to_string(),
        platform_role: "user".to_string(),
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
    let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/api/assets/paginated",
                web::get().to(backend::handlers::get_paginated_devices),
            ),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        TestRequest::get()
            .uri(&format!(
                "/api/assets/paginated?pageSize=100&sortField=name&sortDirection=asc&{query}"
            ))
            .to_request(),
    )
    .await;
    let status = resp.status();
    (status, json(&http_test::read_body(resp).await))
}

fn names(page: &serde_json::Value) -> Vec<String> {
    page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|a| a["name"].as_str().expect("name").to_string())
        .collect()
}

/// `?import=` lists the assets the import created and updated, from the
/// record ids the job keeps, before and after the prune; an unknown job
/// lists none, a bad id is 400, and only a workspace admin may ask.
#[actix_web::test]
async fn the_asset_list_filters_to_an_imports_records() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO assets (name, kind, asset_tag) VALUES \
             ('Old name', 'generic', 'F-1'), ('Bystander', 'generic', 'F-9')",
        )
        .execute(c)
    })
    .expect("seed assets");

    let job = checked(
        &pool,
        ws,
        "name,kind,asset_tag\nNew name,generic,F-1\nCreated one,generic,F-2\nCreated two,generic,F-3\n",
        MAPPING,
        serde_json::json!({}),
    )
    .await;
    let id = id_of(&job);
    commit(&pool, ws, &id).await;

    let (status, page) = assets_page(&pool, ws, ws.admin_uuid, &format!("import={id}")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["Created one", "Created two", "New name"]);
    assert_eq!(page["total"], 3);

    let (_, all) = assets_page(&pool, ws, ws.admin_uuid, "").await;
    assert_eq!(all["total"], 4);

    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET completed_at = now() - interval '31 days' WHERE id = $1",
        &id,
    );
    prune(&db).await;
    assert!(job_row(&pool, &id).expect("job").pruned_at.is_some());
    let (status, page) = assets_page(&pool, ws, ws.admin_uuid, &format!("import={id}")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(names(&page), vec!["Created one", "Created two", "New name"]);

    let (status, page) = assets_page(
        &pool,
        ws,
        ws.admin_uuid,
        &format!("import={}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["total"], 0);

    // Another workspace's admin can't reach this job's records.
    let (status, page) = assets_page(
        &pool,
        &seeded.b,
        seeded.b.admin_uuid,
        &format!("import={id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["total"], 0);

    let (status, _) = assets_page(&pool, ws, ws.admin_uuid, "import=nope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = assets_page(&pool, ws, ws.member_uuid, &format!("import={id}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
