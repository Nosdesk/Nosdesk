//! The rule event outbox: which sync actions the trigger enqueues (and only
//! in a workspace with a live or dry-run automated rule), that the outbox and
//! the recursion budget (`ticket_rule_runs`) keep each workspace's rows to
//! itself, and that both go with a deleted workspace.
//!
//! The dispatcher that drains the outbox is not built yet; these tests drive
//! the trigger through `sync::emit::record` and the repository writes that
//! call it, running as `nosdesk_app` in a workspace the way the app does.
#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::Path;

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use backend::db::DbConnection;
use backend::models::{
    NewRule, NewTicket, RuleState, RuleTriggerKind, SyncAggregate, SyncOp, TicketPriority,
    TicketUpdate,
};
use backend::repository::{rules, ticket_watchers, tickets, workflow_states, workspaces};
use backend::schema::{rule_event_outbox, sync_actions, ticket_rule_runs};
use backend::sync::actor::ActorContext;
use backend::sync::emit::{record, SyncEmit};
use backend::sync::session::{with_actor_bypass_context, with_actor_context};

use crate::common::{self, TestPool, WorkspaceSeed};

/// The ticket events a rule trigger reads (`ticket_created`,
/// `ticket_updated`): creation, and every field write.
const ENQUEUED: &[&str] = &[
    "ticket.created",
    "ticket.updated",
    "ticket.workflow_state_changed",
    "ticket.assignee_changed",
    "ticket.priority_changed",
    "ticket.title_changed",
    "ticket.category_changed",
    "ticket.verification_changed",
    "ticket.resolution_notes_changed",
    "ticket.tags_changed",
];

/// Ticket events no rule trigger reads. The rule's own bookkeeping and the
/// SLA stamps would feed loops; watchers, cycles, links and the rest are not
/// fields a condition reads.
const SKIPPED: &[&str] = &[
    "ticket.rule_applied",
    "ticket.sla_breached",
    "ticket.sla_updated",
    "ticket.watcher_added",
    "ticket.watcher_removed",
    "ticket.watcher_pref_changed",
    "ticket.cycle_changed",
    "ticket.people_cleared",
    "ticket.content_saved",
    "ticket.merged",
    "ticket.merge_notice_sent",
    "ticket.linked",
    "ticket.unlinked",
    "ticket.deleted",
    "ticket.approval_requested",
    "ticket.approval_decided",
    "ticket.rated",
];

fn actor(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id)
}

