//! Remove imported pages (`/admin/import/{id}/docs/remove`) and Publish
//! all (`/admin/import/{id}/docs/publish`): each reason a page is kept,
//! the collection the import created deleted when it ends up empty and
//! kept when not, once only, the 30-day window, the publish counts and the
//! untouched rule, who may call them, and restoring from Trash after.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::{import_workspaces, json};
use crate::imports_docs_flow::{checked, committed, handbook, id_of, on_job, uploaded, zip_of};

const REF: &str = "test:imports_docs_remove";

/// One request as `user` in `ws`, to the import routes and the
/// documentation routes (for restoring from Trash).
async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    use actix_web::dev::Service;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};
    use std::sync::Arc;

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::services::search::SearchService;
    use backend::sync::actor::ActorContext;

    static SEARCH_DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
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
    let search = Arc::new(
        SearchService::new(&search_dir.path().join(Uuid::now_v7().to_string()), pool)
            .expect("init search"),
    );
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .app_data(web::Data::new(crate::common::test_storage()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::imports::config)
                    .configure(backend::handlers::documentation::config),
            ),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await.to_vec();
    (status, json(&body))
}

fn remove_preview(id: &str) -> TestRequest {
    TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/remove"))
}

fn remove(id: &str) -> TestRequest {
    TestRequest::post().uri(&format!("/api/admin/import/{id}/docs/remove"))
}

fn publish_preview(id: &str) -> TestRequest {
    TestRequest::get().uri(&format!("/api/admin/import/{id}/docs/publish"))
}

fn publish(id: &str) -> TestRequest {
    TestRequest::post().uri(&format!("/api/admin/import/{id}/docs/publish"))
}

/// Import `zip` as `filename` into a new collection and return the job id.
async fn imported(pool: &TestPool, ws: &WorkspaceSeed, filename: &str, zip: &[u8]) -> String {
    let id = id_of(&uploaded(pool, ws, filename, zip).await);
    checked(pool, ws, &id, serde_json::json!({})).await;
    let job = committed(pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    id
}

/// Run `sql`, pinned to the workspace.
fn exec(pool: &TestPool, ws: &WorkspaceSeed, sql: &str) {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(sql).execute(c)
    })
    .expect(sql);
}

fn count(pool: &TestPool, ws: &WorkspaceSeed, sql: &str) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(sql).get_result::<Count>(c)
    })
    .expect(sql)
    .n
}

#[derive(QueryableByName, Debug)]
struct Page {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = Nullable<Integer>)]
    collection_id: Option<i32>,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    in_trash: bool,
}

/// The page titled `title`.
fn page(pool: &TestPool, ws: &WorkspaceSeed, title: &str) -> Page {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT p.id, p.status::text AS status, cp.collection_id, \
                    (p.deleted_at IS NOT NULL) AS in_trash \
             FROM documentation_pages p \
             LEFT JOIN documentation_collection_pages cp ON cp.page_id = p.id \
             WHERE p.title = $1",
        )
        .bind::<Text, _>(title)
        .get_result::<Page>(c)
    })
    .unwrap_or_else(|e| panic!("page {title}: {e}"))
}

/// The name the workspace knows `user` by.
fn name_of(pool: &TestPool, user: Uuid) -> String {
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    diesel::sql_query("SELECT name FROM users WHERE uuid = $1")
        .bind::<SqlUuid, _>(user)
        .get_result::<Name>(&mut pool.get().expect("conn"))
        .expect("user")
        .name
}

/// A page outside any import, under `parent` when given, in `collection`
/// when given.
fn page_outside(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    title: &str,
    parent: Option<i32>,
    collection: Option<i32>,
) -> i32 {
    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = Integer)]
        id: i32,
    }
    let id = run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO documentation_pages (uuid, title, slug, status, created_by, last_edited_by, parent_id) \
             VALUES ($1, $2, $3, 'draft', $4, $4, $5) RETURNING id",
        )
        .bind::<SqlUuid, _>(Uuid::now_v7())
        .bind::<Text, _>(title)
        .bind::<Text, _>(title.to_lowercase().replace(' ', "-"))
        .bind::<SqlUuid, _>(ws.member_uuid)
        .bind::<Nullable<Integer>, _>(parent)
        .get_result::<Id>(c)
    })
    .expect("page")
    .id;
    if let Some(collection) = collection {
        exec(
            pool,
            ws,
            &format!(
                "INSERT INTO documentation_collection_pages (collection_id, page_id) VALUES ({collection}, {id})"
            ),
        );
    }
    id
}

