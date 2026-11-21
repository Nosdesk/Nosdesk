//! Time-elapsed rules fire: clocks derived from the ticket and its replies,
//! kept in `ticket_elapsed_queue` by the event consumer and by a rule's
//! requeue, and fired by the worker as the engine's system actor, on
//! `nosdesk_app` the way production runs them. These tests drive
//! `elapsed::fire_due` and the dispatcher directly.
#![allow(clippy::expect_used)]

use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{
    NewTicket, RuleApplicationStatus as Status, RuleState, RuleTriggerKind as Trigger, RuleUpdate,
    TicketPriority, TicketUpdate, WorkflowStateCategory, EMAIL_BACKLOG,
};
use backend::repository::rule_elapsed_queue as queue;
use backend::repository::{rules, tickets, workflow_states};
use backend::schema::{comments, ticket_elapsed_queue as q};
use backend::services::rules::elapsed::{self, Fired};

use crate::common::WorkspaceSeed;
use crate::rules_event_triggers::{leaf, Harness, Spec};

/// A queue row: ticket, rule, clock, started, due.
type Row = (i32, i32, String, DateTime<Utc>, DateTime<Utc>);

fn set_priority(priority: &str) -> Value {
    json!([{ "kind": "set_priority", "config": { "priority": priority } }])
}

impl Harness {
    /// A live time-elapsed rule on `clock`, waiting `minutes`, with every
    /// open ticket queued as putting it live does.
    fn time_rule(&self, ws: &WorkspaceSeed, clock: &str, minutes: i64, actions: Value) -> i32 {
        let id = self.rule(
            ws,
            Spec {
                trigger_config: json!({ "clock_name": clock, "minutes": minutes }),
                ..Spec::live(Trigger::TimeElapsed, actions)
            },
        );
        self.requeue(ws, id);
        id
    }

    fn requeue(&self, ws: &WorkspaceSeed, rule_id: i32) {
        elapsed::requeue_rule(&self.pool, ws.workspace_id, rule_id).expect("requeue");
    }

    fn fire(&self) -> usize {
        elapsed::fire_due(&self.pool, None).expect("fire")
    }

    fn rows(&self, ws: &WorkspaceSeed) -> Vec<Row> {
        self.as_admin(ws, |c| {
            q::table
                .select((
                    q::ticket_id,
                    q::rule_id,
                    q::clock_name,
                    q::clock_started_at,
                    q::due_at,
                ))
                .order((q::ticket_id, q::rule_id))
                .load(c)
        })
    }

    fn row(&self, ws: &WorkspaceSeed, ticket: i32, rule: i32) -> Option<Row> {
        self.rows(ws)
            .into_iter()
            .find(|r| r.0 == ticket && r.1 == rule)
    }

    /// Move a ticket's creation `hours` back.
    fn backdate_ticket(&self, ticket: i32, hours: i64) {
        self.bypass(|c| {
            diesel::sql_query(
                "UPDATE tickets SET created_at = now() - make_interval(hours => $2) WHERE id = $1",
            )
            .bind::<diesel::sql_types::Integer, _>(ticket)
            .bind::<diesel::sql_types::Integer, _>(hours as i32)
            .execute(c)
        });
    }

    /// A public reply by `author`, written `hours` ago. Returns its time.
    fn reply_at(&self, ws: &WorkspaceSeed, ticket: i32, author: Uuid, hours: i64) -> DateTime<Utc> {
        let id = self.reply(ws, ticket, author);
        self.bypass(|c| {
            diesel::update(comments::table.find(id))
                .set(comments::created_at.eq(Utc::now() - chrono::Duration::hours(hours)))
                .returning(comments::created_at)
                .get_result(c)
        })
    }

    fn state_in(&self, ws: &WorkspaceSeed, category: WorkflowStateCategory) -> i32 {
        self.as_admin(ws, |c| workflow_states::first_in_category(c, category))
            .id
    }
}

