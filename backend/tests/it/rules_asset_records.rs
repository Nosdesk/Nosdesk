//! The record steps a rule takes on a ticket's linked assets: opening a work
//! record on each, and completing the ticket's open records. They write
//! through the records repository as `nosdesk_app` in a workspace, so the
//! records are checked as one entered by hand is, carry the rule, the ticket
//! and `source = rule`, and send their sync actions and webhooks. Applying a
//! rule again writes nothing twice; a dry run writes nothing and says what
//! it would; a type a rule can't write is refused when the rule is saved,
//! and fails the step when it was archived since.
#![allow(clippy::expect_used)]

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use chrono::{NaiveDate, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{
    AssetRecord, NewAsset, NewTicket, RuleApplicationStatus, RuleState, RuleTriggerKind,
    UpdateSiteSettings,
};
use backend::repository::rules::{
    self, ApplyContext, ApplyError, ApplyInput, ApplyOverrides, ApplySource, StepRecordTypeProblem,
};
use backend::repository::{
    asset_record_types as types_repo, assets as assets_repo, site_settings, tickets,
    webhooks as webhook_repo,
};
use backend::schema::{
    asset_record_types, asset_records, rule_event_outbox, sync_actions, tickets as tickets_t,
    webhook_deliveries, workflow_states,
};
use backend::services::webhooks::WebhookService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};

use crate::common::{self, TestPool, WorkspaceSeed};

/// Far enough east that its day is often not UTC's.
const ZONE: chrono_tz::Tz = chrono_tz::Pacific::Kiritimati;

fn admin(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, Some(Uuid::now_v7())).with_workspace(ws.workspace_id)
}

fn today() -> NaiveDate {
    Utc::now().with_timezone(&ZONE).date_naive()
}

struct Setup {
    _db: common::TestDb,
    ws: WorkspaceSeed,
    /// Superuser: seeds and reads past row security.
    admin_pool: TestPool,
    /// `nosdesk_app`, as the app runs.
    pool: TestPool,
}

/// Workspace A in [`ZONE`], with the repair and inspection types added.
fn setup() -> Setup {
    let db = common::TestDb::new();
    let admin_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut admin_pool.get().expect("conn"));
    let pool = db.runtime_pool(2);
    let ws = seeded.a;
    with_actor_context(&mut pool.get().expect("conn"), &admin(&ws), |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                default_timezone: Some(ZONE.name().to_string()),
                ..Default::default()
            },
        )?;
        for slug in ["repair", "inspection"] {
            types_repo::add_from_catalogue(c, slug, Some(ws.admin_uuid))
                .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        }
        Ok::<_, diesel::result::Error>(())
    })
    .expect("seed the timezone and record types");
    Setup {
        _db: db,
        ws,
        admin_pool,
        pool,
    }
}

fn rule(s: &Setup, kind: RuleTriggerKind, state: RuleState, actions: Value) -> i32 {
    with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        rules::create(
            c,
            backend::models::NewRule {
                workspace_id: s.ws.workspace_id,
                name: format!("{kind:?} {}", actions[0]["kind"]),
                description: None,
                trigger_kind: kind,
                trigger_config: json!({}),
                conditions: json!([]),
                actions,
                reads_set: Vec::new(),
                writes_set: Vec::new(),
                state,
                priority: 100,
                created_by: Some(s.ws.admin_uuid),
            },
        )
    })
    .expect("save a rule")
    .id
}

fn manual_rule(s: &Setup, actions: Value) -> i32 {
    rule(s, RuleTriggerKind::Manual, RuleState::Live, actions)
}

/// A ticket from the workspace's member, created `days_ago`.
fn ticket(s: &Setup, days_ago: i64) -> i32 {
    let id = with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        let open_state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(s.ws.workspace_id))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets_t::table)
            .values(&NewTicket {
                title: "Camera won't focus".to_string(),
                workflow_state_id: open_state,
                requester_uuid: Some(s.ws.member_uuid),
                ..Default::default()
            })
            .returning(tickets_t::id)
            .get_result(c)
    })
    .expect("seed a ticket");
    if days_ago > 0 {
        diesel::update(tickets_t::table.find(id))
            .set(
                tickets_t::created_at.eq(Utc::now().naive_utc() - chrono::Duration::days(days_ago)),
            )
            .execute(&mut s.admin_pool.get().expect("conn"))
            .expect("backdate the ticket");
    }
    id
}

