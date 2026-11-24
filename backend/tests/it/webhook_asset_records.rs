//! Asset record writes reach webhooks as `asset.record_created`,
//! `asset.record_updated` and `asset.record_deleted`, carrying the record as
//! sync stores it. Records written over REST, opened and completed by a move
//! through repair, and completed by a replacement's move of the old asset
//! each send theirs. A webhook on other asset events, or in another
//! workspace, hears none of them.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, UpdateSiteSettings};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{assets as asset_repo, site_settings, webhooks as webhook_repo};
use backend::schema::webhook_deliveries;
use backend::services::webhooks::WebhookService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:webhook_asset_records";

const RECORD_EVENTS: [&str; 3] = [
    "asset.record_created",
    "asset.record_updated",
    "asset.record_deleted",
];

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    /// Subscribed to the three record events.
    records_hook: i32,
    /// Subscribed to every asset event but the record ones.
    assets_hook: i32,
    /// In workspace B, subscribed to the record events.
    other_hook: i32,
    other_ws: i32,
    /// The newest delivery already read.
    seen: std::cell::Cell<i32>,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

fn hook(pool: &TestPool, ws: i32, name: &str, events: &[&str]) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        webhook_repo::create_webhook(
            c,
            name.to_string(),
            format!("https://example.invalid/{name}"),
            "whsec_test".to_string(),
            events.iter().map(|e| e.to_string()).collect(),
            None,
            None,
        )
    })
    .expect("create a webhook")
    .id
}

fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a;
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        add_membership(
            c,
            ws.workspace_id,
            agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                default_timezone: Some("UTC".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and the timezone");
    let records_hook = hook(&pool, ws.workspace_id, "records", &RECORD_EVENTS);
    let assets_hook = hook(
        &pool,
        ws.workspace_id,
        "assets",
        &[
            "asset.created",
            "asset.updated",
            "asset.deleted",
            "asset.linked",
            "asset.unlinked",
        ],
    );
    let other_hook = hook(&pool, seeded.b.workspace_id, "other", &RECORD_EVENTS);
    Fixture {
        pool,
        ws,
        agent,
        records_hook,
        assets_hook,
        other_hook,
        other_ws: seeded.b.workspace_id,
        seen: std::cell::Cell::new(0),
    }
}

fn new_asset(f: &Fixture, name: &str) -> i32 {
    in_ws(f, |c| {
        asset_repo::create_device(
            c,
            NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: None,
                purchase_date: None,
                asset_tag: Some(name.to_string()),
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
    .id
}

/// Drain the webhook outbox as the dispatcher does, recording a delivery
/// row per subscriber, until it is empty.
fn drain(f: &Fixture) {
    let mut conn = f.pool.get().expect("conn");
    loop {
        let (_, more) = with_actor_bypass_context(
            &mut conn,
            &ActorContext::system("test:webhook_asset_records_drain"),
            WebhookService::drain_batch_txn,
        )
        .expect("drain the outbox");
        if !more {
            break;
        }
    }
}

/// Each delivery recorded for `webhook` in `ws` after the ones already
/// read: its id, event and data.
fn deliveries(f: &Fixture, webhook: i32, ws: i32) -> Vec<(i32, String, Value)> {
    let rows: Vec<(i32, String, Value)> = run_in_workspace(&f.pool, REF, ws, |c| {
        webhook_deliveries::table
            .filter(webhook_deliveries::webhook_id.eq(webhook))
            .filter(webhook_deliveries::id.gt(f.seen.get()))
            .order(webhook_deliveries::id.asc())
            .select((
                webhook_deliveries::id,
                webhook_deliveries::event_type,
                webhook_deliveries::payload,
            ))
            .load(c)
    })
    .expect("load deliveries");
    rows.into_iter()
        .map(|(id, event, payload)| (id, event, payload["data"].clone()))
        .collect()
}

/// Drain and mark everything delivered so far as read.
fn settle(f: &Fixture) {
    drain(f);
    let newest: Option<i32> = with_actor_bypass_context(
        &mut f.pool.get().expect("conn"),
        &ActorContext::system("test:webhook_asset_records_mark"),
        |c| {
            webhook_deliveries::table
                .select(diesel::dsl::max(webhook_deliveries::id))
                .first(c)
        },
    )
    .expect("newest delivery");
    f.seen.set(newest.unwrap_or(0));
}

/// Drain, and the new deliveries the record webhook got; the asset webhook
/// and workspace B's got none.
fn record_deliveries(f: &Fixture) -> Vec<(String, Value)> {
    drain(f);
    let got = deliveries(f, f.records_hook, f.ws.workspace_id);
    // A move through repair is an asset.updated too; that webhook hears it,
    // and none of the record events.
    let asset_events = deliveries(f, f.assets_hook, f.ws.workspace_id);
    assert!(
        asset_events
            .iter()
            .all(|(_, e, _)| !e.starts_with("asset.record_")),
        "a webhook on asset events hears no record events: {asset_events:?}"
    );
    assert!(
        deliveries(f, f.other_hook, f.other_ws).is_empty(),
        "workspace B's webhook hears none of A's records"
    );
    settle(f);
    got.into_iter().map(|(_, e, d)| (e, d)).collect()
}

async fn call(f: &Fixture, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let claims = Claims {
        sub: f.agent.to_string(),
        name: "Agent".to_string(),
        email: format!("{}@example.com", f.agent),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
        file_path: None,
    };
    let workspace = WorkspaceContext {
        workspace_id: f.ws.workspace_id,
        workspace_uuid: f.ws.workspace_uuid,
        slug: f.ws.slug.clone(),
        name: f.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(f.agent, Some(corr)).with_workspace(f.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(f.pool.clone()))
            .app_data(web::Data::new(common::test_storage()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::assets::config)),
    )
    .await;
    let mut req = http_test::TestRequest::default().method(method).uri(uri);
    if let Some(body) = body {
        req = req.set_json(body);
    }
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn add_repair(f: &Fixture) {
    let (status, body) = call(
        f,
        Method::POST,
        "/api/assets/record-types/catalogue/repair",
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
}

fn repair_type_id(f: &Fixture) -> i32 {
    use backend::schema::asset_record_types;
    in_ws(f, |c| {
        asset_record_types::table
            .filter(asset_record_types::slug.eq("repair"))
            .select(asset_record_types::id)
            .first(c)
    })
}

#[actix_web::test]
async fn rest_writes_send_created_updated_and_deleted() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let asset = new_asset(&f, "CAM-014");
    // Setup sends asset.created to the asset webhook only; start clean.
    settle(&f);

    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{asset}/records"),
        Some(json!({
            "record_type": "repair",
            "summary": "Lens mount replaced",
            "occurred_on": "2025-03-04",
            "vendor": "Camera Clinic",
            "cost": { "amount": "120.00", "currency": "AUD" },
            "attributes": { "fault": "Lens mount loose" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["record"]["id"].as_i64().expect("record id");

    let got = record_deliveries(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    let (event, data) = &got[0];
    assert_eq!(event, "asset.record_created");
    assert_eq!(data["id"], id);
    assert_eq!(data["asset_id"], asset);
    assert_eq!(data["record_type_id"], repair_type_id(&f));
    assert_eq!(data["summary"], "Lens mount replaced");
    assert_eq!(data["occurred_on"], "2025-03-04");
    assert_eq!(data["vendor"], "Camera Clinic");
    assert_eq!(data["cost_minor"], 12000);
    assert_eq!(data["cost_currency"], "AUD");
    assert_eq!(data["attributes"]["fault"], "Lens mount loose");
    assert_eq!(data["source"], "manual");
    assert_eq!(data["recorded_by"], f.agent.to_string());
    assert_eq!(data["version"], 1);
    assert!(data.get("workspace_id").is_none(), "{data}");

    let (status, body) = call(
        &f,
        Method::PATCH,
        &format!("/api/assets/{asset}/records/{id}"),
        Some(json!({
            "expected_version": 1,
            "completed_on": "2025-03-06",
            "outcome": "repaired",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = record_deliveries(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    let (event, data) = &got[0];
    assert_eq!(event, "asset.record_updated");
    assert_eq!(data["id"], id);
    assert_eq!(data["completed_on"], "2025-03-06");
    assert_eq!(data["outcome"], "repaired");
    assert_eq!(data["version"], 2);
    assert_eq!(data["summary"], "Lens mount replaced");

    let (status, body) = call(
        &f,
        Method::DELETE,
        &format!("/api/assets/{asset}/records/{id}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let got = record_deliveries(&f);
    assert_eq!(
        got,
        vec![(
            "asset.record_deleted".to_string(),
            json!({ "id": id, "asset_id": asset })
        )]
    );
}

#[actix_web::test]
async fn a_webhook_without_the_record_events_hears_none() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let asset = new_asset(&f, "CAM-014");
    let only_created = hook(
        &f.pool,
        f.ws.workspace_id,
        "created-only",
        &["asset.record_created"],
    );
    settle(&f);

    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{asset}/records"),
        Some(json!({ "record_type": "repair", "occurred_on": "2025-03-04" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["record"]["id"].as_i64().expect("record id");
    let (status, body) = call(
        &f,
        Method::PATCH,
        &format!("/api/assets/{asset}/records/{id}"),
        Some(json!({ "expected_version": 1, "summary": "Checked" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    drain(&f);

    let events: Vec<String> = deliveries(&f, only_created, f.ws.workspace_id)
        .into_iter()
        .map(|(_, e, _)| e)
        .collect();
    assert_eq!(events, vec!["asset.record_created".to_string()]);
    assert!(deliveries(&f, f.assets_hook, f.ws.workspace_id).is_empty());
}

#[actix_web::test]
async fn a_move_through_repair_sends_the_record_it_opens_and_completes() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let asset = new_asset(&f, "CAM-014");
    settle(&f);

    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{asset}/lifecycle"),
        Some(json!({
            "to_status": "in_repair",
            "reason": "Lens won't focus",
            "record": { "summary": "Sent to Camera Clinic" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["metadata"]["record_id"].as_i64().expect("record id");

    let got = record_deliveries(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    let (event, data) = &got[0];
    assert_eq!(event, "asset.record_created");
    assert_eq!(data["id"], id);
    assert_eq!(data["source"], "transition");
    assert_eq!(data["summary"], "Sent to Camera Clinic");
    assert!(data["lifecycle_event_id"].is_i64(), "{data}");
    assert!(data["completed_on"].is_null(), "{data}");

    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{asset}/lifecycle"),
        Some(json!({
            "to_status": "in_service",
            "complete_record": { "outcome": "repaired" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let got = record_deliveries(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    let (event, data) = &got[0];
    assert_eq!(event, "asset.record_updated");
    assert_eq!(data["id"], id);
    assert_eq!(data["outcome"], "repaired");
    assert!(data["completed_on"].is_string(), "{data}");
}

#[actix_web::test]
async fn a_replacement_moving_the_old_asset_sends_the_record_it_completes() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = new_asset(&f, "CAM-014");
    let new = new_asset(&f, "CAM-031");
    settle(&f);
    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{old}/lifecycle"),
        Some(json!({ "to_status": "in_repair", "record": { "summary": "Sent away" } })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let open = body["metadata"]["record_id"].as_i64().expect("record id");
    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{old}/records"),
        Some(json!({
            "record_type": "repair",
            "summary": "Swapped under warranty",
            "occurred_on": "2025-03-04",
            "completed_on": "2025-03-04",
            "outcome": "replaced",
            "replacement_asset_id": new,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let replaced = body["record"]["id"].as_i64().expect("record id");
    let got = record_deliveries(&f);
    let created: Vec<i64> = got
        .iter()
        .filter(|(e, _)| e == "asset.record_created")
        .map(|(_, d)| d["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(created, vec![open, replaced]);
    let replacement = got
        .iter()
        .find(|(_, d)| d["id"] == replaced)
        .map(|(_, d)| d.clone())
        .expect("the replaced record's event");
    assert_eq!(replacement["replacement_asset_id"], new);
    assert_eq!(replacement["outcome"], "replaced");

    let (status, body) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{old}/records/{replaced}/replacement"),
        Some(json!({ "move_old_to": "retired" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["old_asset"]["completed_record_id"], open, "{body}");
    let got = record_deliveries(&f);
    assert_eq!(got.len(), 1, "{got:?}");
    let (event, data) = &got[0];
    assert_eq!(event, "asset.record_updated");
    assert_eq!(data["id"], open);
    assert!(data["completed_on"].is_string(), "{data}");
}