/// The kept entry for `title` in a plan or result.
fn kept<'a>(body: &'a serde_json::Value, title: &str) -> &'a serde_json::Value {
    body["kept"]
        .as_array()
        .expect("kept list")
        .iter()
        .find(|k| k["title"] == title)
        .unwrap_or_else(|| panic!("{title} isn't kept: {body}"))
}

fn codes(kept: &serde_json::Value) -> Vec<String> {
    kept["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|r| r["code"].as_str().expect("code").to_string())
        .collect()
}

/// Untouched pages go to Trash with a sync event each and leave the search
/// index; the collection the import created, empty after, is deleted; the
/// job records when, by whom and what; Recent imports counts it; and a
/// page restores from Trash as usual.
#[actix_web::test]
async fn untouched_pages_go_to_trash_and_the_empty_collection_goes() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = imported(&pool, ws, "IT-Handbook.zip", &handbook()).await;
    let collection = page(&pool, ws, "Network")
        .collection_id
        .expect("in a collection");

    let (status, plan) = call(&pool, ws, ws.admin_uuid, remove_preview(&id)).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["created"], 4, "{plan}");
    assert_eq!(plan["deletable"], 4);
    assert_eq!(plan["gone"], 0);
    assert_eq!(plan["kept"], serde_json::json!([]));
    assert_eq!(plan["collections"]["deleted"][0]["id"], collection);
    assert_eq!(plan["collections"]["deleted"][0]["name"], "IT-Handbook");
    assert!(plan["closes_at"].is_string(), "{plan}");
    let before = count(
        &pool,
        ws,
        "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_page.metadata_changed'",
    );

    let (status, job) = call(&pool, ws, ws.admin_uuid, remove(&id)).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert!(job["removed_at"].is_string(), "{job}");
    assert_eq!(job["removed_by"], ws.admin_uuid.to_string());
    assert!(job["removal_closes_at"].is_string(), "{job}");
    let result = &job["remove_result"];
    assert_eq!(result["kind"], "documentation", "{result}");
    assert_eq!(result["created"], 4);
    assert_eq!(result["deleted"], 4);
    assert_eq!(result["deleted_ids"].as_array().unwrap().len(), 4);
    assert_eq!(result["collections"]["deleted"][0]["id"], collection);

    for title in ["Getting started", "Network", "VPN setup", "Wi-Fi"] {
        let p = page(&pool, ws, title);
        assert_eq!(p.status, "deleted", "{title}");
        assert!(p.in_trash, "{title}");
        assert_eq!(p.collection_id, None, "{title}: its collection is gone");
    }
    assert_eq!(
        count(
            &pool,
            ws,
            &format!("SELECT count(*) AS n FROM documentation_collections WHERE id = {collection}")
        ),
        0
    );
    assert_eq!(
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_page.metadata_changed'",
        ) - before,
        4,
        "one event per page moved"
    );
    assert_eq!(
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_collection.deleted'",
        ),
        1
    );

    // Recent imports shows how many were removed.
    let (_, list) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri("/api/admin/import?limit=20"),
    )
    .await;
    let row = list["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == id.as_str())
        .expect("listed");
    assert_eq!(
        row["removed"],
        serde_json::json!({ "deleted": 4, "created": 4 })
    );

    // A page restores from Trash as usual.
    let vpn = page(&pool, ws, "VPN setup").id;
    let (status, restored) = call(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::post().uri(&format!("/api/documentation/pages/{vpn}/restore")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    let p = page(&pool, ws, "VPN setup");
    assert_eq!(p.status, "draft");
    assert!(!p.in_trash);
}

/// Each change keeps its page, with its reason: an edit to its details
/// (with who made it), to its content, a newer revision, a page added
/// under it, a move to another collection, and a kept page under it. The
/// untouched pages go, and the collection, still holding pages, stays.
#[actix_web::test]
async fn each_change_keeps_its_page_with_the_reason() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let zip = zip_of(&[
        ("alpha.md", b"# Alpha\n\nUntouched.\n"),
        ("bravo.md", b"# Bravo\n\nRenamed.\n"),
        ("charlie.md", b"# Charlie\n\nContent changed.\n"),
        ("delta.md", b"# Delta\n\nA newer revision.\n"),
        ("echo.md", b"# Echo\n\nMoved.\n"),
        ("section/index.md", b"# Section\n\nA page added under it.\n"),
        ("section/foxtrot.md", b"# Foxtrot\n\nUntouched.\n"),
        ("golf/index.md", b"# Golf\n\nA kept page under it.\n"),
        ("golf/hotel.md", b"# Hotel\n\nRenamed.\n"),
    ]);
    let id = imported(&pool, ws, "Phonetic.zip", &zip).await;
    let collection = page(&pool, ws, "Alpha").collection_id.expect("collection");
    let member = ws.member_uuid;

    // Details: who did it is recorded on the page.
    exec(
        &pool,
        ws,
        &format!("UPDATE documentation_pages SET title = 'Bravo!', last_edited_by = '{member}' WHERE title = 'Bravo'"),
    );
    exec(
        &pool,
        ws,
        "UPDATE documentation_pages SET title = 'Hotel!' WHERE title = 'Hotel'",
    );
    // Content only, with `updated_at` put back as it was.
    let charlie = page(&pool, ws, "Charlie").id;
    exec(
        &pool,
        ws,
        &format!(
            "UPDATE documentation_pages SET yjs_document = '\\x00'::bytea WHERE id = {charlie}"
        ),
    );
    exec(
        &pool,
        ws,
        &format!(
            "UPDATE documentation_pages p SET updated_at = (r.value->>'updated_at')::timestamp AT TIME ZONE 'UTC' \
             FROM import_jobs j, jsonb_array_elements(j.record_ids->'pages') r \
             WHERE j.id = '{id}' AND (r.value->>'id')::int = p.id AND p.id = {charlie}"
        ),
    );
    // A revision after the import's, by the member.
    let delta = page(&pool, ws, "Delta").id;
    exec(
        &pool,
        ws,
        &format!(
            "INSERT INTO documentation_revisions (page_id, revision_number, title, yjs_document_snapshot, yjs_state_vector, created_by) \
             VALUES ({delta}, 2, 'Delta', '\\x00'::bytea, '\\x00'::bytea, '{member}')"
        ),
    );
    // A page added under Section.
    let section = page(&pool, ws, "Section").id;
    page_outside(&pool, ws, "Added later", Some(section), Some(collection));
    // Echo moved to another collection.
    let other = crate::imports_docs_flow::new_collection(&pool, ws, "Elsewhere");
    let echo = page(&pool, ws, "Echo").id;
    exec(
        &pool,
        ws,
        &format!("UPDATE documentation_collection_pages SET collection_id = {other} WHERE page_id = {echo}"),
    );

    let (status, plan) = call(&pool, ws, ws.admin_uuid, remove_preview(&id)).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["created"], 9, "{plan}");
    assert_eq!(plan["deletable"], 2, "Alpha and Foxtrot: {plan}");

    let bravo = kept(&plan, "Bravo!");
    assert_eq!(codes(bravo), vec!["edited"]);
    assert_eq!(bravo["reasons"][0]["by"], member.to_string());
    assert_eq!(bravo["reasons"][0]["by_name"], name_of(&pool, member));
    assert_eq!(codes(kept(&plan, "Charlie")), vec!["edited"]);
    assert!(kept(&plan, "Charlie")["reasons"][0].get("by").is_none());
    let delta_kept = kept(&plan, "Delta");
    assert_eq!(codes(delta_kept), vec!["edited"]);
    assert_eq!(delta_kept["reasons"][0]["by"], member.to_string());
    let section_kept = kept(&plan, "Section");
    assert_eq!(codes(section_kept), vec!["pages_added"]);
    assert_eq!(section_kept["reasons"][0]["count"], 1);
    assert_eq!(codes(kept(&plan, "Echo")), vec!["moved"]);
    assert_eq!(codes(kept(&plan, "Hotel!")), vec!["edited"]);
    let golf = kept(&plan, "Golf");
    assert_eq!(codes(golf), vec!["kept_pages_under"]);
    assert_eq!(golf["reasons"][0]["count"], 1);
    assert_eq!(plan["kept"].as_array().unwrap().len(), 7);
    assert_eq!(plan["collections"]["deleted"], serde_json::json!([]));
    assert_eq!(plan["collections"]["kept"][0]["id"], collection);
    // Six kept in it (Echo moved out) and the page added under Section.
    assert_eq!(plan["collections"]["kept"][0]["pages"], 7);

    let (status, job) = call(&pool, ws, ws.admin_uuid, remove(&id)).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let result = &job["remove_result"];
    assert_eq!(result["deleted"], 2, "{result}");
    assert_eq!(
        kept(result, "Bravo!")["reasons"][0]["by_name"],
        name_of(&pool, member),
        "the job names the editors too"
    );
    for title in ["Alpha", "Foxtrot"] {
        assert_eq!(page(&pool, ws, title).status, "deleted", "{title}");
    }
    for title in [
        "Bravo!", "Charlie", "Delta", "Echo", "Section", "Golf", "Hotel!",
    ] {
        assert_eq!(page(&pool, ws, title).status, "draft", "{title}");
    }
    assert_eq!(page(&pool, ws, "Added later").status, "draft");
    assert_eq!(
        count(
            &pool,
            ws,
            &format!("SELECT count(*) AS n FROM documentation_collections WHERE id = {collection}")
        ),
        1,
        "the collection still holds pages"
    );
}