fn asset(s: &Setup, name: &str, holder: Option<Uuid>) -> i32 {
    with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        assets_repo::create_device(
            c,
            NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: holder,
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
        )
    })
    .expect("seed an asset")
    .id
}

fn link(s: &Setup, ticket_id: i32, asset_id: i32) {
    with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        tickets::add_device_to_ticket(c, ticket_id, asset_id)
    })
    .expect("link the asset");
}

fn apply(s: &Setup, rule_id: i32, ticket_id: i32) -> Result<rules::ApplyOutcome, ApplyError> {
    rules::apply_manual(
        &mut s.pool.get().expect("conn"),
        ApplyInput {
            rule_id,
            ticket_id,
            overrides: ApplyOverrides::default(),
        },
        &admin(&s.ws),
        None,
    )
}

fn step(outcome: &rules::ApplyOutcome, i: usize) -> Value {
    outcome.application.actions_taken.as_ref().expect("steps")[i].clone()
}

fn ids(v: &Value) -> Vec<i64> {
    v.as_array()
        .expect("an id list")
        .iter()
        .map(|x| x.as_i64().expect("an id"))
        .collect()
}

/// Every record in the database, lowest id first, read past row security.
fn records(s: &Setup) -> Vec<AssetRecord> {
    asset_records::table
        .order(asset_records::id)
        .load(&mut s.admin_pool.get().expect("conn"))
        .expect("read records")
}

fn type_id(s: &Setup, slug: &str) -> i32 {
    asset_record_types::table
        .filter(asset_record_types::workspace_id.eq(s.ws.workspace_id))
        .filter(asset_record_types::slug.eq(slug))
        .select(asset_record_types::id)
        .first(&mut s.admin_pool.get().expect("conn"))
        .expect("the type")
}

/// Set a type's column past row security.
fn set_type(s: &Setup, slug: &str, core_fields: Option<Value>, field_schema: Option<Value>) {
    let id = type_id(s, slug);
    let mut conn = s.admin_pool.get().expect("conn");
    if let Some(core) = core_fields {
        diesel::update(asset_record_types::table.find(id))
            .set(asset_record_types::core_fields.eq(core))
            .execute(&mut conn)
            .expect("set core fields");
    }
    if let Some(schema) = field_schema {
        diesel::update(asset_record_types::table.find(id))
            .set(asset_record_types::field_schema.eq(schema))
            .execute(&mut conn)
            .expect("set fields");
    }
}

fn record_problem(s: &Setup, actions: Value) -> Option<StepRecordTypeProblem> {
    with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        rules::step_record_type_problem(c, &actions)
    })
    .expect("check the steps")
}

/// Each linked asset gets an open record of the type, dated today in the
/// workspace's timezone, linked to the ticket and credited to the rule.
/// Applying again opens nothing twice.
#[test]
fn opening_writes_one_open_record_on_each_linked_asset() {
    let s = setup();
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    let lens = asset(&s, "Lens 50mm", None);
    link(&s, ticket_id, camera);
    link(&s, ticket_id, lens);
    let id = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair", "summary": "Sent to the vendor" } }]),
    );

    let done = apply(&s, id, ticket_id).expect("apply");
    let taken = step(&done, 0);
    assert_eq!(taken["kind"], "open_asset_record");
    assert_eq!(ids(&taken["asset_ids"]), vec![camera as i64, lens as i64]);
    assert_eq!(
        done.record_assets,
        vec![camera, lens],
        "whose search entries change"
    );
    let rows = records(&s);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        ids(&taken["record_ids"]),
        rows.iter().map(|r| r.id as i64).collect::<Vec<_>>()
    );
    for (row, asset_id) in rows.iter().zip([camera, lens]) {
        assert_eq!(row.asset_id, asset_id);
        assert_eq!(row.record_type_id, type_id(&s, "repair"));
        assert_eq!(row.ticket_id, Some(ticket_id));
        assert_eq!(row.source, "rule");
        assert_eq!(row.rule_id, Some(id));
        assert_eq!(row.summary.as_deref(), Some("Sent to the vendor"));
        assert_eq!(row.occurred_on, today());
        assert!(row.occurred_at.is_some());
        assert_eq!(row.completed_on, None);
        assert_eq!(row.outcome, None);
        assert_eq!(row.recorded_by, Some(s.ws.admin_uuid));
    }

    // A record sets off no rule: its sync actions don't reach the rule
    // event outbox.
    let mut conn = s.admin_pool.get().expect("conn");
    let record_actions: Vec<i64> = sync_actions::table
        .filter(sync_actions::event_type.like("asset_record.%"))
        .select(sync_actions::sync_id)
        .load(&mut conn)
        .expect("read sync actions");
    assert_eq!(record_actions.len(), 2);
    let enqueued: i64 = rule_event_outbox::table
        .filter(rule_event_outbox::sync_id.eq_any(&record_actions))
        .count()
        .get_result(&mut conn)
        .expect("read the outbox");
    assert_eq!(enqueued, 0);

    let again = apply(&s, id, ticket_id).expect("apply again");
    let taken = step(&again, 0);
    assert!(ids(&taken["asset_ids"]).is_empty());
    assert!(again.record_assets.is_empty());
    assert_eq!(
        ids(&taken["already_open_asset_ids"]),
        vec![camera as i64, lens as i64]
    );
    assert_eq!(records(&s).len(), 2, "nothing opened twice");

    // A ticket with no linked assets opens nothing.
    let bare = ticket(&s, 0);
    let none = apply(&s, id, bare).expect("apply to a bare ticket");
    assert!(ids(&step(&none, 0)["asset_ids"]).is_empty());
    assert_eq!(records(&s).len(), 2);
}

