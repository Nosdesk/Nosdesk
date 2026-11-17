//! New tickets are assigned by the rules engine. A ticket's `ticket.created`
//! (or, for a category change, its `ticket.category_changed`) reaches the
//! rule event outbox, and the dispatcher runs the assignment rules on it, so
//! the assignee arrives a moment after the write that made the ticket, not in
//! its response (decision D3). The rules here have the shape the migration
//! gives an assignment rule: an unassigned ticket, an `assign` step, then
//! `stop_processing`.
#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, NewTicket, RuleApplicationStatus as Status, RuleState, RuleTriggerKind as Trigger,
    TicketUpdate,
};
use backend::repository::{tickets, workflow_states};
use backend::schema::{sync_actions, ticket_approvals, ticket_categories};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;

use crate::common::WorkspaceSeed;
use crate::rules_event_triggers::{leaf, Harness, Spec};

/// The condition a migrated assignment rule starts from: nobody is assigned.
fn unassigned() -> Value {
    json!({ "kind": "leaf", "field": "ticket.assignee_uuid", "op": "is_null" })
}

fn assign_to(user: Uuid) -> Value {
    json!([
        { "kind": "assign", "config": { "method": "direct", "user_uuid": user } },
        { "kind": "stop_processing" },
    ])
}

/// A live rule assigning `user` the tickets that meet `also` and that nobody
/// is assigned.
fn assignment_rule(trigger: Trigger, trigger_config: Value, also: &[Value], user: Uuid) -> Spec {
    let mut children = vec![unassigned()];
    children.extend_from_slice(also);
    Spec {
        trigger,
        state: RuleState::Live,
        trigger_config,
        conditions: json!({ "kind": "and", "children": children }),
        actions: assign_to(user),
        priority: 1010,
    }
}

fn on_create(user: Uuid) -> Spec {
    assignment_rule(Trigger::TicketCreated, json!({}), &[], user)
}

fn assignee(h: &Harness, ws: &WorkspaceSeed, ticket: i32) -> Option<Uuid> {
    h.as_admin(ws, |c| tickets::get_ticket_by_id(c, ticket))
        .assignee_uuid
}

#[actix_web::test]
async fn a_new_ticket_is_assigned_once_its_event_is_dispatched() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, on_create(a.admin_uuid));

    let ticket = h.ticket(a, "Printer jam");
    assert_eq!(
        assignee(&h, a, ticket),
        None,
        "not in the write that made it"
    );
    h.drain().await;
    assert_eq!(assignee(&h, a, ticket), Some(a.admin_uuid));
    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);

    // The assignment went out as a change with the previous assignee, which
    // the notification deriver reads to tell the new assignee, credited to
    // the rule rather than to whoever made the ticket.
    let events: Vec<(Value, String, Option<Uuid>, Option<String>)> = h.as_admin(a, |c| {
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(ticket.to_string()))
            .filter(sync_actions::event_type.eq("ticket.assignee_changed"))
            .select((
                sync_actions::data,
                sync_actions::actor_kind,
                sync_actions::actor_uuid,
                sync_actions::actor_ref,
            ))
            .load(c)
    });
    assert_eq!(events.len(), 1, "{events:?}");
    let (data, kind, actor_uuid, actor_ref) = &events[0];
    assert_eq!(data["assignee_uuid"], json!(a.admin_uuid));
    assert!(data.get("previous_assignee_uuid").is_some(), "{data}");
    assert_eq!(
        (kind.as_str(), *actor_uuid, actor_ref.as_deref()),
        (
            "system",
            None,
            Some(format!("rules_engine:{rule}").as_str())
        )
    );
}

