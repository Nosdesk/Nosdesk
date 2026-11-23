//! A move into repair opens a repair record when the caller asks, and the
//! move out completes it. The record and the event name each other; a move
//! that doesn't ask writes no record; a record the rules refuse refuses the
//! move with it; and a workspace that never added the repair type gets it
//! from the catalogue on the first record, while one that archived it keeps
//! it archived.

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
use backend::models::{
    AssetRecord, AssetRecordType, Claims, NewAsset, OutcomeCategory, RecordOutcome,
    UpdateSiteSettings,
};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{
    asset_record_types as types_repo, asset_records, assets as asset_repo, site_settings,
};
use backend::schema::{asset_lifecycle_events, asset_record_types};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_repair_transition_record";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    asset: i32,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

/// Workspace A with an agent, a camera in service, AUD as its currency and
/// no record types.
fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a;
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    let asset = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
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
                currency_code: Some(Some("AUD".to_string())),
                ..Default::default()
            },
        )?;
        asset_repo::create_device(
            c,
            NewAsset {
                name: "Camera 14".to_string(),
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
        )
    })
    .expect("seed the agent and the camera")
    .id;
    Fixture {
        pool,
        ws,
        agent,
        asset,
    }
}

async fn call(f: &Fixture, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let viewer = f.agent;
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Agent".to_string(),
        email: format!("{viewer}@example.com"),
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
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(f.ws.workspace_id);
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

/// Move the camera; the answer.
async fn transition(f: &Fixture, body: Value) -> (StatusCode, Value) {
    let uri = format!("/api/assets/{}/lifecycle", f.asset);
    call(f, Method::POST, &uri, Some(body)).await
}

fn records(f: &Fixture) -> Vec<AssetRecord> {
    let mut rows = in_ws(f, |c| asset_records::list_for_asset(c, f.asset));
    rows.sort_by_key(|r| r.id);
    rows
}

fn status(f: &Fixture) -> String {
    in_ws(f, |c| asset_repo::get_device_by_id(c, f.asset)).status
}

fn event_count(f: &Fixture) -> i64 {
    in_ws(f, |c| {
        asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq(f.asset))
            .count()
            .get_result(c)
    })
}

fn repair_type(f: &Fixture) -> Option<AssetRecordType> {
    in_ws(f, |c| {
        asset_record_types::table
            .filter(asset_record_types::slug.eq("repair"))
            .first(c)
            .optional()
    })
}

fn into_repair(record: Value) -> Value {
    json!({
        "to_status": "in_repair",
        "reason": "Lens won't focus",
        "record": record,
    })
}