/// An asset whose kind the type isn't offered for is passed over and named.
#[test]
fn an_asset_the_type_is_not_for_is_passed_over() {
    let s = setup();
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    diesel::update(asset_record_types::table.find(type_id(&s, "repair")))
        .set(asset_record_types::applies_to_kinds.eq(Some(vec![Some("vehicle".to_string())])))
        .execute(&mut s.admin_pool.get().expect("conn"))
        .expect("narrow the type");
    let id = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }]),
    );
    let done = apply(&s, id, ticket_id).expect("apply");
    assert_eq!(
        ids(&step(&done, 0)["not_for_kind_asset_ids"]),
        vec![camera as i64]
    );
    assert!(records(&s).is_empty());
}

/// A type requiring a field a rule can't fill is refused when the rule is
/// saved. One the step can fill (the summary) saves when the step has it.
/// A requirement only known per asset (the holder) refuses the step by the
/// asset's name when it runs, and nothing is written for any asset.
#[test]
fn a_type_with_required_fields_is_checked_at_save_and_at_run() {
    let s = setup();
    let open = |summary: Option<&str>| json!([{ "kind": "open_asset_record", "config": { "record_type": "repair", "summary": summary } }]);
    assert_eq!(record_problem(&s, open(None)), None);

    set_type(&s, "repair", Some(json!({ "vendor": "required" })), None);
    assert_eq!(
        record_problem(&s, open(Some("x"))),
        Some(StepRecordTypeProblem::Requires {
            step: 1,
            field: "vendor".to_string()
        })
    );

    set_type(&s, "repair", Some(json!({ "summary": "required" })), None);
    assert_eq!(
        record_problem(&s, open(None)),
        Some(StepRecordTypeProblem::Requires {
            step: 1,
            field: "summary".to_string()
        })
    );
    assert_eq!(record_problem(&s, open(Some("Sent away"))), None);
    // A completion that writes no new record only fills in the outcome.
    assert_eq!(
        record_problem(
            &s,
            json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired" } }])
        ),
        None
    );
    assert_eq!(
        record_problem(
            &s,
            json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired", "create_if_none": true } }])
        ),
        Some(StepRecordTypeProblem::Requires {
            step: 1,
            field: "summary".to_string()
        })
    );

    set_type(
        &s,
        "repair",
        Some(json!({})),
        Some(json!({
            "type": "object",
            "properties": { "fault": { "type": "string" } },
            "required": ["fault"],
        })),
    );
    assert_eq!(
        record_problem(&s, open(None)),
        Some(StepRecordTypeProblem::Requires {
            step: 1,
            field: "fault".to_string()
        })
    );

    // The holder comes from each asset, so it is checked when the step runs.
    set_type(
        &s,
        "repair",
        Some(json!({ "holder": "required" })),
        Some(json!({ "type": "object", "properties": {} })),
    );
    assert_eq!(record_problem(&s, open(None)), None);
    let ticket_id = ticket(&s, 0);
    let held = asset(&s, "Camera 14", Some(s.ws.member_uuid));
    let unheld = asset(&s, "Spare body", None);
    link(&s, ticket_id, held);
    link(&s, ticket_id, unheld);
    let id = manual_rule(&s, open(None));
    match apply(&s, id, ticket_id) {
        Err(ApplyError::AssetRefused { index, message }) => {
            assert_eq!(index, 1);
            assert!(message.contains("Spare body"), "{message}");
            assert!(message.contains("holder"), "{message}");
        }
        other => panic!("expected the step to be refused, got {other:?}"),
    }
    assert!(records(&s).is_empty(), "no asset's record is written");
}

