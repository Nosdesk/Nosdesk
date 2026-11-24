//! Event rules run: the rules engine's dispatcher drains the rule event
//! outbox and runs the live and dry-run rules of each event's trigger, as
//! the engine's system actor, on `nosdesk_app` the way production does.
//! These tests drive `Dispatcher::dispatch_batch` directly, like
//! `webhook_outbox_durability.rs` drives its drain.
#![allow(clippy::expect_used)]

use std::time::Duration;

use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{
    NewChannelMessage, NewComment, NewRule, NewTicket, Rule, RuleApplication,
    RuleApplicationStatus as Status, RuleState, RuleTriggerKind as Trigger, TicketPriority,
    TicketUpdate,
};
use backend::repository::{channels, comments, rules, tickets, workflow_states};
use backend::schema::{rule_applications, rule_event_outbox, ticket_tags};
use backend::services::rules::engine::{Dispatcher, MAX_ATTEMPTS};
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};

use crate::common::{self, TestDb, TestPool, TwoWorkspaces, WorkspaceSeed};

/// A sandbox with two workspaces and a dispatcher on the production-shaped
/// pool.
pub struct Harness {
    pub db: TestDb,
    pub pool: TestPool,
    pub seeded: TwoWorkspaces,
    pub engine: Dispatcher,
}

/// What a test rule is: its trigger, state, settings, conditions and steps.
pub struct Spec {
    pub trigger: Trigger,
    pub state: RuleState,
    pub trigger_config: Value,
    pub conditions: Value,
    pub actions: Value,
    pub priority: i32,
}

impl Spec {
    /// A live rule with no conditions.
    pub fn live(trigger: Trigger, actions: Value) -> Self {
        Self {
            trigger,
            state: RuleState::Live,
            trigger_config: json!({}),
            conditions: json!([]),
            actions,
            priority: 100,
        }
    }
}

/// A step that runs and changes nothing on an unassigned ticket, so no
/// event follows it.
pub fn marker() -> Value {
    json!([{ "kind": "unassign", "config": {} }])
}

pub fn leaf(field: &str, op: &str, value: Value) -> Value {
    json!({ "kind": "leaf", "field": field, "op": op, "value": value })
}

pub fn admin(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id)
}

impl Harness {
    pub fn new() -> Self {
        let db = TestDb::new();
        let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
        let pool = db.runtime_pool(4);
        let engine =
            Dispatcher::new(pool.clone(), None).with_lock_timeout(Duration::from_millis(300));
        Self {
            db,
            pool,
            seeded,
            engine,
        }
    }

    pub fn as_admin<T>(
        &self,
        ws: &WorkspaceSeed,
        f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
    ) -> T {
        let mut conn = self.pool.get().expect("conn");
        with_actor_context(&mut conn, &admin(ws), f).expect("as the workspace admin")
    }

    /// Work on the outbox across workspaces, as the dispatcher's claim does.
    pub fn bypass<T>(&self, f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>) -> T {
        let mut conn = self.pool.get().expect("conn");
        with_actor_bypass_context(&mut conn, &ActorContext::system("test:rules_engine"), f)
            .expect("past row security")
    }

    pub fn rule(&self, ws: &WorkspaceSeed, spec: Spec) -> i32 {
        self.as_admin(ws, |c| {
            rules::create(
                c,
                NewRule {
                    workspace_id: ws.workspace_id,
                    name: format!("{:?} {:?} {}", spec.trigger, spec.state, spec.priority),
                    description: None,
                    trigger_kind: spec.trigger,
                    trigger_config: spec.trigger_config,
                    conditions: spec.conditions,
                    actions: spec.actions,
                    reads_set: Vec::new(),
                    writes_set: Vec::new(),
                    state: spec.state,
                    priority: spec.priority,
                    created_by: Some(ws.admin_uuid),
                },
            )
        })
        .id
    }

