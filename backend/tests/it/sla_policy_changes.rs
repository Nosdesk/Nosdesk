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

/// A working calendar open around the clock, so a test's targets don't
/// depend on the time of day it runs.
async fn around_the_clock(pool: &TestPool, ws: &WorkspaceSeed) -> i32 {
    let day = json!([["00:00", "23:59"]]);
    let resp = as_admin(
        pool,
        ws,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/calendars")
            .set_json(json!({
                "name": "Always",
                "timezone": "UTC",
                "schedule": {
                    "mon": day, "tue": day, "wed": day, "thu": day,
                    "fri": day, "sat": day, "sun": day,
                },
            })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let calendar: Value = http_test::read_body_json(resp).await;
    i32::try_from(calendar["id"].as_i64().expect("id")).expect("id")
}

fn policy_body(response_minutes: i64, calendar: i32, clock_start: &str) -> Value {
    json!({
        "name": "Urgent",
        "target_response_minutes": response_minutes,
        "target_resolution_minutes": 6000,
        "working_calendar_id": calendar,
        "priority_filter": "urgent",
        "clock_start": clock_start,
    })
}

/// Set columns on a ticket directly, as history or an older release left them.
fn set_ticket(pool: &TestPool, ws: &WorkspaceSeed, id: i32, sql_set: &str) {
    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        diesel::sql_query(format!("UPDATE tickets SET {sql_set} WHERE id = {id}")).execute(c)
    })
    .expect("set ticket");
}

fn ticket_row(pool: &TestPool, ws: &WorkspaceSeed, id: i32) -> Ticket {
    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        backend::schema::tickets::table.find(id).first(c)
    })
    .expect("ticket")
}

async fn patch_policy(pool: &TestPool, ws: &WorkspaceSeed, id: &Value, body: Value) {
    let resp = as_admin(
        pool,
        ws,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{id}"))
            .set_json(body),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[actix_web::test]
async fn an_unconfirmed_guest_ticket_gets_no_targets_until_it_is_confirmed() {
    use backend::schema::workflow_states;
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    let calendar = around_the_clock(&pool, &ws).await;
    let resp = create(&pool, &ws, policy_body(60, calendar, "created")).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let policy: Value = http_test::read_body_json(resp).await;

    let mut conn = pool.get().expect("conn");
    let guest = with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
        let state: i32 = workflow_states::table
            .filter(workflow_states::name.eq("In Progress"))
            .select(workflow_states::id)
            .first(c)?;
        backend::repository::tickets::create_ticket(
            c,
            NewTicket {
                title: "Guest asks for help".to_string(),
                workflow_state_id: state,
                priority: backend::models::TicketPriority::Urgent,
                requester_uuid: Some(ws.member_uuid),
                submitted_via: Some("guest".to_string()),
                verification_state: Some("pending".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("create");
    drop(conn);
    assert_eq!(
        response_target(&pool, &ws, guest.id),
        None,
        "a ticket waiting for its email to be confirmed can't breach"
    );

    patch_policy(
        &pool,
        &ws,
        &policy["id"],
        policy_body(30, calendar, "created"),
    )
    .await;
    assert_eq!(
        response_target(&pool, &ws, guest.id),
        None,
        "a policy change leaves it alone too"
    );

    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
        backend::repository::tickets::verify_pending_tickets_for_user(c, ws.member_uuid)
    })
    .expect("confirm");
    drop(conn);
    assert!(
        response_target(&pool, &ws, guest.id).is_some(),
        "confirming it starts its SLA"
    );
}

#[actix_web::test]
async fn a_policy_save_that_moves_no_target_writes_no_ticket() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    let calendar = around_the_clock(&pool, &ws).await;
    let ticket = existing_urgent_ticket(&pool, &ws);
    let resp = create(&pool, &ws, policy_body(60, calendar, "created")).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let policy: Value = http_test::read_body_json(resp).await;
    assert!(response_target(&pool, &ws, ticket).is_some());

    let ticket_writes = |pool: &TestPool| -> i64 {
        use backend::schema::audit_log;
        let mut conn = pool.get().expect("conn");
        with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
            audit_log::table
                .filter(audit_log::table_name.eq("tickets"))
                .filter(audit_log::pk_text.eq(ticket.to_string()))
                .count()
                .get_result(c)
        })
        .expect("audit rows")
    };
    let before = ticket_writes(&pool);
    let mut renamed = policy_body(60, calendar, "created");
    renamed["name"] = json!("Urgent (renamed)");
    patch_policy(&pool, &ws, &policy["id"], renamed).await;
    assert_eq!(
        ticket_writes(&pool),
        before,
        "renaming the policy moves no target, so the ticket isn't written"
    );
}