/// Completing fills in today and the outcome on the ticket's open records
/// of the type, and applying again changes nothing. A record of another
/// type, or on another ticket, stays open.
#[test]
fn completing_completes_the_tickets_open_records() {
    let s = setup();
    let ticket_id = ticket(&s, 2);
    let camera = asset(&s, "Camera 14", None);
    let lens = asset(&s, "Lens 50mm", None);
    link(&s, ticket_id, camera);
    link(&s, ticket_id, lens);
    let opener = manual_rule(
        &s,
        json!([
            { "kind": "open_asset_record", "config": { "record_type": "repair" } },
            { "kind": "open_asset_record", "config": { "record_type": "inspection" } }
        ]),
    );
    apply(&s, opener, ticket_id).expect("open");
    let other_ticket = ticket(&s, 0);
    link(&s, other_ticket, camera);
    let other_opener = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }]),
    );
    apply(&s, other_opener, other_ticket).expect("open on another ticket");
    let repair = type_id(&s, "repair");
    let open_repairs: Vec<i32> = records(&s)
        .iter()
        .filter(|r| r.record_type_id == repair && r.ticket_id == Some(ticket_id))
        .map(|r| r.id)
        .collect();
    assert_eq!(open_repairs.len(), 2);

    let id = manual_rule(
        &s,
        json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired" } }]),
    );
    let done = apply(&s, id, ticket_id).expect("complete");
    let taken = step(&done, 0);
    assert_eq!(
        ids(&taken["completed_record_ids"]),
        open_repairs.iter().map(|&r| r as i64).collect::<Vec<_>>()
    );
    assert!(ids(&taken["created_record_ids"]).is_empty());
    assert_eq!(done.record_assets, vec![camera, lens]);
    for row in records(&s) {
        if open_repairs.contains(&row.id) {
            assert_eq!(row.completed_on, Some(today()));
            assert_eq!(row.outcome.as_deref(), Some("repaired"));
            assert_eq!(row.version, 2);
        } else {
            assert_eq!(row.completed_on, None, "record {} stays open", row.id);
        }
    }

    let again = apply(&s, id, ticket_id).expect("complete again");
    assert!(ids(&step(&again, 0)["completed_record_ids"]).is_empty());
    assert!(records(&s).iter().all(|r| r.version <= 2));
}

/// With no type, the step completes the ticket's open records of every type
/// that has the outcome; one whose type hasn't stays open and is listed.
#[test]
fn completing_without_a_type_leaves_records_without_the_outcome_open() {
    let s = setup();
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    let opener = manual_rule(
        &s,
        json!([
            { "kind": "open_asset_record", "config": { "record_type": "repair" } },
            { "kind": "open_asset_record", "config": { "record_type": "inspection" } }
        ]),
    );
    apply(&s, opener, ticket_id).expect("open");
    let (repair, inspection) = {
        let rows = records(&s);
        (rows[0].id, rows[1].id)
    };
    let id = manual_rule(
        &s,
        json!([{ "kind": "complete_asset_records", "config": { "outcome": "repaired" } }]),
    );
    let taken = step(&apply(&s, id, ticket_id).expect("complete"), 0);
    assert_eq!(ids(&taken["completed_record_ids"]), vec![repair as i64]);
    assert_eq!(ids(&taken["left_open_record_ids"]), vec![inspection as i64]);
    assert_eq!(taken["record_type"], Value::Null);
}

