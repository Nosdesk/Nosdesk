//! Paused is its own rule state. Phase 1 stored a paused rule as `dry_run`;
//! the migration moves those rules, and their saved versions, to `paused`
//! without counting it as an edit, and `dry_run` is left to shadow runs of
//! rules with an automated trigger. A manual rule can't be put in dry run.

#![allow(clippy::expect_used)]

use actix_web::http::StatusCode;
use actix_web::test as http_test;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Timestamptz};
use diesel_migrations::MigrationHarness;
use serde_json::{json, Value};

use backend::db::MIGRATIONS;
use backend::models::{NewRule, RuleState, RuleTriggerKind};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;
use crate::migration_backfill_existing_workspace::FreshDb;
use crate::rules_in_workspace::as_admin;

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
    edited_at: DateTime<Utc>,
    paused_manual: i32,
    paused_event: i32,
    live: i32,
}

/// Rules as 1.1 left them: a manual and an event rule an admin paused
/// (`dry_run`), a live one, each with the versions its edits saved.
fn seed(conn: &mut PgConnection) -> Seeded {
    let ws = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('paused-rules', 'Paused Rules') RETURNING id",
    )
    .expect("workspace");
    let edited_at = Utc::now() - Duration::days(3);
    let at = format!(
        "'{}'::timestamptz",
        edited_at.to_rfc3339_opts(SecondsFormat::Micros, true)
    );
    let rule = |conn: &mut PgConnection, name: &str, kind: &str, states: &[&str]| {
        let state = states.last().expect("a state");
        let id = existing(
            conn,
            "rules",
            &format!(
                "INSERT INTO rules (workspace_id, name, trigger_kind, actions, state, updated_at) \
                 VALUES ({ws}, '{name}', '{kind}', \
                         '[{{\"kind\": \"set_priority\", \"config\": {{\"priority\": \"high\"}}}}]', \
                         '{state}', {at}) RETURNING id"
            ),
        )
        .expect("rule");
        for (i, state) in states.iter().enumerate() {
            existing(
                conn,
                "rule_versions",
                &format!(
                    "INSERT INTO rule_versions (rule_id, workspace_id, version, name, trigger_kind, \
                                                trigger_config, conditions, actions, state, priority) \
                     SELECT id, workspace_id, {}, name, trigger_kind, trigger_config, conditions, \
                            actions, '{state}', priority FROM rules WHERE id = {id}",
                    i + 1
                ),
            );
        }
        id
    };
    let paused_manual = rule(
        conn,
        "Paused manual",
        "manual",
        &["draft", "live", "dry_run"],
    );
    let paused_event = rule(
        conn,
        "Paused event",
        "ticket_created",
        &["draft", "dry_run"],
    );
    let live = rule(conn, "Live", "manual", &["draft", "live"]);
    Seeded {
        edited_at,
        paused_manual,
        paused_event,
        live,
    }
}

/// Migrate a fresh database, seeding just before the migration that moves
/// paused rules (after the one that adds the state).
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
        if name.contains("_rules_phase2_schema") {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let seeded = seeded.expect("the migration ran");
    (db, conn, seeded)
}

#[derive(QueryableByName)]
struct RuleRow {
    #[diesel(sql_type = Text)]
    state: String,
    #[diesel(sql_type = Timestamptz)]
    updated_at: DateTime<Utc>,
}

fn rule(conn: &mut PgConnection, id: i32) -> RuleRow {
    diesel::sql_query("SELECT state::text AS state, updated_at FROM rules WHERE id = $1")
        .bind::<Integer, _>(id)
        .get_result(conn)
        .expect("rule")
}

#[derive(QueryableByName)]
struct State {
    #[diesel(sql_type = Text)]
    state: String,
}

/// The rule's saved versions' states, oldest first.
fn versions(conn: &mut PgConnection, id: i32) -> Vec<String> {
    diesel::sql_query(
        "SELECT state::text AS state FROM rule_versions WHERE rule_id = $1 ORDER BY version",
    )
    .bind::<Integer, _>(id)
    .load::<State>(conn)
    .expect("versions")
    .into_iter()
    .map(|v| v.state)
    .collect()
}

