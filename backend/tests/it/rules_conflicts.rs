//! The conflicts inspector names automated rules where one changes a field
//! another checks, and pairs of them that ran on the same ticket lately.
//! Manual, draft and paused rules aren't checked. Driven through the rules
//! routes as `nosdesk_app`, so each workspace sees only its own rules.

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, NewRule, NewRuleApplication, RuleApplicationStatus, RuleState, RuleTriggerKind,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, WorkspaceSeed};

/// Call the rules routes as `ws`'s admin.
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
            .service(web::scope("/api").configure(backend::handlers::rules::config)),
    )
    .await;
    http_test::call_service(&app, req.to_request()).await
}

async fn conflicts(pool: &TestPool, ws: &WorkspaceSeed) -> Value {
    let resp = as_admin(
        pool,
        ws,
        http_test::TestRequest::get().uri("/api/rules/conflicts"),
    )
    .await;
    let status = resp.status();
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn leaf(field: &str, op: &str, value: Value) -> Value {
    json!({ "kind": "leaf", "field": field, "op": op, "value": value })
}

fn in_workspace<T>(
    conn: &mut backend::db::DbConnection,
    ws: &WorkspaceSeed,
    f: impl FnOnce(&mut backend::db::DbConnection) -> diesel::QueryResult<T>,
) -> T {
    let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    with_actor_context(conn, &actor, f).expect("in workspace")
}

/// Save a rule straight through the repository, in `state`.
fn rule(
    conn: &mut backend::db::DbConnection,
    ws: &WorkspaceSeed,
    name: &str,
    trigger_kind: RuleTriggerKind,
    state: RuleState,
    conditions: Value,
    actions: Value,
) -> i32 {
    in_workspace(conn, ws, |c| {
        backend::repository::rules::create(
            c,
            NewRule {
                workspace_id: ws.workspace_id,
                name: name.to_string(),
                description: None,
                trigger_kind,
                trigger_config: json!({}),
                conditions,
                actions,
                reads_set: Vec::new(),
                writes_set: Vec::new(),
                state,
                priority: 100,
                created_by: Some(ws.admin_uuid),
            },
        )
    })
    .id
}

fn ticket(conn: &mut backend::db::DbConnection, ws: &WorkspaceSeed, title: &str) -> i32 {
    use backend::models::{NewTicket, TicketPriority};
    use backend::schema::{tickets, workflow_states};
    use diesel::prelude::*;

    in_workspace(conn, ws, |c| {
        let open: i32 = workflow_states::table
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: open,
                priority: TicketPriority::Medium,
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)
    })
}

fn ran(
    conn: &mut backend::db::DbConnection,
    ws: &WorkspaceSeed,
    rule_id: i32,
    ticket_id: i32,
    status: RuleApplicationStatus,
) {
    in_workspace(conn, ws, |c| {
        backend::repository::rules::record_application(
            c,
            NewRuleApplication {
                workspace_id: ws.workspace_id,
                rule_id,
                rule_version: 1,
                ticket_id,
                status,
                correlation_id: Some(Uuid::now_v7()),
                actor_uuid: None,
                actor_kind: "system".to_string(),
                originating_event_id: None,
                originating_event_kind: None,
                condition_evaluation: None,
                actions_taken: None,
                actions_skipped: None,
                failure_reason: None,
            },
        )
    });
}

fn set_priority() -> Value {
    json!([{ "kind": "set_priority", "config": { "priority": "high" } }])
}

fn ids(list: &Value, key: &str) -> Vec<i64> {
    let mut out: Vec<i64> = list
        .as_array()
        .expect("list")
        .iter()
        .filter_map(|v| v[key].as_i64())
        .collect();
    out.sort();
    out
}

/// An escalation rule that raises priority, and a routing rule that checks
/// it, both running. Returns (escalate, route).
fn running_pair(conn: &mut backend::db::DbConnection, ws: &WorkspaceSeed) -> (i32, i32) {
    let escalate = rule(
        conn,
        ws,
        "Escalate VIP tickets",
        RuleTriggerKind::TicketUpdated,
        RuleState::Live,
        leaf("ticket.title", "contains", json!("vip")),
        set_priority(),
    );
    let route = rule(
        conn,
        ws,
        "Route urgent tickets",
        RuleTriggerKind::TicketUpdated,
        RuleState::DryRun,
        leaf("ticket.priority", "eq", json!("high")),
        json!([{ "kind": "assign", "config": { "method": "direct", "user_uuid": ws.admin_uuid } }]),
    );
    (escalate, route)
}