/// A collection the import created stays when a page from outside the
/// import joined it, even with every imported page in Trash.
#[actix_web::test]
async fn a_collection_with_other_pages_is_kept() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = imported(&pool, ws, "IT-Handbook.zip", &handbook()).await;
    let collection = page(&pool, ws, "Network")
        .collection_id
        .expect("collection");
    page_outside(&pool, ws, "Printers", None, Some(collection));

    let (status, job) = call(&pool, ws, ws.admin_uuid, remove(&id)).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let result = &job["remove_result"];
    assert_eq!(result["deleted"], 4, "{result}");
    assert_eq!(result["collections"]["deleted"], serde_json::json!([]));
    assert_eq!(result["collections"]["kept"][0]["id"], collection);
    assert_eq!(result["collections"]["kept"][0]["pages"], 1);
    assert_eq!(page(&pool, ws, "Printers").collection_id, Some(collection));
    assert_eq!(page(&pool, ws, "Network").collection_id, Some(collection));
    assert_eq!(page(&pool, ws, "Network").status, "deleted");
}

/// Removal runs once, and only within 30 days of the commit; with every
/// page changed there is nothing to remove and nothing is recorded.
#[actix_web::test]
async fn removal_runs_once_within_thirty_days() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = imported(&pool, ws, "IT-Handbook.zip", &handbook()).await;

    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET completed_at = now() - interval '31 days' WHERE id = $1",
        &id,
    );
    for req in [remove_preview(&id), remove(&id)] {
        let (status, body) = call(&pool, ws, ws.admin_uuid, req).await;
        assert_eq!(status, StatusCode::GONE, "{body}");
        assert_eq!(body["code"], "import_removal_window_closed", "{body}");
    }
    assert_eq!(page(&pool, ws, "Network").status, "draft");

    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET completed_at = now() - interval '29 days' WHERE id = $1",
        &id,
    );
    let (status, job) = call(&pool, ws, ws.admin_uuid, remove(&id)).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    for req in [remove_preview(&id), remove(&id)] {
        let (status, body) = call(&pool, ws, ws.admin_uuid, req).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], "import_already_removed", "{body}");
    }

    // Every page changed: nothing to remove, and removal stays open.
    let other = imported(
        &pool,
        ws,
        "Two.zip",
        &zip_of(&[("one.md", b"# One\n"), ("two.md", b"# Two\n")]),
    )
    .await;
    exec(
        &pool,
        ws,
        "UPDATE documentation_pages SET title = title || '!' WHERE title IN ('One', 'Two')",
    );
    let (status, body) = call(&pool, ws, ws.admin_uuid, remove(&other)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_nothing_to_remove", "{body}");
    assert_eq!(body["args"]["reason"], "all_changed", "{body}");
    assert_eq!(
        count(
            &pool,
            ws,
            &format!(
                "SELECT count(*) AS n FROM import_jobs WHERE id = '{other}' AND removed_at IS NULL"
            )
        ),
        1
    );
}

