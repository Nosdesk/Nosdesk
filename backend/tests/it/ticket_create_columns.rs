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
    create_with(pool, ws, user, body, false).await
}

/// [`create_as`], with an API token of `user`'s when `token` is set.
async fn create_with(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    body: &Value,
    token: bool,
) -> i32 {
    let (status, created) = send_as(
        pool,
        ws,
        user,
        http_test::TestRequest::post()
            .uri("/api/tickets")
            .set_json(body),
        token,
    )
    .await;
    assert!(status.is_success(), "create: {status}");
    created["id"].as_i64().expect("ticket id") as i32
}

/// Send `request` to the ticket routes as `user` in `ws`, with an API token of
/// theirs when `token` is set; the status and the JSON body.
async fn send_as(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    user: Uuid,
    request: http_test::TestRequest,
    token: bool,
) -> (actix_web::http::StatusCode, Value) {
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
        file_path: None,
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
                if token {
                    req.extensions_mut()
                        .insert(backend::middleware::api_token::ApiTokenRequest {
                            token_uuid: Uuid::new_v4(),
                            made_for_someone_else: false,
                        });
                }
                srv.call(req)
            })
            .route(
                "/api/tickets",
                web::post().to(backend::handlers::create_ticket),
            )
            .route(
                "/api/tickets/import/json",
                web::post().to(backend::handlers::import_tickets_from_json_string),
            )
            .route(
                "/api/tickets/{id}",
                web::patch().to(backend::handlers::update_ticket_partial),
            ),
    )
    .await;
    let resp = http_test::call_service(&app, request.to_request()).await;
    let status = resp.status();
    let bytes = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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

/// The `created_via` of the ticket's `ticket.created` event.
fn created_via(pool: &TestPool, ws: i32, id: i32) -> Value {
    use backend::schema::sync_actions as s;
    run_in_workspace(pool, REF, ws, |c| {
        s::table
            .filter(s::event_type.eq("ticket.created"))
            .filter(s::aggregate_id.eq(id.to_string()))
            .select(s::data)
            .first::<Value>(c)
    })
    .expect("ticket.created")["created_via"]
        .clone()
}

/// A ticket an API token creates is stamped `submitted_via = "api"` by the
/// server, whatever the body says, and `acknowledge: false` files it without
/// the requester's acknowledgement. A session's ticket is stamped nothing.
#[actix_web::test]
async fn a_token_ticket_is_stamped_api_and_may_skip_the_acknowledgement() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;
    let open = run_in_workspace(&pool, REF, ws.workspace_id, workflow_states::default_state)
        .expect("default state")
        .id;
    let body = |acknowledge: Option<bool>| {
        let mut body = json!({
            "title": "From the website",
            "workflow_state_id": open,
            "priority": "medium",
            "requester_uuid": ws.member_uuid,
            "submitted_via": "portal",
        });
        if let Some(a) = acknowledge {
            body["acknowledge"] = json!(a);
        }
        body
    };

    let quiet = create_with(&pool, ws, ws.admin_uuid, &body(Some(false)), true).await;
    assert_eq!(
        stored(&pool, ws.workspace_id, quiet)
            .submitted_via
            .as_deref(),
        Some("api")
    );
    assert_eq!(
        created_via(&pool, ws.workspace_id, quiet)["acknowledge"],
        json!(false)
    );

    let told = create_with(&pool, ws, ws.admin_uuid, &body(None), true).await;
    assert_eq!(
        stored(&pool, ws.workspace_id, told)
            .submitted_via
            .as_deref(),
        Some("api")
    );
    assert_eq!(
        created_via(&pool, ws.workspace_id, told).get("acknowledge"),
        None
    );

    let by_session = create_with(&pool, ws, ws.admin_uuid, &body(Some(true)), false).await;
    assert_eq!(
        stored(&pool, ws.workspace_id, by_session).submitted_via,
        None
    );
    assert_eq!(
        created_via(&pool, ws.workspace_id, by_session).get("acknowledge"),
        None
    );
}