#[actix_web::test]
async fn conflicts_pair_running_automated_rules_and_their_co_fires() {
    use RuleApplicationStatus::{DryRun, Failed, Succeeded};

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.pool_with_size(2).get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (&seeded.a, &seeded.b);

    let (escalate, route) = running_pair(&mut conn, a);
    // Each of these would pair with `route` or `escalate` if it were checked.
    let reads_priority = leaf("ticket.priority", "eq", json!("high"));
    let draft = rule(
        &mut conn,
        a,
        "Draft router",
        RuleTriggerKind::TicketUpdated,
        RuleState::Draft,
        reads_priority.clone(),
        set_priority(),
    );
    let paused = rule(
        &mut conn,
        a,
        "Paused router",
        RuleTriggerKind::TicketCreated,
        RuleState::Paused,
        reads_priority,
        set_priority(),
    );
    let manual = rule(
        &mut conn,
        a,
        "Escalate",
        RuleTriggerKind::Manual,
        RuleState::Live,
        json!([]),
        set_priority(),
    );

    // Both running rules ran on the printer ticket (escalate twice). On the
    // VPN ticket the routing rule failed, so that is no co-fire. The paused
    // and manual rules ran on the printer ticket too but aren't checked.
    let printer = ticket(&mut conn, a, "Printer jammed");
    let vpn = ticket(&mut conn, a, "VPN down");
    ran(&mut conn, a, escalate, printer, Succeeded);
    ran(&mut conn, a, escalate, printer, Succeeded);
    ran(&mut conn, a, route, printer, DryRun);
    ran(&mut conn, a, paused, printer, Succeeded);
    ran(&mut conn, a, manual, printer, Succeeded);
    ran(&mut conn, a, escalate, vpn, Succeeded);
    ran(&mut conn, a, route, vpn, Failed);

    let (b_escalate, b_route) = running_pair(&mut conn, b);
    let b_ticket = ticket(&mut conn, b, "Printer jammed in B");
    ran(&mut conn, b, b_escalate, b_ticket, Succeeded);
    ran(&mut conn, b, b_route, b_ticket, Succeeded);
    drop(conn);
    let pool = db.runtime_pool(4);

    let body = conflicts(&pool, a).await;
    let mut checked = vec![escalate as i64, route as i64];
    checked.sort();
    assert_eq!(ids(&body["rules"], "id"), checked, "{body}");
    for left_out in [draft, paused, manual] {
        assert!(!checked.contains(&(left_out as i64)), "rule {left_out}");
    }
    assert_eq!(
        body["overlaps"],
        json!([{
            "writer_rule_id": escalate,
            "reader_rule_id": route,
            "fields": ["ticket.priority"],
        }]),
        "{body}"
    );
    let (first, second) = (escalate.min(route), escalate.max(route));
    let co_fires = body["co_fires"].as_array().expect("co_fires");
    assert_eq!(co_fires.len(), 1, "{body}");
    assert_eq!(co_fires[0]["first_rule_id"], first);
    assert_eq!(co_fires[0]["second_rule_id"], second);
    assert_eq!(co_fires[0]["ticket_count"], 1);
    assert_eq!(co_fires[0]["sample_ticket_ids"], json!([printer]));
    assert!(co_fires[0]["last_co_fired_at"].is_string(), "{body}");

    // B sees its own pair and nothing of A's.
    let body = conflicts(&pool, b).await;
    let mut checked = vec![b_escalate as i64, b_route as i64];
    checked.sort();
    assert_eq!(ids(&body["rules"], "id"), checked, "{body}");
    assert_eq!(body["overlaps"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(body["overlaps"][0]["writer_rule_id"], b_escalate);
    assert_eq!(body["co_fires"][0]["sample_ticket_ids"], json!([b_ticket]));
}

#[actix_web::test]
async fn a_workspace_with_one_automated_rule_has_no_conflicts() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.pool_with_size(2).get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let a = &seeded.a;
    rule(
        &mut conn,
        a,
        "Escalate VIP tickets",
        RuleTriggerKind::TicketUpdated,
        RuleState::Live,
        leaf("ticket.priority", "eq", json!("low")),
        set_priority(),
    );
    drop(conn);
    let pool = db.runtime_pool(4);

    let body = conflicts(&pool, a).await;
    assert_eq!(body["rules"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(body["overlaps"], json!([]), "a rule reading its own write");
    assert_eq!(body["co_fires"], json!([]));
    assert!(body["since"].is_string());
}
