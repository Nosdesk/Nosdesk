//! One rule run on one ticket from an automated source: a shadow run
//! records what the rule would do and writes nothing else; a failed run
//! rolls back and leaves a `failed` row; a live run writes as the engine's
//! system actor and replies as the rule's author. A manual apply that fails
//! still leaves no row, and a rule that runs on its own can't be saved with
//! a reply step. Runs go through `nosdesk_app` the way the engine will.
#![allow(clippy::expect_used)]

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{
    NewRule, NewTicket, RuleApplicationStatus, RuleState, RuleTriggerKind, TicketPriority,
};
use backend::repository::rules::{
    self, ApplyContext, ApplyError, ApplyInput, ApplyOverrides, ApplySource, OriginatingEvent,
};
use backend::schema::{comments, rule_applications, sync_actions, ticket_tags, tickets};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, WorkspaceSeed};
use crate::rules_in_workspace::as_admin;

fn admin(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id)
}

/// The engine's actor for `rule_id`, with a chain id like the consumer's.
fn engine(ws: &WorkspaceSeed, rule_id: i32) -> ActorContext {
    let mut actor =
        ActorContext::system(format!("rules_engine:{rule_id}")).with_workspace(ws.workspace_id);
    actor.correlation_id = Some(Uuid::now_v7());
    actor
}

/// A rule saved by `ws`'s admin straight through the repository, so any
/// trigger, state and steps can be set up.
fn rule(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    kind: RuleTriggerKind,
    state: RuleState,
    actions: Value,
) -> i32 {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        rules::create(
            c,
            NewRule {
                workspace_id: ws.workspace_id,
                name: format!("{kind:?} {state:?}"),
                description: None,
                trigger_kind: kind,
                trigger_config: json!({}),
                conditions: json!([]),
                actions,
                reads_set: Vec::new(),
                writes_set: Vec::new(),
                state,
                priority: 100,
                created_by: Some(ws.admin_uuid),
            },
        )
    })
    .expect("save a rule")
    .id
}

/// An unassigned ticket in the default state and a tag, both in `ws`.
fn ticket_and_tag(pool: &TestPool, ws: &WorkspaceSeed) -> (i32, i32) {
    use backend::schema::{tags, workflow_states};
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(ws.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        let ticket: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer offline".to_string(),
                workflow_state_id: open_state,
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)?;
        let tag: i32 = diesel::insert_into(tags::table)
            .values(tags::name.eq("printers"))
            .returning(tags::id)
            .get_result(c)?;
        Ok::<_, diesel::result::Error>((ticket, tag))
    })
    .expect("seed a ticket and a tag")
}

/// What a run could have changed: the ticket's priority, assignee, tags and
/// comments, its sync actions, and the rule's fire count.
#[derive(Debug, PartialEq)]
struct Footprint {
    priority: TicketPriority,
    assignee: Option<Uuid>,
    tags: Vec<i32>,
    comments: i64,
    sync_events: Vec<String>,
    fire_count: i32,
}

fn footprint(pool: &TestPool, ws: &WorkspaceSeed, ticket: i32, rule_id: i32) -> Footprint {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        let (priority, assignee) = tickets::table
            .find(ticket)
            .select((tickets::priority, tickets::assignee_uuid))
            .first(c)?;
        let tags = ticket_tags::table
            .filter(ticket_tags::ticket_id.eq(ticket))
            .select(ticket_tags::tag_id)
            .order(ticket_tags::tag_id)
            .load(c)?;
        let comments = comments::table
            .filter(comments::ticket_id.eq(ticket))
            .count()
            .get_result(c)?;
        let sync_events = sync_actions::table
            .filter(sync_actions::workspace_id.eq(ws.workspace_id))
            .filter(sync_actions::aggregate_id.eq(ticket.to_string()))
            .order(sync_actions::sync_id)
            .select(sync_actions::event_type)
            .load(c)?;
        let fire_count = backend::schema::rules::table
            .find(rule_id)
            .select(backend::schema::rules::fire_count)
            .first(c)?;
        Ok::<_, diesel::result::Error>(Footprint {
            priority,
            assignee,
            tags,
            comments,
            sync_events,
            fire_count,
        })
    })
    .expect("read the ticket")
}

fn applications(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    rule_id: i32,
) -> Vec<backend::models::RuleApplication> {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        rule_applications::table
            .filter(rule_applications::rule_id.eq(rule_id))
            .load(c)
    })
    .expect("read applications")
}

