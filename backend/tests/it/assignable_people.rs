//! `GET /api/users/paginated?assignable=true` lists exactly the people who can
//! be assigned tickets (`assignees::is_assignable`), filtered and paginated by
//! the server. The assignee pickers ask for it, so an agent who sorts after
//! many requesters is still offered, and no requester ever is.
//!
//! Also the `role` filter: a comma list, in the words the people list and the
//! plugin API send.

use std::collections::HashSet;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::Value;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::repository::workspaces::{add_membership, remove_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:assignable_people";
/// More requesters than a picker's first page holds.
const REQUESTERS: usize = 60;

struct Fixture {
    _db: common::TestDb,
    pool: TestPool,
    ws: WorkspaceSeed,
    /// The workspace admin and the agents: everyone who can be assigned.
    assignable: HashSet<Uuid>,
    agents: HashSet<Uuid>,
}

fn seed() -> Fixture {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let two = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = two.a;
    let mut conn = pool.get().expect("conn");

    // Requesters sort first by name, the agents after all of them.
    let requesters: Vec<Uuid> = (0..REQUESTERS)
        .map(|i| common::insert_plain_user(&mut conn, &format!("Aa Requester {i:02}")))
        .collect();
    let agents: Vec<Uuid> = (0..3)
        .map(|i| common::insert_plain_user(&mut conn, &format!("Zz Agent {i}")))
        .collect();
    let removed_agent = common::insert_plain_user(&mut conn, "Zz Removed Agent");
    let elsewhere_agent = common::insert_plain_user(&mut conn, "Zz Agent Elsewhere");

    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        for user in &requesters {
            add_membership(
                c,
                ws.workspace_id,
                *user,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
        }
        for user in agents.iter().chain([&removed_agent]) {
            add_membership(
                c,
                ws.workspace_id,
                *user,
                "agent",
                SeatWriteAuthority::ControlPlane,
            )?;
        }
        remove_membership(
            c,
            ws.workspace_id,
            removed_agent,
            SeatWriteAuthority::ControlPlane,
        )?;
        Ok::<_, diesel::result::Error>(())
    })
    .expect("seed workspace a");
    run_in_workspace(&pool, REF, two.b.workspace_id, |c| {
        add_membership(
            c,
            two.b.workspace_id,
            elsewhere_agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )
    })
    .expect("seed workspace b");
    drop(conn);

    let agents: HashSet<Uuid> = agents.into_iter().collect();
    let mut assignable = agents.clone();
    assignable.insert(ws.admin_uuid);
    Fixture {
        _db: db,
        pool,
        ws,
        assignable,
        agents,
    }
}

/// `GET /api/users/paginated?{query}` as the workspace admin.
async fn people(f: &Fixture, query: &str) -> Value {
    let viewer = f.ws.admin_uuid;
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Admin".to_string(),
        email: "admin@assignable.test".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: f.ws.workspace_id,
        workspace_uuid: f.ws.workspace_uuid,
        slug: f.ws.slug.clone(),
        name: f.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(f.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(f.pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/api/users/paginated",
                web::get().to(backend::handlers::get_paginated_users),
            ),
    )
    .await;
    let uri = format!("/api/users/paginated?{query}");
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(&uri).to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    serde_json::from_str(&body).expect("json")
}

fn uuids(page: &Value) -> Vec<Uuid> {
    page["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|r| Uuid::parse_str(r["uuid"].as_str().expect("uuid")).expect("uuid"))
        .collect()
}

#[actix_web::test]
async fn assignable_lists_exactly_who_can_be_assigned() {
    let f = seed();
    let page = people(
        &f,
        "page=1&pageSize=50&sortField=name&sortDirection=asc&assignable=true",
    )
    .await;
    let got: HashSet<Uuid> = uuids(&page).into_iter().collect();
    assert_eq!(
        got, f.assignable,
        "the first page is every assignable person: {page}"
    );
    assert_eq!(page["total"], f.assignable.len());
}

#[actix_web::test]
async fn assignable_paginates_on_the_server() {
    let f = seed();
    let first = people(
        &f,
        "page=1&pageSize=2&sortField=name&sortDirection=asc&assignable=true",
    )
    .await;
    let second = people(
        &f,
        "page=2&pageSize=2&sortField=name&sortDirection=asc&assignable=true",
    )
    .await;
    assert_eq!(first["total"], f.assignable.len());
    assert_eq!(first["totalPages"], 2);
    let (a, b) = (uuids(&first), uuids(&second));
    assert_eq!((a.len(), b.len()), (2, 2));
    let all: HashSet<Uuid> = a.into_iter().chain(b).collect();
    assert_eq!(all, f.assignable);
}

#[actix_web::test]
async fn assignable_still_searches() {
    let f = seed();
    let page = people(&f, "page=1&pageSize=50&search=zz&assignable=true").await;
    let got: HashSet<Uuid> = uuids(&page).into_iter().collect();
    assert_eq!(
        got, f.agents,
        "a search for the agents' names finds the current agents only"
    );
}

#[actix_web::test]
async fn role_filter_takes_a_comma_list() {
    let f = seed();
    for query in [
        "role=admin,technician",
        "role=technician,admin",
        "role=admin,agent",
    ] {
        let page = people(&f, &format!("page=1&pageSize=50&{query}")).await;
        let got: HashSet<Uuid> = uuids(&page).into_iter().collect();
        assert_eq!(got, f.assignable, "{query}");
    }
    // The plugin API says "agent", the people list "technician".
    for query in ["role=agent", "role=technician"] {
        let page = people(&f, &format!("page=1&pageSize=50&{query}")).await;
        let got: HashSet<Uuid> = uuids(&page).into_iter().collect();
        assert_eq!(got, f.agents, "{query}");
    }
}