/// Save a rule in `ws` as its admin, through the repository the API uses.
fn rule(pool: &TestPool, ws: &WorkspaceSeed, kind: RuleTriggerKind, state: RuleState) -> i32 {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &actor(ws), |c| {
        rules::create(
            c,
            NewRule {
                workspace_id: ws.workspace_id,
                name: format!("{kind:?} {state:?}"),
                description: None,
                trigger_kind: kind,
                trigger_config: serde_json::json!({}),
                conditions: serde_json::json!([]),
                actions: serde_json::json!([
                    { "kind": "set_priority", "config": { "priority": "high" } }
                ]),
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

fn set_state(pool: &TestPool, ws: &WorkspaceSeed, id: i32, state: RuleState) {
    use backend::schema::rules::dsl;
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &actor(ws), |c| {
        diesel::update(dsl::rules.find(id))
            .set(dsl::state.eq(state))
            .execute(c)
    })
    .expect("change the rule's state");
}

/// Whether a `ticket.updated` recorded in `ws`, as the app, is enqueued.
fn an_update_enqueues(pool: &TestPool, ws: &WorkspaceSeed) -> bool {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &actor(ws), |c| {
        let id = emit(
            c,
            SyncAggregate::Ticket,
            "ticket.updated",
            serde_json::json!({"id": 1}),
        )?;
        enqueued_in(c, id)
    })
    .expect("emit as the app")
    .is_some()
}

fn emit(
    c: &mut DbConnection,
    aggregate: SyncAggregate,
    event_type: &str,
    data: serde_json::Value,
) -> QueryResult<i64> {
    record(
        c,
        SyncEmit {
            aggregate,
            aggregate_id: "1".into(),
            op: SyncOp::Update,
            event_type,
            data,
            groups: vec!["workspace".into()],
            causation_id: None,
        },
    )
}

/// The workspace of `sync_id`'s outbox row, when the trigger enqueued it and
/// row security shows it.
fn enqueued_in(c: &mut DbConnection, sync_id: i64) -> QueryResult<Option<i32>> {
    rule_event_outbox::table
        .find(sync_id)
        .select(rule_event_outbox::workspace_id)
        .first(c)
        .optional()
}

#[test]
fn the_trigger_enqueues_only_what_a_rule_trigger_reads() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    rule(&pool, a, RuleTriggerKind::TicketUpdated, RuleState::Live);
    let mut conn = pool.get().expect("conn");

    with_actor_context(&mut conn, &actor(a), |c| {
        for &event_type in ENQUEUED {
            let id = emit(
                c,
                SyncAggregate::Ticket,
                event_type,
                serde_json::json!({"id": 1}),
            )?;
            assert_eq!(
                enqueued_in(c, id)?,
                Some(a.workspace_id),
                "{event_type} is enqueued in its workspace"
            );
        }
        for &event_type in SKIPPED {
            let id = emit(
                c,
                SyncAggregate::Ticket,
                event_type,
                serde_json::json!({"id": 1}),
            )?;
            assert_eq!(enqueued_in(c, id)?, None, "{event_type} is not enqueued");
        }

        // A public reply is `ticket_replied`; an internal note isn't a reply,
        // and a row that doesn't say is not taken for one.
        for (data, expected) in [
            (
                serde_json::json!({"id": 5, "ticket_id": 1, "is_internal": false}),
                true,
            ),
            (
                serde_json::json!({"id": 6, "ticket_id": 1, "is_internal": true}),
                false,
            ),
            (serde_json::json!({"id": 7, "ticket_id": 1}), false),
        ] {
            let id = emit(c, SyncAggregate::Comment, "comment.created", data.clone())?;
            assert_eq!(enqueued_in(c, id)?.is_some(), expected, "{data}");
        }
        let id = emit(
            c,
            SyncAggregate::Comment,
            "comment.updated",
            serde_json::json!({"id": 5, "ticket_id": 1, "is_internal": false}),
        )?;
        assert_eq!(
            enqueued_in(c, id)?,
            None,
            "an edited reply is not a new one"
        );

        // Plugin rows wait for the plugin trigger.
        let id = emit(
            c,
            SyncAggregate::Plugin,
            "plugin.deploy_finished",
            serde_json::json!({"ticket_id": 1}),
        )?;
        assert_eq!(enqueued_in(c, id)?, None, "a plugin event is not enqueued");
        Ok::<_, diesel::result::Error>(())
    })
    .expect("emit as the app");
}

/// A ticket edit through the one ticket write path enqueues; adding a
/// watcher, which records its own event, does not.
#[test]
fn a_ticket_edit_enqueues_and_a_watcher_does_not() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    rule(&pool, a, RuleTriggerKind::TicketUpdated, RuleState::Live);
    let mut conn = pool.get().expect("conn");

    with_actor_context(&mut conn, &actor(a), |c| {
        let open = workflow_states::default_state(c)?;
        let ticket: i32 = diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Printer offline".to_string(),
                workflow_state_id: open.id,
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)?;
        let events = |c: &mut DbConnection| -> QueryResult<Vec<(i64, String)>> {
            sync_actions::table
                .filter(sync_actions::aggregate_id.eq(ticket.to_string()))
                .order(sync_actions::sync_id.asc())
                .select((sync_actions::sync_id, sync_actions::event_type))
                .load(c)
        };
        let before = events(c)?.len();

        tickets::update_ticket_partial(
            c,
            ticket,
            TicketUpdate {
                priority: Some(TicketPriority::High),
                ..Default::default()
            },
            None,
        )
        .map_err(diesel::result::Error::from)?;
        ticket_watchers::add_watcher(c, ticket, a.member_uuid, false)?;

        let recorded: Vec<(i64, String)> = events(c)?.into_iter().skip(before).collect();
        let priority = recorded
            .iter()
            .find(|(_, t)| t == "ticket.priority_changed")
            .expect("the edit records ticket.priority_changed");
        assert_eq!(enqueued_in(c, priority.0)?, Some(a.workspace_id));
        let watcher = recorded
            .iter()
            .find(|(_, t)| t == "ticket.watcher_added")
            .expect("adding a watcher records ticket.watcher_added");
        assert_eq!(enqueued_in(c, watcher.0)?, None);
        Ok::<_, diesel::result::Error>(())
    })
    .expect("edit as the app");
}