/// A rule in dry run records each step it would take, as the live run would
/// resolve it, and changes nothing: not the ticket, not the rule's fire
/// count, and no sync action goes out.
#[test]
fn a_shadow_run_records_what_it_would_do_and_writes_nothing_else() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    let a = &seeded.a;
    let (ticket, tag) = ticket_and_tag(&pool, a);
    let id = rule(
        &pool,
        a,
        RuleTriggerKind::TicketUpdated,
        RuleState::DryRun,
        json!([
            { "kind": "set_priority", "config": { "priority": "urgent" } },
            { "kind": "assign", "config": { "method": "direct", "user_uuid": a.admin_uuid } },
            { "kind": "add_tags", "config": { "tag_ids": [tag] } },
            { "kind": "reply", "config": { "visibility": "public", "body": "On it" } },
            { "kind": "stop_processing" }
        ]),
    );
    let before = footprint(&pool, a, ticket, id);

    let actor = engine(a, id);
    let event = Uuid::now_v7();
    let outcome = rules::run_rule_on_ticket(
        &mut pool.get().expect("conn"),
        ApplyContext {
            shadow: true,
            originating_event: Some(OriginatingEvent {
                id: event,
                kind: "ticket.updated".to_string(),
            }),
            ..ApplyContext::new(&actor, id, ticket, ApplySource::Event)
        },
    )
    .expect("shadow run");

    assert_eq!(outcome.application.status, RuleApplicationStatus::DryRun);
    assert_eq!(outcome.actions_executed, 0);
    assert_eq!(outcome.comment_id, None);
    assert!(outcome.stop_processing);
    assert_eq!(outcome.application.actor_kind, "system");
    assert_eq!(outcome.application.actor_uuid, None);
    assert_eq!(outcome.application.correlation_id, actor.correlation_id);
    assert_eq!(outcome.application.originating_event_id, Some(event));
    let taken = outcome.application.actions_taken.expect("would-do steps");
    assert_eq!(taken[0]["priority"], "urgent");
    assert_eq!(taken[1]["assigned_to_uuid"], json!(a.admin_uuid));
    assert_eq!(taken[2]["tag_ids_added"], json!([tag]));
    assert_eq!(taken[3]["kind"], "reply");
    assert_eq!(taken.as_array().map(Vec::len), Some(5));

    assert_eq!(footprint(&pool, a, ticket, id), before, "nothing written");
    assert_eq!(applications(&pool, a, id).len(), 1);
}

/// Shadow is the dry-run state's run, and only its: a live rule doesn't run
/// in shadow, and a rule in dry run doesn't run for real.
#[test]
fn shadow_follows_the_rule_state() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    let a = &seeded.a;
    let (ticket, _) = ticket_and_tag(&pool, a);
    let steps = json!([{ "kind": "set_priority", "config": { "priority": "high" } }]);
    let live = rule(
        &pool,
        a,
        RuleTriggerKind::TicketCreated,
        RuleState::Live,
        steps.clone(),
    );
    let shadow = rule(
        &pool,
        a,
        RuleTriggerKind::TicketCreated,
        RuleState::DryRun,
        steps,
    );
    let manual = rule(
        &pool,
        a,
        RuleTriggerKind::Manual,
        RuleState::Live,
        json!([{ "kind": "unassign" }]),
    );

    let run = |id: i32, shadow: bool| {
        let actor = engine(a, id);
        rules::run_rule_on_ticket(
            &mut pool.get().expect("conn"),
            ApplyContext {
                shadow,
                ..ApplyContext::new(&actor, id, ticket, ApplySource::Event)
            },
        )
    };
    for (id, shadow) in [(live, true), (shadow, false)] {
        let failed = run(id, shadow).expect_err("wrong state");
        assert!(
            matches!(failed.error, ApplyError::NotLive(..)),
            "{:?}",
            failed.error
        );
        assert!(failed.recorded.is_none(), "turned away, not failed");
    }
    let failed = run(manual, false).expect_err("manual rules aren't events'");
    assert!(
        matches!(failed.error, ApplyError::WrongTrigger(..)),
        "{:?}",
        failed.error
    );
    for id in [live, shadow, manual] {
        assert!(applications(&pool, a, id).is_empty());
    }
}

/// A step that fails rolls back the steps before it, and the failure is
/// recorded on its own: one `failed` row naming the step, carrying the run's
/// chain id. The same rule applied by an agent fails with no row.
#[test]
fn a_failed_automated_run_rolls_back_and_records_a_failed_row() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    let a = &seeded.a;
    let (ticket, _) = ticket_and_tag(&pool, a);
    // The member is a requester, who can't be assigned tickets.
    let steps = json!([
        { "kind": "set_priority", "config": { "priority": "urgent" } },
        { "kind": "assign", "config": { "method": "direct", "user_uuid": a.member_uuid } }
    ]);
    let id = rule(
        &pool,
        a,
        RuleTriggerKind::TicketCreated,
        RuleState::Live,
        steps.clone(),
    );
    let before = footprint(&pool, a, ticket, id);

    let actor = engine(a, id);
    let failed = rules::run_rule_on_ticket(
        &mut pool.get().expect("conn"),
        ApplyContext::new(&actor, id, ticket, ApplySource::Event),
    )
    .expect_err("the assign step fails");
    assert!(
        matches!(failed.error, ApplyError::IneligibleAssignee(u) if u == a.member_uuid),
        "{:?}",
        failed.error
    );

    assert_eq!(footprint(&pool, a, ticket, id), before, "rolled back");
    let rows = applications(&pool, a, id);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(Some(row.id), failed.recorded.as_ref().map(|r| r.id));
    assert_eq!(row.status, RuleApplicationStatus::Failed);
    assert_eq!(row.actor_kind, "system");
    assert_eq!(row.correlation_id, actor.correlation_id);
    assert_eq!(row.actions_taken, None);
    let reason = row.failure_reason.as_deref().expect("failure reason");
    assert!(reason.starts_with("Step 2: "), "{reason}");

    // An agent's apply of the same steps fails the same way and records
    // nothing: the agent sees the error.
    let manual = rule(&pool, a, RuleTriggerKind::Manual, RuleState::Live, steps);
    let err = rules::apply_manual(
        &mut pool.get().expect("conn"),
        ApplyInput {
            rule_id: manual,
            ticket_id: ticket,
            overrides: ApplyOverrides::default(),
        },
        &admin(a),
        None,
    )
    .expect_err("the assign step fails");
    assert!(matches!(err, ApplyError::IneligibleAssignee(_)), "{err:?}");
    assert!(applications(&pool, a, manual).is_empty());
    assert_eq!(footprint(&pool, a, ticket, id), before);
}