/// A token that changes the title or the requester of a ticket it didn't file
/// makes its text the token's: the ticket is stamped `"api"`, or an import its
/// token variant. A change to something else leaves it as it was, and so does
/// a session's edit.
#[actix_web::test]
async fn a_token_that_retitles_a_ticket_stamps_it_api() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;
    let open = run_in_workspace(&pool, REF, ws.workspace_id, workflow_states::default_state)
        .expect("default state")
        .id;
    let filed = json!({
        "title": "From the portal",
        "workflow_state_id": open,
        "priority": "medium",
        "requester_uuid": ws.member_uuid,
    });
    let patch = |id: i32, body: Value| {
        http_test::TestRequest::patch()
            .uri(&format!("/api/tickets/{id}"))
            .set_json(body)
    };

    let by_session = create_as(&pool, ws, ws.admin_uuid, &filed).await;
    let (status, _) = send_as(
        &pool,
        ws,
        ws.admin_uuid,
        patch(by_session, json!({ "title": "Renamed" })),
        false,
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_eq!(
        stored(&pool, ws.workspace_id, by_session).submitted_via,
        None
    );

    let priority_only = create_as(&pool, ws, ws.admin_uuid, &filed).await;
    let (status, _) = send_as(
        &pool,
        ws,
        ws.admin_uuid,
        patch(priority_only, json!({ "priority": "high" })),
        true,
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_eq!(
        stored(&pool, ws.workspace_id, priority_only).submitted_via,
        None
    );

    // An imported ticket takes its import's token variant: still history, so
    // its clocks don't start, and a stranger's words, so its mail is redacted.
    let imported = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::tickets::create_ticket(
            c,
            backend::models::NewTicket {
                title: "From the old desk".into(),
                workflow_state_id: open,
                requester_uuid: Some(ws.member_uuid),
                submitted_via: Some(backend::models::CSV_IMPORT.into()),
                ..Default::default()
            },
        )
    })
    .expect("imported ticket")
    .id;
    let (status, _) = send_as(
        &pool,
        ws,
        ws.admin_uuid,
        patch(imported, json!({ "title": "Claim a refund" })),
        true,
    )
    .await;
    assert!(status.is_success(), "{status}");
    let retitled = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::tickets::get_ticket_by_id(c, imported)
    })
    .expect("retitled ticket");
    assert_eq!(
        retitled.submitted_via.as_deref(),
        Some(backend::models::CSV_IMPORT_BY_TOKEN)
    );
    let open_state = Some(backend::models::WorkflowStateCategory::Active);
    assert!(!backend::services::rules::clocks::Eligibility {
        category: open_state,
        submitted_via: retitled.submitted_via.as_deref(),
        verification_state: None,
        merged: false,
        held_for_approval: false,
    }
    .runs_clocks());
    assert!(backend::services::ticket_mail_redaction::stranger_provenance(&retitled));

    for body in [
        json!({ "title": "Win a prize" }),
        json!({ "requester": ws.admin_uuid.to_string() }),
    ] {
        let id = create_as(&pool, ws, ws.admin_uuid, &filed).await;
        let (status, _) = send_as(&pool, ws, ws.admin_uuid, patch(id, body.clone()), true).await;
        assert!(status.is_success(), "{status}");
        assert_eq!(
            stored(&pool, ws.workspace_id, id).submitted_via.as_deref(),
            Some("api"),
            "{body}"
        );
    }
}

/// Tickets a token imports from JSON are stamped `"api"`, as if it had filed
/// them one by one.
#[actix_web::test]
async fn a_token_import_stamps_its_tickets_api() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;
    let body = json!({ "tickets": [{
        "id": 1, "title": "Imported by a token", "status": "open", "priority": "medium",
        "created": "", "modified": "", "assignee": "", "requester": ws.member_uuid.to_string(),
        "device": null, "comments": null, "article_content": null
    }] });
    let (status, answer) = send_as(
        &pool,
        ws,
        ws.admin_uuid,
        http_test::TestRequest::post()
            .uri("/api/tickets/import/json")
            .set_json(&body),
        true,
    )
    .await;
    assert!(status.is_success(), "{status}");
    assert_eq!(answer["imported"], 1, "{answer}");
    let via: Vec<Option<String>> = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        tickets::table
            .filter(tickets::title.eq("Imported by a token"))
            .select(tickets::submitted_via)
            .load(c)
    })
    .expect("imported ticket");
    assert_eq!(via, vec![Some("api".to_string())]);
}
