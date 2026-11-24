//! The asset steps a manual rule takes on a ticket: assigning its linked
//! assets to the requester, lending them, and taking back everything the
//! requester has. Each runs through `apply_manual` as `nosdesk_app` in a
//! workspace, the way an agent's apply does. A ticket with no linked assets,
//! or a requester with nothing out, is a clean no-op; another workspace's
//! assets and loans are never touched; the asset changes never enqueue rule
//! events; and the conflicts inspector sees the steps' asset write keys.
#![allow(clippy::expect_used)]

use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{
    AssetLifecycleEvent, AssetLoan, AssetStatus, NewAsset, NewTicket, RuleApplicationStatus,
    RuleState, RuleTriggerKind,
};
use backend::repository::rules::{
    self, ApplyContext, ApplyError, ApplyInput, ApplyOverrides, ApplySource,
};
use backend::repository::{asset_lifecycle, asset_loans, tickets};
use backend::schema::{
    asset_lifecycle_events, asset_loans as loans_t, assets, rule_applications, rule_event_outbox,
    sync_actions, ticket_assets, workflow_states,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, TestPool, TwoWorkspaces, WorkspaceSeed};

fn admin(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, Some(Uuid::now_v7())).with_workspace(ws.workspace_id)
}

/// A rule saved by `ws`'s admin straight through the repository, which
/// derives its reads and writes the way a save through the API does.
fn rule(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    kind: RuleTriggerKind,
    conditions: Value,
    actions: Value,
) -> i32 {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        rules::create(
            c,
            backend::models::NewRule {
                workspace_id: ws.workspace_id,
                name: format!("{kind:?} {}", actions[0]["kind"]),
                description: None,
                trigger_kind: kind,
                trigger_config: json!({}),
                conditions,
                actions,
                reads_set: Vec::new(),
                writes_set: Vec::new(),
                state: RuleState::Live,
                priority: 100,
                created_by: Some(ws.admin_uuid),
            },
        )
    })
    .expect("save a rule")
    .id
}

fn manual_rule(pool: &TestPool, ws: &WorkspaceSeed, actions: Value) -> i32 {
    rule(pool, ws, RuleTriggerKind::Manual, json!([]), actions)
}

/// A ticket in `ws` in its default state, from `requester`.
fn ticket(pool: &TestPool, ws: &WorkspaceSeed, requester: Option<Uuid>) -> i32 {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(ws.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "New starter".to_string(),
                workflow_state_id: open_state,
                requester_uuid: requester,
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)
    })
    .expect("seed a ticket")
}

/// An asset in `ws` in `status`, used by `user`.
fn asset(pool: &TestPool, ws: &WorkspaceSeed, name: &str, status: &str, user: Option<Uuid>) -> i32 {
    let mut conn = pool.get().expect("conn");
    let status = AssetStatus::from_str_opt(status).expect("a status key");
    with_actor_context(&mut conn, &admin(ws), |c| {
        let (asset, _) = asset_lifecycle::create(
            c,
            NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: user,
                purchase_date: None,
                asset_tag: None,
                kind: "device".to_string(),
                attributes: json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            },
            None,
            status,
            None,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        Ok::<_, diesel::result::Error>(asset.id)
    })
    .expect("seed an asset")
}

fn link(pool: &TestPool, ws: &WorkspaceSeed, ticket_id: i32, asset_id: i32) {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        tickets::add_device_to_ticket(c, ticket_id, asset_id)
    })
    .expect("link the asset");
}

