//! Documentation imports as jobs: the upload, the check, the page list,
//! preview and report, who may reach them, what a changed plan does at
//! commit, a check whose process died, and the prune.

use std::io::Write as _;
use std::sync::OnceLock;

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::Uuid as SqlUuid;
use uuid::Uuid;

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{import_workspaces, job_files, json};

const REF: &str = "test:imports_docs";

/// The smallest valid PNG.
pub(crate) const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

thread_local! {
    /// Storage this test's requests use in place of the shared test root.
    static PRIVATE_STORAGE: std::cell::RefCell<Option<std::sync::Arc<dyn backend::utils::storage::Storage>>> =
        const { std::cell::RefCell::new(None) };
}

/// Give this test's requests a storage root of their own, so what it finds
/// there is only what it wrote. Returns it.
pub(crate) fn private_storage() -> std::sync::Arc<dyn backend::utils::storage::Storage> {
    static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
    let root = ROOT.get_or_init(|| tempfile::tempdir().expect("storage root"));
    let dir = root.path().join(Uuid::now_v7().to_string());
    let storage: std::sync::Arc<dyn backend::utils::storage::Storage> =
        std::sync::Arc::new(backend::utils::storage::LocalStorage::new(
            dir.to_string_lossy().into_owned(),
            "/files".into(),
        ));
    PRIVATE_STORAGE.with(|s| *s.borrow_mut() = Some(storage.clone()));
    storage
}

/// One request to the import routes as `user` in `ws`, with an upload cap
/// when given.
pub(crate) async fn call_docs(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    req: TestRequest,
    cap: Option<u64>,
) -> (StatusCode, serde_json::Value, Vec<u8>) {
    use actix_web::dev::Service;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};
    use std::sync::Arc;

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::services::search::SearchService;
    use backend::sync::actor::ActorContext;

    static SEARCH_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let search_dir = SEARCH_DIR.get_or_init(|| tempfile::tempdir().expect("search dir"));

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Importer".to_string(),
        email: "importer@example.com".to_string(),
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
    // One index per call: a background commit may still be indexing into an
    // earlier call's.
    let search = Arc::new(
        SearchService::new(&search_dir.path().join(Uuid::now_v7().to_string()), pool)
            .expect("init search"),
    );
    let mut app = App::new()
        .app_data(web::Data::new(pool.clone()))
        .app_data(web::Data::new(search))
        .app_data(web::Data::new(
            PRIVATE_STORAGE
                .with(|s| s.borrow().clone())
                .unwrap_or_else(crate::common::test_storage),
        ));
    if let Some(cap) = cap {
        app = app.app_data(web::Data::new(backend::handlers::imports_docs::UploadCap(
            cap,
        )));
    }
    let app = http_test::init_service(
        app.wrap_fn(move |req, srv| {
            req.extensions_mut().insert(workspace.clone());
            req.extensions_mut().insert(claims.clone());
            req.extensions_mut()
                .insert(RequestContext::new(corr, actor.clone()));
            srv.call(req)
        })
        .service(web::scope("/api").configure(backend::handlers::imports::config)),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await.to_vec();
    (status, json(&body), body)
}

/// A zip of `files`.
pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("start file");
        w.write_all(bytes).expect("write file");
    }
    w.finish().expect("finish zip").into_inner()
}