/// With `create_if_none`, an asset with no record of the type on the
/// ticket gets one already complete, started when the ticket was created.
/// Without it nothing is written. Applying again writes nothing.
#[test]
fn completing_with_create_if_none_writes_a_completed_record() {
    let s = setup();
    let ticket_id = ticket(&s, 3);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);

    let plain = manual_rule(
        &s,
        json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired" } }]),
    );
    let taken = step(&apply(&s, plain, ticket_id).expect("complete nothing"), 0);
    assert!(ids(&taken["completed_record_ids"]).is_empty());
    assert!(records(&s).is_empty());

    let id = manual_rule(
        &s,
        json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired", "create_if_none": true } }]),
    );
    let taken = step(&apply(&s, id, ticket_id).expect("create"), 0);
    let rows = records(&s);
    assert_eq!(rows.len(), 1);
    assert_eq!(ids(&taken["created_asset_ids"]), vec![camera as i64]);
    assert_eq!(ids(&taken["created_record_ids"]), vec![rows[0].id as i64]);
    let created_at: chrono::NaiveDateTime = tickets_t::table
        .find(ticket_id)
        .select(tickets_t::created_at)
        .first(&mut s.admin_pool.get().expect("conn"))
        .expect("ticket created_at");
    let row = &rows[0];
    assert_eq!(
        row.occurred_on,
        created_at.and_utc().with_timezone(&ZONE).date_naive()
    );
    assert!(row.occurred_on < today());
    assert_eq!(row.completed_on, Some(today()));
    assert_eq!(row.outcome.as_deref(), Some("repaired"));
    assert_eq!(row.ticket_id, Some(ticket_id));
    assert_eq!(row.source, "rule");
    assert_eq!(row.rule_id, Some(id));

    let again = step(&apply(&s, id, ticket_id).expect("apply again"), 0);
    assert!(ids(&again["created_asset_ids"]).is_empty());
    assert!(ids(&again["completed_record_ids"]).is_empty());
    assert_eq!(records(&s).len(), 1, "a completed record counts as one");
}

/// A dry run writes no record and no sync action, and its log names the
/// assets it would write and the records it would complete.
#[test]
fn a_dry_run_writes_nothing_and_says_what_it_would() {
    let s = setup();
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    let opener = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "inspection" } }]),
    );
    apply(&s, opener, ticket_id).expect("open an inspection");
    let before = records(&s);

    let id = rule(
        &s,
        RuleTriggerKind::TicketUpdated,
        RuleState::DryRun,
        json!([
            { "kind": "open_asset_record", "config": { "record_type": "repair" } },
            { "kind": "complete_asset_records", "config": { "record_type": "inspection", "outcome": "passed" } }
        ]),
    );
    let mut actor =
        ActorContext::system(format!("rules_engine:{id}")).with_workspace(s.ws.workspace_id);
    actor.correlation_id = Some(Uuid::now_v7());
    let outcome = rules::run_rule_on_ticket(
        &mut s.pool.get().expect("conn"),
        ApplyContext {
            shadow: true,
            ..ApplyContext::new(&actor, id, ticket_id, ApplySource::Event)
        },
    )
    .expect("shadow run");

    assert_eq!(outcome.application.status, RuleApplicationStatus::DryRun);
    assert!(
        outcome.record_assets.is_empty(),
        "a dry run writes no record"
    );
    let opened = step(&outcome, 0);
    assert_eq!(ids(&opened["asset_ids"]), vec![camera as i64]);
    assert!(ids(&opened["record_ids"]).is_empty());
    let completed = step(&outcome, 1);
    assert_eq!(
        ids(&completed["completed_record_ids"]),
        vec![before[0].id as i64]
    );
    let after = records(&s);
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0].version, before[0].version);
    assert_eq!(after[0].completed_on, None);
    let events: i64 = sync_actions::table
        .filter(sync_actions::correlation_id.eq(actor.correlation_id.expect("correlation")))
        .filter(sync_actions::event_type.like("asset_record.%"))
        .count()
        .get_result(&mut s.admin_pool.get().expect("conn"))
        .expect("count sync actions");
    assert_eq!(events, 0);
}