/// The rules route only a ticket nobody is assigned: one made with an
/// assignee keeps them.
#[actix_web::test]
async fn a_ticket_made_with_an_assignee_keeps_them() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let team = h.as_admin(a, |c| {
        use backend::schema::{groups, user_groups};
        let team: i32 = diesel::insert_into(groups::table)
            .values(groups::name.eq("Service desk"))
            .returning(groups::id)
            .get_result(c)?;
        diesel::insert_into(user_groups::table)
            .values((
                user_groups::group_id.eq(team),
                user_groups::user_uuid.eq(a.admin_uuid),
            ))
            .execute(c)?;
        Ok(team)
    });
    let rule = h.rule(
        a,
        Spec {
            actions: json!([
                { "kind": "assign", "config": { "method": "group", "group_id": team } },
                { "kind": "stop_processing" },
            ]),
            ..on_create(a.admin_uuid)
        },
    );
    let ticket = h.as_admin(a, |c| {
        let open = workflow_states::default_state(c)?;
        tickets::create_ticket(
            c,
            NewTicket {
                title: "Laptop".to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(a.member_uuid),
                assignee_uuid: Some(a.admin_uuid),
                ..Default::default()
            },
        )
    });
    h.drain().await;
    assert_eq!(assignee(&h, a, ticket.id), Some(a.admin_uuid));
    assert!(h.statuses(a, rule).is_empty(), "the rule didn't run");
    assert_eq!(h.rule_row(a, rule).skipped_condition_unmet_count, 1);
}

/// A category change runs the category change rules: an unassigned ticket
/// moved into the category is assigned, an assigned one keeps its assignee.
#[actix_web::test]
async fn a_category_change_assigns_an_unassigned_ticket() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let hardware: i32 = h.as_admin(a, |c| {
        diesel::insert_into(ticket_categories::table)
            .values(ticket_categories::name.eq("Hardware"))
            .returning(ticket_categories::id)
            .get_result(c)
    });
    let rule = h.rule(
        a,
        assignment_rule(
            Trigger::TicketUpdated,
            json!({ "changed_fields_include": ["ticket.category_id"] }),
            &[leaf("ticket.category_id", "eq", json!(hardware))],
            a.admin_uuid,
        ),
    );
    let unassigned = h.ticket(a, "Monitor flickers");
    let taken = h.ticket(a, "Dock dead");
    h.update(
        a,
        taken,
        TicketUpdate {
            assignee_uuid: Some(Some(a.admin_uuid)),
            ..Default::default()
        },
    );
    h.drain().await;
    for ticket in [unassigned, taken] {
        h.update(
            a,
            ticket,
            TicketUpdate {
                category_id: Some(Some(hardware)),
                ..Default::default()
            },
        );
    }
    assert_eq!(assignee(&h, a, unassigned), None);
    h.drain().await;
    assert_eq!(assignee(&h, a, unassigned), Some(a.admin_uuid));
    assert_eq!(assignee(&h, a, taken), Some(a.admin_uuid));
    let applications = h.applications(a, rule);
    assert_eq!(applications.len(), 1, "only the unassigned ticket");
    assert_eq!(applications[0].ticket_id, unassigned);
    assert_eq!(
        applications[0].originating_event_kind.as_deref(),
        Some("ticket.category_changed")
    );
}

/// Call `config`'s routes as `ws`'s admin.
async fn call_as_admin(
    h: &Harness,
    ws: &WorkspaceSeed,
    config: fn(&mut web::ServiceConfig),
    req: http_test::TestRequest,
) -> (StatusCode, Value) {
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
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &h.pool).expect("init search"));
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(h.pool.clone()))
            .app_data(web::Data::new(search))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(config)),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let bytes = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A ticket added from a board's column (the kanban quick-add) is assigned