    /// A ticket from `ws`'s member, created the way the app creates one, so
    /// its `ticket.created` reaches the outbox.
    pub fn ticket(&self, ws: &WorkspaceSeed, title: &str) -> i32 {
        self.as_admin(ws, |c| {
            let open = workflow_states::default_state(c)?;
            tickets::create_ticket(
                c,
                NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open.id,
                    requester_uuid: Some(ws.member_uuid),
                    ..Default::default()
                },
            )
        })
        .id
    }

    pub fn update(&self, ws: &WorkspaceSeed, ticket: i32, update: TicketUpdate) {
        self.as_admin(ws, |c| {
            tickets::update_ticket_partial(c, ticket, update, None).map_err(Into::into)
        });
    }

    pub fn set_priority(&self, ws: &WorkspaceSeed, ticket: i32, priority: TicketPriority) {
        self.update(
            ws,
            ticket,
            TicketUpdate {
                priority: Some(priority),
                ..Default::default()
            },
        );
    }

    /// A public reply on `ticket` by `author`.
    pub fn reply(&self, ws: &WorkspaceSeed, ticket: i32, author: Uuid) -> i32 {
        let actor = ActorContext::user(author, None).with_workspace(ws.workspace_id);
        let mut conn = self.pool.get().expect("conn");
        with_actor_context(&mut conn, &actor, |c| {
            comments::create_comment(
                c,
                NewComment {
                    content: "<p>Any news?</p>".to_string(),
                    ticket_id: ticket,
                    user_uuid: author,
                    ..NewComment::default()
                },
                None,
            )
        })
        .expect("post a reply")
        .id
    }

    pub fn tag(&self, ws: &WorkspaceSeed, name: &str) -> i32 {
        use backend::schema::tags;
        self.as_admin(ws, |c| {
            diesel::insert_into(tags::table)
                .values(tags::name.eq(name))
                .returning(tags::id)
                .get_result(c)
        })
    }

    /// Run the dispatcher until it claims nothing.
    pub async fn drain(&self) {
        for _ in 0..50 {
            if self.engine.dispatch_batch().await.expect("dispatch") == 0 {
                return;
            }
        }
        panic!("the outbox never drained");
    }

    /// Every application row of `rule_id`, oldest first.
    pub fn applications(&self, ws: &WorkspaceSeed, rule_id: i32) -> Vec<RuleApplication> {
        self.as_admin(ws, |c| {
            rule_applications::table
                .filter(rule_applications::rule_id.eq(rule_id))
                .order(rule_applications::id)
                .load(c)
        })
    }

    pub fn statuses(&self, ws: &WorkspaceSeed, rule_id: i32) -> Vec<Status> {
        self.applications(ws, rule_id)
            .into_iter()
            .map(|a| a.status)
            .collect()
    }

    pub fn rule_row(&self, ws: &WorkspaceSeed, rule_id: i32) -> Rule {
        self.as_admin(ws, |c| rules::find(c, rule_id))
            .expect("the rule")
    }

    pub fn priority(&self, ws: &WorkspaceSeed, ticket: i32) -> TicketPriority {
        self.as_admin(ws, |c| tickets::get_ticket_by_id(c, ticket))
            .priority
    }

    pub fn tags(&self, ws: &WorkspaceSeed, ticket: i32) -> Vec<i32> {
        self.as_admin(ws, |c| {
            ticket_tags::table
                .filter(ticket_tags::ticket_id.eq(ticket))
                .select(ticket_tags::tag_id)
                .order(ticket_tags::tag_id)
                .load(c)
        })
    }

    /// Make every waiting outbox row due now.
    pub fn due_now(&self) {
        self.bypass(|c| {
            diesel::update(rule_event_outbox::table.filter(rule_event_outbox::dead_at.is_null()))
                .set((
                    rule_event_outbox::next_attempt_at.eq(diesel::dsl::now),
                    rule_event_outbox::claimed_at.eq(None::<chrono::DateTime<chrono::Utc>>),
                ))
                .execute(c)
        });
    }

    /// Hold `ticket`'s lock, the one a rule run takes, on a connection of
    /// its own until the guard drops.
    pub fn hold_ticket_lock(&self, ws: &WorkspaceSeed, ticket: i32) -> TicketLock {
        let key = ((ws.workspace_id as i64) << 32) | (ticket as i64 & 0xffff_ffff);
        let mut conn = self.db.conn();
        diesel::sql_query("SELECT pg_advisory_lock($1)")
            .bind::<diesel::sql_types::BigInt, _>(key)
            .execute(&mut conn)
            .expect("take the ticket's lock");
        TicketLock { conn, key }
    }
}