/// A type that is gone or archived is refused when the rule is saved or
/// goes live, and one archived after the rule went live fails the step
/// without writing anything.
#[actix_web::test]
async fn an_archived_type_is_refused_at_save_and_fails_the_step() {
    let s = setup();
    let open = json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }]);
    assert_eq!(
        record_problem(
            &s,
            json!([{ "kind": "open_asset_record", "config": { "record_type": "calibration" } }])
        ),
        Some(StepRecordTypeProblem::Missing {
            step: 1,
            slug: "calibration".to_string()
        })
    );
    assert_eq!(
        record_problem(
            &s,
            json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "passed" } }])
        ),
        Some(StepRecordTypeProblem::UnknownOutcome {
            step: 1,
            outcome: "passed".to_string()
        })
    );
    assert_eq!(
        record_problem(
            &s,
            json!([{ "kind": "complete_asset_records", "config": { "outcome": "nonsense" } }])
        ),
        Some(StepRecordTypeProblem::UnknownOutcome {
            step: 1,
            outcome: "nonsense".to_string()
        })
    );

    // Live rules, an open record, and then the type is archived.
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    let id = manual_rule(&s, open.clone());
    apply(&s, id, ticket_id).expect("open a record");
    let complete = json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired" } }]);
    let create = json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired", "create_if_none": true } }]);
    let completer = manual_rule(&s, complete.clone());
    let creator = manual_rule(&s, create.clone());
    let other_ticket = ticket(&s, 0);
    link(&s, other_ticket, camera);
    with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        types_repo::archive(c, type_id(&s, "repair"))
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("archive the type");
    // Writing a new record of it fails the step and writes nothing.
    for (rule_id, on) in [(id, other_ticket), (creator, other_ticket)] {
        match apply(&s, rule_id, on) {
            Err(ApplyError::ActionFailed { index, message }) => {
                assert_eq!(index, 1);
                assert!(message.contains("archived"), "{message}");
            }
            other => panic!("expected the step to fail, got {other:?}"),
        }
    }
    assert_eq!(records(&s).len(), 1);
    for actions in [open.clone(), create.clone()] {
        assert_eq!(
            record_problem(&s, actions),
            Some(StepRecordTypeProblem::Archived {
                step: 1,
                slug: "repair".to_string()
            })
        );
    }
    // Completing the open records of it still runs, and still saves.
    assert_eq!(record_problem(&s, complete.clone()), None);
    let taken = step(&apply(&s, completer, ticket_id).expect("complete"), 0);
    assert_eq!(
        ids(&taken["completed_record_ids"]),
        vec![records(&s)[0].id as i64]
    );
    assert_eq!(records(&s)[0].outcome.as_deref(), Some("repaired"));

    // Over the API: a save is refused with its code, and so is going live.
    let saved = crate::rules_in_workspace::as_admin(
        &s.pool,
        &s.ws,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Open a repair record",
                "trigger_kind": "manual",
                "actions": open,
            })),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::BAD_REQUEST);
    let body: Value = http_test::read_body_json(saved).await;
    assert_eq!(body["code"], "RULE_RECORD_TYPE_INVALID", "{body}");

    let paused = rule(
        &s,
        RuleTriggerKind::Manual,
        RuleState::Paused,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }]),
    );
    let live = crate::rules_in_workspace::as_admin(
        &s.pool,
        &s.ws,
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{paused}/state"))
            .set_json(json!({ "state": "live" })),
    )
    .await;
    assert_eq!(live.status(), StatusCode::BAD_REQUEST);
    let body: Value = http_test::read_body_json(live).await;
    assert_eq!(body["code"], "RULE_RECORD_TYPE_INVALID", "{body}");
}

/// A record a rule opens reaches a webhook on the record events as
/// `asset.record_created` carrying the rule, and its completion as
/// `asset.record_updated`.
#[test]
fn rule_records_reach_webhooks() {
    let s = setup();
    let hook = with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        webhook_repo::create_webhook(
            c,
            "records".into(),
            "https://example.invalid/records".into(),
            "whsec_test".into(),
            vec!["asset.record_created".into(), "asset.record_updated".into()],
            None,
            None,
        )
    })
    .expect("create a webhook");
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    let opener = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair", "summary": "Sent away" } }]),
    );
    let closer = manual_rule(
        &s,
        json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired" } }]),
    );
    apply(&s, opener, ticket_id).expect("open");
    apply(&s, closer, ticket_id).expect("complete");

    let mut conn = s.admin_pool.get().expect("conn");
    loop {
        let (_, more) = with_actor_bypass_context(
            &mut conn,
            &ActorContext::system("test:rules_asset_records_drain"),
            WebhookService::drain_batch_txn,
        )
        .expect("drain the outbox");
        if !more {
            break;
        }
    }
    let sent: Vec<(String, Value)> = webhook_deliveries::table
        .filter(webhook_deliveries::webhook_id.eq(hook.id))
        .order(webhook_deliveries::id)
        .select((webhook_deliveries::event_type, webhook_deliveries::payload))
        .load(&mut conn)
        .expect("read deliveries");
    let record = records(&s).remove(0);
    assert_eq!(
        sent.iter().map(|(e, _)| e.as_str()).collect::<Vec<_>>(),
        vec!["asset.record_created", "asset.record_updated"]
    );
    let created = &sent[0].1["data"];
    assert_eq!(created["id"], record.id);
    assert_eq!(created["source"], "rule");
    assert_eq!(created["rule_id"], opener);
    assert_eq!(created["ticket_id"], ticket_id);
    assert_eq!(created["summary"], "Sent away");
    let updated = &sent[1].1["data"];
    assert_eq!(updated["id"], record.id);
    assert_eq!(updated["outcome"], "repaired");
    assert_eq!(updated["rule_id"], opener, "the record stays the opener's");
}

