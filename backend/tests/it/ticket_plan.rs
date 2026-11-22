//! A ticket's plan (due and start times, its place in a day, its size) is
//! written the same way through `PATCH /api/tickets/{id}` and
//! `/api/sync/push`, keeps its rules on both, and a reorder is quiet: no
//! activity row, no notification, no rule run, no webhook and no "updated"
//! bump.
#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewRule, NewTicket, RuleState, RuleTriggerKind, Ticket};
use backend::repository::workflow_states;
use backend::services::search::SearchService;
use backend::services::webhooks::WebhookService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:ticket_plan";

struct Fixture {
    _db: common::TestDb,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
    ws: WorkspaceSeed,
    search: Arc<SearchService>,
}

fn day(d: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 11, d)
        .expect("date")
        .and_time(NaiveTime::MIN)
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("time")
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        Self {
            _db: db,
            _search_dir: search_dir,
            pool,
            ws,
            search,
        }
    }

    fn run<T>(&self, f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>) -> T {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, f).expect("run in workspace")
    }

    fn ticket(&self, title: &str) -> i32 {
        self.run(|c| {
            let open = workflow_states::default_state(c)?.id;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(self.ws.member_uuid),
                    ..Default::default()
                })
                .returning(backend::schema::tickets::id)
                .get_result(c)
        })
    }

    fn read(&self, id: i32) -> Ticket {
        self.run(|c| backend::repository::get_ticket_by_id(c, id))
    }

    /// The newest sync action of the ticket: its sync id, type and data.
    fn last_event(&self, id: i32) -> (i64, String, Value) {
        self.run(|c| {
            use backend::schema::sync_actions;
            sync_actions::table
                .filter(sync_actions::aggregate_id.eq(id.to_string()))
                .filter(sync_actions::event_type.like("ticket.%"))
                .order(sync_actions::sync_id.desc())
                .select((
                    sync_actions::sync_id,
                    sync_actions::event_type,
                    sync_actions::data,
                ))
                .first(c)
        })
    }

    async fn patch(&self, ticket_id: i32, body: Value) -> (StatusCode, Value) {
        self.send(Method::PATCH, &format!("/api/tickets/{ticket_id}"), body)
            .await
    }

    /// Push one ticket update; `Ok` when applied, `Err(reason)` when refused.
    async fn push(&self, ticket_id: i32, patch: Value) -> Result<(), String> {
        let (status, body) = self
            .send(
                Method::POST,
                "/api/sync/push",
                json!([{
                    "tx_id": Uuid::now_v7().to_string(),
                    "aggregate": "ticket",
                    "model_id": ticket_id.to_string(),
                    "op": "U",
                    "patch": patch,
                }]),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        match body["rejected"].as_array().and_then(|r| r.first()) {
            Some(rejected) => Err(rejected["reason"].as_str().unwrap_or_default().to_string()),
            None => Ok(()),
        }
    }

    async fn activity(&self, ticket_id: i32) -> Vec<String> {
        let (status, body) = self
            .send(
                Method::GET,
                &format!("/api/tickets/{ticket_id}/activity"),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|e| e["event_type"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// Send as the workspace's admin.
    async fn send(&self, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
        let user = self.ws.admin_uuid;
        let claims = Claims {
            sub: user.to_string(),
            name: "Admin".to_string(),
            email: "someone@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
            file_path: None,
        };
        let workspace = WorkspaceContext {
            workspace_id: self.ws.workspace_id,
            workspace_uuid: self.ws.workspace_uuid,
            slug: self.ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(self.ws.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(
                    web::scope("/api")
                        .route(
                            "/tickets/{id}",
                            web::patch().to(backend::handlers::update_ticket_partial),
                        )
                        .route(
                            "/tickets/{id}/activity",
                            web::get().to(backend::handlers::get_ticket_activity),
                        )
                        .route(
                            "/sync/push",
                            web::post().to(backend::handlers::sync::push::push),
                        ),
                ),
        )
        .await;
        let mut req = http_test::TestRequest::default().method(method).uri(uri);
        if !body.is_null() {
            req = req.set_json(body);
        }
        let resp = http_test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let bytes = http_test::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

/// One date write rule on both paths: the calendar day written, whatever
/// time or offset comes with it; a time as `HH:MM`; `null` clears, and a
/// cleared date takes its time with it.
#[actix_web::test]
async fn patch_and_push_write_a_date_the_same_way() {
    let f = Fixture::new();
    let (by_patch, by_push) = (f.ticket("By PATCH"), f.ticket("By push"));
    for raw in [
        "2026-11-21",
        "2026-11-21T00:00:00",
        "2026-11-21T00:00:00+11:00",
        "2026-11-21T20:00:00-05:00",
    ] {
        let (status, body) = f
            .patch(by_patch, json!({ "due_date": raw, "start_date": raw }))
            .await;
        assert_eq!(status, StatusCode::OK, "{raw}: {body}");
        f.push(by_push, json!({ "due_date": raw, "start_date": raw }))
            .await
            .unwrap_or_else(|e| panic!("{raw}: {e}"));
        for id in [by_patch, by_push] {
            let t = f.read(id);
            assert_eq!(
                (t.due_date, t.start_date),
                (Some(day(21)), Some(day(21))),
                "{raw}"
            );
        }
    }

    let times = json!({ "due_time": "14:30", "start_time": "09:15:00" });
    assert_eq!(f.patch(by_patch, times.clone()).await.0, StatusCode::OK);
    f.push(by_push, times).await.expect("push times");
    for id in [by_patch, by_push] {
        let t = f.read(id);
        assert_eq!(
            (t.due_time, t.start_time),
            (Some(hm(14, 30)), Some(hm(9, 15)))
        );
    }

    let cleared = json!({ "due_date": null });
    assert_eq!(f.patch(by_patch, cleared.clone()).await.0, StatusCode::OK);
    f.push(by_push, cleared).await.expect("push a cleared date");
    for id in [by_patch, by_push] {
        let t = f.read(id);
        assert_eq!(
            (t.due_date, t.due_time),
            (None, None),
            "cleared with its time"
        );
        assert_eq!(t.start_time, Some(hm(9, 15)), "the start keeps its own");
    }
}

/// What breaks the plan's rules is refused on both paths, and nothing saves.
#[actix_web::test]
async fn a_plan_that_breaks_the_rules_is_refused() {
    let f = Fixture::new();
    let id = f.ticket("Undated");
    for body in [
        json!({ "due_time": "09:00" }),
        json!({ "estimate_minutes": 0 }),
        json!({ "estimate_minutes": 7 * 24 * 60 + 1 }),
        json!({ "rank": "a 0" }),
        json!({ "due_date": "next week" }),
        json!({ "due_time": "9am" }),
    ] {
        let (status, _) = f.patch(id, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "PATCH {body}");
        assert_eq!(
            f.push(id, body.clone()).await,
            Err("invalid_patch".to_string()),
            "push {body}"
        );
    }
    let t = f.read(id);
    assert_eq!(
        (t.due_date, t.due_time, t.estimate_minutes, t.rank),
        (None, None, None, None)
    );

    assert_eq!(
        f.patch(id, json!({ "estimate_minutes": 120, "rank": "a0" }))
            .await
            .0,
        StatusCode::OK
    );
    let t = f.read(id);
    assert_eq!(
        (t.estimate_minutes, t.rank.as_deref()),
        (Some(120), Some("a0"))
    );
}

/// A place in a day belongs to that day and that person. Moving the ticket
/// to another day or another assignee without placing it clears it; a drop
/// that places it there keeps the new one; closing it or setting a time
/// keeps it.
#[actix_web::test]
async fn the_place_follows_the_day_and_the_person() {
    let f = Fixture::new();
    let id = f.ticket("Replace the switch");
    f.push(id, json!({ "due_date": "2026-11-21", "rank": "a0" }))
        .await
        .expect("place it");
    assert_eq!(f.read(id).rank.as_deref(), Some("a0"));

    assert_eq!(
        f.patch(id, json!({ "due_time": "10:00" })).await.0,
        StatusCode::OK
    );
    assert_eq!(f.read(id).rank.as_deref(), Some("a0"), "a time keeps it");

    f.push(id, json!({ "due_date": "2026-11-22" }))
        .await
        .expect("move it");
    assert_eq!(f.read(id).rank, None, "another day, unplaced");

    f.push(id, json!({ "due_date": "2026-11-23", "rank": "a1" }))
        .await
        .expect("drop it on a day");
    assert_eq!(f.read(id).rank.as_deref(), Some("a1"));

    let (status, body) = f
        .patch(id, json!({ "assignee": f.ws.admin_uuid.to_string() }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(f.read(id).rank, None, "someone else's day, unplaced");

    f.push(id, json!({ "rank": "a2" })).await.expect("place it");
    let done = f.run(|c| {
        workflow_states::first_in_category(c, backend::models::WorkflowStateCategory::Done)
    });
    f.push(id, json!({ "workflow_state_id": done.id }))
        .await
        .expect("close it");
    assert_eq!(f.read(id).rank.as_deref(), Some("a2"), "a close keeps it");
}

/// A reorder through either path changes the rank and nothing a person would
/// read as activity: `updated_at` stays, the event is `ticket.rank_changed`,
/// and the activity feed, the notification and rule outboxes and the webhook
/// drain each leave it out, while an ordinary edit next to it gets through.
#[actix_web::test]
async fn a_reorder_is_quiet_for_every_consumer() {
    let f = Fixture::new();
    // A live rule on updates, so the rule outbox takes ticket events, and a
    // webhook on ticket updates.
    f.run(|c| {
        backend::repository::rules::create(
            c,
            NewRule {
                workspace_id: f.ws.workspace_id,
                name: "Watch updates".to_string(),
                description: None,
                trigger_kind: RuleTriggerKind::TicketUpdated,
                trigger_config: json!({}),
                conditions: json!([]),
                actions: json!([{ "kind": "set_priority", "config": { "priority": "high" } }]),
                reads_set: Vec::new(),
                writes_set: Vec::new(),
                state: RuleState::Live,
                priority: 100,
                created_by: Some(f.ws.admin_uuid),
            },
        )?;
        backend::repository::webhooks::create_webhook(
            c,
            "updates".into(),
            "https://example.invalid/hook".into(),
            "secret".into(),
            vec!["ticket.updated".into()],
            None,
            None,
        )
    });
    let drain = || {
        let mut conn = f.pool.get().expect("conn");
        with_actor_bypass_context(
            &mut conn,
            &ActorContext::system("test:ticket_plan_webhooks"),
            WebhookService::drain_batch_txn,
        )
        .expect("drain the webhook outbox")
        .0
        .into_iter()
        .filter(|t| t.workspace_id == f.ws.workspace_id)
        .map(|t| t.payload.event_type)
        .collect::<Vec<_>>()
    };

    let id = f.ticket("Swap the toner");
    f.push(
        id,
        json!({ "due_date": "2026-11-21", "assignee_uuid": f.ws.admin_uuid }),
    )
    .await
    .expect("plan it");
    drain();
    let before = f.read(id).updated_at;

    let mut reorders = Vec::new();
    f.push(id, json!({ "rank": "a0" }))
        .await
        .expect("push a reorder");
    reorders.push(f.last_event(id));
    // A drop within the same day names the day again.
    assert_eq!(
        f.patch(id, json!({ "rank": "a1", "due_date": "2026-11-21" }))
            .await
            .0,
        StatusCode::OK
    );
    reorders.push(f.last_event(id));

    let after = f.read(id);
    assert_eq!(after.rank.as_deref(), Some("a1"));
    assert_eq!(after.updated_at, before, "a reorder is no edit");
    for (sync_id, event_type, data) in &reorders {
        assert_eq!(event_type, "ticket.rank_changed");
        assert!(data["rank"].is_string(), "the pool gets the new rank");
        assert_eq!(data["changed_fields"], json!([]), "no field a rule reads");
        let (notified, ruled) = f.run(|c| {
            use backend::schema::{notification_outbox, rule_event_outbox};
            let notified: i64 = notification_outbox::table
                .filter(notification_outbox::sync_id.eq(sync_id))
                .count()
                .get_result(c)?;
            let ruled: i64 = rule_event_outbox::table
                .filter(rule_event_outbox::sync_id.eq(sync_id))
                .count()
                .get_result(c)?;
            Ok((notified, ruled))
        });
        assert_eq!((notified, ruled), (0, 0), "no notification, no rule run");
    }
    assert!(drain().is_empty(), "no webhook");

    // An ordinary edit gets through to each of them.
    assert_eq!(
        f.patch(id, json!({ "priority": "low" })).await.0,
        StatusCode::OK
    );
    let (sync_id, event_type, _) = f.last_event(id);
    assert_eq!(event_type, "ticket.priority_changed");
    let ruled: i64 = f.run(|c| {
        use backend::schema::rule_event_outbox;
        rule_event_outbox::table
            .filter(rule_event_outbox::sync_id.eq(sync_id))
            .count()
            .get_result(c)
    });
    assert_eq!(ruled, 1, "an edit runs rules");
    assert!(f.read(id).updated_at > before, "an edit is an edit");
    assert_eq!(drain(), vec!["ticket.updated".to_string()]);

    let feed = f.activity(id).await;
    assert!(
        feed.contains(&"ticket.priority_changed".to_string()),
        "{feed:?}"
    );
    assert!(
        !feed.contains(&"ticket.rank_changed".to_string()),
        "{feed:?}"
    );
}