/// like any other new ticket, after its response.
#[actix_web::test]
async fn a_ticket_added_from_a_board_is_assigned_after_its_response() {
    use diesel::sql_types::Integer;

    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = Integer)]
        id: i32,
    }

    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    h.rule(a, on_create(a.admin_uuid));
    let (project, open_state) = h.as_admin(a, |c| {
        let project =
            diesel::sql_query("INSERT INTO projects (name) VALUES ('Office move') RETURNING id")
                .get_result::<Id>(c)?
                .id;
        Ok((project, workflow_states::default_state(c)?.id))
    });

    let (status, created) = call_as_admin(
        &h,
        a,
        backend::handlers::projects::config,
        http_test::TestRequest::post()
            .uri(&format!("/api/projects/{project}/tickets/new"))
            .set_json(json!({ "title": "Move the printers", "workflow_state_id": open_state })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    // The response comes first: the ticket is unassigned in it.
    assert!(
        matches!(created["assignee"].as_str(), None | Some("")),
        "{created}"
    );
    let id = created["id"].as_i64().expect("ticket id") as i32;

    h.drain().await;
    assert_eq!(
        assignee(&h, a, id),
        Some(a.admin_uuid),
        "the rule assigned it"
    );
}

/// Make `ws` hold requests back while they wait for approval, with a request
/// type its admin approves. Returns the request type.
fn holding_requests(h: &Harness, ws: &WorkspaceSeed) -> i32 {
    use backend::schema::site_settings;
    h.as_admin(ws, |c| {
        backend::repository::site_settings::get_site_settings(c)?;
        diesel::update(site_settings::table)
            .set(site_settings::approval_waiting_display.eq("held"))
            .execute(c)?;
        let laptops: i32 = diesel::insert_into(ticket_categories::table)
            .values((
                ticket_categories::name.eq("New laptop"),
                ticket_categories::approval_required.eq(true),
                ticket_categories::approval_rule.eq("any"),
            ))
            .returning(ticket_categories::id)
            .get_result(c)?;
        backend::repository::categories::set_category_approvers(c, laptops, &[ws.admin_uuid])?;
        Ok(laptops)
    })
}

/// A request of `request_type` from `ws`'s member, made the way the app
/// makes one.
fn request(h: &Harness, ws: &WorkspaceSeed, request_type: i32, title: &str) -> i32 {
    h.as_admin(ws, |c| {
        let open = workflow_states::default_state(c)?;
        tickets::create_ticket(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(ws.member_uuid),
                category_id: Some(request_type),
                ..Default::default()
            },
        )
    })
    .id
}

/// A request held back for approval is turned away from its rules until it
/// goes ahead: approved in the app, or its approval skipped. Then its
/// creation rules run, and they assign it.
#[actix_web::test]
async fn a_held_request_is_assigned_once_it_goes_ahead() {
    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, on_create(a.admin_uuid));
    let laptops = holding_requests(&h, a);
    let approved = request(&h, a, laptops, "New laptop for Sam");
    let skipped = request(&h, a, laptops, "New laptop for Jo");

    h.drain().await;
    for ticket in [approved, skipped] {
        assert_eq!(assignee(&h, a, ticket), None, "held while it waits");
    }
    assert!(h.statuses(a, rule).is_empty());
    assert_eq!(h.rule_row(a, rule).skipped_preflight_count, 2);

    for (ticket, action, body) in [
        (approved, "decide", json!({ "decision": "approve" })),
        (skipped, "skip", json!({ "comment": "Urgent" })),
    ] {
        let (status, body) = call_as_admin(
            &h,
            a,
            backend::handlers::approvals::config,
            http_test::TestRequest::post()
                .uri(&format!("/api/tickets/{ticket}/approval/{action}"))
                .set_json(body),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{action}: {body}");
    }

    h.drain().await;
    for ticket in [approved, skipped] {
        assert_eq!(assignee(&h, a, ticket), Some(a.admin_uuid));
    }
    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded; 2]);
}