/// A record dated from the ticket's creation rarely knows who held the
/// asset or where it was, so a completion that writes one refuses a type
/// requiring either; one that only completes records doesn't.
#[test]
fn a_backdated_record_refuses_a_type_requiring_holder_or_location() {
    let s = setup();
    let complete = |create: bool| json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired", "create_if_none": create } }]);
    for field in ["holder", "location"] {
        set_type(&s, "repair", Some(json!({ field: "required" })), None);
        assert_eq!(
            record_problem(&s, complete(true)),
            Some(StepRecordTypeProblem::Requires {
                step: 1,
                field: field.to_string()
            }),
            "{field}"
        );
        assert_eq!(record_problem(&s, complete(false)), None, "{field}");
        // Opening dates the record today, when the asset's holder and
        // location are known, so it is checked per asset when it runs.
        assert_eq!(
            record_problem(
                &s,
                json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }])
            ),
            None,
            "{field}"
        );
    }
}

/// A type that no longer has the step's outcome fails the step rather than
/// leaving every record open.
#[test]
fn a_type_without_the_outcome_fails_the_completion() {
    let s = setup();
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    let opener = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }]),
    );
    apply(&s, opener, ticket_id).expect("open");
    let id = manual_rule(
        &s,
        json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired" } }]),
    );
    diesel::update(asset_record_types::table.find(type_id(&s, "repair")))
        .set(
            asset_record_types::outcomes
                .eq(json!([{ "key": "fixed", "category": "done", "label": "Fixed" }])),
        )
        .execute(&mut s.admin_pool.get().expect("conn"))
        .expect("rename the outcome");
    match apply(&s, id, ticket_id) {
        Err(ApplyError::ActionFailed { index, message }) => {
            assert_eq!(index, 1);
            assert!(message.contains("no outcome \"repaired\""), "{message}");
        }
        other => panic!("expected the step to fail, got {other:?}"),
    }
    assert_eq!(records(&s)[0].completed_on, None);
}