pub struct TicketLock {
    conn: backend::db::DbConnection,
    key: i64,
}

impl Drop for TicketLock {
    fn drop(&mut self) {
        let _ = diesel::sql_query("SELECT pg_advisory_unlock($1)")
            .bind::<diesel::sql_types::BigInt, _>(self.key)
            .execute(&mut self.conn);
    }
}

/// A new ticket runs the live `ticket_created` rules whose conditions it
/// meets, as the engine's system actor in the event's chain. A live rule
/// whose conditions it doesn't meet leaves no row, only a count.
#[actix_rt::test]
async fn a_new_ticket_runs_the_created_rules_it_meets() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let meets = h.rule(
        a,
        Spec {
            conditions: leaf("ticket.title", "contains", json!("printer")),
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
            )
        },
    );
    let misses = h.rule(
        a,
        Spec {
            conditions: leaf("ticket.title", "contains", json!("laptop")),
            ..Spec::live(Trigger::TicketCreated, marker())
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.drain().await;

    let fired = h.applications(a, meets);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].status, Status::Succeeded);
    assert_eq!(fired[0].actor_kind, "system");
    assert_eq!(
        fired[0].originating_event_kind.as_deref(),
        Some("ticket.created")
    );
    assert_eq!(
        fired[0].condition_evaluation.as_ref().expect("evaluation")["matched"],
        true
    );
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
    assert_eq!(h.rule_row(a, meets).fire_count, 1);

    assert!(
        h.applications(a, misses).is_empty(),
        "no row for a live rule's skip"
    );
    let missed = h.rule_row(a, misses);
    assert_eq!(missed.skipped_condition_unmet_count, 1);
    assert_eq!(missed.fire_count, 0);

    // The rule's write is credited to the engine, in the creation's chain.
    let (actor_ref, correlation): (Option<String>, Option<Uuid>) = h.as_admin(a, |c| {
        use backend::schema::sync_actions;
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(ticket.to_string()))
            .filter(sync_actions::event_type.eq("ticket.priority_changed"))
            .select((sync_actions::actor_ref, sync_actions::correlation_id))
            .first(c)
    });
    assert_eq!(actor_ref, Some(format!("rules_engine:{meets}")));
    assert_eq!(correlation, fired[0].correlation_id);
    assert!(correlation.is_some());
}

