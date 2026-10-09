//! A new ticket goes through the assignment rules whichever way it arrives.
//! `assign_new_ticket` is what the app, the portal, the guest form and the
//! email pipeline all call, so it is tested once here.

use diesel::prelude::*;
use serde_json::{json, Value};

use backend::models::{AssignmentMethod, NewAssignmentRule, NewTicket, Ticket};
use backend::schema::{assignment_rules, sync_actions, tickets, workflow_states};
use backend::services::ticket_updates::{assign_new_ticket, ActorConn};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

#[test]
fn a_new_ticket_is_assigned_by_the_rules_and_the_assignee_is_told() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let mut conn = db.pool_with_size(1).get().expect("conn");

    let admin = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
    let ticket: Ticket = with_actor_context(&mut conn, &admin, |c| {
        diesel::insert_into(assignment_rules::table)
            .values(&NewAssignmentRule {
                name: "Everything to the admin".into(),
                description: None,
                priority: 1,
                is_active: true,
                method: AssignmentMethod::DirectUser,
                target_user_uuid: Some(a.admin_uuid),
                target_group_id: None,
                trigger_on_create: true,
                trigger_on_category_change: false,
                category_id: None,
                conditions: None,
                created_by: Some(a.admin_uuid),
            })
            .execute(c)?;
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(a.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer jam".into(),
                workflow_state_id: open_state,
                requester_uuid: Some(a.member_uuid),
                ..Default::default()
            })
            .get_result(c)
    })
    .expect("seed rule and ticket");

    // The email pipeline runs the rules as the system.
    let system = ActorContext::system("assignment_rules").with_workspace(a.workspace_id);
    let assigned = assign_new_ticket(
        &mut ActorConn {
            conn: &mut conn,
            actor: &system,
        },
        None,
        ticket,
    );
    assert_eq!(assigned.assignee_uuid, Some(a.admin_uuid));

    // The assignment went out as a change with the previous assignee, which
    // is what the notification deriver reads to tell the new assignee.
    let events: Vec<(String, Value)> = sync_actions::table
        .filter(sync_actions::aggregate_id.eq(assigned.id.to_string()))
        .select((sync_actions::event_type, sync_actions::data))
        .load(&mut conn)
        .expect("sync actions");
    assert!(
        events
            .iter()
            .any(|(kind, data)| kind == "ticket.assignee_changed"
                && data["assignee_uuid"] == json!(a.admin_uuid)
                && data.get("previous_assignee_uuid").is_some()),
        "{events:?}"
    );
}

/// A ticket added from a board's column (the kanban quick-add) goes through
/// the assignment rules like any other new ticket.
#[actix_web::test]
async fn a_ticket_added_from_a_board_is_assigned_by_the_rules() {
    use std::sync::Arc;

    use actix_web::dev::Service;
    use actix_web::http::StatusCode;
    use actix_web::test as http_test;
    use actix_web::{web, App, HttpMessage};
    use diesel::sql_types::Integer;

    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::services::search::SearchService;

    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = Integer)]
        id: i32,
    }

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    let admin = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
    let (project, open_state) = with_actor_context(&mut pool.get().expect("conn"), &admin, |c| {
        diesel::insert_into(assignment_rules::table)
            .values(&NewAssignmentRule {
                name: "Everything to the admin".into(),
                description: None,
                priority: 1,
                is_active: true,
                method: AssignmentMethod::DirectUser,
                target_user_uuid: Some(a.admin_uuid),
                target_group_id: None,
                trigger_on_create: true,
                trigger_on_category_change: false,
                category_id: None,
                conditions: None,
                created_by: Some(a.admin_uuid),
            })
            .execute(c)?;
        let project =
            diesel::sql_query("INSERT INTO projects (name) VALUES ('Office move') RETURNING id")
                .get_result::<Id>(c)?
                .id;
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(a.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        Ok::<_, diesel::result::Error>((project, open_state))
    })
    .expect("seed rule and project");

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: a.admin_uuid.to_string(),
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
        workspace_id: a.workspace_id,
        workspace_uuid: a.workspace_uuid,
        slug: a.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = uuid::Uuid::now_v7();
    let actor = ActorContext::user(a.admin_uuid, Some(corr)).with_workspace(a.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
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
            .service(web::scope("/api").configure(backend::handlers::projects::config)),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/projects/{project}/tickets/new"))
            .set_json(json!({ "title": "Move the printers", "workflow_state_id": open_state }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = http_test::read_body_json(resp).await;
    let id = created["id"].as_i64().expect("ticket id") as i32;

    let assignee: Option<uuid::Uuid> =
        with_actor_context(&mut pool.get().expect("conn"), &admin, |c| {
            tickets::table
                .find(id)
                .select(tickets::assignee_uuid)
                .first(c)
        })
        .expect("ticket");
    assert_eq!(assignee, Some(a.admin_uuid), "the rule assigned it");
    assert_eq!(
        created["assignee"],
        json!(a.admin_uuid.to_string()),
        "and the response says so"
    );
}

/// An assignment made by a rule is credited to the rule, not to whoever
/// created the ticket: the `ticket.assignee_changed` it records carries a
/// system actor naming the rule.
#[test]
fn an_assignment_by_a_rule_is_credited_to_the_rule() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let mut conn = db.pool_with_size(1).get().expect("conn");

    let creator = ActorContext::user(a.member_uuid, None).with_workspace(a.workspace_id);
    let (rule_id, ticket): (i32, Ticket) = with_actor_context(&mut conn, &creator, |c| {
        let rule_id = diesel::insert_into(assignment_rules::table)
            .values(&NewAssignmentRule {
                name: "Everything to the admin".into(),
                description: None,
                priority: 1,
                is_active: true,
                method: AssignmentMethod::DirectUser,
                target_user_uuid: Some(a.admin_uuid),
                target_group_id: None,
                trigger_on_create: true,
                trigger_on_category_change: false,
                category_id: None,
                conditions: None,
                created_by: Some(a.admin_uuid),
            })
            .returning(assignment_rules::id)
            .get_result::<i32>(c)?;
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(a.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        let ticket = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Laptop won't charge".into(),
                workflow_state_id: open_state,
                requester_uuid: Some(a.member_uuid),
                ..Default::default()
            })
            .get_result(c)?;
        Ok::<_, diesel::result::Error>((rule_id, ticket))
    })
    .expect("seed rule and ticket");

    // Created in the app: the rules run under the creating request's actor.
    let assigned = assign_new_ticket(
        &mut ActorConn {
            conn: &mut conn,
            actor: &creator,
        },
        None,
        ticket,
    );
    assert_eq!(assigned.assignee_uuid, Some(a.admin_uuid));

    let credited: Vec<(String, Option<uuid::Uuid>, Option<String>)> = sync_actions::table
        .filter(sync_actions::aggregate_id.eq(assigned.id.to_string()))
        .filter(sync_actions::event_type.eq("ticket.assignee_changed"))
        .select((
            sync_actions::actor_kind,
            sync_actions::actor_uuid,
            sync_actions::actor_ref,
        ))
        .load(&mut conn)
        .expect("sync actions");
    assert_eq!(
        credited,
        vec![(
            "system".to_string(),
            None,
            Some(format!("assignment_rule:{rule_id}"))
        )]
    );
}
