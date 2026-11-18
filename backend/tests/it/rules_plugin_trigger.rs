//! Rules run on a plugin's events: a `plugin_event` rule names a plugin and
//! one of the events its manifest declares (`<plugin name>:<event>`), and
//! runs on the ticket the event names, but only when a staff member emitted
//! it and the plugin may read the ticket (it is active and declares the
//! event, it was granted `ticket:read`, and that member can see the
//! ticket). Drives the dispatcher directly, like `rules_event_triggers`.
#![allow(clippy::expect_used)]

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
    Claims, NewTicket, PluginState, RuleApplicationStatus as Status, RuleState,
    RuleTriggerKind as Trigger, SyncAggregate, SyncOp, TicketPriority,
};
use backend::repository::{tickets, workflow_states};
use backend::schema::{plugins, rule_event_outbox, sync_actions};
use backend::services::plugins::access::event_emitter_ref;
use backend::sync::actor::{ActorContext, ActorKind};
use backend::sync::emit::{self, SyncEmit};
use backend::sync::groups;
use backend::sync::session::with_actor_context;

use crate::common::WorkspaceSeed;
use crate::rules_event_triggers::{Harness, Spec};

/// One of `ws`'s plugin's two events.
fn failed(ws: &WorkspaceSeed) -> String {
    format!("{}:build_failed", ws.plugin_name)
}

fn fixed(ws: &WorkspaceSeed) -> String {
    format!("{}:build_fixed", ws.plugin_name)
}

fn high() -> Value {
    json!([{ "kind": "set_priority", "config": { "priority": "high" } }])
}

/// A live `plugin_event` rule on `plugin`'s `event` that raises priority.
fn on(plugin: Uuid, event: &str) -> Spec {
    Spec {
        trigger_config: json!({ "plugin_uuid": plugin.to_string(), "event_type": event }),
        ..Spec::live(Trigger::PluginEvent, high())
    }
}

/// The workspace's plugin declares both its events, asks for `ticket:read`, and
/// was granted `granted`.
fn set_up_plugin(h: &Harness, ws: &WorkspaceSeed, granted: &[&str]) {
    h.as_admin(ws, |c| {
        let manifest: Value = plugins::table
            .filter(plugins::uuid.eq(ws.plugin_uuid))
            .select(plugins::manifest)
            .first(c)?;
        let mut manifest = manifest;
        manifest["events"] = json!([failed(ws), fixed(ws)]);
        manifest["permissions"] = json!(["ticket:read"]);
        diesel::update(plugins::table.filter(plugins::uuid.eq(ws.plugin_uuid)))
            .set((
                plugins::manifest.eq(manifest),
                plugins::consented_permissions.eq(Some(json!(granted))),
            ))
            .execute(c)
    });
}

fn set_state(h: &Harness, ws: &WorkspaceSeed, state: PluginState) {
    h.as_admin(ws, |c| {
        diesel::update(plugins::table.filter(plugins::uuid.eq(ws.plugin_uuid)))
            .set(plugins::state.eq(state))
            .execute(c)
    });
}

/// A ticket `requester` raised in `ws`.
fn ticket_for(h: &Harness, ws: &WorkspaceSeed, requester: Uuid) -> i32 {
    h.as_admin(ws, |c| {
        let open = workflow_states::default_state(c)?;
        tickets::create_ticket(
            c,
            NewTicket {
                title: "Build failing".to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(requester),
                ..Default::default()
            },
        )
    })
    .id
}