/// An update runs the `ticket_updated` rules whose trigger names a field it
/// changed; a rule that names none runs on any change.
#[actix_rt::test]
async fn an_update_runs_the_rules_whose_fields_changed() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let on_priority = h.rule(
        a,
        Spec {
            trigger_config: json!({ "changed_fields_include": ["ticket.priority"] }),
            ..Spec::live(Trigger::TicketUpdated, marker())
        },
    );
    let on_title = h.rule(
        a,
        Spec {
            trigger_config: json!({ "changed_fields_include": ["ticket.title"] }),
            ..Spec::live(Trigger::TicketUpdated, marker())
        },
    );
    let on_any = h.rule(
        a,
        Spec {
            trigger_config: json!({ "changed_fields_include": [] }),
            ..Spec::live(Trigger::TicketUpdated, marker())
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.set_priority(a, ticket, TicketPriority::Urgent);
    h.drain().await;

    assert_eq!(h.statuses(a, on_priority), vec![Status::Succeeded]);
    assert!(h.statuses(a, on_title).is_empty());
    assert_eq!(h.statuses(a, on_any), vec![Status::Succeeded]);
    let fired = &h.applications(a, on_priority)[0];
    assert_eq!(
        fired.condition_evaluation.as_ref().expect("evaluation")["changed_fields"],
        json!(["ticket.priority"])
    );

    // A title change runs the title rule and the any-change rule.
    h.update(
        a,
        ticket,
        TicketUpdate {
            title: Some("Printer on fire".to_string()),
            ..Default::default()
        },
    );
    h.drain().await;
    assert_eq!(h.statuses(a, on_priority).len(), 1);
    assert_eq!(h.statuses(a, on_title), vec![Status::Succeeded]);
    assert_eq!(h.statuses(a, on_any).len(), 2);
}

/// A public reply runs the `ticket_replied` rules of its kind: a
/// requester's reply is a customer's, a staff member's an agent's, and
/// `any` takes both. An internal note and the message a ticket was opened
/// with run none.
#[actix_rt::test]
async fn a_reply_runs_the_rules_of_its_kind() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let replied = |kind: &str| {
        h.rule(
            a,
            Spec {
                trigger_config: json!({ "reply_kind": kind }),
                ..Spec::live(Trigger::TicketReplied, marker())
            },
        )
    };
    let (customer, agent, any) = (replied("customer"), replied("agent"), replied("any"));
    let ticket = h.ticket(a, "Printer offline");

    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert_eq!(h.statuses(a, customer), vec![Status::Succeeded]);
    assert!(h.statuses(a, agent).is_empty());
    assert_eq!(h.statuses(a, any), vec![Status::Succeeded]);
    let fired = &h.applications(a, customer)[0];
    assert_eq!(
        fired.condition_evaluation.as_ref().expect("evaluation")["reply_kind"],
        "customer"
    );

    h.reply(a, ticket, a.admin_uuid);
    h.drain().await;
    assert_eq!(h.statuses(a, customer).len(), 1);
    assert_eq!(h.statuses(a, agent), vec![Status::Succeeded]);
    assert_eq!(h.statuses(a, any).len(), 2);

    // An internal note is no reply.
    h.as_admin(a, |c| {
        comments::create_comment(
            c,
            NewComment {
                content: "<p>Checking the toner</p>".to_string(),
                ticket_id: ticket,
                user_uuid: a.admin_uuid,
                is_internal: true,
                ..NewComment::default()
            },
            None,
        )
    });
    // Nor is the message a ticket is opened with, written with it.
    h.as_admin(a, |c| {
        let open = workflow_states::default_state(c)?;
        let opened = tickets::create_ticket(
            c,
            NewTicket {
                title: "Scanner jammed".to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(a.member_uuid),
                ..Default::default()
            },
        )?;
        comments::create_comment(
            c,
            NewComment {
                content: "<p>It jammed again</p>".to_string(),
                ticket_id: opened.id,
                user_uuid: a.member_uuid,
                ..NewComment::default()
            },
            None,
        )
    });
    h.drain().await;
    assert_eq!(h.statuses(a, customer).len(), 1);
    assert_eq!(h.statuses(a, agent).len(), 1);
    assert_eq!(h.statuses(a, any).len(), 2);
}

/// A ticket held for approval is turned away before any rule runs: no rows,
/// a preflight count on each rule it would have run. Once approved, its
/// `ticket_created` rules run.
#[actix_rt::test]
async fn a_held_ticket_runs_nothing_until_it_is_approved() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let created = h.rule(
        a,
        Spec::live(
            Trigger::TicketCreated,
            json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
        ),
    );
    let ticket = h.ticket(a, "New laptop");
    h.as_admin(a, |c| {
        use backend::schema::{ticket_approvals, tickets as t};
        diesel::update(t::table.find(ticket))
            .set((
                t::approval_state.eq(Some("pending")),
                t::rules_held_at.eq(Some(chrono::Utc::now().naive_utc())),
            ))
            .execute(c)?;
        diesel::insert_into(ticket_approvals::table)
            .values((
                ticket_approvals::ticket_id.eq(ticket),
                ticket_approvals::approver_uuid.eq(a.admin_uuid),
                ticket_approvals::round.eq(1),
            ))
            .execute(c)
    });
    h.drain().await;

    assert!(
        h.applications(a, created).is_empty(),
        "no row for a held ticket"
    );
    let rule = h.rule_row(a, created);
    assert_eq!(rule.skipped_preflight_count, 1);
    assert_eq!(rule.fire_count, 0);
    assert_eq!(h.priority(a, ticket), TicketPriority::Medium);

    // Approved: its creation rules run.
    let approved = h.as_admin(a, |c| {
        backend::repository::ticket_approvals::auto_approve(c, ticket)
    });
    assert!(approved);
    h.drain().await;
    assert_eq!(h.statuses(a, created), vec![Status::Succeeded]);
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
    assert_eq!(h.rule_row(a, created).skipped_preflight_count, 1);
}