/// Clocks run from replies however long ago they were written: the last
/// requester message for `since_last_customer_reply`, and the last staff
/// reply nobody answered for `awaiting_requester`. Each fires once, when
/// due, and has nothing more to wait for.
#[actix_rt::test]
async fn backdated_replies_start_the_clocks_and_each_fires_once() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Printer offline");
    h.backdate_ticket(ticket, 72);
    let asked = h.reply_at(a, ticket, a.member_uuid, 48);
    let answered = h.reply_at(a, ticket, a.admin_uuid, 24);

    let customer = h.time_rule(a, "since_last_customer_reply", 60, set_priority("high"));
    let waiting = h.time_rule(a, "awaiting_requester", 23 * 60, set_priority("high"));
    let later = h.time_rule(a, "awaiting_requester", 48 * 60, set_priority("urgent"));

    let row = h.row(a, ticket, customer).expect("a customer clock");
    assert_eq!(
        (row.2.as_str(), row.3),
        ("since_last_customer_reply", asked)
    );
    assert_eq!(row.4, asked + chrono::Duration::minutes(60));
    let row = h.row(a, ticket, waiting).expect("an awaiting clock");
    assert_eq!(
        (row.3, row.4),
        (answered, answered + chrono::Duration::hours(23))
    );
    let row = h.row(a, ticket, later).expect("a later awaiting clock");
    assert!(row.4 > Utc::now(), "not due for another day");

    assert_eq!(h.fire(), 2, "the two due clocks");
    for rule in [customer, waiting] {
        let fired = h.applications(a, rule);
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].status, Status::Succeeded);
        assert_eq!(fired[0].actor_kind, "system");
        let evaluation = fired[0].condition_evaluation.as_ref().expect("evaluation");
        assert_eq!(evaluation["trigger"], "time_elapsed");
        assert_eq!(evaluation["matched"], true);
        assert!(
            h.row(a, ticket, rule).is_none(),
            "fired, and doesn't repeat"
        );
    }
    assert_eq!(
        h.applications(a, waiting)[0]
            .originating_event_kind
            .as_deref(),
        Some("clock.awaiting_requester")
    );
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
    assert!(h.row(a, ticket, later).is_some(), "still waiting");

    // The writes' events reach the consumer; nothing fires again, and
    // neither does a rebuild of the queue.
    h.drain().await;
    for rule in [customer, waiting, later] {
        h.requeue(a, rule);
    }
    assert_eq!(h.fire(), 0);
    assert_eq!(h.applications(a, customer).len(), 1);
    assert_eq!(h.applications(a, waiting).len(), 1);
    assert!(h.row(a, ticket, customer).is_none());
}

/// A staff reply restarts `since_last_agent_reply`: the clock that was due
/// waits again from the new reply.
#[actix_rt::test]
async fn an_agent_reply_restarts_the_clock() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "VPN drops");
    let first = h.reply_at(a, ticket, a.admin_uuid, 3);
    let rule = h.time_rule(a, "since_last_agent_reply", 60, set_priority("high"));
    h.drain().await;
    let row = h.row(a, ticket, rule).expect("a clock");
    assert_eq!(row.3, first);
    assert!(row.4 <= Utc::now(), "due");

    h.reply(a, ticket, a.admin_uuid);
    h.drain().await;
    let row = h.row(a, ticket, rule).expect("the clock again");
    assert!(row.3 > first, "restarted from the new reply");
    assert!(row.4 > Utc::now(), "not due for an hour");
    assert_eq!(h.fire(), 0);
    assert!(h.applications(a, rule).is_empty());
    assert_eq!(h.priority(a, ticket), TicketPriority::Medium);
}

/// A closed ticket waits on nothing: closing it removes its rows, and
/// opening it again brings them back.
#[actix_rt::test]
async fn closing_a_ticket_removes_its_rows() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Monitor flickers");
    let rule = h.time_rule(a, "since_created", 600, set_priority("high"));
    h.drain().await;
    assert!(h.row(a, ticket, rule).is_some());

    let done = h.state_in(a, WorkflowStateCategory::Done);
    h.update(
        a,
        ticket,
        TicketUpdate {
            workflow_state_id: Some(done),
            ..Default::default()
        },
    );
    h.drain().await;
    assert!(h.row(a, ticket, rule).is_none(), "closed");

    let open = h.state_in(a, WorkflowStateCategory::Active);
    h.update(
        a,
        ticket,
        TicketUpdate {
            workflow_state_id: Some(open),
            ..Default::default()
        },
    );
    h.drain().await;
    assert!(h.row(a, ticket, rule).is_some(), "reopened");
}