/// Record `ws`'s plugin emitting `event` for `emitter`, as the emit
/// endpoint records it.
fn emitted(h: &Harness, ws: &WorkspaceSeed, emitter: Uuid, event: &str, data: Value) {
    let actor = ActorContext {
        kind: ActorKind::Plugin,
        uuid: Some(ws.plugin_uuid),
        reference: Some(event_emitter_ref(&ws.plugin_name, &emitter.to_string())),
        correlation_id: None,
        client_tx_id: None,
        workspace_id: Some(ws.workspace_id),
    };
    let mut conn = h.pool.get().expect("conn");
    with_actor_context(&mut conn, &actor, |c| {
        emit::record(
            c,
            SyncEmit {
                aggregate: SyncAggregate::Plugin,
                aggregate_id: ws.plugin_uuid.to_string(),
                op: SyncOp::Update,
                event_type: event,
                data: json!({ "event": data }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )
    })
    .expect("record the plugin's event");
}

fn queued(h: &Harness) -> i64 {
    h.bypass(|c| rule_event_outbox::table.count().get_result(c))
}

/// Nothing ran and nothing was counted.
fn untouched(h: &Harness, ws: &WorkspaceSeed, rule: i32) {
    assert!(h.applications(ws, rule).is_empty());
    let row = h.rule_row(ws, rule);
    assert_eq!(
        (
            row.fire_count,
            row.skipped_preflight_count,
            row.skipped_condition_unmet_count
        ),
        (0, 0, 0)
    );
}

/// Emit `event` through the emit endpoint, as `user` running `ws`'s
/// plugin.
async fn emit_as(
    h: &Harness,
    ws: &WorkspaceSeed,
    user: Uuid,
    event: &str,
    data: Value,
) -> StatusCode {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Someone".to_string(),
        email: "someone@example.com".to_string(),
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
    let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(h.pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/api/plugins/{uuid}/events",
                web::post().to(backend::handlers::plugin_events::emit_plugin_event),
            ),
    )
    .await;
    http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/plugins/{}/events", ws.plugin_uuid))
            .set_json(json!({ "aggregate": "plugin", "event_type": event, "data": data }))
            .to_request(),
    )
    .await
    .status()
}

/// An agent's plugin emits an event naming a ticket through the emit
/// endpoint (what `api.emit` calls): the rule on that plugin and event runs
/// on the ticket, and its row names who the event was emitted for. A rule on
/// another of the plugin's events, or on another plugin, doesn't run.
#[actix_rt::test]
async fn a_plugin_event_naming_a_ticket_runs_its_rules() {
    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);
    set_up_plugin(&h, a, &["ticket:read"]);
    let fires = h.rule(a, on(a.plugin_uuid, &failed(a)));
    let other_event = h.rule(a, on(a.plugin_uuid, &fixed(a)));
    let other_plugin = h.rule(a, on(b.plugin_uuid, &failed(b)));
    let ticket = ticket_for(&h, a, a.member_uuid);
    // Only plugin event rules here, so the ticket's own events aren't queued.
    assert_eq!(queued(&h), 0);

    let data = json!({ "ticket_id": ticket, "repository": "acme/api" });
    assert_eq!(
        emit_as(&h, a, a.admin_uuid, &failed(a), data).await,
        StatusCode::CREATED
    );
    assert_eq!(queued(&h), 1);
    h.drain().await;

    let fired = h.applications(a, fires);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].status, Status::Succeeded);
    assert_eq!(fired[0].ticket_id, ticket);
    assert_eq!(fired[0].actor_kind, "system");
    assert_eq!(
        fired[0].originating_event_kind.as_deref(),
        Some(failed(a).as_str())
    );
    let evaluation = fired[0].condition_evaluation.as_ref().expect("evaluation");
    assert_eq!(evaluation["emitted_by"], json!(a.admin_uuid));
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
    untouched(&h, a, other_event);
    untouched(&h, a, other_plugin);
}

/// Only staff fire rules: a requester's plugin event is recorded, but runs
/// nothing and leaves no rule row.
#[actix_rt::test]
async fn a_requesters_plugin_event_runs_no_rules() {
    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let a = &h.seeded.a;
    set_up_plugin(&h, a, &["ticket:read"]);
    let rule = h.rule(a, on(a.plugin_uuid, &failed(a)));
    // The requester's own ticket, which they can see.
    let ticket = ticket_for(&h, a, a.member_uuid);

    let data = json!({ "ticket_id": ticket });
    assert_eq!(
        emit_as(&h, a, a.member_uuid, &failed(a), data).await,
        StatusCode::CREATED
    );
    let recorded: i64 = h.bypass(|c| {
        sync_actions::table
            .filter(sync_actions::event_type.eq(failed(a)))
            .count()
            .get_result(c)
    });
    assert_eq!(recorded, 1, "the event is still recorded");
    h.drain().await;
    untouched(&h, a, rule);
    assert_ne!(h.priority(a, ticket), TicketPriority::High);
}