/// An event on a ticket merged before its rules ran runs none of them.
#[actix_rt::test]
async fn a_merged_ticket_runs_nothing() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let updated = h.rule(a, Spec::live(Trigger::TicketUpdated, marker()));
    let source = h.ticket(a, "Printer offline");
    let target = h.ticket(a, "Printers on floor 2");
    h.set_priority(a, source, TicketPriority::High);
    h.as_admin(a, |c| {
        use backend::schema::ticket_merges;
        diesel::insert_into(ticket_merges::table)
            .values((
                ticket_merges::ticket_id.eq(source),
                ticket_merges::merged_into_ticket_id.eq(target),
                ticket_merges::merged_at.eq(chrono::Utc::now()),
            ))
            .execute(c)
    });
    h.drain().await;

    assert!(h.applications(a, updated).is_empty());
    assert_eq!(h.rule_row(a, updated).skipped_preflight_count, 1);
}

/// A dry-run rule records what it would do and changes nothing; one whose
/// conditions aren't met records the skip as a row.
#[actix_rt::test]
async fn a_dry_run_writes_a_shadow_row_only() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let shadow = h.rule(
        a,
        Spec {
            state: RuleState::DryRun,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "set_priority", "config": { "priority": "urgent" } }]),
            )
        },
    );
    let unmet = h.rule(
        a,
        Spec {
            state: RuleState::DryRun,
            conditions: leaf("ticket.title", "contains", json!("laptop")),
            ..Spec::live(Trigger::TicketCreated, marker())
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.drain().await;

    let rows = h.applications(a, shadow);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::DryRun);
    assert_eq!(
        rows[0].actions_taken.as_ref().expect("would do")[0]["priority"],
        "urgent"
    );
    assert_eq!(
        h.priority(a, ticket),
        TicketPriority::Medium,
        "nothing written"
    );
    assert_eq!(h.rule_row(a, shadow).fire_count, 0);

    let skipped = h.applications(a, unmet);
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].status, Status::SkippedConditionUnmet);
    assert_eq!(
        skipped[0]
            .condition_evaluation
            .as_ref()
            .expect("evaluation")["matched"],
        false
    );
    assert_eq!(h.rule_row(a, unmet).skipped_condition_unmet_count, 0);
}

/// A dry-run rule's `stop_processing` stops nothing: the live rule after
/// it still runs, and the shadow row says it would have stopped them.
#[actix_rt::test]
async fn a_dry_run_stop_does_not_stop_the_live_rules_after_it() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let tag = h.tag(a, "triaged");
    let shadow = h.rule(
        a,
        Spec {
            state: RuleState::DryRun,
            priority: 1,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "stop_processing" }]),
            )
        },
    );
    let live = h.rule(
        a,
        Spec {
            priority: 2,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "add_tags", "config": { "tag_ids": [tag] } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.drain().await;

    let rows = h.applications(a, shadow);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::DryRun);
    let step = &rows[0].actions_taken.as_ref().expect("would do")[0];
    assert_eq!(step["kind"], "stop_processing");
    assert_eq!(step["would_stop_processing"], true);
    assert_eq!(h.statuses(a, live), vec![Status::Succeeded]);
    assert_eq!(h.tags(a, ticket), vec![tag]);
}