/// Nothing would claim a row in a workspace with no rule an event can run,
/// so none is written there: manual rules, and automated ones that are
/// paused, draft or archived, don't count. A live or dry-run automated rule
/// does, time-elapsed included (its clocks follow the same events), and only
/// in its own workspace.
#[test]
fn only_a_workspace_with_a_rule_events_can_run_enqueues() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);

    assert!(!an_update_enqueues(&pool, a), "no rules at all");
    rule(&pool, a, RuleTriggerKind::Manual, RuleState::Live);
    rule(&pool, a, RuleTriggerKind::Manual, RuleState::Paused);
    rule(&pool, a, RuleTriggerKind::TicketUpdated, RuleState::Paused);
    rule(&pool, a, RuleTriggerKind::TicketCreated, RuleState::Draft);
    rule(
        &pool,
        a,
        RuleTriggerKind::TicketReplied,
        RuleState::Archived,
    );
    assert!(
        !an_update_enqueues(&pool, a),
        "manual, paused, draft and archived rules"
    );

    rule(&pool, b, RuleTriggerKind::TimeElapsed, RuleState::Live);
    assert!(an_update_enqueues(&pool, b), "a live time-elapsed rule");
    assert!(!an_update_enqueues(&pool, a), "B's rule is B's");

    let shadow = rule(&pool, a, RuleTriggerKind::TicketUpdated, RuleState::DryRun);
    assert!(an_update_enqueues(&pool, a), "a dry-run event rule");
    set_state(&pool, a, shadow, RuleState::Paused);
    assert!(!an_update_enqueues(&pool, a), "paused again");
    set_state(&pool, a, shadow, RuleState::Live);
    assert!(an_update_enqueues(&pool, a), "a live event rule");
}

/// Every `ticket.*_changed` event the backend names is a decision: enqueued
/// or skipped above. A new one fails here until it is placed in one list and
/// the trigger agrees.
#[test]
fn every_changed_event_is_enqueued_or_skipped_on_purpose() {
    fn walk(dir: &Path, found: &mut BTreeSet<String>, pattern: &regex::Regex) {
        for entry in std::fs::read_dir(dir).expect("read src") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, found, pattern);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("read source");
                for cap in pattern.captures_iter(&text) {
                    found.insert(cap[1].to_string());
                }
            }
        }
    }
    let pattern = regex::Regex::new(r#""(ticket\.[a-z_]+_changed)""#).expect("regex");
    let mut found = BTreeSet::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
        &pattern,
    );
    assert!(found.contains("ticket.priority_changed"), "{found:?}");
    let unplaced: Vec<_> = found
        .iter()
        .filter(|t| !ENQUEUED.contains(&t.as_str()) && !SKIPPED.contains(&t.as_str()))
        .collect();
    assert!(
        unplaced.is_empty(),
        "decide whether rules react to these, then list them in ENQUEUED (and \
         rule_event_outbox_enqueue) or SKIPPED: {unplaced:?}"
    );
}

/// One outbox row and one budget row in `ws`, written as the app.
fn seed_rows(pool: &TestPool, ws: &WorkspaceSeed) -> (i64, Uuid) {
    rule(pool, ws, RuleTriggerKind::TicketCreated, RuleState::Live);
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &actor(ws), |c| {
        let sync_id = emit(
            c,
            SyncAggregate::Ticket,
            "ticket.updated",
            serde_json::json!({"id": 1}),
        )?;
        let root = Uuid::now_v7();
        diesel::insert_into(ticket_rule_runs::table)
            .values((
                ticket_rule_runs::root_id.eq(root),
                ticket_rule_runs::ticket_id.eq(1),
                ticket_rule_runs::rule_id.eq(1),
            ))
            .execute(c)?;
        Ok::<_, diesel::result::Error>((sync_id, root))
    })
    .expect("seed outbox and budget rows")
}

/// What `ws` sees of both tables: outbox sync ids and budget root ids, each
/// with the workspace the row names.
struct Visible {
    outbox: Vec<(i64, i32)>,
    runs: Vec<(Uuid, i32)>,
}

impl Visible {
    fn of(pool: &TestPool, ws: &WorkspaceSeed) -> Self {
        let mut conn = pool.get().expect("conn");
        with_actor_context(&mut conn, &actor(ws), |c| {
            let outbox = rule_event_outbox::table
                .select((rule_event_outbox::sync_id, rule_event_outbox::workspace_id))
                .load(c)?;
            let runs = ticket_rule_runs::table
                .select((ticket_rule_runs::root_id, ticket_rule_runs::workspace_id))
                .load(c)?;
            Ok::<_, diesel::result::Error>(Self { outbox, runs })
        })
        .expect("read as the app")
    }