/// An event no rule names isn't queued at all.
#[actix_rt::test]
async fn an_event_no_rule_names_is_not_queued() {
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);
    set_up_plugin(&h, a, &["ticket:read"]);
    let other_event = h.rule(a, on(a.plugin_uuid, &fixed(a)));
    let other_plugin = h.rule(a, on(b.plugin_uuid, &failed(b)));
    let paused = h.rule(
        a,
        Spec {
            state: RuleState::Paused,
            ..on(a.plugin_uuid, &failed(a))
        },
    );
    let ticket = ticket_for(&h, a, a.member_uuid);

    emitted(
        &h,
        a,
        a.admin_uuid,
        &failed(a),
        json!({ "ticket_id": ticket }),
    );
    // Naming no ticket, or not by number.
    emitted(&h, a, a.admin_uuid, &fixed(a), json!({ "issue": 12 }));
    emitted(
        &h,
        a,
        a.admin_uuid,
        &fixed(a),
        json!({ "ticket_id": ticket.to_string() }),
    );
    assert_eq!(queued(&h), 0);
    h.drain().await;
    for rule in [other_event, other_plugin, paused] {
        untouched(&h, a, rule);
    }
    assert_ne!(h.priority(a, ticket), TicketPriority::High);
}

/// A plugin runs no rule on a ticket it may not read: one it wasn't granted
/// `ticket:read` for.
#[actix_rt::test]
async fn a_ticket_the_plugin_cannot_read_runs_nothing() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, on(a.plugin_uuid, &failed(a)));
    let ticket = ticket_for(&h, a, a.member_uuid);

    // Asked for ticket:read, but the admin granted nothing.
    set_up_plugin(&h, a, &[]);
    emitted(
        &h,
        a,
        a.admin_uuid,
        &failed(a),
        json!({ "ticket_id": ticket }),
    );
    assert_eq!(queued(&h), 1);
    h.drain().await;
    untouched(&h, a, rule);
    assert_ne!(h.priority(a, ticket), TicketPriority::High);

    // Granted.
    set_up_plugin(&h, a, &["ticket:read"]);
    emitted(
        &h,
        a,
        a.admin_uuid,
        &failed(a),
        json!({ "ticket_id": ticket }),
    );
    h.drain().await;
    assert_eq!(h.statuses(a, rule), vec![Status::Succeeded]);
    assert_eq!(h.priority(a, ticket), TicketPriority::High);
}

/// A plugin's event runs only its own workspace's rules, on its own
/// workspace's tickets.
#[actix_rt::test]
async fn plugin_events_stay_in_their_workspace() {
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);
    set_up_plugin(&h, a, &["ticket:read"]);
    set_up_plugin(&h, b, &["ticket:read"]);
    let in_a = h.rule(a, on(a.plugin_uuid, &failed(a)));
    // B's rule names A's plugin.
    let in_b = h.rule(b, on(a.plugin_uuid, &failed(a)));
    let b_own = h.rule(b, on(b.plugin_uuid, &failed(b)));
    let ticket_a = ticket_for(&h, a, a.member_uuid);

    // B's plugin, by B's admin, naming A's ticket: B can't see it.
    emitted(
        &h,
        b,
        b.admin_uuid,
        &failed(b),
        json!({ "ticket_id": ticket_a }),
    );
    h.drain().await;
    untouched(&h, b, b_own);
    untouched(&h, a, in_a);
    assert_ne!(h.priority(a, ticket_a), TicketPriority::High);

    // A's plugin runs A's rule only.
    emitted(
        &h,
        a,
        a.admin_uuid,
        &failed(a),
        json!({ "ticket_id": ticket_a }),
    );
    h.drain().await;
    assert_eq!(h.statuses(a, in_a), vec![Status::Succeeded]);
    untouched(&h, b, in_b);
    let row = &h.applications(a, in_a)[0];
    assert_eq!(row.workspace_id, a.workspace_id);
}