#[actix_web::test]
async fn a_restamp_starts_a_running_ticket_from_when_it_was_opened_and_keeps_it() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    let calendar = around_the_clock(&pool, &ws).await;
    let ticket = existing_urgent_ticket(&pool, &ws);
    // Opened two hours ago and answered 30 minutes ago: late for a 60-minute
    // response target counted from when it was opened.
    set_ticket(
        &pool,
        &ws,
        ticket,
        "created_at = now() - interval '2 hours', first_response_at = now() - interval '30 minutes'",
    );

    let resp = create(&pool, &ws, policy_body(60, calendar, "activated")).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let row = ticket_row(&pool, &ws, ticket);
    assert_eq!(
        row.sla_clock_started_at,
        Some(row.created_at),
        "the clock starts when the ticket was opened, not at the policy save"
    );
    let mut conn = pool.get().expect("conn");
    let pill = with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
        Ok(backend::services::sla::pill_json_for_ticket(c, &row))
    })
    .expect("pill");
    drop(conn);
    assert_eq!(pill["response"]["breached"], json!(true), "{pill}");

    // Another SLA save, such as a holiday, doesn't move it.
    let resp = as_admin(
        &pool,
        &ws,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/sla/calendars/{calendar}/holidays"))
            .set_json(json!({ "date": "2031-12-25", "label": "Closed" })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(
        ticket_row(&pool, &ws, ticket).sla_clock_started_at,
        row.sla_clock_started_at
    );
}

#[actix_web::test]
async fn a_longer_target_clears_a_breach_that_no_longer_stands() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    let calendar = around_the_clock(&pool, &ws).await;
    let ticket = existing_urgent_ticket(&pool, &ws);
    set_ticket(
        &pool,
        &ws,
        ticket,
        "created_at = now() - interval '2 hours'",
    );
    let resp = create(&pool, &ws, policy_body(60, calendar, "created")).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let policy: Value = http_test::read_body_json(resp).await;
    // The sweep stamped the breach an hour after it opened.
    set_ticket(
        &pool,
        &ws,
        ticket,
        "sla_response_breached_at = now() - interval '59 minutes'",
    );

    patch_policy(
        &pool,
        &ws,
        &policy["id"],
        policy_body(600, calendar, "created"),
    )
    .await;
    assert_eq!(
        ticket_row(&pool, &ws, ticket).sla_response_breached_at,
        None,
        "under a 10-hour target it hasn't breached, so a later breach notifies again"
    );
}