/// Opting in adds the repair type, opens one record at the move and names it
/// on the event beside the keys older readers know; a `record_id` a client
/// sends is not kept.
#[actix_web::test]
async fn opting_in_opens_one_record_linked_both_ways() {
    let db = common::TestDb::new();
    let f = seed(&db);
    assert!(repair_type(&f).is_none(), "the workspace starts without it");

    let mut body = into_repair(json!({
        "vendor": " Camera Clinic ",
        "reference": "RMA-4471",
        "attributes": {
            "fault": "Lens mount loose",
            "offsite": true,
            "expected_return": "2026-12-01",
        },
    }));
    body["metadata"] = json!({ "record_id": 999_999, "note": "kept" });
    let (code, event) = transition(&f, body).await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    assert_eq!(status(&f), "in_repair");

    let rtype = repair_type(&f).expect("added from the catalogue");
    assert!(rtype.is_builtin);
    assert_eq!(rtype.opens_with_status.as_deref(), Some("in_repair"));

    let rows = records(&f);
    assert_eq!(rows.len(), 1);
    let record = &rows[0];
    assert_eq!(record.record_type_id, rtype.id);
    assert_eq!(record.source, "transition");
    assert_eq!(
        record.lifecycle_event_id,
        event["id"].as_i64().map(|i| i as i32)
    );
    assert_eq!(record.completed_on, None, "open");
    assert_eq!(record.outcome, None);
    assert_eq!(record.vendor.as_deref(), Some("Camera Clinic"));
    assert_eq!(record.reference.as_deref(), Some("RMA-4471"));
    assert_eq!(record.recorded_by, Some(f.agent));
    let occurred_at = event["occurred_at"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    assert_eq!(
        record.occurred_at, occurred_at,
        "the work starts at the move"
    );
    assert_eq!(
        record.attributes,
        json!({ "fault": "Lens mount loose", "offsite": true, "expected_return": "2026-12-01" })
    );

    assert_eq!(
        event["metadata"],
        json!({
            "record_id": record.id,
            "note": "kept",
            "vendor": "Camera Clinic",
            "rma_number": "RMA-4471",
            "offsite": true,
            "expected_return": "2026-12-01",
        })
    );
}

/// Without `record` the move writes none, adds no type, and keeps no
/// `record_id` a client sends.
#[actix_web::test]
async fn a_move_that_doesnt_ask_writes_no_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (code, event) = transition(
        &f,
        json!({
            "to_status": "in_repair",
            "metadata": { "vendor": "Camera Clinic", "record_id": 12 },
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    assert_eq!(event["metadata"], json!({ "vendor": "Camera Clinic" }));
    assert!(records(&f).is_empty());
    assert!(repair_type(&f).is_none());

    // Nor does the move out, without `complete_record`.
    let (code, _) = transition(&f, json!({ "to_status": "in_service" })).await;
    assert_eq!(code, StatusCode::CREATED);
    assert!(records(&f).is_empty());
}

/// Leaving repair completes the open record with the outcome, the cost in
/// the workspace currency and the day given; nothing else about it changes.
#[actix_web::test]
async fn leaving_repair_completes_the_open_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (code, _) = transition(&f, into_repair(json!({ "vendor": "Camera Clinic" }))).await;
    assert_eq!(code, StatusCode::CREATED);
    let opened = records(&f).remove(0);

    let today = chrono::Utc::now().date_naive();
    let (code, event) = transition(
        &f,
        json!({
            "to_status": "in_service",
            "complete_record": {
                "outcome": "repaired",
                "cost": { "amount": "120.50" },
                "completed_on": today,
            },
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    assert_eq!(status(&f), "in_service");
    assert!(event["metadata"].get("record_id").is_none());

    let rows = records(&f);
    assert_eq!(rows.len(), 1, "completed, not a second record");
    let done = &rows[0];
    assert_eq!(done.id, opened.id);
    assert_eq!(done.completed_on, Some(today));
    assert_eq!(done.completed_at, None, "a day was given");
    assert_eq!(done.outcome.as_deref(), Some("repaired"));
    assert_eq!(done.cost_minor, Some(12050));
    assert_eq!(done.cost_currency.as_deref(), Some("AUD"));
    assert_eq!(done.version, opened.version + 1);
    assert_eq!(done.updated_by, Some(f.agent));
    assert_eq!(done.vendor, opened.vendor);
    assert_eq!(done.lifecycle_event_id, opened.lifecycle_event_id);

    // Nothing open is left to complete.
    transition(&f, json!({ "to_status": "in_repair" })).await;
    let (code, refused) = transition(
        &f,
        json!({ "to_status": "in_service", "complete_record": {} }),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "NO_OPEN_RECORD");
    assert_eq!(status(&f), "in_repair", "the refusal moves nothing");
}

/// With no completion moment given, the record completes at the move.
#[actix_web::test]
async fn a_completion_without_a_date_is_the_moment_of_the_move() {
    let db = common::TestDb::new();
    let f = seed(&db);
    transition(&f, into_repair(json!({}))).await;
    let (code, event) = transition(
        &f,
        json!({ "to_status": "in_stock", "complete_record": { "outcome": "no_fault_found" } }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    let done = records(&f).remove(0);
    let moved_at = event["occurred_at"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    assert_eq!(done.completed_at, moved_at);
    assert_eq!(done.outcome.as_deref(), Some("no_fault_found"));
}

/// With two open repairs the move out names one; without a name it is
/// refused with both ids, and a record that isn't open is refused too.
#[actix_web::test]
async fn more_than_one_open_record_needs_a_choice() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (code, _) = transition(&f, into_repair(json!({ "reference": "RMA-1" }))).await;
    assert_eq!(code, StatusCode::CREATED);
    // A second open repair, written on its own.
    let uri = format!("/api/assets/{}/records", f.asset);
    let (code, second) = call(
        &f,
        Method::POST,
        &uri,
        Some(json!({
            "record_type": "repair",
            "occurred_on": chrono::Utc::now().date_naive(),
            "reference": "RMA-2",
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{second}");
    let second = second["record"]["id"].as_i64().expect("id") as i32;
    let first = records(&f)[0].id;

    let (code, refused) = transition(
        &f,
        json!({ "to_status": "in_service", "complete_record": { "outcome": "repaired" } }),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "RECORD_CHOICE_REQUIRED");
    // Oldest work first; on the same day, the one dated only by its day.
    assert_eq!(refused["open_record_ids"], json!([second, first]));
    assert_eq!(status(&f), "in_repair");

    let (code, refused) = transition(
        &f,
        json!({ "to_status": "in_service", "complete_record": { "record_id": 987_654 } }),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "RECORD_NOT_OPEN");

    let (code, _) = transition(
        &f,
        json!({
            "to_status": "in_service",
            "complete_record": { "record_id": second, "outcome": "replaced" },
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    let rows = records(&f);
    assert_eq!(rows[0].completed_on, None, "the other stays open");
    assert_eq!(rows[1].outcome.as_deref(), Some("replaced"));
}

/// A record the rules refuse refuses the move: the status, the event and
/// the record are all left unwritten.
#[actix_web::test]
async fn a_refused_record_refuses_the_move() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (code, refused) = transition(
        &f,
        into_repair(json!({ "attributes": { "offsite": "yes" } })),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "INVALID_ATTRIBUTES");
    assert_eq!(status(&f), "in_service");
    assert_eq!(event_count(&f), 0);
    assert!(records(&f).is_empty());
    assert!(
        repair_type(&f).is_none(),
        "the type added for it is rolled back"
    );

    // An outcome the type doesn't have refuses the move out the same way.
    transition(&f, into_repair(json!({}))).await;
    let (code, refused) = transition(
        &f,
        json!({ "to_status": "in_service", "complete_record": { "outcome": "melted" } }),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "UNKNOWN_OUTCOME");
    assert_eq!(status(&f), "in_repair");
    assert_eq!(records(&f)[0].completed_on, None);
}

/// A record for a status no type opens is refused. An archived repair type
/// stays archived: the move doesn't add it again or use it.
#[actix_web::test]
async fn no_live_type_for_the_status_refuses_the_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (code, refused) = transition(
        &f,
        json!({ "to_status": "retired", "record": { "vendor": "Anyone" } }),
    )
    .await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "NO_RECORD_TYPE_FOR_STATUS");
    assert_eq!(status(&f), "in_service");

    let (code, added) = call(
        &f,
        Method::POST,
        "/api/assets/record-types/catalogue/repair",
        None,
    )
    .await;
    assert!(code.is_success(), "{added}");
    let id = added["id"].as_i64().expect("id");
    in_ws(&f, |c| {
        types_repo::archive(c, id as i32)
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });
    let (code, refused) = transition(&f, into_repair(json!({}))).await;
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "NO_RECORD_TYPE_FOR_STATUS");
    assert!(records(&f).is_empty());
    assert!(repair_type(&f).and_then(|t| t.archived_at).is_some());
}

/// The type an admin points at a status is the one its moves open, whatever
/// its slug.
#[actix_web::test]
async fn the_type_pointed_at_the_status_is_the_one_opened() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let calibration = in_ws(&f, |c| {
        types_repo::create(
            c,
            types_repo::NewRecordTypeInput {
                slug: "calibration".into(),
                label: "Calibration".into(),
                description: None,
                icon: None,
                color: None,
                field_schema: json!({ "type": "object", "properties": {} }),
                field_order: vec![],
                core_fields: json!({}),
                outcomes: vec![RecordOutcome {
                    key: "in_tolerance".into(),
                    category: OutcomeCategory::Done,
                    label: Some("In tolerance".into()),
                }],
                applies_to_kinds: None,
                opens_with_status: Some("in_transit".into()),
                sort_order: 0,
                created_by: None,
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .id;
    let (code, event) = transition(
        &f,
        json!({ "to_status": "in_transit", "record": { "reference": "WO-9" } }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    let rows = records(&f);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record_type_id, calibration);
    assert_eq!(event["metadata"]["rma_number"], "WO-9");
    assert!(repair_type(&f).is_none(), "nothing else is added");

    let (code, _) = transition(
        &f,
        json!({
            "to_status": "in_service",
            "complete_record": { "outcome": "in_tolerance" },
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(records(&f)[0].outcome.as_deref(), Some("in_tolerance"));
}

/// A record a move into the status opened is completed by the move out,
/// whatever its type: here a repair whose type lost the status to another
/// type, as a backfilled repair can. It is listed for the move too.
#[actix_web::test]
async fn a_record_opened_by_a_move_into_the_status_is_completed_by_the_move_out() {
    let db = common::TestDb::new();
    let f = seed(&db);
    // Calibration takes in_repair first, so the repair type comes without it.
    let calibration = in_ws(&f, |c| {
        types_repo::create(
            c,
            types_repo::NewRecordTypeInput {
                slug: "calibration".into(),
                label: "Calibration".into(),
                description: None,
                icon: None,
                color: None,
                field_schema: json!({ "type": "object", "properties": {} }),
                field_order: vec![],
                core_fields: json!({}),
                outcomes: vec![],
                applies_to_kinds: None,
                opens_with_status: Some("in_repair".into()),
                sort_order: 0,
                created_by: None,
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .id;
    let repair = in_ws(&f, |c| {
        types_repo::add_from_catalogue(c, "repair", None)
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });
    assert_eq!(repair.opens_with_status, None);

    let (code, event) = transition(&f, json!({ "to_status": "in_repair" })).await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    let event_id = event["id"].as_i64().expect("event id") as i32;
    // The repair the backfill would have written for that move.
    let backfilled = in_ws(&f, |c| {
        asset_records::create(
            c,
            asset_records::NewRecord {
                asset_id: f.asset,
                record_type_id: repair.id,
                summary: None,
                notes: None,
                occurred: asset_records::WorkDate::Day(chrono::Utc::now().date_naive()),
                completed: None,
                outcome: None,
                vendor: Some("Camera Clinic".into()),
                performed_by_uuid: None,
                reference: None,
                cost: None,
                ticket_id: None,
                replacement_asset_id: None,
                snapshot: None,
                attributes: json!({}),
                source: backend::models::RecordSource::Backfill,
                source_ref: None,
                import_job_id: None,
                rule_id: None,
                lifecycle_event_id: Some(event_id),
                recorded_by: None,
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .record
    .id;
    // Another repair, opened by no move: not this move's to complete.
    let (code, unrelated) = call(
        &f,
        Method::POST,
        &format!("/api/assets/{}/records", f.asset),
        Some(json!({
            "record_type": "repair",
            "occurred_on": chrono::Utc::now().date_naive(),
        })),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{unrelated}");
    let unrelated = unrelated["record"]["id"].as_i64().expect("id") as i32;

    let (code, listed) = call(
        &f,
        Method::GET,
        &format!(
            "/api/assets/{}/records?open=true&leaving=in_repair",
            f.asset
        ),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{listed}");
    let ids: Vec<i64> = listed["records"]
        .as_array()
        .expect("records")
        .iter()
        .map(|r| r["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(ids, vec![backfilled as i64]);

    let (code, done) = transition(
        &f,
        json!({ "to_status": "in_service", "complete_record": { "outcome": "repaired" } }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{done}");
    let rows = records(&f);
    let completed = rows
        .iter()
        .find(|r| r.id == backfilled)
        .expect("the repair");
    assert_eq!(completed.outcome.as_deref(), Some("repaired"));
    assert!(completed.completed_on.is_some());
    let other = rows.iter().find(|r| r.id == unrelated).expect("the other");
    assert_eq!(other.completed_on, None, "left open");
    assert!(rows.iter().all(|r| r.record_type_id != calibration));
}

/// Metadata that isn't an object becomes one when the move writes a record,
/// so the event still names the record and carries the repair keys.
#[actix_web::test]
async fn metadata_that_isnt_an_object_still_names_the_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let mut body = into_repair(json!({ "vendor": "Camera Clinic" }));
    body["metadata"] = json!(["not", "an", "object"]);
    let (code, event) = transition(&f, body).await;
    assert_eq!(code, StatusCode::CREATED, "{event}");
    let record = records(&f).remove(0);
    assert_eq!(
        event["metadata"],
        json!({ "record_id": record.id, "vendor": "Camera Clinic" })
    );
}