/// An event queued while the plugin ran doesn't fire once the plugin is
/// disabled or uninstalled.
#[actix_rt::test]
async fn a_disabled_or_uninstalled_plugins_events_run_nothing() {
    let h = Harness::new();
    let a = &h.seeded.a;
    set_up_plugin(&h, a, &["ticket:read"]);
    let rule = h.rule(a, on(a.plugin_uuid, &failed(a)));
    let ticket = ticket_for(&h, a, a.member_uuid);

    for state in [PluginState::Disabled, PluginState::Uninstalled] {
        set_state(&h, a, PluginState::Installed);
        emitted(
            &h,
            a,
            a.admin_uuid,
            &failed(a),
            json!({ "ticket_id": ticket }),
        );
        set_state(&h, a, state);
        assert_eq!(queued(&h), 1, "{state:?}");
        h.drain().await;
        assert_eq!(queued(&h), 0, "{state:?}");
        untouched(&h, a, rule);
    }
    assert_ne!(h.priority(a, ticket), TicketPriority::High);

    // Gone altogether.
    set_state(&h, a, PluginState::Installed);
    emitted(
        &h,
        a,
        a.admin_uuid,
        &failed(a),
        json!({ "ticket_id": ticket }),
    );
    h.as_admin(a, |c| {
        diesel::delete(plugins::table.filter(plugins::uuid.eq(a.plugin_uuid))).execute(c)
    });
    h.drain().await;
    untouched(&h, a, rule);
}

/// Saving a plugin event rule checks its plugin is installed in the
/// workspace and declares the event. A rule whose plugin has gone since can
/// still be edited as long as its trigger stays as it is.
#[actix_rt::test]
async fn a_plugin_event_rule_names_a_plugin_here_and_one_of_its_events() {
    use crate::rules_in_workspace::as_admin;

    crate::common::ensure_test_keyring();
    let h = Harness::new();
    let (a, b) = (&h.seeded.a, &h.seeded.b);
    set_up_plugin(&h, a, &["ticket:read"]);
    let create = |plugin: Uuid, event: &str| {
        http_test::TestRequest::post().uri("/api/rules").set_json(json!({
            "name": "Escalate builds",
            "trigger_kind": "plugin_event",
            "trigger_config": { "plugin_uuid": plugin.to_string().to_uppercase(), "event_type": event },
            "actions": high(),
        }))
    };
    let refused = |status: StatusCode, body: &Value| {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "RULE_VALIDATION", "{body}");
    };

    for (plugin, event) in [
        // Another workspace's plugin.
        (b.plugin_uuid, failed(b)),
        // A host event, never a plugin's.
        (a.plugin_uuid, "ticket:created".to_string()),
        // One the plugin doesn't declare.
        (a.plugin_uuid, format!("{}:deployed", a.plugin_name)),
        (a.plugin_uuid, failed(b)),
    ] {
        let resp = as_admin(&h.pool, a, create(plugin, &event)).await;
        let status = resp.status();
        refused(status, &http_test::read_body_json(resp).await);
    }

    let resp = as_admin(&h.pool, a, create(a.plugin_uuid, &failed(a))).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let rule: Value = http_test::read_body_json(resp).await;
    assert_eq!(
        rule["trigger_config"],
        json!({ "plugin_uuid": a.plugin_uuid.to_string(), "event_type": failed(a) }),
        "saved as events record the plugin"
    );
    let id = rule["id"].as_i64().expect("id");

    h.as_admin(a, |c| {
        diesel::delete(plugins::table.filter(plugins::uuid.eq(a.plugin_uuid))).execute(c)
    });
    let edit = |event: &str| {
        http_test::TestRequest::put()
            .uri(&format!("/api/rules/{id}"))
            .set_json(json!({
                "name": "Escalate failing builds",
                "trigger_kind": "plugin_event",
                "trigger_config": { "plugin_uuid": a.plugin_uuid.to_string(), "event_type": event },
            }))
    };
    let resp = as_admin(&h.pool, a, edit(&failed(a))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = as_admin(&h.pool, a, edit(&fixed(a))).await;
    let status = resp.status();
    refused(status, &http_test::read_body_json(resp).await);
}