#[actix_web::test]
async fn a_target_that_goes_away_reaches_clients() {
    use backend::schema::sync_actions;
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    let calendar = around_the_clock(&pool, &ws).await;
    let ticket = existing_urgent_ticket(&pool, &ws);
    let resp = create(&pool, &ws, policy_body(60, calendar, "created")).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let policy: Value = http_test::read_body_json(resp).await;
    assert!(response_target(&pool, &ws, ticket).is_some());

    let mut no_sla = policy_body(60, calendar, "created");
    no_sla["no_sla"] = json!(true);
    patch_policy(&pool, &ws, &policy["id"], no_sla).await;
    assert_eq!(response_target(&pool, &ws, ticket), None);

    let mut conn = pool.get().expect("conn");
    let last: Value =
        with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
            sync_actions::table
                .filter(sync_actions::event_type.eq("ticket.sla_updated"))
                .filter(sync_actions::aggregate_id.eq(ticket.to_string()))
                .order(sync_actions::sync_id.desc())
                .select(sync_actions::data)
                .first(c)
        })
        .expect("an sla update for the ticket");
    assert_eq!(last["sla"], Value::Null, "the pill goes away: {last}");
}

/// A ticket in `state`, opened two hours ago, with no SLA clock start: as
/// tickets created before creation stamped the SLA were left.
fn old_ticket_in(pool: &TestPool, ws: &WorkspaceSeed, state: &str) -> i32 {
    use backend::schema::{tickets, workflow_states};
    let mut conn = pool.get().expect("conn");
    let id = with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        let state: i32 = workflow_states::table
            .filter(workflow_states::name.eq(state))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer offline".to_string(),
                workflow_state_id: state,
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)
    })
    .expect("ticket");
    drop(conn);
    set_ticket(pool, ws, id, "created_at = now() - interval '2 hours'");
    id
}

/// Edit a ticket the way the app does.
fn edit(pool: &TestPool, ws: &WorkspaceSeed, id: i32, update: backend::models::TicketUpdate) {
    let mut conn = pool.get().expect("conn");
    with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(ws), |c| {
        Ok(backend::repository::tickets::update_ticket_partial(
            c, id, update, None,
        ))
    })
    .expect("transaction")
    .expect("edit");
}

#[actix_web::test]
async fn an_edit_starts_a_running_tickets_missing_clock_from_when_it_was_opened() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    // The seeded default policy counts from activation.
    let ticket = old_ticket_in(&pool, &ws, "In Progress");
    assert_eq!(ticket_row(&pool, &ws, ticket).sla_clock_started_at, None);

    edit(
        &pool,
        &ws,
        ticket,
        backend::models::TicketUpdate {
            priority: Some(backend::models::TicketPriority::High),
            ..Default::default()
        },
    );
    let row = ticket_row(&pool, &ws, ticket);
    assert_eq!(
        row.sla_clock_started_at,
        Some(row.created_at),
        "it has been running since it was opened, not since the edit"
    );

    edit(
        &pool,
        &ws,
        ticket,
        backend::models::TicketUpdate {
            priority: Some(backend::models::TicketPriority::Low),
            ..Default::default()
        },
    );
    assert_eq!(
        ticket_row(&pool, &ws, ticket).sla_clock_started_at,
        Some(row.created_at),
        "stored once"
    );
}

#[actix_web::test]
async fn moving_a_ticket_into_a_running_state_starts_its_clock_then() {
    use backend::schema::workflow_states;
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
    let pool = db.runtime_pool(4);
    // Waited in Backlog, which pauses the clock, for two hours.
    let ticket = old_ticket_in(&pool, &ws, "Backlog");
    let mut conn = pool.get().expect("conn");
    let in_progress: i32 =
        with_actor_context::<_, diesel::result::Error>(&mut conn, &pinned(&ws), |c| {
            workflow_states::table
                .filter(workflow_states::name.eq("In Progress"))
                .select(workflow_states::id)
                .first(c)
        })
        .expect("state");
    drop(conn);

    let before = chrono::Utc::now().naive_utc() - chrono::Duration::seconds(5);
    edit(
        &pool,
        &ws,
        ticket,
        backend::models::TicketUpdate {
            workflow_state_id: Some(in_progress),
            ..Default::default()
        },
    );
    let started = ticket_row(&pool, &ws, ticket)
        .sla_clock_started_at
        .expect("started");
    assert!(
        started >= before,
        "work starts now, not when it was opened: {started}"
    );
}