/// A live rule's `stop_processing` stops the rules after it.
#[actix_rt::test]
async fn a_live_stop_stops_the_rules_after_it() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let tag = h.tag(a, "triaged");
    let first = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "stop_processing" }]),
            )
        },
    );
    let second = h.rule(
        a,
        Spec {
            priority: 2,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "add_tags", "config": { "tag_ids": [tag] } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.drain().await;

    assert_eq!(h.statuses(a, first), vec![Status::Succeeded]);
    assert!(h.statuses(a, second).is_empty());
    assert!(h.tags(a, ticket).is_empty());
}

/// A rule that fails for good (its assignee can't work tickets) leaves a
/// `failed` row at once, and the rule after it still runs.
#[actix_rt::test]
async fn a_deterministic_failure_writes_a_failed_row_and_the_next_rule_runs() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let refused = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "assign", "config": { "method": "direct", "user_uuid": a.member_uuid } }]),
            )
        },
    );
    let next = h.rule(
        a,
        Spec {
            priority: 2,
            ..Spec::live(
                Trigger::TicketCreated,
                json!([{ "kind": "set_priority", "config": { "priority": "low" } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.drain().await;

    let failed = h.applications(a, refused);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].status, Status::Failed);
    assert!(
        failed[0]
            .failure_reason
            .as_deref()
            .is_some_and(|r| r.contains("Only agents and admins")),
        "{:?}",
        failed[0].failure_reason
    );
    assert_eq!(h.statuses(a, next), vec![Status::Succeeded]);
    assert_eq!(h.priority(a, ticket), TicketPriority::Low);
    let left: i64 = h.bypass(|c| rule_event_outbox::table.count().get_result(c));
    assert_eq!(left, 0, "the event is done");
}

/// A transient database error (here a lock timeout behind another holder
/// of the ticket's lock) leaves no row: the event is retried, and the rule
/// runs once on the next attempt.
#[actix_rt::test]
async fn a_transient_error_retries_the_event_without_a_failed_row() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let first = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(Trigger::TicketCreated, marker())
        },
    );
    let ticket = h.ticket(a, "Printer offline");

    let lock = h.hold_ticket_lock(a, ticket);
    h.drain().await;
    assert!(
        h.applications(a, first).is_empty(),
        "no row for a transient error"
    );
    let (attempts, last_error, dead): (i16, Option<String>, Option<chrono::DateTime<chrono::Utc>>) =
        h.bypass(|c| {
            rule_event_outbox::table
                .select((
                    rule_event_outbox::attempts,
                    rule_event_outbox::last_error,
                    rule_event_outbox::dead_at,
                ))
                .first(c)
        });
    assert_eq!(
        (attempts, last_error.as_deref(), dead),
        (1, Some("transient"), None)
    );
    drop(lock);

    h.due_now();
    h.drain().await;
    assert_eq!(h.statuses(a, first), vec![Status::Succeeded]);
    let left: i64 = h.bypass(|c| rule_event_outbox::table.count().get_result(c));
    assert_eq!(left, 0);
}

/// A rule that already ran on an earlier attempt at the event is skipped
/// quietly on the retry, while the rule the error stopped at runs.
#[actix_rt::test]
async fn a_retried_event_runs_each_rule_once() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let before = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(Trigger::TicketCreated, marker())
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    // A run of `before` on this event already committed, as on an attempt
    // that went on to fail at a later rule.
    h.drain().await;
    assert_eq!(h.statuses(a, before), vec![Status::Succeeded]);
    let sync_id = h
        .as_admin(a, |c| rules::created_event(c, ticket))
        .expect("created");
    h.bypass(|c| {
        diesel::insert_into(rule_event_outbox::table)
            .values((
                rule_event_outbox::sync_id.eq(sync_id),
                rule_event_outbox::workspace_id.eq(a.workspace_id),
                rule_event_outbox::attempts.eq(1),
            ))
            .execute(c)
    });
    h.drain().await;
    assert_eq!(
        h.statuses(a, before),
        vec![Status::Succeeded],
        "ran once, no suppression row"
    );
}

