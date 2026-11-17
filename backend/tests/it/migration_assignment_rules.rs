//! Assignment rules become rules. Runs the migration set against a database
//! seeded just before `assignment_rules_become_rules` with each shape 1.1 could
//! leave: a rule on create only, on a category change only and on both, each
//! method, each filter, an inactive rule, and rules the old engine never ran
//! (a deleted person, no trigger, a filter no ticket meets). Each becomes the
//! event rules the plan maps it to, valid as the rules API would save them,
//! and the assignment log becomes their applications.

#![allow(clippy::expect_used)]

use std::collections::HashMap;

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable, Text};
use diesel_migrations::MigrationHarness;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::MIGRATIONS;
use backend::models::{Rule, RuleApplication, RuleApplicationStatus, RuleState, RuleTriggerKind};
use backend::repository::rules::{derive_reads_set, derive_writes_set};
use backend::schema::{rule_applications, rules};
use backend::services::rules::conditions;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` the way the rows would already exist: the table's user triggers
/// off. Returns the id it hands back, if any.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) -> Option<i32> {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    let id = if sql.contains("RETURNING id") {
        Some(
            diesel::sql_query(sql)
                .get_result::<Id>(conn)
                .unwrap_or_else(|e| panic!("seed {table}: {e}"))
                .id,
        )
    } else {
        diesel::sql_query(sql)
            .execute(conn)
            .unwrap_or_else(|e| panic!("seed {table}: {e}"));
        None
    };
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
    id
}

struct Seeded {
    ws: i32,
    other_ws: i32,
    agent: Uuid,
    team: i32,
    hardware: i32,
    ticket: i32,
}

/// An assignment rule as 1.1 stored it.
#[derive(Clone)]
struct Legacy<'a> {
    ws: i32,
    name: &'a str,
    priority: i32,
    active: bool,
    method: &'a str,
    user: Option<Uuid>,
    group: Option<i32>,
    on_create: bool,
    on_category: bool,
    category: Option<i32>,
    conditions: Option<Value>,
}

fn legacy_rule(conn: &mut PgConnection, agent: Uuid, r: Legacy) -> i32 {
    let sql_opt = |v: Option<String>| v.unwrap_or_else(|| "NULL".to_string());
    existing(
        conn,
        "assignment_rules",
        &format!(
            "INSERT INTO assignment_rules (workspace_id, name, description, priority, is_active, \
               method, target_user_uuid, target_group_id, trigger_on_create, \
               trigger_on_category_change, category_id, conditions, created_by, created_at, \
               updated_at) \
             VALUES ({}, '{}', 'Routes {}', {}, {}, '{}', {}, {}, {}, {}, {}, {}, '{agent}', \
               '2026-09-01T00:00:00Z', '2026-09-02T00:00:00Z') RETURNING id",
            r.ws,
            r.name,
            r.name,
            r.priority,
            r.active,
            r.method,
            sql_opt(r.user.map(|u| format!("'{u}'"))),
            sql_opt(r.group.map(|g| g.to_string())),
            r.on_create,
            r.on_category,
            sql_opt(r.category.map(|c| c.to_string())),
            sql_opt(r.conditions.map(|c| format!("'{c}'::jsonb"))),
        ),
    )
    .expect("assignment rule")
}

fn log(
    conn: &mut PgConnection,
    s: &Seeded,
    rule: Option<i32>,
    trigger: &str,
    method: &str,
    assignee: Option<Uuid>,
    at: &str,
) {
    existing(
        conn,
        "assignment_log",
        &format!(
            "INSERT INTO assignment_log (workspace_id, ticket_id, rule_id, trigger_type, \
               new_assignee_uuid, method, context, assigned_at) \
             VALUES ({}, {}, {}, '{trigger}', {}, '{method}', '{{\"rule_name\": \"x\"}}', '{at}')",
            s.ws,
            s.ticket,
            rule.map_or("NULL".to_string(), |r| r.to_string()),
            assignee.map_or("NULL".to_string(), |u| format!("'{u}'")),
        ),
    );
}

/// The legacy rows, by the name each rule was given.
fn seed(conn: &mut PgConnection) -> (Seeded, HashMap<&'static str, i32>) {
    let workspace = |conn: &mut PgConnection, slug: &str| {
        existing(
            conn,
            "workspaces",
            &format!(
                "INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"
            ),
        )
        .expect("workspace")
    };
    let ws = workspace(conn, "assignment-cutover");
    let other_ws = workspace(conn, "assignment-cutover-b");
    let agent = Uuid::new_v4();
    existing(
        conn,
        "users",
        &format!("INSERT INTO users (uuid, name) VALUES ('{agent}', 'Agent')"),
    );
    let team = existing(
        conn,
        "groups",
        &format!("INSERT INTO groups (workspace_id, name) VALUES ({ws}, 'Desk') RETURNING id"),
    )
    .expect("group");
    let hardware = existing(
        conn,
        "ticket_categories",
        &format!(
            "INSERT INTO ticket_categories (workspace_id, name) VALUES ({ws}, 'Hardware') RETURNING id"
        ),
    )
    .expect("category");
    let state = existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({ws}, 'Open', 'backlog', 'gray', 0, true) RETURNING id"
        ),
    )
    .expect("state");
    let ticket = existing(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, number) \
             VALUES ({ws}, 'Printer jam', {state}, 1) RETURNING id"
        ),
    )
    .expect("ticket");
    let seeded = Seeded {
        ws,
        other_ws,
        agent,
        team,
        hardware,
        ticket,
    };

    let base = Legacy {
        ws,
        name: "",
        priority: 0,
        active: true,
        method: "direct_user",
        user: Some(agent),
        group: None,
        on_create: true,
        on_category: false,
        category: None,
        conditions: None,
    };
    let mut ids = HashMap::new();
    // Made before the rule that runs ahead of it: order follows priority.
    for (name, rule) in [
        (
            "Category only",
            Legacy {
                name: "Category only",
                priority: 2,
                method: "group_round_robin",
                user: None,
                group: Some(team),
                on_create: false,
                on_category: true,
                category: Some(hardware),
                conditions: Some(json!({})),
                ..base.clone()
            },
        ),
        (
            "Create only",
            Legacy {
                name: "Create only",
                priority: 1,
                conditions: Some(json!({ "priority": " Normal " })),
                ..base.clone()
            },
        ),
        (
            "Both",
            Legacy {
                name: "Both",
                priority: 3,
                method: "group_random",
                user: None,
                group: Some(team),
                on_category: true,
                conditions: Some(json!({
                    "categories": ["backlog", "active", "bogus", 3, "active"],
                    "title_contains": "Printer",
                })),
                ..base.clone()
            },
        ),
        (
            "Queue",
            Legacy {
                name: "Queue",
                priority: 4,
                method: "group_queue",
                user: None,
                group: Some(team),
                ..base.clone()
            },
        ),
        (
            "Inactive",
            Legacy {
                name: "Inactive",
                priority: 5,
                active: false,
                ..base.clone()
            },
        ),
        (
            "Gone",
            Legacy {
                name: "Gone",
                priority: 6,
                user: None,
                ..base.clone()
            },
        ),
        (
            "Never",
            Legacy {
                name: "Never",
                priority: 7,
                on_create: false,
                ..base.clone()
            },
        ),
        (
            "Asap",
            Legacy {
                name: "Asap",
                priority: 8,
                conditions: Some(json!({ "priority": "asap" })),
                ..base.clone()
            },
        ),
        (
            "No statuses",
            Legacy {
                name: "No statuses",
                priority: 9,
                conditions: Some(json!({ "categories": [] })),
                ..base.clone()
            },
        ),
        (
            "Empty title",
            Legacy {
                name: "Empty title",
                priority: 10,
                conditions: Some(json!({ "title_contains": "", "priority": 5 })),
                ..base.clone()
            },
        ),
        (
            "Other workspace",
            Legacy {
                ws: other_ws,
                name: "Other workspace",
                priority: 50,
                ..base.clone()
            },
        ),
    ] {
        ids.insert(name, legacy_rule(conn, agent, rule));
    }

    let s = &seeded;
    log(
        conn,
        s,
        Some(ids["Create only"]),
        "ticket_created",
        "direct_user",
        Some(agent),
        "2026-10-01T10:00:00Z",
    );
    log(
        conn,
        s,
        Some(ids["Both"]),
        "category_changed",
        "group_random",
        Some(agent),
        "2026-10-02T10:00:00Z",
    );
    log(
        conn,
        s,
        Some(ids["Both"]),
        "manual",
        "group_random",
        Some(agent),
        "2026-10-03T10:00:00Z",
    );
    log(
        conn,
        s,
        Some(ids["Queue"]),
        "ticket_created",
        "group_queue",
        None,
        "2026-10-04T10:00:00Z",
    );
    // Its rule was deleted: nothing to belong to.
    log(
        conn,
        s,
        None,
        "ticket_created",
        "direct_user",
        Some(agent),
        "2026-10-05T10:00:00Z",
    );
    // A created run of a rule left with only its category change rule.
    log(
        conn,
        s,
        Some(ids["Category only"]),
        "ticket_created",
        "group_round_robin",
        Some(agent),
        "2026-10-06T10:00:00Z",
    );
    (seeded, ids)
}

/// Migrate a fresh database, seeding just before the migration.
fn upgrade() -> (FreshDb, PgConnection, Seeded) {
    let db = FreshDb::new();
    let mut conn = PgConnection::establish(&db.url).expect("connect fresh db");
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&mut conn)
    .expect("create migrations table");

    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_assignment_rules_become_rules") {
            seeded = Some(seed(&mut conn).0);
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let seeded = seeded.expect("the migration ran");
    (db, conn, seeded)
}

fn rules_by_name(conn: &mut PgConnection) -> HashMap<String, Rule> {
    rules::table
        .load::<Rule>(conn)
        .expect("rules")
        .into_iter()
        .map(|r| (r.name.clone(), r))
        .collect()
}

fn unassigned() -> Value {
    json!({ "kind": "leaf", "field": "ticket.assignee_uuid", "op": "is_null" })
}

fn and(extra: &[Value]) -> Value {
    let mut children = vec![unassigned()];
    children.extend_from_slice(extra);
    json!({ "kind": "and", "children": children })
}

fn steps(assign: Value) -> Value {
    json!([{ "kind": "assign", "config": assign }, { "kind": "stop_processing" }])
}

/// What one migrated rule should be.
struct Expected {
    trigger: RuleTriggerKind,
    trigger_config: Value,
    conditions: Value,
    actions: Value,
    state: RuleState,
    priority: i32,
    /// Text the description carries besides the rule's own.
    notes: &'static [&'static str],
}

#[test]
fn each_assignment_rule_becomes_the_rules_its_triggers_map_to() {
    let (_db, mut conn, s) = upgrade();
    let by_name = rules_by_name(&mut conn);
    let direct = |user: Uuid| steps(json!({ "method": "direct", "user_uuid": user }));
    let team = steps(json!({ "method": "group", "group_id": s.team }));
    let on_category = json!({ "changed_fields_include": ["ticket.category_id"] });
    let both_conditions = and(&[
        json!({ "kind": "leaf", "field": "ticket.workflow_state.category", "op": "in",
                "value": ["active", "backlog"] }),
        json!({ "kind": "leaf", "field": "ticket.title", "op": "contains", "value": "Printer" }),
    ]);
    const DIRECT: &str = "It assigns one person.";
    const FEWEST: &str = "It now assigns whoever on the team has the fewest open tickets.";
    let expected: Vec<(&str, Expected)> = vec![
        (
            "Create only",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[json!({ "kind": "leaf", "field": "ticket.priority",
                                         "op": "eq", "value": "medium" })]),
                actions: direct(s.agent),
                state: RuleState::Live,
                priority: 1010,
                notes: &[DIRECT],
            },
        ),
        (
            "Category only",
            Expected {
                trigger: RuleTriggerKind::TicketUpdated,
                trigger_config: on_category.clone(),
                conditions: and(&[json!({ "kind": "leaf", "field": "ticket.category_id",
                                         "op": "eq", "value": s.hardware })]),
                actions: team.clone(),
                state: RuleState::Live,
                priority: 1020,
                notes: &["took turns around the team (round robin)", FEWEST],
            },
        ),
        (
            "Both",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: both_conditions.clone(),
                actions: team.clone(),
                state: RuleState::Live,
                priority: 1030,
                notes: &["picked someone on the team at random", FEWEST],
            },
        ),
        (
            "Both (category change)",
            Expected {
                trigger: RuleTriggerKind::TicketUpdated,
                trigger_config: on_category,
                conditions: both_conditions,
                actions: team.clone(),
                state: RuleState::Live,
                priority: 1030,
                notes: &["picked someone on the team at random", FEWEST],
            },
        ),
        (
            "Queue",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: team,
                state: RuleState::Live,
                priority: 1040,
                notes: &["left the ticket in the team's queue", FEWEST],
            },
        ),
        (
            "Inactive",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: direct(s.agent),
                state: RuleState::Archived,
                priority: 1050,
                notes: &[DIRECT],
            },
        ),
        (
            "Gone",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: steps(json!({ "method": "direct" })),
                state: RuleState::Archived,
                priority: 1060,
                notes: &["Archived because the person it assigned no longer exists."],
            },
        ),
        (
            "Never",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: direct(s.agent),
                state: RuleState::Archived,
                priority: 1070,
                notes: &["Archived because it ran neither when a ticket was created"],
            },
        ),
        (
            "Asap",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: direct(s.agent),
                state: RuleState::Archived,
                priority: 1080,
                notes: &["Archived because its priority filter (asap) names no priority"],
            },
        ),
        (
            "No statuses",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: direct(s.agent),
                state: RuleState::Archived,
                priority: 1090,
                notes: &["Archived because its status filter names no status category"],
            },
        ),
        (
            "Empty title",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: direct(s.agent),
                state: RuleState::Live,
                priority: 1100,
                notes: &[DIRECT],
            },
        ),
        (
            "Other workspace",
            Expected {
                trigger: RuleTriggerKind::TicketCreated,
                trigger_config: json!({}),
                conditions: and(&[]),
                actions: direct(s.agent),
                state: RuleState::Live,
                priority: 1010,
                notes: &[DIRECT],
            },
        ),
    ];
    assert_eq!(by_name.len(), expected.len(), "{:?}", by_name.keys());

    for (name, want) in expected {
        let rule = by_name
            .get(name)
            .unwrap_or_else(|| panic!("no rule named {name}"));
        let ws = if name == "Other workspace" {
            s.other_ws
        } else {
            s.ws
        };
        assert_eq!(rule.workspace_id, ws, "{name}");
        assert_eq!(rule.trigger_kind, want.trigger, "{name}");
        assert_eq!(rule.trigger_config, want.trigger_config, "{name}");
        assert_eq!(rule.conditions, want.conditions, "{name}");
        assert_eq!(rule.actions, want.actions, "{name}");
        assert_eq!(rule.state, want.state, "{name}");
        assert_eq!(
            rule.archived_at.is_some(),
            want.state == RuleState::Archived,
            "{name}"
        );
        assert_eq!(rule.priority, want.priority, "{name}");
        assert_eq!(rule.created_by, Some(s.agent), "{name}");
        let description = rule.description.as_deref().unwrap_or_default();
        let own = name.trim_end_matches(" (category change)");
        assert!(
            description.starts_with(&format!("Routes {own}\n\n")),
            "{name}: {description}"
        );
        assert!(
            description.contains("Moved from Assignment Rules in Nosdesk 1.2"),
            "{name}: {description}"
        );
        for note in want.notes {
            assert!(description.contains(note), "{name}: {description}");
        }

        // As the rules API would have saved it.
        conditions::compile(&rule.conditions, rule.trigger_kind)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(rule.reads_set, derive_reads_set(&rule.conditions), "{name}");
        assert_eq!(rule.writes_set, derive_writes_set(&rule.actions), "{name}");
    }
}

#[derive(QueryableByName)]
struct Version {
    #[diesel(sql_type = Integer)]
    rule_id: i32,
    #[diesel(sql_type = Integer)]
    version: i32,
    #[diesel(sql_type = Text)]
    state: String,
}

#[test]
fn each_rule_starts_at_version_one() {
    let (_db, mut conn, _) = upgrade();
    let by_name = rules_by_name(&mut conn);
    let versions: Vec<Version> = diesel::sql_query(
        "SELECT rule_id, version, state::text AS state FROM rule_versions ORDER BY rule_id",
    )
    .load(&mut conn)
    .expect("versions");
    assert_eq!(versions.len(), by_name.len(), "one version each");
    for rule in by_name.values() {
        let v = versions
            .iter()
            .find(|v| v.rule_id == rule.id)
            .expect("its version");
        assert_eq!(v.version, 1);
        assert_eq!(v.state, rule.state.as_str());
    }
}

#[test]
fn the_assignment_log_becomes_applications_of_the_rule_it_ran_as() {
    let (_db, mut conn, s) = upgrade();
    let by_name = rules_by_name(&mut conn);
    let applications: Vec<RuleApplication> = rule_applications::table
        .order(rule_applications::applied_at)
        .load(&mut conn)
        .expect("applications");
    let id = |name: &str| by_name[name].id;
    let assigned = json!([{ "index": 1, "kind": "assign", "assigned_to_uuid": s.agent }]);
    let expected = [
        (
            id("Create only"),
            Some("ticket.created"),
            "ticket_created",
            "direct_user",
            assigned.clone(),
        ),
        (
            id("Both (category change)"),
            Some("ticket.category_changed"),
            "ticket_updated",
            "group_random",
            assigned.clone(),
        ),
        (
            id("Both"),
            None,
            "ticket_created",
            "group_random",
            assigned.clone(),
        ),
        (
            id("Queue"),
            Some("ticket.created"),
            "ticket_created",
            "group_queue",
            json!([]),
        ),
        (
            id("Category only"),
            Some("ticket.created"),
            "ticket_updated",
            "group_round_robin",
            assigned,
        ),
    ];
    assert_eq!(
        applications.len(),
        expected.len(),
        "the row whose rule was deleted isn't copied"
    );
    for (a, (rule_id, event, trigger, method, actions)) in applications.iter().zip(expected) {
        assert_eq!(a.rule_id, rule_id);
        assert_eq!(a.workspace_id, s.ws);
        assert_eq!(a.ticket_id, s.ticket);
        assert_eq!(a.rule_version, 1);
        assert_eq!(a.status, RuleApplicationStatus::Succeeded);
        assert_eq!((a.actor_kind.as_str(), a.actor_uuid), ("system", None));
        assert_eq!(a.originating_event_kind.as_deref(), event);
        assert_eq!(
            a.condition_evaluation,
            Some(json!({ "matched": true, "trigger": trigger,
                         "migrated_from": "assignment_log", "method": method }))
        );
        assert_eq!(a.actions_taken, Some(actions));
    }

    // Each rule counts the runs it was given.
    for (name, fired, last) in [
        ("Create only", 1, Some("2026-10-01T10:00:00+00:00")),
        ("Both", 1, Some("2026-10-03T10:00:00+00:00")),
        (
            "Both (category change)",
            1,
            Some("2026-10-02T10:00:00+00:00"),
        ),
        ("Queue", 1, Some("2026-10-04T10:00:00+00:00")),
        ("Category only", 1, Some("2026-10-06T10:00:00+00:00")),
        ("Inactive", 0, None),
    ] {
        let rule = &by_name[name];
        assert_eq!(rule.fire_count, fired, "{name}");
        assert_eq!(
            rule.last_fired_at.map(|t| t.to_rfc3339()),
            last.map(str::to_string),
            "{name}"
        );
    }
}

#[derive(QueryableByName)]
struct Gone {
    #[diesel(sql_type = Nullable<Text>)]
    rules: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    state: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    log: Option<String>,
    #[diesel(sql_type = BigInt)]
    method: i64,
}

#[test]
fn the_assignment_tables_and_type_are_gone() {
    let (_db, mut conn, _) = upgrade();
    let gone: Gone = diesel::sql_query(
        "SELECT to_regclass('public.assignment_rules')::text AS rules, \
                to_regclass('public.assignment_rule_state')::text AS state, \
                to_regclass('public.assignment_log')::text AS log, \
                (SELECT count(*) FROM pg_type WHERE typname = 'assignment_method') AS method",
    )
    .get_result(&mut conn)
    .expect("catalog");
    assert_eq!(
        (gone.rules, gone.state, gone.log, gone.method),
        (None, None, None, 0)
    );
}