    fn has(&self, (sync_id, root): (i64, Uuid)) -> (bool, bool) {
        (
            self.outbox.iter().any(|(id, _)| *id == sync_id),
            self.runs.iter().any(|(id, _)| *id == root),
        )
    }

    /// Every row it sees names `ws`.
    fn only(&self, ws: &WorkspaceSeed) -> bool {
        self.outbox.iter().all(|(_, w)| *w == ws.workspace_id)
            && self.runs.iter().all(|(_, w)| *w == ws.workspace_id)
    }
}

#[test]
fn each_workspace_sees_and_writes_only_its_own_rows() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);

    let a_rows = seed_rows(&pool, a);
    let b_rows = seed_rows(&pool, b);
    let (b_sync, b_root) = b_rows;

    let seen = Visible::of(&pool, a);
    assert!(seen.only(a), "A sees only its own rows");
    assert_eq!(seen.has(a_rows), (true, true));
    assert_eq!(seen.has(b_rows), (false, false));
    let seen = Visible::of(&pool, b);
    assert!(seen.only(b), "B sees only its own rows");
    assert_eq!(seen.has(b_rows), (true, true));
    assert_eq!(seen.has(a_rows), (false, false));

    // Pinned to A, a row naming B is refused.
    let mut conn = pool.get().expect("conn");
    let into_b = with_actor_context(&mut conn, &actor(a), |c| {
        diesel::insert_into(ticket_rule_runs::table)
            .values((
                ticket_rule_runs::root_id.eq(Uuid::now_v7()),
                ticket_rule_runs::ticket_id.eq(1),
                ticket_rule_runs::rule_id.eq(1),
                ticket_rule_runs::workspace_id.eq(b.workspace_id),
            ))
            .execute(c)
    });
    assert!(into_b.is_err(), "a budget row for another workspace");
    let into_b = with_actor_context(&mut conn, &actor(a), |c| {
        diesel::insert_into(rule_event_outbox::table)
            .values((
                rule_event_outbox::sync_id.eq(b_sync + 1_000_000),
                rule_event_outbox::workspace_id.eq(b.workspace_id),
            ))
            .execute(c)
    });
    assert!(into_b.is_err(), "an outbox row for another workspace");

    // Nor can A clear B's rows.
    let cleared = with_actor_context(&mut conn, &actor(a), |c| {
        let outbox = diesel::delete(rule_event_outbox::table.find(b_sync)).execute(c)?;
        let runs =
            diesel::delete(ticket_rule_runs::table.filter(ticket_rule_runs::root_id.eq(b_root)))
                .execute(c)?;
        Ok::<_, diesel::result::Error>(outbox + runs)
    })
    .expect("delete as A");
    assert_eq!(cleared, 0);
    assert_eq!(Visible::of(&pool, b).has(b_rows), (true, true));
}

#[test]
fn deleting_a_workspace_removes_its_outbox_and_budget_rows() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);

    let kept = seed_rows(&pool, a);
    seed_rows(&pool, b);

    let mut conn = pool.get().expect("conn");
    let deleted = with_actor_bypass_context(
        &mut conn,
        &ActorContext::system("test:rule_event_outbox"),
        |c| {
            workspaces::archive_workspace(c, b.workspace_id)?;
            workspaces::hard_delete_workspace(
                c,
                b.workspace_id,
                Utc::now() + chrono::Duration::hours(1),
            )
        },
    )
    .expect("hard-delete the workspace");
    assert_eq!(deleted, 1);

    let (outbox, runs) = with_actor_bypass_context(
        &mut conn,
        &ActorContext::system("test:rule_event_outbox"),
        |c| {
            let outbox: Vec<i32> = rule_event_outbox::table
                .select(rule_event_outbox::workspace_id)
                .load(c)?;
            let runs: Vec<i32> = ticket_rule_runs::table
                .select(ticket_rule_runs::workspace_id)
                .load(c)?;
            Ok::<_, diesel::result::Error>((outbox, runs))
        },
    )
    .expect("read every workspace");
    assert!(
        !outbox.contains(&b.workspace_id),
        "B's outbox rows went with it"
    );
    assert!(
        !runs.contains(&b.workspace_id),
        "B's budget rows went with it"
    );
    assert_eq!(Visible::of(&pool, a).has(kept), (true, true), "A's stay");
}