/// A retried event stops at an earlier rule only when that rule's run
/// stopped the rules after it. A rule with `stop_processing` that failed (or
/// was suppressed) stopped nothing, so on the retry the rules after it run.
#[actix_rt::test]
async fn a_retried_event_stops_only_where_a_run_stopped() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let stop = |assign_to: Uuid| {
        json!([
            { "kind": "assign", "config": { "method": "direct", "user_uuid": assign_to } },
            { "kind": "stop_processing" },
        ])
    };
    // The member can't work tickets, so this one fails at its first step.
    let failing = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(Trigger::TicketCreated, stop(a.member_uuid))
        },
    );
    let failed_ticket = h.ticket(a, "Printer offline");
    h.drain().await;
    assert_eq!(h.statuses(a, failing), vec![Status::Failed]);

    // An attempt at the event that went on to fail later: the rule after the
    // failed one is retried and runs.
    let after = h.rule(
        a,
        Spec {
            priority: 2,
            ..Spec::live(Trigger::TicketCreated, marker())
        },
    );
    let requeue = |ticket: i32| {
        let sync_id = h
            .as_admin(a, |c| rules::created_event(c, ticket))
            .expect("created");
        h.bypass(|c| {
            diesel::insert_into(rule_event_outbox::table)
                .values((
                    rule_event_outbox::sync_id.eq(sync_id),
                    rule_event_outbox::workspace_id.eq(a.workspace_id),
                    rule_event_outbox::attempts.eq(1),
                ))
                .execute(c)
        });
    };
    requeue(failed_ticket);
    h.drain().await;
    assert_eq!(
        h.statuses(a, failing),
        vec![Status::Failed],
        "not run again"
    );
    assert_eq!(
        h.statuses(a, after),
        vec![Status::Succeeded],
        "runs on the retry"
    );

    // A run that did stop the rules after it still stops them on a retry.
    h.as_admin(a, |c| {
        rules::archive(c, failing, chrono::Utc::now()).map_err(|e| match e {
            rules::WriteError::Db(e) => e,
            other => diesel::result::Error::QueryBuilderError(other.to_string().into()),
        })
    });
    let stopping = h.rule(
        a,
        Spec {
            priority: 1,
            ..Spec::live(Trigger::TicketCreated, stop(a.admin_uuid))
        },
    );
    let stopped_ticket = h.ticket(a, "Monitor flickers");
    h.drain().await;
    assert_eq!(h.statuses(a, stopping), vec![Status::Succeeded]);
    let after_runs = h.statuses(a, after).len();
    requeue(stopped_ticket);
    h.drain().await;
    assert_eq!(h.statuses(a, stopping), vec![Status::Succeeded]);
    assert_eq!(h.statuses(a, after).len(), after_runs, "still stopped");
}

/// On an event's last attempt a transient error is recorded as `failed`
/// like any other and the event is dead-lettered.
#[actix_rt::test]
async fn the_last_attempt_records_the_failure_and_dead_letters_the_event() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, Spec::live(Trigger::TicketCreated, marker()));
    let ticket = h.ticket(a, "Printer offline");
    h.bypass(|c| {
        diesel::update(rule_event_outbox::table)
            .set(rule_event_outbox::attempts.eq(MAX_ATTEMPTS - 1))
            .execute(c)
    });

    let lock = h.hold_ticket_lock(a, ticket);
    h.drain().await;
    drop(lock);

    let failed = h.applications(a, rule);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].status, Status::Failed);
    assert!(
        failed[0]
            .failure_reason
            .as_deref()
            .is_some_and(|r| r.contains("lock timeout")),
        "{:?}",
        failed[0].failure_reason
    );
    let dead: Vec<Option<chrono::DateTime<chrono::Utc>>> = h.bypass(|c| {
        rule_event_outbox::table
            .select(rule_event_outbox::dead_at)
            .load(c)
    });
    assert_eq!(dead.len(), 1);
    assert!(dead[0].is_some(), "dead-lettered");
}

