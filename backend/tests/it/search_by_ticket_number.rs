//! Global search finds a ticket by its number, "2" or "#2", in the request's
//! workspace, for a caller who can see the ticket. The index holds titles and
//! text, not numbers, so these hits come from the number itself.
#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::Value;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket};
use backend::repository::{tickets, workflow_states};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:search_by_ticket_number";

fn open(pool: &TestPool, ws: i32, title: &str, requester: Option<Uuid>) -> Ticket {
    run_in_workspace(pool, REF, ws, |c| {
        let state = workflow_states::default_state(c)?.id;
        tickets::create_ticket(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: state,
                requester_uuid: requester,
                ..Default::default()
            },
        )
    })
    .expect("open ticket")
}

/// The ids of the ticket hits `user` gets for `q` in workspace `ws`, with
/// the link each carries.
async fn ticket_hits(
    pool: &TestPool,
    search: &Arc<SearchService>,
    ws: &WorkspaceSeed,
    user: Uuid,
    q: &str,
) -> Vec<(i64, String)> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Searcher".to_string(),
        email: "searcher@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let context = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let workspace_id = ws.workspace_id;
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search.clone()))
            .wrap_fn(move |req, srv| {
                let corr = Uuid::now_v7();
                let actor = ActorContext::user(user, Some(corr)).with_workspace(workspace_id);
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut().insert(context.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::search::config)),
    )
    .await;
    let uri = format!("/api/search?q={}", q.replace('#', "%23"));
    let body: Value = http_test::call_and_read_body_json(
        &app,
        http_test::TestRequest::get().uri(&uri).to_request(),
    )
    .await;
    body["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter(|r| r["entity_type"] == "ticket")
        .map(|r| {
            (
                r["entity_id"].as_i64().expect("entity_id"),
                r["url"].as_str().expect("url").to_string(),
            )
        })
        .collect()
}

#[actix_web::test]
async fn a_ticket_number_finds_the_ticket() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seed_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut seed_pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);

    // B opens tickets first, so A's numbers differ from its ids.
    for title in ["Laptop won't boot", "VPN drops", "Badge lost"] {
        open(&seed_pool, b.workspace_id, title, None);
    }
    let theirs = open(
        &seed_pool,
        a.workspace_id,
        "Printer jammed",
        Some(a.member_uuid),
    );
    let not_theirs = open(&seed_pool, a.workspace_id, "Payroll export", None);
    assert_eq!((theirs.number, not_theirs.number), (1, 2));

    let pool = db.runtime_pool(2);
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    let found = |id: i32, number: i32| vec![(i64::from(id), format!("/tickets/{number}"))];

    assert_eq!(
        ticket_hits(&pool, &search, a, a.admin_uuid, "#2").await,
        found(not_theirs.id, 2)
    );
    assert_eq!(
        ticket_hits(&pool, &search, a, a.admin_uuid, "2").await,
        found(not_theirs.id, 2)
    );
    assert!(
        ticket_hits(&pool, &search, a, a.admin_uuid, "#3")
            .await
            .is_empty(),
        "3 is only the other workspace's"
    );

    // A requester finds their own ticket by number, and not someone else's.
    assert_eq!(
        ticket_hits(&pool, &search, a, a.member_uuid, "#1").await,
        found(theirs.id, 1)
    );
    assert!(ticket_hits(&pool, &search, a, a.member_uuid, "#2")
        .await
        .is_empty());
}