/// After a carry-over links the ticket to the asset that replaced the one
/// worked on, neither step writes a record on the replacement: opening
/// again passes it over, and so does a completion writing records where
/// there are none, in a dry run as when it runs.
#[test]
fn the_replacement_a_carry_over_links_gets_no_record() {
    use backend::models::{PlatformRole, WorkspaceRole};
    use backend::repository::asset_records::{self as records_repo, NewRecord, WorkDate};
    use backend::repository::asset_replacement::{self, CarryOver};
    use backend::repository::ticket_visibility::VisibilityContext;

    let s = setup();
    let ticket_id = ticket(&s, 2);
    let old = asset(&s, "Camera 14", None);
    let new = asset(&s, "Camera 31", None);
    link(&s, ticket_id, old);
    let opener = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair" } }]),
    );
    apply(&s, opener, ticket_id).expect("open on the old camera");

    // The old camera was swapped under warranty, recorded on the ticket, and
    // its open tickets carried over to the new one.
    with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
        let replaced = records_repo::create(
            c,
            NewRecord {
                asset_id: old,
                record_type_id: type_id(&s, "repair"),
                summary: Some("Swapped under warranty".to_string()),
                notes: None,
                occurred: WorkDate::Day(today()),
                completed: Some(WorkDate::Day(today())),
                outcome: Some("replaced".to_string()),
                vendor: None,
                performed_by_uuid: None,
                reference: None,
                cost: None,
                ticket_id: Some(ticket_id),
                replacement_asset_id: Some(new),
                snapshot: None,
                attributes: json!({}),
                source: backend::models::RecordSource::Manual,
                source_ref: None,
                import_job_id: None,
                rule_id: None,
                lifecycle_event_id: None,
                recorded_by: Some(s.ws.admin_uuid),
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        asset_replacement::carry_over(
            c,
            old,
            replaced.record.id,
            CarryOver {
                link_open_tickets: true,
                ..Default::default()
            },
            &VisibilityContext::new(
                s.ws.admin_uuid,
                PlatformRole::User,
                Some(WorkspaceRole::Admin),
            ),
            Some(s.ws.admin_uuid),
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("record the replacement and carry the ticket over");
    let linked: Vec<i32> =
        with_actor_context(&mut s.pool.get().expect("conn"), &admin(&s.ws), |c| {
            tickets::get_devices_for_ticket(c, ticket_id)
        })
        .expect("linked assets")
        .iter()
        .map(|a| a.id)
        .collect();
    assert!(
        linked.contains(&new),
        "the carry-over links the ticket to the new camera"
    );
    let before = records(&s).len();

    let taken = step(&apply(&s, opener, ticket_id).expect("open again"), 0);
    assert!(ids(&taken["asset_ids"]).is_empty());
    assert_eq!(ids(&taken["already_open_asset_ids"]), vec![old as i64]);
    assert_eq!(ids(&taken["replacement_asset_ids"]), vec![new as i64]);
    assert_eq!(records(&s).len(), before);

    let create = json!([{ "kind": "complete_asset_records", "config": { "record_type": "repair", "outcome": "repaired", "create_if_none": true } }]);
    let shadow_rule = rule(
        &s,
        RuleTriggerKind::TicketUpdated,
        RuleState::DryRun,
        create.clone(),
    );
    let mut actor = ActorContext::system(format!("rules_engine:{shadow_rule}"))
        .with_workspace(s.ws.workspace_id);
    actor.correlation_id = Some(Uuid::now_v7());
    let shadow = rules::run_rule_on_ticket(
        &mut s.pool.get().expect("conn"),
        ApplyContext {
            shadow: true,
            ..ApplyContext::new(&actor, shadow_rule, ticket_id, ApplySource::Event)
        },
    )
    .expect("shadow run");
    let would = step(&shadow, 0);
    assert_eq!(ids(&would["replacement_asset_ids"]), vec![new as i64]);
    assert!(ids(&would["created_asset_ids"]).is_empty());

    let creator = manual_rule(&s, create);
    let taken = step(&apply(&s, creator, ticket_id).expect("complete"), 0);
    assert_eq!(ids(&taken["replacement_asset_ids"]), vec![new as i64]);
    assert!(ids(&taken["created_record_ids"]).is_empty());
    assert_eq!(
        ids(&taken["completed_record_ids"]).len(),
        1,
        "the old camera's open record"
    );
    assert_eq!(records(&s).len(), before);
    assert!(records(&s).iter().all(|r| r.asset_id != new));
}

/// A rule applied by hand that opens records reindexes their assets, so the
/// summary it wrote finds the asset in search.
#[actix_web::test]
async fn a_rules_records_reach_their_assets_search_entries() {
    use actix_web::dev::Service;
    use actix_web::{web, App, HttpMessage};
    use backend::extractors::WorkspaceContext;
    use backend::middleware::RequestContext;
    use backend::models::Claims;
    use backend::services::search::{SearchQuery, SearchService};
    use std::sync::Arc;

    common::ensure_test_keyring();
    let s = setup();
    let ticket_id = ticket(&s, 0);
    let camera = asset(&s, "Camera 14", None);
    link(&s, ticket_id, camera);
    let id = manual_rule(
        &s,
        json!([{ "kind": "open_asset_record", "config": { "record_type": "repair", "summary": "Sent under RMA-55501" } }]),
    );
    // Built before the run: an empty index is filled once, here, without
    // the record.
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &s.pool).expect("init search"));

    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: s.ws.admin_uuid.to_string(),
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
        workspace_id: s.ws.workspace_id,
        workspace_uuid: s.ws.workspace_uuid,
        slug: s.ws.slug.clone(),
        name: s.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(s.ws.admin_uuid, Some(corr)).with_workspace(s.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(s.pool.clone()))
            .app_data(web::Data::new(search.clone()))
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
    let applied = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/rules/{id}/apply"))
            .set_json(json!({ "ticket_id": ticket_id }))
            .to_request(),
    )
    .await;
    assert_eq!(applied.status(), StatusCode::OK);

    let query = SearchQuery {
        q: "RMA-55501".to_string(),
        limit: 20,
        types: Some("device".to_string()),
        sort: None,
        author: None,
    };
    let mut hits = Vec::new();
    for _ in 0..50 {
        hits = search
            .search(&query, true, i64::from(s.ws.workspace_id))
            .expect("search")
            .results
            .into_iter()
            .map(|r| r.entity_id)
            .collect();
        if !hits.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(hits, vec![i64::from(camera)]);
}