/// A rule that would email the requester doesn't answer an automated email
/// (a mailing list here): it is suppressed by the loop guard and posts
/// nothing. A rule that only tags the ticket still runs.
#[actix_rt::test]
async fn a_reply_to_an_automated_email_is_suppressed() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let tag = h.tag(a, "list-mail");
    let answers = h.rule(
        a,
        Spec {
            trigger_config: json!({ "reply_kind": "any" }),
            ..Spec::live(
                Trigger::TicketReplied,
                json!([{ "kind": "reply", "config": { "visibility": "public", "body": "Thanks, we're on it" } }]),
            )
        },
    );
    let tags = h.rule(
        a,
        Spec {
            trigger_config: json!({ "reply_kind": "any" }),
            ..Spec::live(
                Trigger::TicketReplied,
                json!([{ "kind": "add_tags", "config": { "tag_ids": [tag] } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    let comment = h.reply(a, ticket, a.member_uuid);
    h.as_admin(a, |c| {
        let channel = channels::ensure_managed_channel(c, a.workspace_id)?;
        channels::record_message(
            c,
            NewChannelMessage {
                channel_id: channel.id,
                external_id: "<digest-1@lists.example>".to_string(),
                direction: backend::models::CHANNEL_DIRECTION_INBOUND.to_string(),
                ticket_id: Some(ticket),
                comment_id: Some(comment),
                in_reply_to: None,
                from_address: Some("team@lists.example".to_string()),
                author_user_uuid: Some(a.member_uuid),
                raw_metadata: Some(json!({ "headers": [["List-Id", "<team.lists.example>"]] })),
            },
        )
    });
    h.drain().await;

    let rows = h.applications(a, answers);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::SuppressedLoopGuard);
    let comments: i64 = h.as_admin(a, |c| {
        use backend::schema::comments;
        comments::table
            .filter(comments::ticket_id.eq(ticket))
            .count()
            .get_result(c)
    });
    assert_eq!(comments, 1, "no automated reply");
    assert_eq!(h.statuses(a, tags), vec![Status::Succeeded]);
    assert_eq!(h.tags(a, ticket), vec![tag]);
}

/// Each workspace's events run its own rules only.
#[actix_rt::test]
async fn rules_run_only_in_their_own_workspace() {
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);
    let in_a = h.rule(a, Spec::live(Trigger::TicketCreated, marker()));
    let in_b = h.rule(b, Spec::live(Trigger::TicketCreated, marker()));

    let ticket_a = h.ticket(a, "Printer offline");
    h.drain().await;
    assert_eq!(h.statuses(a, in_a), vec![Status::Succeeded]);
    assert!(h.statuses(b, in_b).is_empty());
    assert_eq!(h.rule_row(b, in_b).fire_count, 0);

    let ticket_b = h.ticket(b, "Printer offline");
    h.drain().await;
    let rows_a = h.applications(a, in_a);
    let rows_b = h.applications(b, in_b);
    assert_eq!((rows_a.len(), rows_b.len()), (1, 1));
    assert_eq!(
        (rows_a[0].ticket_id, rows_b[0].ticket_id),
        (ticket_a, ticket_b)
    );
    assert_eq!(
        (rows_a[0].workspace_id, rows_b[0].workspace_id),
        (a.workspace_id, b.workspace_id)
    );
}

/// The asset steps run from an event like any other step: an update gives
/// the ticket's linked assets to its requester.
#[actix_rt::test]
async fn an_event_rule_runs_asset_steps() {
    use backend::models::NewAsset;
    use backend::schema::assets;
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(
        a,
        Spec {
            trigger_config: json!({ "changed_fields_include": ["ticket.priority"] }),
            ..Spec::live(
                Trigger::TicketUpdated,
                json!([{ "kind": "assign_assets_to_requester", "config": {} }]),
            )
        },
    );
    let ticket = h.ticket(a, "New starter");
    let laptop = h.as_admin(a, |c| {
        let (asset, _) = backend::repository::asset_lifecycle::create(
            c,
            NewAsset {
                name: "Laptop".to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: None,
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
            backend::models::AssetStatus::InStock,
            None,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        tickets::add_device_to_ticket(c, ticket, asset.id)?;
        Ok(asset.id)
    });
    h.set_priority(a, ticket, TicketPriority::High);
    h.drain().await;

    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);
    let (status, user): (String, Option<Uuid>) = h.as_admin(a, |c| {
        assets::table
            .find(laptop)
            .select((assets::status, assets::primary_user_uuid))
            .first(c)
    });
    assert_eq!((status.as_str(), user), ("in_service", Some(a.member_uuid)));
}

/// A public reply step posts as the rule's author and is handed to the
/// relay once the run commits. Its own reply, an agent's, doesn't run a
/// rule on requesters' replies.
#[actix_rt::test]
async fn a_reply_step_posts_and_relays_the_reply() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let answers = h.rule(
        a,
        Spec {
            trigger_config: json!({ "reply_kind": "customer" }),
            ..Spec::live(
                Trigger::TicketReplied,
                json!([{ "kind": "reply", "config": { "visibility": "public", "body": "Thanks, we're on it" } }]),
            )
        },
    );
    let ticket = h.ticket(a, "Printer offline");
    h.reply(a, ticket, a.member_uuid);
    h.drain().await;

    let rows = h.applications(a, answers);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, Status::Succeeded);
    let replies: Vec<(Uuid, bool)> = h.as_admin(a, |c| {
        use backend::schema::comments;
        comments::table
            .filter(comments::ticket_id.eq(ticket))
            .order(comments::id)
            .select((comments::user_uuid, comments::is_internal))
            .load(c)
    });
    assert_eq!(replies, vec![(a.member_uuid, false), (a.admin_uuid, false)]);
}
