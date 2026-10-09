//! An SLA policy change reaches the open tickets it covers, a policy without
//! its own calendar is measured on the workspace's default calendar, and a
//! target of zero minutes is refused. Before, a new or edited policy left
//! existing tickets without targets (so they could never breach), a policy
//! without a calendar quietly switched SLA off for every ticket it matched,
//! and a zero target was saved and then read as "no target".

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, WorkspaceSeed};

async fn as_admin(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: http_test::TestRequest,
) -> ServiceResponse {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
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
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::sla::config)),
    )
    .await;
    http_test::call_service(&app, req.to_request()).await
}

fn pinned(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id)
}

/// An urgent ticket in In Progress, inserted directly so it starts with no
/// SLA targets, as tickets did before a policy covered them.
fn existing_urgent_ticket(pool: &TestPool, ws: &WorkspaceSeed) -> i32 {
    use backend::schema::{tickets, workflow_states};
    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        let state: i32 = workflow_states::table
            .filter(workflow_states::name.eq("In Progress"))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Mail server down".to_string(),
                workflow_state_id: state,
                priority: backend::models::TicketPriority::Urgent,
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)
    })
    .expect("ticket")
}

/// The ticket's materialised response target, which the breach sweep scans.
fn response_target(pool: &TestPool, ws: &WorkspaceSeed, id: i32) -> Option<chrono::NaiveDateTime> {
    use backend::schema::tickets;
    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        tickets::table
            .find(id)
            .select(tickets::sla_response_target_at)
            .first(c)
    })
    .expect("ticket")
}

fn default_calendar(pool: &TestPool, ws: &WorkspaceSeed) -> i32 {
    use backend::schema::working_calendars;
    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        working_calendars::table
            .filter(working_calendars::is_default.eq(true))
            .select(working_calendars::id)
            .first(c)
    })
    .expect("default calendar")
}

fn urgent_policy(response_minutes: i64, calendar: Option<i32>) -> Value {
    json!({
        "name": "Urgent",
        "target_response_minutes": response_minutes,
        "target_resolution_minutes": 480,
        "working_calendar_id": calendar,
        "priority_filter": "urgent",
        "clock_start": "created",
    })
}

async fn create(pool: &TestPool, ws: &WorkspaceSeed, body: Value) -> ServiceResponse {
    as_admin(
        pool,
        ws,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/policies")
            .set_json(body),
    )
    .await
}

#[actix_web::test]
async fn creating_editing_and_deleting_a_policy_restamps_the_open_tickets_it_covers() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    // The app's own role, as in production.
    let pool = db.runtime_pool(4);
    let calendar = default_calendar(&pool, &ws);
    let ticket = existing_urgent_ticket(&pool, &ws);
    assert_eq!(response_target(&pool, &ws, ticket), None, "no target yet");

    let resp = create(&pool, &ws, urgent_policy(60, Some(calendar))).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let policy: Value = http_test::read_body_json(resp).await;
    let created = response_target(&pool, &ws, ticket);
    assert!(
        created.is_some(),
        "the new policy gives the open ticket a target"
    );

    let resp = as_admin(
        &pool,
        &ws,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{}", policy["id"]))
            .set_json(urgent_policy(600, Some(calendar))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let edited = response_target(&pool, &ws, ticket);
    assert!(
        edited > created,
        "a longer target moves the ticket's target later"
    );

    let resp = as_admin(
        &pool,
        &ws,
        http_test::TestRequest::delete().uri(&format!("/api/admin/sla/policies/{}", policy["id"])),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let after = response_target(&pool, &ws, ticket);
    assert_ne!(
        after, edited,
        "the ticket falls back to the default policy's target"
    );
}

#[actix_web::test]
async fn a_policy_without_a_calendar_is_measured_on_the_workspace_default_calendar() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    // The app's own role, as in production.
    let pool = db.runtime_pool(4);
    let ticket_id = existing_urgent_ticket(&pool, &ws);

    let resp = create(&pool, &ws, urgent_policy(60, None)).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let mut conn = pool.get().expect("conn");
    let pill = with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
        let ticket: Ticket = backend::schema::tickets::table.find(ticket_id).first(c)?;
        Ok(backend::services::sla::pill_json_for_ticket(c, &ticket))
    })
    .expect("pill");
    assert!(
        pill.get("response").is_some(),
        "the ticket has a response timer, not SLA switched off: {pill}"
    );
}

#[actix_web::test]
async fn a_target_of_zero_minutes_is_refused() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    // The app's own role, as in production.
    let pool = db.runtime_pool(4);
    let calendar = default_calendar(&pool, &ws);

    let resp = create(&pool, &ws, urgent_policy(0, Some(calendar))).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "create with 0");

    let resp = create(&pool, &ws, urgent_policy(60, Some(calendar))).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let policy: Value = http_test::read_body_json(resp).await;
    let resp = as_admin(
        &pool,
        &ws,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{}", policy["id"]))
            .set_json(urgent_policy(0, Some(calendar))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "edit to 0");
}

#[actix_web::test]
async fn a_new_ticket_gets_its_targets_when_it_is_created() {
    use backend::schema::workflow_states;
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    // The app's own role, as in production.
    let pool = db.runtime_pool(4);
    let calendar = default_calendar(&pool, &ws);
    let resp = create(&pool, &ws, urgent_policy(60, Some(calendar))).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let mut conn = pool.get().expect("conn");
    let ticket = with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
        let state: i32 = workflow_states::table
            .filter(workflow_states::name.eq("In Progress"))
            .select(workflow_states::id)
            .first(c)?;
        backend::repository::tickets::create_ticket(
            c,
            NewTicket {
                title: "VPN down".to_string(),
                workflow_state_id: state,
                priority: backend::models::TicketPriority::Urgent,
                ..Default::default()
            },
        )
    })
    .expect("create");
    assert!(
        response_target(&pool, &ws, ticket.id).is_some(),
        "a new ticket the policy covers can breach without waiting for an edit"
    );
}