/// Editing a rule while tickets wait on it recomputes when it is due: a
/// shorter wait brings it due at once, and it fires once.
#[actix_rt::test]
async fn a_rule_edited_mid_wait_recomputes() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Laptop slow");
    h.backdate_ticket(ticket, 2);
    let rule = h.time_rule(a, "since_created", 600, set_priority("high"));
    let row = h.row(a, ticket, rule).expect("a clock");
    assert!(row.4 > Utc::now());

    h.as_admin(a, |c| {
        rules::update(
            c,
            rule,
            RuleUpdate {
                trigger_config: Some(json!({
                    "clock_name": "since_created", "minutes": 60, "repeat_every_minutes": null
                })),
                ..Default::default()
            },
        )
        .map_err(|_| diesel::result::Error::NotFound)
    });
    h.requeue(a, rule);
    let edited = h.row(a, ticket, rule).expect("the clock");
    assert_eq!(edited.3, row.3, "the same start");
    assert_eq!(edited.4, row.3 + chrono::Duration::minutes(60));
    assert_eq!(h.fire(), 1);
    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);

    // Moved to another clock: the old row goes, and the clock it waits on
    // now has its own.
    let other = h.ticket(a, "Keyboard sticky");
    h.requeue(a, rule);
    assert_eq!(h.row(a, other, rule).expect("a clock").2, "since_created");
    h.as_admin(a, |c| {
        rules::update(
            c,
            rule,
            RuleUpdate {
                trigger_config: Some(
                    json!({ "clock_name": "since_status_change", "minutes": 600 }),
                ),
                ..Default::default()
            },
        )
        .map_err(|_| diesel::result::Error::NotFound)
    });
    h.requeue(a, rule);
    let rows: Vec<Row> = h.rows(a).into_iter().filter(|r| r.1 == rule).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].0, rows[0].2.as_str()),
        (other, "since_status_change")
    );

    // Paused: it waits on nothing.
    h.as_admin(a, |c| {
        rules::update(
            c,
            rule,
            RuleUpdate {
                state: Some(RuleState::Paused),
                ..Default::default()
            },
        )
        .map_err(|_| diesel::result::Error::NotFound)
    });
    h.requeue(a, rule);
    assert!(h.rows(a).iter().all(|r| r.1 != rule));
}

/// Two fires of one due clock at once, as when a lapsed claim is taken
/// again while the first fire still runs, run the rule once: the second
/// reads the clock again under the ticket's lock and finds it fired.
#[actix_rt::test]
async fn two_concurrent_fires_fire_once() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Phone won't ring");
    h.backdate_ticket(ticket, 2);
    let rule = h.time_rule(a, "since_created", 60, set_priority("high"));
    let claimed = h.bypass(|c| queue::claim_due(c, 10, 10, 300.0));
    assert_eq!(claimed.len(), 1);
    let row = claimed[0].clone();

    let lock = h.hold_ticket_lock(a, ticket);
    let fires: Vec<_> = (0..2)
        .map(|_| {
            let pool = h.pool.clone();
            let row = row.clone();
            std::thread::spawn(move || elapsed::fire_claimed(&pool, None, &row))
        })
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    drop(lock);
    let mut outcomes: Vec<Fired> = fires
        .into_iter()
        .map(|f| f.join().expect("a fire"))
        .collect();
    outcomes.sort_by_key(|f| format!("{f:?}"));
    assert_eq!(outcomes, vec![Fired::NotDue, Fired::Ran]);
    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);
    assert_eq!(h.rule_row(a, rule).fire_count, 1);
    assert!(h.row(a, ticket, rule).is_none());
}

