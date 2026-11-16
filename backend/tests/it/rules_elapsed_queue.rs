//! The time-elapsed clock queue (`ticket_elapsed_queue`): one row per
//! (ticket, rule, clock), rows that go with their ticket, their rule and
//! their workspace, and row security that keeps each workspace's clocks to
//! itself when the app writes them as `nosdesk_app`.
#![allow(clippy::expect_used)]

use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error};

use backend::db::DbConnection;
use backend::models::{NewRule, NewTicket, RuleState, RuleTriggerKind};
use backend::repository::{rules, tickets, workflow_states, workspaces};
use backend::schema::ticket_elapsed_queue as q;
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const CLOCK: &str = "awaiting_requester";

fn actor(ws: &WorkspaceSeed) -> ActorContext {
    ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id)
}

/// Run `f` pinned to `ws` as the app role.
fn as_app<T>(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    f: impl FnOnce(&mut DbConnection) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &actor(ws), |c| {
        let role: String = diesel::select(diesel::dsl::sql::<diesel::sql_types::Text>(
            "current_user::text",
        ))
        .get_result(c)?;
        assert_eq!(role, "nosdesk_app");
        f(c)
    })
}

/// A live time-elapsed rule in `ws`, saved through the repository.
fn rule(pool: &TestPool, ws: &WorkspaceSeed, name: &str) -> i32 {
    as_app(pool, ws, |c| {
        rules::create(
            c,
            NewRule {
                workspace_id: ws.workspace_id,
                name: name.to_string(),
                description: None,
                trigger_kind: RuleTriggerKind::TimeElapsed,
                trigger_config: serde_json::json!({}),
                conditions: serde_json::json!([]),
                actions: serde_json::json!([
                    { "kind": "set_priority", "config": { "priority": "high" } }
                ]),
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

fn ticket(pool: &TestPool, ws: &WorkspaceSeed, title: &str) -> i32 {
    as_app(pool, ws, |c| {
        let open = workflow_states::default_state(c)?;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: open.id,
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)
    })
    .expect("open a ticket")
}

/// Start `clock` for `rule` on `ticket`, as `ws`'s app connection, with the
/// row naming `workspace` (the session's own unless a test says otherwise).
fn start(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    workspace: i32,
    (ticket, rule, clock): (i32, i32, &str),
) -> Result<usize, Error> {
    let started: DateTime<Utc> = Utc::now();
    as_app(pool, ws, |c| {
        diesel::insert_into(q::table)
            .values((
                q::workspace_id.eq(workspace),
                q::ticket_id.eq(ticket),
                q::rule_id.eq(rule),
                q::clock_name.eq(clock),
                q::clock_started_at.eq(started),
                q::due_at.eq(started + Duration::days(30)),
            ))
            .execute(c)
    })
}

/// The (ticket, rule, workspace) of every clock `ws` sees.
fn clocks(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<(i32, i32, i32)> {
    as_app(pool, ws, |c| {
        q::table
            .select((q::ticket_id, q::rule_id, q::workspace_id))
            .order((q::ticket_id, q::rule_id))
            .load(c)
    })
    .expect("read clocks as the app")
}

/// Every workspace's clocks, read with row security bypassed.
fn all_clocks(pool: &TestPool) -> Vec<(i32, i32, i32)> {
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(
        &mut conn,
        &ActorContext::system("test:ticket_elapsed_queue"),
        |c| {
            q::table
                .select((q::ticket_id, q::rule_id, q::workspace_id))
                .order((q::ticket_id, q::rule_id))
                .load(c)
        },
    )
    .expect("read every workspace's clocks")
}

#[test]
fn a_ticket_runs_each_rules_clock_once() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    let ws = a.workspace_id;
    let (t, r) = (ticket(&pool, a, "Printer offline"), rule(&pool, a, "Nudge"));

    assert_eq!(
        start(&pool, a, ws, (t, r, CLOCK)).expect("start a clock"),
        1
    );
    let again = start(&pool, a, ws, (t, r, CLOCK));
    assert!(
        matches!(
            again,
            Err(Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _))
        ),
        "the same clock twice: {again:?}"
    );

    // Another clock of the same rule, or the same clock of another rule, is
    // another row.
    assert_eq!(
        start(&pool, a, ws, (t, r, "since_created")).expect("start a clock"),
        1
    );
    let other = rule(&pool, a, "Close");
    assert_eq!(
        start(&pool, a, ws, (t, other, CLOCK)).expect("start a clock"),
        1
    );
    assert_eq!(clocks(&pool, a).len(), 3);
}

#[test]
fn deleting_the_ticket_or_the_rule_removes_its_clocks() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    let ws = a.workspace_id;
    let (t1, t2) = (ticket(&pool, a, "Printer offline"), ticket(&pool, a, "VPN"));
    let (r1, r2) = (rule(&pool, a, "Nudge"), rule(&pool, a, "Close"));
    for t in [t1, t2] {
        for r in [r1, r2] {
            start(&pool, a, ws, (t, r, CLOCK)).expect("start a clock");
        }
    }

    as_app(&pool, a, |c| tickets::delete_ticket_with_cleanup(c, t1))
        .expect("delete the ticket as the app");
    assert_eq!(clocks(&pool, a), vec![(t2, r1, ws), (t2, r2, ws)]);

    let mut conn = pool.get().expect("conn");
    with_actor_context(&mut conn, &actor(a), |c| {
        rules::archive(c, r1, Utc::now())?;
        rules::hard_delete(c, r1)
    })
    .expect("delete the rule as the app");
    assert_eq!(clocks(&pool, a), vec![(t2, r2, ws)]);
}

#[test]
fn each_workspace_sees_and_writes_only_its_own_clocks() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);
    let (ta, ra) = (ticket(&pool, a, "Printer offline"), rule(&pool, a, "Nudge"));
    let (tb, rb) = (ticket(&pool, b, "Printer offline"), rule(&pool, b, "Nudge"));
    start(&pool, a, a.workspace_id, (ta, ra, CLOCK)).expect("A's clock");
    start(&pool, b, b.workspace_id, (tb, rb, CLOCK)).expect("B's clock");

    assert_eq!(clocks(&pool, a), vec![(ta, ra, a.workspace_id)]);
    assert_eq!(clocks(&pool, b), vec![(tb, rb, b.workspace_id)]);

    // Pinned to A, a clock naming B is refused.
    let into_b = start(&pool, a, b.workspace_id, (tb, rb, "since_created"));
    assert!(
        matches!(
            &into_b,
            Err(Error::DatabaseError(_, info)) if info.message().contains("row-level security")
        ),
        "a clock for another workspace: {into_b:?}"
    );

    // Nor can A claim or clear B's.
    let touched = as_app(&pool, a, |c| {
        let claimed = diesel::update(q::table.filter(q::ticket_id.eq(tb)))
            .set(q::claimed_at.eq(Some(Utc::now())))
            .execute(c)?;
        let deleted = diesel::delete(q::table.filter(q::ticket_id.eq(tb))).execute(c)?;
        Ok(claimed + deleted)
    })
    .expect("write as A");
    assert_eq!(touched, 0);
    let b_claimed: Option<DateTime<Utc>> = as_app(&pool, b, |c| {
        q::table
            .filter(q::ticket_id.eq(tb))
            .select(q::claimed_at)
            .first(c)
    })
    .expect("B's clock is still there");
    assert_eq!(b_claimed, None);
}

#[test]
fn deleting_a_workspace_removes_its_clocks() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);
    let (ta, ra) = (ticket(&pool, a, "Printer offline"), rule(&pool, a, "Nudge"));
    let (tb, rb) = (ticket(&pool, b, "Printer offline"), rule(&pool, b, "Nudge"));
    start(&pool, a, a.workspace_id, (ta, ra, CLOCK)).expect("A's clock");
    start(&pool, b, b.workspace_id, (tb, rb, CLOCK)).expect("B's clock");

    let mut conn = pool.get().expect("conn");
    let deleted = with_actor_bypass_context(
        &mut conn,
        &ActorContext::system("test:ticket_elapsed_queue"),
        |c| {
            workspaces::archive_workspace(c, b.workspace_id)?;
            workspaces::hard_delete_workspace(c, b.workspace_id, Utc::now() + Duration::hours(1))
        },
    )
    .expect("hard-delete the workspace");
    assert_eq!(deleted, 1);

    assert_eq!(all_clocks(&pool), vec![(ta, ra, a.workspace_id)]);
}
