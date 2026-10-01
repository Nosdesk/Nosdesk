//! Creating a ticket through the API takes only the columns the client owns.
//! Provenance, guest access, verification, triage and the spam flag are the
//! server's, and someone who doesn't handle tickets files their own ticket in
//! the default state, as the portal does.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, TicketPriority};
use backend::repository::workflow_states;
use backend::schema::{tickets, workflow_states as states};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:ticket_create_columns";

/// POST /api/tickets as `user` in `ws`; the new ticket's id.
async fn create_as(pool: &TestPool, ws: &WorkspaceSeed, user: Uuid, body: &Value) -> i32 {
    let account =
        backend::repository::users::get_user_by_uuid(&user, &mut pool.get().expect("conn"))
            .expect("load user");
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: account.uuid.to_string(),
        name: account.name.clone(),
        email: "someone@example.com".to_string(),
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
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(account.uuid, Some(corr)).with_workspace(ws.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), pool).expect("init search"));
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/api/tickets",
                web::post().to(backend::handlers::create_ticket),
            ),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/api/tickets")
            .set_json(body)
            .to_request(),
    )
    .await;
    assert!(resp.status().is_success(), "create: {}", resp.status());
    let created: Value = http_test::read_body_json(resp).await;
    created["id"].as_i64().expect("ticket id") as i32
}

#[derive(Debug, PartialEq, Queryable)]
struct Stored {
    requester_uuid: Option<Uuid>,
    assignee_uuid: Option<Uuid>,
    workflow_state_id: i32,
    priority: TicketPriority,
    submitted_via: Option<String>,
    guest_lookup_token: Option<Uuid>,
    verification_state: Option<String>,
    triage_state: Option<String>,
    spam_suspected: bool,
    resolution_notes: Option<String>,
}

fn stored(pool: &TestPool, ws: i32, id: i32) -> Stored {
    run_in_workspace(pool, REF, ws, |c| {
        tickets::table
            .find(id)
            .select((
                tickets::requester_uuid,
                tickets::assignee_uuid,
                tickets::workflow_state_id,
                tickets::priority,
                tickets::submitted_via,
                tickets::guest_lookup_token,
                tickets::verification_state,
                tickets::triage_state,
                tickets::spam_suspected,
                tickets::resolution_notes,
            ))
            .first::<Stored>(c)
    })
    .expect("reload ticket")
}

#[actix_web::test]
async fn a_new_ticket_takes_only_the_clients_columns() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;

    let default_state =
        run_in_workspace(&pool, REF, ws.workspace_id, workflow_states::default_state)
            .expect("default state")
            .id;
    let other_state: i32 = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        states::table
            .filter(states::workspace_id.eq(ws.workspace_id))
            .filter(states::id.ne(default_state))
            .select(states::id)
            .first(c)
    })
    .expect("another state");

    let body = json!({
        "title": "Monitor flickers",
        "workflow_state_id": other_state,
        "priority": "urgent",
        "requester_uuid": ws.admin_uuid,
        "assignee_uuid": ws.admin_uuid,
        "submitted_via": "email",
        "guest_lookup_token": Uuid::new_v4(),
        "verification_state": "verified",
        "triage_state": "triaged",
        "spam_suspected": true,
        "resolution_notes": "Replaced the cable",
    });

    let filed = create_as(&pool, ws, ws.member_uuid, &body).await;
    assert_eq!(
        stored(&pool, ws.workspace_id, filed),
        Stored {
            requester_uuid: Some(ws.member_uuid),
            assignee_uuid: None,
            workflow_state_id: default_state,
            priority: TicketPriority::default(),
            submitted_via: None,
            guest_lookup_token: None,
            verification_state: None,
            triage_state: None,
            spam_suspected: false,
            resolution_notes: None,
        },
        "a member files their own ticket in the default state"
    );

    let logged = create_as(&pool, ws, ws.admin_uuid, &body).await;
    assert_eq!(
        stored(&pool, ws.workspace_id, logged),
        Stored {
            requester_uuid: Some(ws.admin_uuid),
            assignee_uuid: Some(ws.admin_uuid),
            workflow_state_id: other_state,
            priority: TicketPriority::Urgent,
            submitted_via: None,
            guest_lookup_token: None,
            verification_state: None,
            triage_state: None,
            spam_suspected: false,
            resolution_notes: Some("Replaced the cable".to_string()),
        },
        "staff set the rest, and none of the server's columns"
    );
}
