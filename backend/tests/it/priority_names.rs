//! Every place that reads a priority by name reads the same five, through one
//! parser: assignment-rule conditions, the guest form's default, and the CSV
//! importer's error.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{AssignmentMethod, Claims, NewAssignmentRule, NewTicket, Ticket};
use backend::schema::{assignment_rules, tickets, workflow_states};
use backend::services::imports::{self, csv_parser, ImportType};
use backend::services::ticket_updates::{assign_new_ticket, ActorConn};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

/// A rule condition naming a priority the way people write it ("normal",
/// "High") matches tickets of that priority.
#[test]
fn an_assignment_condition_reads_the_priority_by_name() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let a = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let admin = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
    let mut conn = pool.get().expect("conn");
    let ticket: Ticket = with_actor_context(&mut conn, &admin, |c| {
        diesel::insert_into(assignment_rules::table)
            .values(&NewAssignmentRule {
                name: "Medium to the admin".into(),
                description: None,
                priority: 1,
                is_active: true,
                method: AssignmentMethod::DirectUser,
                target_user_uuid: Some(a.admin_uuid),
                target_group_id: None,
                trigger_on_create: true,
                trigger_on_category_change: false,
                category_id: None,
                conditions: Some(json!({ "priority": "normal" })),
                created_by: Some(a.admin_uuid),
            })
            .execute(c)?;
        let open: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(a.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer jammed".into(),
                workflow_state_id: open,
                ..Default::default()
            })
            .get_result(c)
    })
    .expect("seed");

    let assigned = assign_new_ticket(
        &mut ActorConn {
            conn: &mut conn,
            actor: &admin,
        },
        None,
        ticket,
    );
    assert_eq!(assigned.assignee_uuid, Some(a.admin_uuid));
}

/// The guest form's default priority can be any of the five.
#[actix_web::test]
async fn the_guest_default_priority_can_be_any_priority() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let ws = &seeded.a;
    let pool = db.runtime_pool(4);
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
            .service(web::scope("/api").configure(backend::handlers::guest_settings::config)),
    )
    .await;
    let set = |priority: &str| {
        http_test::TestRequest::patch()
            .uri("/api/admin/guest-settings")
            .set_json(json!({ "guest_ticket_default_priority": priority }))
            .to_request()
    };
    for priority in ["none", "low", "medium", "high", "urgent"] {
        let resp = http_test::call_service(&app, set(priority)).await;
        assert_eq!(resp.status(), StatusCode::OK, "{priority}");
    }
    let resp = http_test::call_service(&app, set("critical")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// A CSV row with a priority that isn't one says which ones are.
#[test]
fn a_csv_priority_error_names_all_five() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("tickets.csv");
    std::fs::write(
        &path,
        "title,workflow_state,priority,requester_email,assignee_email,category,due_date\n\
         Printer jammed,Backlog,critical,,,,\n",
    )
    .expect("write csv");
    let parsed = csv_parser::parse_file(&path).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Tickets, &parsed).expect("dry run");
    let message = summary
        .errors
        .iter()
        .find(|e| e.column.as_deref() == Some("priority"))
        .map(|e| e.message.clone())
        .expect("a priority error");
    for name in ["none", "low", "medium", "high", "urgent"] {
        assert!(message.contains(name), "{message}");
    }
}