/// A live automated run writes through the same steps as an agent's apply,
/// credited to the engine, and replies as the rule's author.
#[test]
fn a_live_automated_run_writes_and_replies_as_the_rules_author() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    let a = &seeded.a;
    let (ticket, _) = ticket_and_tag(&pool, a);
    let id = rule(
        &pool,
        a,
        RuleTriggerKind::TicketCreated,
        RuleState::Live,
        json!([
            { "kind": "set_priority", "config": { "priority": "urgent" } },
            { "kind": "reply", "config": { "visibility": "internal", "body": "Triaged" } }
        ]),
    );

    let actor = engine(a, id);
    let outcome = rules::run_rule_on_ticket(
        &mut pool.get().expect("conn"),
        ApplyContext::new(&actor, id, ticket, ApplySource::Event),
    )
    .expect("run");
    assert_eq!(outcome.application.status, RuleApplicationStatus::Succeeded);
    assert_eq!(outcome.application.actor_kind, "system");
    assert_eq!(outcome.actions_executed, 2);
    assert!(!outcome.stop_processing);

    let after = footprint(&pool, a, ticket, id);
    assert_eq!(after.priority, TicketPriority::Urgent);
    assert_eq!(after.fire_count, 1);
    assert!(after.sync_events.iter().any(|e| e == "ticket.rule_applied"));
    let mut conn = pool.get().expect("conn");
    let author: Uuid = with_actor_context(&mut conn, &admin(a), |c| {
        comments::table
            .find(outcome.comment_id.expect("a reply"))
            .select(comments::user_uuid)
            .first(c)
    })
    .expect("read the reply");
    assert_eq!(author, a.admin_uuid);
}

/// A rule that runs on its own saves with a reply step, on create or by
/// switching a manual rule's trigger: it sends as its author. One that sends
/// as someone who can't work tickets can be parked, not run.
#[actix_web::test]
async fn a_rule_that_runs_on_its_own_replies_as_an_agent() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let a = &seeded.a;
    let reply =
        json!([{ "kind": "reply", "config": { "visibility": "public", "body": "Thanks" } }]);
    let high = json!({ "kind": "and", "children": [
        { "kind": "leaf", "field": "ticket.priority", "op": "eq", "value": "high" }
    ]});

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Acknowledge",
                "trigger_kind": "ticket_created",
                "conditions": high,
                "actions": reply,
            })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);

    let manual = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Acknowledge",
                "trigger_kind": "manual",
                "actions": reply,
            })),
    )
    .await;
    assert_eq!(manual.status(), StatusCode::CREATED);
    let manual: Value = http_test::read_body_json(manual).await;
    let id = manual["id"].as_i64().expect("rule id");

    let switched = as_admin(
        &pool,
        a,
        http_test::TestRequest::put()
            .uri(&format!("/api/rules/{id}"))
            .set_json(json!({ "trigger_kind": "ticket_created", "conditions": high })),
    )
    .await;
    assert_eq!(switched.status(), StatusCode::OK);

    // Sending as a plain member, who can't work tickets: parked, not run.
    let saved = rule(
        &pool,
        a,
        RuleTriggerKind::TicketCreated,
        RuleState::Draft,
        json!([{ "kind": "reply", "config": {
            "visibility": "public", "body": "Thanks", "send_as_uuid": a.member_uuid,
        } }]),
    );
    let set_state = |state: &str| {
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{saved}/state"))
            .set_json(json!({ "state": state }))
    };
    for state in ["live", "dry_run"] {
        let refused = as_admin(&pool, a, set_state(state)).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{state}");
        let body: Value = http_test::read_body_json(refused).await;
        assert_eq!(body["code"], "RULE_SEND_AS_INELIGIBLE", "{state}: {body}");
    }
    let paused = as_admin(&pool, a, set_state("paused")).await;
    assert_eq!(paused.status(), StatusCode::OK);
    let body: Value = http_test::read_body_json(paused).await;
    assert_eq!(body["state"], "paused", "{body}");
}