/// Publish all publishes the drafts nobody changed, a status change with a
/// sync event each, and leaves a changed page as it is; a second run finds
/// nothing; and Remove afterward still sees the published pages untouched.
#[actix_web::test]
async fn publish_all_publishes_untouched_drafts_and_remove_still_sees_them() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = imported(&pool, ws, "IT-Handbook.zip", &handbook()).await;
    exec(
        &pool,
        ws,
        "UPDATE documentation_pages SET title = 'Getting started!' WHERE title = 'Getting started'",
    );

    let (status, plan) = call(&pool, ws, ws.admin_uuid, publish_preview(&id)).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan, serde_json::json!({ "publishable": 3, "left": 1 }));
    let before = count(
        &pool,
        ws,
        "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_page.metadata_changed'",
    );

    let (status, done) = call(&pool, ws, ws.admin_uuid, publish(&id)).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done, serde_json::json!({ "published": 3, "left": 1 }));
    for title in ["Network", "VPN setup", "Wi-Fi"] {
        assert_eq!(page(&pool, ws, title).status, "published", "{title}");
    }
    assert_eq!(page(&pool, ws, "Getting started!").status, "draft");
    assert_eq!(
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_page.metadata_changed'",
        ) - before,
        3
    );

    let (_, again) = call(&pool, ws, ws.admin_uuid, publish(&id)).await;
    assert_eq!(again, serde_json::json!({ "published": 0, "left": 1 }));

    let (status, plan) = call(&pool, ws, ws.admin_uuid, remove_preview(&id)).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(
        plan["deletable"], 3,
        "published by the import, not changed: {plan}"
    );
    assert_eq!(codes(kept(&plan, "Getting started!")), vec!["edited"]);
}

/// Remove and Publish all are open only to an import that was committed.
#[actix_web::test]
async fn an_uncommitted_import_has_nothing_to_publish_or_remove() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = id_of(&uploaded(&pool, ws, "IT-Handbook.zip", &handbook()).await);
    for req in [
        remove_preview(&id),
        remove(&id),
        publish_preview(&id),
        publish(&id),
    ] {
        let (status, body) = call(&pool, ws, ws.admin_uuid, req).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], "import_not_committed", "{body}");
    }
}

/// Only a workspace admin may remove or publish, and another workspace's
/// import is not found.
#[actix_web::test]
async fn only_admins_of_the_workspace_reach_remove_and_publish() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = imported(&pool, ws, "IT-Handbook.zip", &handbook()).await;
    let routes = || {
        [
            remove_preview(&id),
            remove(&id),
            publish_preview(&id),
            publish(&id),
        ]
    };
    for req in routes() {
        let (status, body) = call(&pool, ws, ws.member_uuid, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    for req in routes() {
        let (status, body) = call(&pool, &seeded.b, seeded.b.admin_uuid, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    }
    for title in ["Getting started", "Network", "VPN setup", "Wi-Fi"] {
        assert_eq!(page(&pool, ws, title).status, "draft", "{title}");
    }
}