/// A multipart upload of `files` (name, bytes) as a documentation import.
pub(crate) fn upload_files(files: &[(&str, &[u8])]) -> TestRequest {
    let mut payload = Vec::new();
    for (name, bytes) in files {
        payload.extend_from_slice(
            format!(
                "--x\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        payload.extend_from_slice(bytes);
        payload.extend_from_slice(b"\r\n");
    }
    payload.extend_from_slice(b"--x--\r\n");
    TestRequest::post()
        .uri("/api/admin/import?type=documentation")
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .set_payload(payload)
}

pub(crate) fn id_of(job: &serde_json::Value) -> String {
    job["id"].as_str().expect("job id").to_string()
}

/// Upload `zip` as `filename` and return the job.
pub(crate) async fn uploaded(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    filename: &str,
    zip: &[u8],
) -> serde_json::Value {
    let (status, job, _) = call_docs(
        pool,
        ws,
        ws.admin_uuid,
        upload_files(&[(filename, zip)]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "parsed", "{job}");
    job
}

/// The job, read until it leaves `status` (a background run's).
pub(crate) async fn wait_out_of(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    status: &str,
) -> serde_json::Value {
    for _ in 0..1200 {
        let (code, job, _) = call_docs(
            pool,
            ws,
            ws.admin_uuid,
            TestRequest::get().uri(&format!("/api/admin/import/{id}")),
            None,
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{job}");
        if job["status"] != status {
            return job;
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("job {id} stayed in {status}");
}

pub(crate) fn options_request(id: &str, options: serde_json::Value, check: bool) -> TestRequest {
    TestRequest::put()
        .uri(&format!("/api/admin/import/{id}/docs/options"))
        .set_json(serde_json::json!({ "options": options, "check": check }))
}

/// Check job `id` under `options` and return it checked.
pub(crate) async fn checked(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    options: serde_json::Value,
) -> serde_json::Value {
    let (status, job, _) = call_docs(
        pool,
        ws,
        ws.admin_uuid,
        options_request(id, options, true),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["status"], "checking", "{job}");
    let job = wait_out_of(pool, ws, id, "checking").await;
    assert_eq!(job["status"], "dry_run_done", "{job}");
    job
}

pub(crate) fn commit_request(id: &str, body: serde_json::Value) -> TestRequest {
    TestRequest::post()
        .uri(&format!("/api/admin/import/{id}/docs/commit"))
        .set_json(body)
}

/// Commit job `id` and return it once the background commit is through.
pub(crate) async fn committed(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    id: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let (status, job, _) = call_docs(pool, ws, ws.admin_uuid, commit_request(id, body), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["status"], "committing", "{job}");
    wait_out_of(pool, ws, id, "committing").await
}

/// Run `sql` binding the job id, pinned to the workspace.
pub(crate) fn on_job(pool: &TestPool, ws: &WorkspaceSeed, sql: &str, id: &str) {
    let id = Uuid::parse_str(id).expect("uuid");
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(sql).bind::<SqlUuid, _>(id).execute(c)
    })
    .expect(sql);
}

pub(crate) fn pages_request(id: &str, query: &str) -> TestRequest {
    TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/pages{query}"))
}

/// A small handbook: a page with a link, an archive image and a `data:`
/// image; a section with two pages linking back.
pub(crate) fn handbook() -> Vec<u8> {
    let data = format!(
        "data:image/png;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, PNG)
    );
    let start = format!(
        "# Getting started\n\nRead [the VPN guide](network/vpn.md).\n\n![Logo](images/logo.png)\n\n![Dot]({data})\n"
    );
    zip_of(&[
        ("getting-started.md", start.as_bytes()),
        ("network/index.md", b"# Network\n\nAbout the network.\n"),
        (
            "network/vpn.md",
            b"---\ntitle: VPN setup\n---\n\nConnect with the client. Back to [the start](../getting-started.md).\n",
        ),
        ("network/wifi.md", b"# Wi-Fi\n\nJoin the office network.\n"),
        ("images/logo.png", PNG),
    ])
}

/// Every route a documentation import has, for job `id`.
fn every_route(id: &str) -> Vec<TestRequest> {
    vec![
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        options_request(id, serde_json::json!({}), true),
        pages_request(id, ""),
        TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/pages/0/preview")),
        TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/report")),
        commit_request(id, serde_json::json!({})),
    ]
}

/// Only a workspace admin may upload, check, read, preview or commit.
#[actix_web::test]
async fn only_admins_reach_documentation_imports() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    let (status, _, _) = call_docs(
        &pool,
        ws,
        ws.member_uuid,
        upload_files(&[("handbook.zip", &handbook())]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let job = uploaded(&pool, ws, "handbook.zip", &handbook()).await;
    let id = id_of(&job);
    for req in every_route(&id) {
        let (status, body, _) = call_docs(&pool, ws, ws.member_uuid, req, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
}

/// Another workspace's import is not found on every route.
#[actix_web::test]
async fn another_workspaces_import_is_not_found() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let job = uploaded(&pool, &seeded.a, "handbook.zip", &handbook()).await;
    let id = id_of(&job);
    let job = checked(&pool, &seeded.a, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["would_create"], 4, "{job}");

    for req in every_route(&id) {
        let (status, body, _) = call_docs(&pool, &seeded.b, seeded.b.admin_uuid, req, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
}

/// An upload past the cap stops at the first chunk past it: refused, with
/// no job and no file kept.
#[actix_web::test]
async fn an_upload_past_the_cap_is_refused_mid_stream() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    // Bytes that don't compress.
    let mut state: u32 = 0x1234_5678;
    let big: Vec<u8> = (0..64 * 1024)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    let zip = zip_of(&[("a.md", b"# A\n"), ("noise.bin", &big)]);
    let (status, body, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        upload_files(&[("big.zip", &zip)]),
        Some(4 * 1024),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "upload_too_large", "{body}");

    let (status, list, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["jobs"], serde_json::json!([]), "{list}");

    // The same file under the real cap is read.
    let job = uploaded(&pool, ws, "big.zip", &zip).await;
    assert_eq!(job["source"]["tree"]["pages"][0]["title"], "A", "{job}");
}

/// An upload is read into a tree on the spot, or refused with a code the
/// client words: not a zip, no Markdown. Loose `.md` files are gathered.
#[actix_web::test]
async fn uploads_are_read_into_a_tree_or_refused() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    let (status, body, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        upload_files(&[("handbook.zip", b"not a zip at all")]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "archive_unreadable", "{body}");

    let (status, body, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        upload_files(&[("pictures.zip", &zip_of(&[("a.png", PNG)]))]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "docs_import_no_markdown", "{body}");

    let (status, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        upload_files(&[
            ("vpn.md", b"# VPN\n\nConnect.\n"),
            ("printers.md", b"# Printers\n\nPrint.\n"),
        ]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["job_type"], "documentation");
    let titles: Vec<&str> = job["source"]["tree"]["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .map(|p| p["title"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(titles, vec!["Printers", "VPN"], "{job}");

    // The tree as Pages shows it, before any check.
    let job = uploaded(&pool, ws, "handbook.zip", &handbook()).await;
    let tree = &job["source"]["tree"];
    assert_eq!(tree["profile"], "plain", "{tree}");
    assert_eq!(tree["images"], 1, "{tree}");
    let pages = tree["pages"].as_array().expect("pages");
    let titles: Vec<&str> = pages.iter().map(|p| p["title"].as_str().unwrap()).collect();
    assert_eq!(
        titles,
        vec!["Getting started", "Network", "VPN setup", "Wi-Fi"]
    );
    assert_eq!(pages[2]["parent"], 1);
    assert_eq!(pages[2]["path"], "network/vpn.md");
    assert_eq!(job["state"], "draft");
    let files = job_files(ws.workspace_id, &id_of(&job)).await;
    assert_eq!(files, vec!["upload.zip"]);
}

/// A draft goes Pages, Check, Review and back, and every state reads back
/// from the job: options re-read the tree without a check, the check runs
/// in the background, pages, previews and the report follow it, and a check
/// whose process died is put back.
#[actix_web::test]
async fn a_draft_resumes_through_its_states() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let job = uploaded(&pool, ws, "handbook.zip", &handbook()).await;
    let id = id_of(&job);

    // Pages: leave a page out; the tree is read again, no check.
    let (status, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        options_request(
            &id,
            serde_json::json!({ "leave_out": ["**/wifi.md"] }),
            false,
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["status"], "parsed");
    assert_eq!(job["source"]["tree"]["pages"].as_array().unwrap().len(), 3);
    assert_eq!(job["options"]["leave_out"][0], "**/wifi.md");

    // Not checked yet.
    let (status, _, _) = call_docs(&pool, ws, ws.admin_uuid, pages_request(&id, ""), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        commit_request(&id, serde_json::json!({})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Options that can't go together are refused.
    let (status, _, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        options_request(&id, serde_json::json!({ "collections": "existing" }), true),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The check.
    let job = checked(
        &pool,
        ws,
        &id,
        serde_json::json!({ "collection_name": "IT Handbook" }),
    )
    .await;
    let summary = &job["summary"];
    assert_eq!(summary["row_count"], 4, "{summary}");
    assert_eq!(summary["would_create"], 4);
    assert_eq!(summary["skipped"], 0);
    assert_eq!(summary["images"], 2, "{summary}");
    assert_eq!(summary["links_rewritten"], 2, "{summary}");
    assert_eq!(summary["destination"]["new_collections"][0], "IT Handbook");
    assert_eq!(summary["status"], "drafts");
    assert_eq!(job["state"], "draft");

    let (status, list, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        pages_request(&id, "?limit=2"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["total"], 4);
    assert_eq!(list["pages"].as_array().unwrap().len(), 2);
    assert_eq!(list["pages"][0]["outcome"], "new");
    let images = list["pages"][0]["images"].as_array().expect("images");
    assert_eq!(images.len(), 2, "{list}");
    assert!(images.iter().all(|i| i["problem"].is_null()), "{list}");
    assert!(
        !list.to_string().contains("base64"),
        "no data URI in the rows"
    );

    let (status, preview, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/pages/0/preview")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["doc"]["type"], "doc");
    assert_eq!(preview["title"], "Getting started");
    let text = preview["doc"].to_string();
    assert!(text.contains("nosdesk-import:page/2"), "{text}");
    assert!(!text.contains("data:"), "{text}");
    let names: Vec<&str> = preview["images"]
        .as_object()
        .unwrap()
        .values()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"logo.png"), "{preview}");

    let (status, _, csv) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/report")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csv = String::from_utf8(csv).unwrap();
    assert!(csv.contains("Path,Title,Outcome,Notes"), "{csv}");
    assert!(csv.contains("network/vpn.md,VPN setup,New"), "{csv}");

    // Checked again with other options: the job follows.
    let job = checked(&pool, ws, &id, serde_json::json!({ "status": "published" })).await;
    assert_eq!(job["summary"]["status"], "published");

    // A check whose process died is put back to Pages.
    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET status = 'checking', updated_at = now() - interval '10 minutes' WHERE id = $1",
        &id,
    );
    let (status, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(job["status"], "parsed", "{job}");
    assert_eq!(
        job["error_message"],
        "The check stopped before it finished. Check the pages again."
    );
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["would_create"], 4);
}

/// The commit holds the plan to what Review showed: a page that turned up
/// in the workspace since the check, or a collection to import into that
/// went, stops it with the new plan and nothing written.
#[actix_web::test]
async fn a_changed_plan_is_refused_at_commit() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;

    let page_uuid = Uuid::now_v7();
    let zip = zip_of(&[
        (
            "policy.md",
            format!("---\ntitle: Policy\nuuid: {page_uuid}\n---\n\nBe kind.\n").as_bytes(),
        ),
        ("faq.md", b"# FAQ\n\nAsk away.\n"),
    ]);
    let id = id_of(&uploaded(&pool, ws, "policies.zip", &zip).await);
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["duplicates"], 0);

    // A page with that uuid appears.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO documentation_pages (uuid, title, slug, status, created_by, last_edited_by) \
             VALUES ($1, 'Policy', 'policy-existing', 'published', $2, $2)",
        )
        .bind::<SqlUuid, _>(page_uuid)
        .bind::<SqlUuid, _>(ws.admin_uuid)
        .execute(c)
    })
    .expect("insert page");

    let (status, body, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        commit_request(&id, serde_json::json!({})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_plan_changed", "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("1 more page is already in Nosdesk"),
        "{body}"
    );
    assert_eq!(body["job"]["status"], "dry_run_done");
    assert_eq!(body["job"]["summary"]["skipped"], 1, "{body}");
    assert_eq!(page_count(&pool, ws), 1, "nothing was written");

    // The new plan is the reviewed one now: it commits, skipping the page.
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 1);
    assert_eq!(page_count(&pool, ws), 2);

    // Into an existing collection that goes before the commit.
    let collection = new_collection(&pool, ws, "Policy library");
    let id = id_of(&uploaded(&pool, ws, "policies.zip", &zip).await);
    checked(
        &pool,
        ws,
        &id,
        serde_json::json!({ "collections": "existing", "collection_id": collection }),
    )
    .await;
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("DELETE FROM documentation_collections WHERE id = $1")
            .bind::<diesel::sql_types::Integer, _>(collection)
            .execute(c)
    })
    .expect("delete collection");
    let (status, body, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        commit_request(&id, serde_json::json!({})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_plan_changed");
    assert_eq!(page_count(&pool, ws), 2);
}

/// Thirty days on, the prune drops the zip, the previews, the downloaded
/// images and every page's detail; the counts and the record of what was
/// written stay, and the detail routes answer 410.
#[actix_web::test]
async fn the_prune_drops_the_files_and_page_detail() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = id_of(&uploaded(&pool, ws, "handbook.zip", &handbook()).await);
    checked(&pool, ws, &id, serde_json::json!({})).await;
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done");
    let files = job_files(ws.workspace_id, &id).await;
    assert!(files.contains(&"upload.zip".to_string()), "{files:?}");
    assert!(files.iter().any(|f| f.starts_with("preview/")), "{files:?}");

    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET completed_at = now() - interval '31 days', \
         updated_at = now() - interval '31 days' WHERE id = $1",
        &id,
    );
    let cutoff = chrono::Utc::now() - backend::services::imports::history::details_window();
    backend::services::scheduled_jobs::prune_import_history_before(&db.job_pool(), cutoff)
        .await
        .expect("prune");

    assert!(job_files(ws.workspace_id, &id).await.is_empty());
    for req in [
        pages_request(&id, ""),
        TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/pages/0/preview")),
        TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/report")),
    ] {
        let (status, body, _) = call_docs(&pool, ws, ws.admin_uuid, req, None).await;
        assert_eq!(status, StatusCode::GONE, "{body}");
    }
    let (_, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        None,
    )
    .await;
    assert_eq!(job["state"], "imported");
    assert_eq!(job["summary"]["would_create"], 4, "{job}");
    assert!(job["pruned_at"].is_string());
    let record: Option<serde_json::Value> = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        #[derive(QueryableByName)]
        struct Row {
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
            record_ids: Option<serde_json::Value>,
        }
        diesel::sql_query("SELECT record_ids FROM import_jobs WHERE id = $1")
            .bind::<SqlUuid, _>(Uuid::parse_str(&id).unwrap())
            .get_result::<Row>(c)
            .map(|r| r.record_ids)
    })
    .expect("record");
    assert_eq!(record.expect("kept")["pages"].as_array().unwrap().len(), 4);
}

/// Pages in the workspace, Trash included.
pub(crate) fn page_count(pool: &TestPool, ws: &WorkspaceSeed) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("SELECT count(*) AS n FROM documentation_pages").get_result::<Count>(c)
    })
    .expect("count")
    .n
}

/// A collection named `name`, by id.
pub(crate) fn new_collection(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = diesel::sql_types::Integer)]
        id: i32,
    }
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO documentation_collections (uuid, name, slug) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind::<SqlUuid, _>(Uuid::now_v7())
        .bind::<diesel::sql_types::Text, _>(name)
        .bind::<diesel::sql_types::Text, _>(name.to_lowercase().replace(' ', "-"))
        .get_result::<Id>(c)
    })
    .expect("collection")
    .id
}
