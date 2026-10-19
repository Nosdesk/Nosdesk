//! Reindexing one workspace's search documents
//! (`SearchService::reindex_workspace`): its rows are indexed again and its
//! deleted rows drop out, while every other workspace's documents, people
//! included, stay as they were.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::Value;

use backend::models::{Claims, NewTicket};
use backend::repository::workflow_states;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::search::indexing_tasks::{spawn_reindex_workspace, WorkspaceReindex};
use backend::services::search::workspace_reindex::Admission;
use backend::services::search::{SearchQuery, SearchService};
use backend::sync::session::run_in_workspace;

use crate::common;

const REF: &str = "test:workspace_search_reindex";

fn seed_ticket(pool: &common::TestPool, workspace_id: i32, title: &str) -> i32 {
    run_in_workspace(pool, REF, workspace_id, |c| {
        let state = workflow_states::default_state(c)?.id;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: state,
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)
    })
    .expect("seed ticket")
}

/// The ids of the documents `q` finds from `workspace_id`.
fn hits(search: &SearchService, workspace_id: i32, q: &str) -> Vec<String> {
    let query = SearchQuery {
        q: q.to_string(),
        limit: 50,
        types: None,
        sort: None,
        author: None,
    };
    search
        .search(&query, true, workspace_id as i64)
        .expect("search")
        .results
        .into_iter()
        .map(|r| r.id)
        .collect()
}

#[actix_web::test]
async fn reindexing_one_workspace_leaves_the_others_alone() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seed_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut seed_pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);

    // A's admin belongs to B as well, so their one document carries both.
    run_in_workspace(&seed_pool, REF, b, |c| {
        add_membership(
            c,
            b,
            seeded.a.admin_uuid,
            "member",
            SeatWriteAuthority::ControlPlane,
        )
    })
    .expect("add A's admin to B");

    let kept_a = seed_ticket(&seed_pool, a, "Printer jammed");
    let deleted_a = seed_ticket(&seed_pool, a, "Projector overheating");
    let kept_b = seed_ticket(&seed_pool, b, "Laptop won't boot");

    let pool = db.runtime_pool(2);
    let index_dir = tempfile::tempdir().expect("index dir");
    // An empty index is built whole on start.
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));

    // Rows written past the index hooks, as an import writes them.
    let imported_a = seed_ticket(&seed_pool, a, "Scanner offline");
    seed_ticket(&seed_pool, b, "Monitor flickering");
    run_in_workspace(&seed_pool, REF, a, |c| {
        diesel::delete(backend::schema::tickets::table.find(deleted_a)).execute(c)
    })
    .expect("delete ticket");

    let stats = match spawn_reindex_workspace(search.clone(), a) {
        WorkspaceReindex::Started(handle) => handle.await.expect("join").expect("reindex"),
        WorkspaceReindex::Joined => panic!("nothing else was reindexing A"),
    };
    assert_eq!(stats.indexed.tickets, 2, "{stats:?}");
    assert_eq!(stats.removed, 1, "{stats:?}");
    search.reload().expect("reload");

    let ticket = |id: i32| format!("ticket-{id}");
    let user = |uuid: uuid::Uuid| format!("user-{uuid}");

    // A: the imported ticket is found, the deleted one is gone, the rest stays.
    assert_eq!(hits(&search, a, "Scanner"), vec![ticket(imported_a)]);
    assert!(hits(&search, a, "Projector").is_empty());
    assert_eq!(hits(&search, a, "Printer"), vec![ticket(kept_a)]);

    // B: untouched. Its indexed ticket is still found, and the row written
    // past the hooks is not, because B was not reindexed.
    assert_eq!(hits(&search, b, "Laptop"), vec![ticket(kept_b)]);
    assert!(hits(&search, b, "Monitor").is_empty(), "B was reindexed");

    // People: A's admin is still found from both workspaces, B's only from B.
    let from_b = hits(&search, b, "Admin");
    assert!(from_b.contains(&user(seeded.a.admin_uuid)), "{from_b:?}");
    assert!(from_b.contains(&user(seeded.b.admin_uuid)), "{from_b:?}");
    let from_a = hits(&search, a, "Admin");
    assert!(from_a.contains(&user(seeded.a.admin_uuid)), "{from_a:?}");
    assert!(!from_a.contains(&user(seeded.b.admin_uuid)), "{from_a:?}");
}

#[actix_web::test]
async fn a_second_request_for_the_same_workspace_joins_the_first() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seed_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut seed_pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let pool = db.runtime_pool(2);
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    let ticket = seed_ticket(&seed_pool, a, "Scanner offline");

    // A request admitted and not yet reading; the next two join it.
    assert_eq!(search.admit_workspace_reindex(a), Admission::Start);
    assert!(matches!(
        spawn_reindex_workspace(search.clone(), a),
        WorkspaceReindex::Joined
    ));
    assert!(search.reindex_workspace(a).expect("reindex").is_none());
    // Another workspace is not held up.
    assert!(search.reindex_workspace(b).expect("reindex B").is_some());

    // The one pass covers all three requests, then lets go.
    let stats = search
        .run_admitted_workspace_reindex(a)
        .expect("run admitted");
    assert_eq!(stats.indexed.tickets, 1);
    search.reload().expect("reload");
    assert_eq!(
        hits(&search, a, "Scanner"),
        vec![format!("ticket-{ticket}")]
    );

    // Two requests back to back: the first starts, the second joins it.
    let first = spawn_reindex_workspace(search.clone(), a);
    let second = spawn_reindex_workspace(search.clone(), a);
    assert!(matches!(second, WorkspaceReindex::Joined));
    match first {
        WorkspaceReindex::Started(handle) => {
            handle.await.expect("join").expect("reindex");
        }
        WorkspaceReindex::Joined => panic!("the first request should start"),
    }
}

#[actix_web::test]
async fn the_rebuild_route_reindexes_one_workspace() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seed_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut seed_pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    seed_ticket(&seed_pool, a, "Printer jammed");
    seed_ticket(&seed_pool, b, "Laptop won't boot");

    let pool = db.runtime_pool(2);
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: seeded.a.admin_uuid.to_string(),
        name: "Operator".to_string(),
        email: "operator@example.com".to_string(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(claims.clone());
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::search::config)),
    )
    .await;

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/search/rebuild?workspace_id={a}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["workspace_id"], a, "{body}");
    assert_eq!(body["stats"]["tickets"], 1, "{body}");
    assert_eq!(body["stats"]["removed"], 0, "{body}");
    assert!(!search.is_rebuilding());
}