/// Lend `asset_id` in `ws` to `borrower`, from no ticket.
fn lend(pool: &TestPool, ws: &WorkspaceSeed, asset_id: i32, borrower: Uuid) -> i32 {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &admin(ws), |c| {
        asset_loans::issue(
            c,
            asset_loans::IssueLoan {
                asset_id,
                borrower_user_uuid: borrower,
                loaned_at: None,
                due_back: None,
                ticket_id: None,
                notes: None,
                actor_uuid: Some(ws.admin_uuid),
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("lend the asset")
    .id
}

/// Apply `rule_id` to `ticket_id` as `ws`'s admin, the way the apply route does.
fn apply(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    actor: &ActorContext,
    rule_id: i32,
    ticket_id: i32,
) -> Result<rules::ApplyOutcome, ApplyError> {
    let _ = ws;
    rules::apply_manual(
        &mut pool.get().expect("conn"),
        ApplyInput {
            rule_id,
            ticket_id,
            overrides: ApplyOverrides::default(),
        },
        actor,
        None,
    )
}

/// The first step's entry in a run's log.
fn first_step(outcome: &rules::ApplyOutcome) -> Value {
    outcome.application.actions_taken.as_ref().expect("steps")[0].clone()
}

fn ids(v: &Value) -> Vec<i64> {
    v.as_array()
        .expect("an id list")
        .iter()
        .map(|x| x.as_i64().expect("an id"))
        .collect()
}

/// An asset's status and user, read past row security.
fn asset_state(admin_pool: &TestPool, asset_id: i32) -> (String, Option<Uuid>) {
    assets::table
        .find(asset_id)
        .select((assets::status, assets::primary_user_uuid))
        .first(&mut admin_pool.get().expect("conn"))
        .expect("read the asset")
}

fn loans_of(admin_pool: &TestPool, asset_id: i32) -> Vec<AssetLoan> {
    loans_t::table
        .filter(loans_t::asset_id.eq(asset_id))
        .order(loans_t::id)
        .load(&mut admin_pool.get().expect("conn"))
        .expect("read loans")
}

fn lifecycle_of(admin_pool: &TestPool, asset_id: i32) -> Vec<AssetLifecycleEvent> {
    asset_lifecycle_events::table
        .filter(asset_lifecycle_events::asset_id.eq(asset_id))
        .order(asset_lifecycle_events::id)
        .load(&mut admin_pool.get().expect("conn"))
        .expect("read lifecycle events")
}

fn applications(admin_pool: &TestPool, rule_id: i32) -> Vec<backend::models::RuleApplication> {
    rule_applications::table
        .filter(rule_applications::rule_id.eq(rule_id))
        .load(&mut admin_pool.get().expect("conn"))
        .expect("read applications")
}

/// The sync actions a run wrote, by its correlation id: event type and
/// whether the rule event outbox took it.
fn run_events(admin_pool: &TestPool, actor: &ActorContext) -> Vec<(String, bool)> {
    let mut conn = admin_pool.get().expect("conn");
    let rows: Vec<(i64, String)> = sync_actions::table
        .filter(sync_actions::correlation_id.eq(actor.correlation_id.expect("a correlation id")))
        .order(sync_actions::sync_id)
        .select((sync_actions::sync_id, sync_actions::event_type))
        .load(&mut conn)
        .expect("read sync actions");
    rows.into_iter()
        .map(|(sync_id, event_type)| {
            let enqueued = diesel::select(diesel::dsl::exists(
                rule_event_outbox::table.filter(rule_event_outbox::sync_id.eq(sync_id)),
            ))
            .get_result(&mut conn)
            .expect("read the outbox");
            (event_type, enqueued)
        })
        .collect()
}

struct Setup {
    _db: common::TestDb,
    seeded: TwoWorkspaces,
    /// Superuser: seeds and reads past row security.
    admin_pool: TestPool,
    /// `nosdesk_app`, as the app runs.
    pool: TestPool,
}

fn setup() -> Setup {
    let db = common::TestDb::new();
    let admin_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut admin_pool.get().expect("conn"));
    let pool = db.runtime_pool(2);
    Setup {
        _db: db,
        seeded,
        admin_pool,
        pool,
    }
}

/// Each linked asset gets the requester as its user; one in stock goes in
/// service, the move linked to the ticket. Applying again changes nothing.
#[test]
fn assigning_gives_the_linked_assets_to_the_requester() {
    let s = setup();
    let a = &s.seeded.a;
    let requester = a.member_uuid;
    let ticket_id = ticket(&s.pool, a, Some(requester));
    let stocked = asset(&s.pool, a, "Laptop", "in_stock", None);
    let in_use = asset(&s.pool, a, "Monitor", "in_service", Some(a.admin_uuid));
    link(&s.pool, a, ticket_id, stocked);
    link(&s.pool, a, ticket_id, in_use);
    let id = manual_rule(
        &s.pool,
        a,
        json!([{ "kind": "assign_assets_to_requester", "config": {} }]),
    );

    let outcome = apply(&s.pool, a, &admin(a), id, ticket_id).expect("apply");
    let step = first_step(&outcome);
    assert_eq!(step["kind"], "assign_assets_to_requester");
    assert_eq!(ids(&step["asset_ids"]), vec![stocked as i64, in_use as i64]);
    assert_eq!(step["requester_uuid"], json!(requester));

    assert_eq!(
        asset_state(&s.admin_pool, stocked),
        ("in_service".to_string(), Some(requester))
    );
    assert_eq!(
        asset_state(&s.admin_pool, in_use),
        ("in_service".to_string(), Some(requester))
    );
    let moved = lifecycle_of(&s.admin_pool, stocked);
    let last = moved.last().expect("a lifecycle event");
    assert_eq!(last.from_status.as_deref(), Some("in_stock"));
    assert_eq!(last.to_status, "in_service");
    assert_eq!(last.ticket_id, Some(ticket_id));
    assert_eq!(last.actor_uuid, Some(a.admin_uuid));
    assert_eq!(last.reason.as_deref(), Some(rules::RULE_ASSIGNED_REASON));
    // Already in service: only its user changed, so no lifecycle event.
    assert!(lifecycle_of(&s.admin_pool, in_use).is_empty());

    let again = apply(&s.pool, a, &admin(a), id, ticket_id).expect("apply again");
    let step = first_step(&again);
    assert!(ids(&step["asset_ids"]).is_empty());
    assert_eq!(
        ids(&step["unchanged_asset_ids"]),
        vec![stocked as i64, in_use as i64]
    );
    assert_eq!(lifecycle_of(&s.admin_pool, stocked).len(), moved.len());
}

/// Each linked asset goes on loan to the requester, linked to the ticket and
/// due back when the step says. Applying again lends nothing twice. An asset
/// that can't be lent refuses the step by name, and nothing is written.
#[test]
fn lending_puts_the_linked_assets_on_loan_to_the_requester() {
    let s = setup();
    let a = &s.seeded.a;
    let requester = a.member_uuid;
    let ticket_id = ticket(&s.pool, a, Some(requester));
    let spare = asset(&s.pool, a, "Spare laptop", "in_stock", None);
    let phone = asset(&s.pool, a, "Phone", "in_service", None);
    link(&s.pool, a, ticket_id, spare);
    link(&s.pool, a, ticket_id, phone);
    let id = manual_rule(
        &s.pool,
        a,
        json!([{ "kind": "lend_assets", "config": { "due_in_days": 14 } }]),
    );

    let outcome = apply(&s.pool, a, &admin(a), id, ticket_id).expect("apply");
    let step = first_step(&outcome);
    let due = chrono::Utc::now().date_naive() + chrono::Duration::days(14);
    assert_eq!(ids(&step["asset_ids"]), vec![spare as i64, phone as i64]);
    assert_eq!(step["due_back"], json!(due));
    for (asset_id, before) in [(spare, "in_stock"), (phone, "in_service")] {
        let loans = loans_of(&s.admin_pool, asset_id);
        assert_eq!(loans.len(), 1);
        let loan = &loans[0];
        assert_eq!(loan.borrower_user_uuid, Some(requester));
        assert_eq!(loan.ticket_id, Some(ticket_id));
        assert_eq!(loan.due_back, Some(due));
        assert_eq!(loan.status_before, before);
        assert_eq!(loan.actor_uuid, Some(a.admin_uuid));
        assert!(loan.returned_at.is_none());
        assert_eq!(asset_state(&s.admin_pool, asset_id).0, "on_loan");
    }
    assert_eq!(
        ids(&step["loan_ids"]),
        vec![
            loans_of(&s.admin_pool, spare)[0].id as i64,
            loans_of(&s.admin_pool, phone)[0].id as i64
        ]
    );

    let again = apply(&s.pool, a, &admin(a), id, ticket_id).expect("apply again");
    let step = first_step(&again);
    assert!(ids(&step["loan_ids"]).is_empty());
    assert_eq!(
        ids(&step["already_lent_asset_ids"]),
        vec![spare as i64, phone as i64]
    );
    assert_eq!(loans_of(&s.admin_pool, spare).len(), 1);

    // A device in repair can't go out. The whole apply is refused, so the
    // other new link isn't lent either, and a manual apply leaves no row.
    let rows_before = applications(&s.admin_pool, id).len();
    let ticket2 = ticket(&s.pool, a, Some(requester));
    let tablet = asset(&s.pool, a, "Tablet", "in_stock", None);
    let broken = asset(&s.pool, a, "Broken laptop", "in_repair", None);
    link(&s.pool, a, ticket2, tablet);
    link(&s.pool, a, ticket2, broken);
    match apply(&s.pool, a, &admin(a), id, ticket2) {
        Err(ApplyError::AssetRefused { index: 1, message }) => {
            assert!(message.contains("Broken laptop"), "{message}");
            assert!(message.contains("in repair"), "{message}");
        }
        other => panic!("expected the step to be refused, got {other:?}"),
    }
    assert!(loans_of(&s.admin_pool, tablet).is_empty());
    assert_eq!(asset_state(&s.admin_pool, tablet).0, "in_stock");
    assert_eq!(applications(&s.admin_pool, id).len(), rows_before);
}

/// Offboarding: every loan the requester has out in the workspace comes
/// back, and every asset they use loses them as its user and goes back in
/// stock. Their loan and asset in another workspace stay as they are.
#[test]
fn taking_back_returns_the_requesters_loans_and_clears_them_as_user() {
    let s = setup();
    let (a, b) = (&s.seeded.a, &s.seeded.b);
    let leaver = a.member_uuid;
    let ticket_id = ticket(&s.pool, a, Some(leaver));

    let loaner = asset(&s.pool, a, "Loaner", "in_stock", None);
    let loan_id = lend(&s.pool, a, loaner, leaver);
    let laptop = asset(&s.pool, a, "Laptop", "in_service", Some(leaver));
    let lost = asset(&s.pool, a, "Lost phone", "lost", Some(leaver));
    let someone_elses = asset(&s.pool, a, "Desk phone", "in_service", Some(a.admin_uuid));
    // The same person in workspace B.
    let b_loaner = asset(&s.pool, b, "B loaner", "in_service", None);
    let b_loan = lend(&s.pool, b, b_loaner, leaver);
    let b_laptop = asset(&s.pool, b, "B laptop", "in_service", Some(leaver));

    let id = manual_rule(
        &s.pool,
        a,
        json!([{ "kind": "return_requester_assets", "config": {} }]),
    );
    let outcome = apply(&s.pool, a, &admin(a), id, ticket_id).expect("apply");
    let step = first_step(&outcome);
    assert_eq!(ids(&step["loan_ids"]), vec![loan_id as i64]);
    assert_eq!(ids(&step["returned_asset_ids"]), vec![loaner as i64]);
    assert_eq!(
        ids(&step["cleared_asset_ids"]),
        vec![laptop as i64, lost as i64]
    );

    let loan = &loans_of(&s.admin_pool, loaner)[0];
    assert!(loan.returned_at.is_some());
    assert_eq!(loan.returned_by_uuid, Some(a.admin_uuid));
    assert_eq!(
        asset_state(&s.admin_pool, loaner),
        ("in_stock".to_string(), None)
    );
    let returned = lifecycle_of(&s.admin_pool, loaner);
    assert_eq!(
        returned.last().and_then(|e| e.reason.as_deref()),
        Some(rules::RULE_RETURNED_REASON)
    );

    assert_eq!(
        asset_state(&s.admin_pool, laptop),
        ("in_stock".to_string(), None)
    );
    let stocked = lifecycle_of(&s.admin_pool, laptop);
    let last = stocked.last().expect("a lifecycle event");
    assert_eq!(
        (last.from_status.as_deref(), last.to_status.as_str()),
        (Some("in_service"), "in_stock")
    );
    assert_eq!(last.ticket_id, Some(ticket_id));
    // Only its user changes: a lost asset stays lost.
    assert_eq!(asset_state(&s.admin_pool, lost), ("lost".to_string(), None));
    assert_eq!(
        asset_state(&s.admin_pool, someone_elses),
        ("in_service".to_string(), Some(a.admin_uuid))
    );

    // Workspace B is untouched.
    let b_loan_row = &loans_of(&s.admin_pool, b_loaner)[0];
    assert_eq!(b_loan_row.id, b_loan);
    assert!(b_loan_row.returned_at.is_none());
    assert_eq!(asset_state(&s.admin_pool, b_loaner).0, "on_loan");
    assert_eq!(
        asset_state(&s.admin_pool, b_laptop),
        ("in_service".to_string(), Some(leaver))
    );
}

/// A ticket with no linked assets, and a requester with nothing out, run
/// clean: the apply succeeds, says it found nothing, and writes nothing to
/// any asset.
#[test]
fn nothing_to_act_on_is_a_clean_noop() {
    let s = setup();
    let a = &s.seeded.a;
    let ticket_id = ticket(&s.pool, a, Some(a.member_uuid));
    let id = manual_rule(
        &s.pool,
        a,
        json!([
            { "kind": "assign_assets_to_requester", "config": {} },
            { "kind": "lend_assets", "config": {} },
            { "kind": "return_requester_assets", "config": {} }
        ]),
    );
    let actor = admin(a);
    let outcome = apply(&s.pool, a, &actor, id, ticket_id).expect("apply");
    assert_eq!(outcome.application.status, RuleApplicationStatus::Succeeded);
    assert_eq!(outcome.actions_executed, 3);
    let steps = outcome.application.actions_taken.expect("steps");
    assert!(ids(&steps[0]["asset_ids"]).is_empty());
    assert!(ids(&steps[1]["asset_ids"]).is_empty());
    assert!(ids(&steps[1]["loan_ids"]).is_empty());
    for key in ["loan_ids", "returned_asset_ids", "cleared_asset_ids"] {
        assert!(ids(&steps[2][key]).is_empty(), "{key}");
    }
    let events: Vec<String> = run_events(&s.admin_pool, &actor)
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    assert_eq!(events, vec!["ticket.rule_applied".to_string()]);
}

/// A ticket reaches only its own workspace's assets: the link's key is
/// workspace scoped, so a link to another workspace's asset can't be written
/// even past row security. The steps in one workspace leave the requester's
/// assets, links and loans in another as they are.
#[test]
fn another_workspaces_assets_are_never_touched() {
    let s = setup();
    let (a, b) = (&s.seeded.a, &s.seeded.b);
    let requester = a.member_uuid;
    let ticket_id = ticket(&s.pool, a, Some(requester));
    let ours = asset(&s.pool, a, "Ours", "in_stock", None);
    link(&s.pool, a, ticket_id, ours);
    // The same requester in workspace B, with a linked asset in stock and
    // one they use.
    let b_ticket = ticket(&s.pool, b, Some(requester));
    let theirs = asset(&s.pool, b, "Theirs", "in_stock", None);
    link(&s.pool, b, b_ticket, theirs);
    let theirs_in_use = asset(&s.pool, b, "Theirs in use", "in_service", Some(requester));

    let cross_link = diesel::insert_into(ticket_assets::table)
        .values((
            ticket_assets::ticket_id.eq(ticket_id),
            ticket_assets::asset_id.eq(theirs),
            ticket_assets::workspace_id.eq(a.workspace_id),
        ))
        .execute(&mut s.admin_pool.get().expect("conn"));
    assert!(
        matches!(
            cross_link,
            Err(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                _
            ))
        ),
        "{cross_link:?}"
    );

    let assign = manual_rule(
        &s.pool,
        a,
        json!([{ "kind": "assign_assets_to_requester", "config": {} }]),
    );
    let outcome = apply(&s.pool, a, &admin(a), assign, ticket_id).expect("assign");
    assert_eq!(ids(&first_step(&outcome)["asset_ids"]), vec![ours as i64]);
    let lend_rule = manual_rule(&s.pool, a, json!([{ "kind": "lend_assets", "config": {} }]));
    let ticket2 = ticket(&s.pool, a, Some(requester));
    let outcome = apply(&s.pool, a, &admin(a), lend_rule, ticket2).expect("lend");
    assert!(ids(&first_step(&outcome)["asset_ids"]).is_empty());

    assert_eq!(
        asset_state(&s.admin_pool, theirs),
        ("in_stock".to_string(), None)
    );
    assert_eq!(
        asset_state(&s.admin_pool, theirs_in_use),
        ("in_service".to_string(), Some(requester))
    );
    for id in [theirs, theirs_in_use] {
        assert!(loans_of(&s.admin_pool, id).is_empty());
        assert!(lifecycle_of(&s.admin_pool, id).is_empty());
    }
}

/// The steps need a requester. An agent's apply on a ticket without one is
/// refused and leaves no row; the same rule run on its own, as the engine's
/// system actor, leaves a `failed` row. A run that does lend credits the loan
/// to no person.
#[test]
fn a_ticket_without_a_requester_refuses_the_step() {
    let s = setup();
    let a = &s.seeded.a;
    let anonymous = ticket(&s.pool, a, None);
    let laptop = asset(&s.pool, a, "Laptop", "in_stock", None);
    link(&s.pool, a, anonymous, laptop);

    let manual = manual_rule(&s.pool, a, json!([{ "kind": "lend_assets", "config": {} }]));
    match apply(&s.pool, a, &admin(a), manual, anonymous) {
        Err(ApplyError::NoRequester { index: 1 }) => {}
        other => panic!("expected NoRequester, got {other:?}"),
    }
    assert!(applications(&s.admin_pool, manual).is_empty());

    let automated = rule(
        &s.pool,
        a,
        RuleTriggerKind::TicketUpdated,
        json!([]),
        json!([{ "kind": "lend_assets", "config": {} }]),
    );
    let mut engine =
        ActorContext::system(format!("rules_engine:{automated}")).with_workspace(a.workspace_id);
    engine.correlation_id = Some(Uuid::now_v7());
    let failed = rules::run_rule_on_ticket(
        &mut s.pool.get().expect("conn"),
        ApplyContext::new(&engine, automated, anonymous, ApplySource::Event),
    )
    .expect_err("no requester");
    assert!(matches!(failed.error, ApplyError::NoRequester { index: 1 }));
    let row = failed.recorded.expect("a failed row");
    assert_eq!(row.status, RuleApplicationStatus::Failed);
    assert!(loans_of(&s.admin_pool, laptop).is_empty());

    let with_requester = ticket(&s.pool, a, Some(a.member_uuid));
    link(&s.pool, a, with_requester, laptop);
    rules::run_rule_on_ticket(
        &mut s.pool.get().expect("conn"),
        ApplyContext::new(&engine, automated, with_requester, ApplySource::Event),
    )
    .expect("the engine lends");
    let loan = &loans_of(&s.admin_pool, laptop)[0];
    assert_eq!(loan.borrower_user_uuid, Some(a.member_uuid));
    assert_eq!(loan.actor_uuid, None);
}

/// Asset, loan and lifecycle sync actions don't go to the rule event outbox,
/// so an asset step can't set off a ticket rule. The ticket write in the same
/// run does, which shows the outbox is taking events in this workspace.
#[test]
fn asset_changes_never_enqueue_rule_events() {
    let s = setup();
    let a = &s.seeded.a;
    // A running automated rule, so the outbox takes this workspace's events.
    rule(
        &s.pool,
        a,
        RuleTriggerKind::TicketUpdated,
        json!([]),
        json!([{ "kind": "unassign" }]),
    );
    let requester = a.member_uuid;
    let ticket_id = ticket(&s.pool, a, Some(requester));
    let laptop = asset(&s.pool, a, "Laptop", "in_stock", None);
    let loaner = asset(&s.pool, a, "Loaner", "in_stock", None);
    // Out already, so the take-back step returns a loan.
    let earlier = asset(&s.pool, a, "Earlier loaner", "in_stock", None);
    lend(&s.pool, a, earlier, requester);
    link(&s.pool, a, ticket_id, laptop);
    link(&s.pool, a, ticket_id, loaner);
    let id = manual_rule(
        &s.pool,
        a,
        json!([
            { "kind": "assign_assets_to_requester", "config": {} },
            { "kind": "return_requester_assets", "config": {} },
            { "kind": "lend_assets", "config": { "due_in_days": 7 } },
            { "kind": "set_priority", "config": { "priority": "high" } }
        ]),
    );
    let actor = admin(a);
    apply(&s.pool, a, &actor, id, ticket_id).expect("apply");

    let events = run_events(&s.admin_pool, &actor);
    let asset_events: Vec<&(String, bool)> = events
        .iter()
        .filter(|(e, _)| e.starts_with("asset"))
        .collect();
    for kind in [
        "asset.updated",
        "asset_lifecycle_event.created",
        "asset_loan.issued",
        "asset_loan.updated",
    ] {
        assert!(
            asset_events.iter().any(|(e, _)| e == kind),
            "the run should send {kind}: {events:?}"
        );
    }
    for (event, enqueued) in &events {
        let expected = event == "ticket.priority_changed";
        assert_eq!(*enqueued, expected, "{event} enqueued: {events:?}");
    }
}

/// The conflicts inspector sees the asset steps' writes: a rule whose
/// condition reads a loan pairs with one that lends, and a rule reading a
/// ticket field pairs with neither, since the steps write no ticket field.
#[test]
fn conflicts_see_the_asset_write_keys() {
    let s = setup();
    let a = &s.seeded.a;
    let leaf =
        |field: &str| json!({ "kind": "leaf", "field": field, "op": "is_not_null", "value": null });
    let writer = rule(
        &s.pool,
        a,
        RuleTriggerKind::TicketUpdated,
        json!([]),
        json!([
            { "kind": "lend_assets", "config": {} },
            { "kind": "assign_assets_to_requester", "config": {} }
        ]),
    );
    let loan_reader = rule(
        &s.pool,
        a,
        RuleTriggerKind::TicketUpdated,
        leaf("asset.loan"),
        json!([{ "kind": "assign_assets_to_requester", "config": {} }]),
    );
    let ticket_reader = rule(
        &s.pool,
        a,
        RuleTriggerKind::TicketUpdated,
        leaf("ticket.updated_at"),
        json!([{ "kind": "return_requester_assets", "config": {} }]),
    );

    let mut conn = s.pool.get().expect("conn");
    let found = with_actor_context(&mut conn, &admin(a), |c| {
        backend::repository::rule_conflicts::find(
            c,
            chrono::Utc::now() - chrono::Duration::days(30),
        )
    })
    .expect("find conflicts");
    let saved = |id: i32| {
        found
            .rules
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("rule {id} is checked"))
    };
    let _ = (saved(writer), saved(loan_reader), saved(ticket_reader));
    let pairs: Vec<(i32, i32, Vec<String>)> = found
        .overlaps
        .iter()
        .map(|o| (o.writer_rule_id, o.reader_rule_id, o.fields.clone()))
        .collect();
    assert!(
        pairs.contains(&(writer, loan_reader, vec!["asset.loan".to_string()])),
        "{pairs:?}"
    );
    assert!(
        pairs.contains(&(ticket_reader, loan_reader, vec!["asset.loan".to_string()])),
        "{pairs:?}"
    );
    assert!(
        !pairs.iter().any(|(_, reader, _)| *reader == ticket_reader),
        "no asset step writes a ticket field: {pairs:?}"
    );
}