#[test]
fn a_rule_paused_before_the_upgrade_is_paused_after_it() {
    let (_db, mut conn, s) = upgrade();
    let micros = |t: DateTime<Utc>| t.timestamp_micros();

    for id in [s.paused_manual, s.paused_event] {
        let r = rule(&mut conn, id);
        assert_eq!(r.state, "paused", "rule {id}");
        assert_eq!(
            micros(r.updated_at),
            micros(s.edited_at),
            "rule {id}: the move isn't an edit"
        );
    }
    assert_eq!(
        versions(&mut conn, s.paused_manual),
        ["draft", "live", "paused"]
    );
    assert_eq!(versions(&mut conn, s.paused_event), ["draft", "paused"]);

    let r = rule(&mut conn, s.live);
    assert_eq!(r.state, "live");
    assert_eq!(micros(r.updated_at), micros(s.edited_at));
    assert_eq!(versions(&mut conn, s.live), ["draft", "live"]);

    // The version trigger is back on: the next change is versioned.
    diesel::sql_query("UPDATE rules SET state = 'live' WHERE id = $1")
        .bind::<Integer, _>(s.paused_manual)
        .execute(&mut conn)
        .expect("resume the rule");
    assert_eq!(
        versions(&mut conn, s.paused_manual),
        ["draft", "live", "paused", "live"]
    );

    // A manual rule has no trigger to shadow.
    let refused = diesel::sql_query("UPDATE rules SET state = 'dry_run' WHERE id = $1")
        .bind::<Integer, _>(s.live)
        .execute(&mut conn);
    assert!(refused.is_err(), "a manual rule in dry run");
    diesel::sql_query("UPDATE rules SET state = 'dry_run' WHERE id = $1")
        .bind::<Integer, _>(s.paused_event)
        .execute(&mut conn)
        .expect("an event rule goes to dry run");
}

/// Pause and resume go through `paused`; a manual rule refuses `dry_run`
/// from any state, and a rule in dry run can't switch to the manual trigger.
#[actix_web::test]
async fn pausing_uses_paused_and_dry_run_is_for_automated_rules() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let pool = db.runtime_pool(4);

    let set_state = |id: i64, state: &str| {
        http_test::TestRequest::patch()
            .uri(&format!("/api/rules/{id}/state"))
            .set_json(json!({ "state": state }))
    };

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(json!({
                "name": "Escalate",
                "trigger_kind": "manual",
                "actions": [{ "kind": "set_priority", "config": { "priority": "high" } }],
            })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let manual = http_test::read_body_json::<Value, _>(created).await["id"]
        .as_i64()
        .expect("rule id");

    for (state, expected) in [
        ("dry_run", StatusCode::CONFLICT),
        ("live", StatusCode::OK),
        ("dry_run", StatusCode::CONFLICT),
        ("paused", StatusCode::OK),
        ("dry_run", StatusCode::CONFLICT),
        ("live", StatusCode::OK),
    ] {
        let resp = as_admin(&pool, a, set_state(manual, state)).await;
        assert_eq!(resp.status(), expected, "manual rule to {state}");
        let body: Value = http_test::read_body_json(resp).await;
        if expected == StatusCode::OK {
            assert_eq!(body["state"], state, "{body}");
        } else {
            assert_eq!(body["code"], "RULE_STATE_TRANSITION", "{body}");
        }
    }

    // An event rule, saved directly: shadow runs are its to use.
    let mut conn = db.pool_with_size(1).get().expect("conn");
    let event = with_actor_context(
        &mut conn,
        &ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id),
        |c| {
            backend::repository::rules::create(
                c,
                NewRule {
                    workspace_id: a.workspace_id,
                    name: "Tag new tickets".to_string(),
                    description: None,
                    trigger_kind: RuleTriggerKind::TicketCreated,
                    trigger_config: json!({}),
                    conditions: json!([]),
                    actions: json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
                    reads_set: Vec::new(),
                    writes_set: Vec::new(),
                    state: RuleState::Draft,
                    priority: 100,
                    created_by: Some(a.admin_uuid),
                },
            )
        },
    )
    .expect("save an event rule")
    .id as i64;

    let resp = as_admin(&pool, a, set_state(event, "dry_run")).await;
    assert_eq!(resp.status(), StatusCode::OK, "event rule to dry_run");
    let to_manual = || {
        http_test::TestRequest::put()
            .uri(&format!("/api/rules/{event}"))
            .set_json(json!({ "trigger_kind": "manual" }))
    };
    let refused = as_admin(&pool, a, to_manual()).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let body: Value = http_test::read_body_json(refused).await;
    assert_eq!(body["code"], "RULE_STATE_TRANSITION", "{body}");

    let resp = as_admin(&pool, a, set_state(event, "paused")).await;
    assert_eq!(resp.status(), StatusCode::OK, "dry run to paused");
    let resp = as_admin(&pool, a, to_manual()).await;
    assert_eq!(resp.status(), StatusCode::OK, "a paused rule can switch");
}