/// A held request approved before the engine got to its `ticket.created`
/// runs its creation rules once: the release runs them, and its creation,
/// dispatched after the approval, runs nothing.
#[actix_web::test]
async fn a_request_approved_before_its_creation_is_dispatched_runs_its_rules_once() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let tag = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
            )
        },
    );
    let assign = h.rule(a, on_create(a.admin_uuid));
    let laptops = holding_requests(&h, a);
    let ticket = request(&h, a, laptops, "New laptop for Lee");
    let approved = h.as_admin(a, |c| {
        backend::repository::ticket_approvals::auto_approve(c, ticket)
    });
    assert!(approved);

    // Both events are queued: the creation first, then the release.
    h.drain().await;
    assert_eq!(assignee(&h, a, ticket), Some(a.admin_uuid));
    assert_eq!(h.statuses(a, tag), vec![Status::Succeeded]);
    assert_eq!(h.statuses(a, assign), vec![Status::Succeeded]);
    let assignments: i64 = h.as_admin(a, |c| {
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(ticket.to_string()))
            .filter(sync_actions::event_type.eq("ticket.assignee_changed"))
            .count()
            .get_result(c)
    });
    assert_eq!(assignments, 1);
}

/// A held request a workspace import brought in has no `ticket.created` in
/// this server's history. Its approval still runs its creation rules: the
/// hold travels on the ticket.
#[actix_web::test]
async fn a_held_request_moved_by_import_is_assigned_once_approved() {
    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, on_create(a.admin_uuid));
    let laptops = holding_requests(&h, a);
    // Written the way the import writes rows: no events.
    let ticket = h.as_admin(a, |c| {
        use backend::schema::tickets as t;
        let open = workflow_states::default_state(c)?;
        let id: i32 = diesel::insert_into(t::table)
            .values((
                t::title.eq("New laptop for Ana"),
                t::workflow_state_id.eq(open.id),
                t::requester_uuid.eq(Some(a.member_uuid)),
                t::category_id.eq(Some(laptops)),
                t::approval_state.eq(Some("pending")),
                t::rules_held_at.eq(Some(chrono::Utc::now().naive_utc())),
            ))
            .returning(t::id)
            .get_result(c)?;
        diesel::insert_into(ticket_approvals::table)
            .values((
                ticket_approvals::ticket_id.eq(id),
                ticket_approvals::approver_uuid.eq(a.admin_uuid),
                ticket_approvals::round.eq(1),
            ))
            .execute(c)?;
        Ok(id)
    });
    assert!(
        h.as_admin(a, |c| backend::repository::rules::created_event(c, ticket))
            .is_none(),
        "no creation event to replay"
    );

    let (status, body) = call_as_admin(
        &h,
        a,
        backend::handlers::approvals::config,
        http_test::TestRequest::post()
            .uri(&format!("/api/tickets/{ticket}/approval/decide"))
            .set_json(json!({ "decision": "approve" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    h.drain().await;
    assert_eq!(assignee(&h, a, ticket), Some(a.admin_uuid));
    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);
}

/// A CSV import brings in history, not requests: an imported ticket of a
/// request type that needs approval starts no round, and its creation runs
/// no rules.
#[actix_web::test]
async fn an_imported_ticket_starts_no_approval_and_runs_no_rules() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, on_create(a.admin_uuid));
    let laptops = holding_requests(&h, a);
    let ticket = h.as_admin(a, |c| {
        let open = workflow_states::default_state(c)?;
        tickets::create_ticket(
            c,
            NewTicket {
                title: "Laptop from 2019".to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(a.member_uuid),
                category_id: Some(laptops),
                submitted_via: Some(backend::models::CSV_IMPORT.to_string()),
                ..Default::default()
            },
        )
    });
    assert_eq!(ticket.approval_state, None, "no approval round");
    let approvals: i64 = h.as_admin(a, |c| {
        ticket_approvals::table
            .filter(ticket_approvals::ticket_id.eq(ticket.id))
            .count()
            .get_result(c)
    });
    assert_eq!(approvals, 0);
    h.drain().await;
    assert_eq!(assignee(&h, a, ticket.id), None);
    assert!(h.statuses(a, rule).is_empty());
    let row = h.rule_row(a, rule);
    assert_eq!(
        (
            row.skipped_preflight_count,
            row.skipped_condition_unmet_count
        ),
        (0, 0),
        "not counted as a skip"
    );
}