/// A rule's moves write no asset record, even into a status a record type
/// opens with, and add no type from the catalogue: records from a
/// transition are opt-in, and a rule never opts in.
#[test]
fn rule_moves_write_no_records() {
    use backend::models::{OutcomeCategory, RecordOutcome};
    use backend::repository::asset_record_types as types_repo;
    use backend::schema::{asset_record_types, asset_records};

    let s = setup();
    let a = &s.seeded.a;
    let requester = a.member_uuid;
    // Types an admin pointed at the statuses the rule's moves enter.
    for (slug, status) in [("commissioning", "in_service"), ("handover", "on_loan")] {
        let mut conn = s.pool.get().expect("conn");
        with_actor_context(&mut conn, &admin(a), |c| {
            types_repo::create(
                c,
                types_repo::NewRecordTypeInput {
                    slug: slug.into(),
                    label: slug.into(),
                    description: None,
                    icon: None,
                    color: None,
                    field_schema: json!({ "type": "object", "properties": {} }),
                    field_order: vec![],
                    core_fields: json!({}),
                    outcomes: vec![RecordOutcome {
                        key: "done".into(),
                        category: OutcomeCategory::Done,
                        label: Some("Done".into()),
                    }],
                    applies_to_kinds: None,
                    opens_with_status: Some(status.into()),
                    sort_order: 0,
                    created_by: None,
                },
            )
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
        })
        .expect("save a record type");
    }
    let ticket_id = ticket(&s.pool, a, Some(requester));
    let laptop = asset(&s.pool, a, "Laptop", "in_stock", None);
    link(&s.pool, a, ticket_id, laptop);
    let assign = manual_rule(
        &s.pool,
        a,
        json!([{ "kind": "assign_assets_to_requester", "config": {} }]),
    );
    apply(&s.pool, a, &admin(a), assign, ticket_id).expect("assign");
    assert_eq!(asset_state(&s.admin_pool, laptop).0, "in_service");

    let ticket2 = ticket(&s.pool, a, Some(requester));
    let spare = asset(&s.pool, a, "Spare", "in_stock", None);
    link(&s.pool, a, ticket2, spare);
    let lend = manual_rule(&s.pool, a, json!([{ "kind": "lend_assets", "config": {} }]));
    apply(&s.pool, a, &admin(a), lend, ticket2).expect("lend");
    assert_eq!(asset_state(&s.admin_pool, spare).0, "on_loan");

    let mut conn = s.admin_pool.get().expect("conn");
    let records: i64 = asset_records::table
        .filter(asset_records::workspace_id.eq(a.workspace_id))
        .count()
        .get_result(&mut conn)
        .expect("count records");
    assert_eq!(records, 0);
    let types: i64 = asset_record_types::table
        .filter(asset_record_types::workspace_id.eq(a.workspace_id))
        .count()
        .get_result(&mut conn)
        .expect("count types");
    assert_eq!(types, 2, "no type added from the catalogue");
    let moved = lifecycle_of(&s.admin_pool, laptop);
    assert!(moved.iter().all(|e| e.metadata.get("record_id").is_none()));
}

/// A move the status rules refuse reads in their words, naming the asset:
/// a loan whose asset drifted off on loan can't be returned by a step.
#[test]
fn a_refused_move_reads_in_the_status_rules_words() {
    let s = setup();
    let a = &s.seeded.a;
    let leaver = a.member_uuid;
    let ticket_id = ticket(&s.pool, a, Some(leaver));
    let loaner = asset(&s.pool, a, "Drifted loaner", "in_stock", None);
    lend(&s.pool, a, loaner, leaver);
    diesel::update(assets::table.find(loaner))
        .set(assets::status.eq("in_repair"))
        .execute(&mut s.admin_pool.get().expect("conn"))
        .expect("drift the status");

    let id = manual_rule(
        &s.pool,
        a,
        json!([{ "kind": "return_requester_assets", "config": {} }]),
    );
    match apply(&s.pool, a, &admin(a), id, ticket_id) {
        Err(ApplyError::AssetRefused { index: 1, message }) => assert_eq!(
            message,
            "\"Drifted loaner\": Asset isn't on loan, so the loan can't be returned"
        ),
        other => panic!("expected the step to be refused, got {other:?}"),
    }
    assert!(loans_of(&s.admin_pool, loaner)[0].returned_at.is_none());
}