/// Old mail a newly connected mailbox brought in opens imported tickets:
/// their clocks never run, so no time rule fires on them.
#[actix_rt::test]
async fn tickets_from_a_mailbox_backlog_wait_on_nothing() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let backlog = h
        .as_admin(a, |c| {
            let open = workflow_states::default_state(c)?;
            tickets::create_ticket(
                c,
                NewTicket {
                    title: "Old mail".to_string(),
                    workflow_state_id: open.id,
                    requester_uuid: Some(a.member_uuid),
                    submitted_via: Some(EMAIL_BACKLOG.to_string()),
                    ..Default::default()
                },
            )
        })
        .id;
    let current = h.ticket(a, "New mail");
    h.backdate_ticket(backlog, 48);
    h.backdate_ticket(current, 48);
    h.reply_at(a, backlog, a.member_uuid, 47);

    let created = h.time_rule(a, "since_created", 60, set_priority("high"));
    let customer = h.time_rule(a, "since_last_customer_reply", 60, set_priority("high"));
    h.drain().await;
    assert!(h.rows(a).iter().all(|r| r.0 != backlog), "{:?}", h.rows(a));
    assert!(h.row(a, current, created).is_some());

    h.fire();
    assert_eq!(h.priority(a, backlog), TicketPriority::Medium);
    assert_eq!(h.priority(a, current), TicketPriority::High);
    assert!(h
        .applications(a, created)
        .iter()
        .chain(h.applications(a, customer).iter())
        .all(|app| app.ticket_id != backlog));
}

/// The auto-close warning doesn't reset its own clock: a reply a rule wrote
/// on its own is no staff reply, so `awaiting_requester` still counts from
/// the agent's.
#[actix_rt::test]
async fn a_rules_own_warning_does_not_restart_awaiting_requester() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Shared drive missing");
    h.backdate_ticket(ticket, 24 * 10);
    h.reply_at(a, ticket, a.member_uuid, 24 * 10);
    let answered = h.reply_at(a, ticket, a.admin_uuid, 24 * 5);

    let warn = h.time_rule(
        a,
        "awaiting_requester",
        3 * 24 * 60,
        json!([{ "kind": "reply", "config": {
            "body": "We'll close this soon unless we hear from you.",
            "visibility": "public",
        } }]),
    );
    let done = h.state_in(a, WorkflowStateCategory::Done);
    let close = h.time_rule(
        a,
        "awaiting_requester",
        30 * 24 * 60,
        json!([{ "kind": "set_status", "config": { "workflow_state_id": done } }]),
    );
    assert_eq!(h.fire(), 1);
    let fired = h.applications(a, warn);
    assert_eq!(fired.len(), 1);
    assert_eq!(
        fired[0].status,
        Status::Succeeded,
        "{:?}",
        fired[0].failure_reason
    );
    let comment_id = fired[0].actions_taken.as_ref().expect("steps")[0]["comment_id"]
        .as_i64()
        .expect("the warning");
    let metadata: Option<Value> = h.as_admin(a, |c| {
        comments::table
            .find(comment_id as i32)
            .select(comments::channel_metadata)
            .first(c)
    });
    assert_eq!(metadata.expect("metadata")["rule_id"], warn);

    // The warning's event reaches the consumer and the clocks are read
    // again: the close still counts from the agent's reply.
    h.drain().await;
    let row = h.row(a, ticket, close).expect("the close still waits");
    assert_eq!(row.3, answered);
    assert_eq!(row.4, answered + chrono::Duration::days(30));
    assert!(h.row(a, ticket, warn).is_none(), "warned once");

    // A requester's answer stops both clocks.
    h.reply(a, ticket, a.member_uuid);
    h.drain().await;
    assert!(h.row(a, ticket, close).is_none());

    // A condition the ticket doesn't meet keeps a due clock out of the queue.
    let unmet = h.rule(
        a,
        Spec {
            trigger_config: json!({ "clock_name": "since_created", "minutes": 60 }),
            conditions: leaf("ticket.priority", "eq", json!("urgent")),
            ..Spec::live(Trigger::TimeElapsed, set_priority("low"))
        },
    );
    h.requeue(a, unmet);
    assert!(h.row(a, ticket, unmet).is_none());
}

/// A due date is a calendar day: `until_due_at` counts down to the end of
/// that day in the workspace's timezone, not to midnight UTC.
#[actix_rt::test]
async fn a_due_day_ends_at_midnight_where_the_workspace_is() {
    use chrono::TimeZone;
    let h = Harness::new();
    let a = &h.seeded.a;
    h.as_admin(a, |c| {
        use backend::schema::site_settings::dsl as ss;
        diesel::update(ss::site_settings)
            .set(ss::default_timezone.eq("Australia/Sydney"))
            .execute(c)
    });
    let ticket = h.ticket(a, "Renew the certificate");
    let day = Utc.with_ymd_and_hms(2036, 11, 21, 0, 0, 0).unwrap();
    h.update(
        a,
        ticket,
        TicketUpdate {
            due_date: Some(Some(day.naive_utc())),
            ..Default::default()
        },
    );
    let rule = h.time_rule(a, "until_due_at", 24 * 60, set_priority("high"));
    let row = h.row(a, ticket, rule).expect("a countdown");
    // Sydney is UTC+11 in November: the 21st ends at 13:00 UTC on the 21st.
    let end = Utc.with_ymd_and_hms(2036, 11, 21, 13, 0, 0).unwrap();
    assert_eq!((row.2.as_str(), row.3), ("until_due_at", end));
    assert_eq!(row.4, end - chrono::Duration::days(1));
}

/// A dry-run time rule shadows a due clock once: one `dry_run` row, no
/// change to the ticket. Put live, it runs for real on the clock it
/// shadowed.
#[actix_rt::test]
async fn a_dry_run_rule_shadows_once_then_fires_when_live() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Badge reader offline");
    h.backdate_ticket(ticket, 2);
    let rule = h.rule(
        a,
        Spec {
            state: RuleState::DryRun,
            trigger_config: json!({ "clock_name": "since_created", "minutes": 60 }),
            ..Spec::live(Trigger::TimeElapsed, set_priority("high"))
        },
    );
    h.requeue(a, rule);
    assert_eq!(h.fire(), 1);
    assert_eq!(h.statuses(a, rule), vec![Status::DryRun]);
    assert_eq!(h.priority(a, ticket), TicketPriority::Medium);
    h.requeue(a, rule);
    assert_eq!(h.fire(), 0, "shadowed once");

    h.as_admin(a, |c| {
        rules::update(
            c,
            rule,
            RuleUpdate {
                state: Some(RuleState::Live),
                ..Default::default()
            },
        )
        .map_err(|_| diesel::result::Error::NotFound)
    });
    h.requeue(a, rule);
    assert_eq!(h.fire(), 1);
    assert_eq!(h.statuses(a, rule), vec![Status::DryRun, Status::Succeeded]);
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
}

/// Through the API: a time-elapsed rule's settings are checked and saved in
/// one shape, going live queues the open tickets, and pausing clears them.
#[actix_rt::test]
async fn the_api_checks_settings_and_starts_and_stops_the_clocks() {
    use crate::rules_in_workspace::as_admin;
    use actix_web::http::StatusCode;
    use actix_web::test as http_test;

    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.ticket(a, "Projector bulb");
    let create = |config: Value| {
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Escalate after a day",
                "trigger_kind": "time_elapsed",
                "trigger_config": config,
                "actions": set_priority("high"),
            }))
    };

    for bad in [
        json!({}),
        json!({ "clock_name": "until_sla_due_at", "minutes": 60 }),
        json!({ "clock_name": "since_created", "minutes": 0 }),
        json!({ "clock_name": "since_created", "minutes": 60, "repeat_every_minutes": 5 }),
    ] {
        let resp = as_admin(&h.pool, a, create(bad.clone())).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad}");
        let body: Value = http_test::read_body_json(resp).await;
        assert_eq!(body["code"], "RULE_VALIDATION", "{body}");
    }

    let resp = as_admin(
        &h.pool,
        a,
        create(json!({ "clock_name": "since_created", "minutes": 1440, "extra": true })),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(
        body["trigger_config"],
        json!({ "clock_name": "since_created", "minutes": 1440, "repeat_every_minutes": null })
    );
    let rule = body["id"].as_i64().expect("rule id") as i32;
    assert!(h.row(a, ticket, rule).is_none(), "a draft waits on nothing");

    let set_state = |state: &str| {
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{rule}/state"))
            .set_json(json!({ "state": state }))
    };
    let resp = as_admin(&h.pool, a, set_state("live")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(h.row(a, ticket, rule).is_some(), "live: queued");

    let resp = as_admin(
        &h.pool,
        a,
        http_test::TestRequest::put()
            .uri(&format!("/api/rules/{rule}"))
            .set_json(
                json!({ "trigger_config": { "clock_name": "since_created", "minutes": 60 } }),
            ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let row = h.row(a, ticket, rule).expect("still queued");
    assert_eq!(row.4, row.3 + chrono::Duration::minutes(60), "recomputed");

    let resp = as_admin(&h.pool, a, set_state("paused")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(h.row(a, ticket, rule).is_none(), "paused: cleared");
}
