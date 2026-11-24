//! Carry-overs to a replacement asset: nothing runs unless asked; open
//! tickets the caller can see are linked to the new asset as well, the old
//! links stay and a retry adds none; holder, managed by, location and
//! groups copy without clearing anything; the old asset moves through the
//! normal transition, a disposal needs its record and a refusal undoes the
//! whole call; only a replaced record naming a replacement runs; requesters
//! and other workspaces get 404; and the new asset's timeline reads the
//! old one's history when asked.

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
    Claims, NewAsset, NewAssetGroup, NewTicket, RecordSource, UpdateSiteSettings,
    WorkflowStateCategory,
};
use backend::repository::asset_records::{self as records_repo, NewRecord, WorkDate};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{
    asset_groups, asset_record_types, assets as asset_repo, site_settings, workflow_states,
};
use backend::schema::{
    asset_disposals, asset_group_assignments, asset_lifecycle_events, asset_records, ticket_assets,
    tickets,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_replacement";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    other: WorkspaceSeed,
    agent: Uuid,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    run_in_workspace(&pool, REF, seeded.a.workspace_id, |c| {
        add_membership(
            c,
            seeded.a.workspace_id,
            agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                requester_asset_visibility: Some("all".to_string()),
                default_timezone: Some("UTC".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and settings");
    Fixture {
        pool,
        ws: seeded.a,
        other: seeded.b,
        agent,
    }
}

/// An asset in workspace `ws`: its holder, manager and location as given.
fn asset_in(
    pool: &TestPool,
    ws: i32,
    name: &str,
    holder: Option<Uuid>,
    location: Option<&str>,
) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        asset_repo::create_device(
            c,
            NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: location.map(str::to_string),
                notes: None,
                primary_user_uuid: holder,
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
    .expect("create an asset")
    .id
}

fn asset(f: &Fixture, name: &str) -> i32 {
    asset_in(&f.pool, f.ws.workspace_id, name, None, None)
}

fn read_asset(f: &Fixture, id: i32) -> backend::models::Asset {
    in_ws(f, |c| asset_repo::get_device_by_id(c, id))
}

/// Send `method uri` with `body` as `viewer` in workspace A.
async fn call(
    f: &Fixture,
    viewer: Uuid,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_scoped(f, viewer, "full", method, uri, body).await
}

/// [`call`] with a credential of `scope`, as an API token narrowed to it
/// carries.
async fn call_scoped(
    f: &Fixture,
    viewer: Uuid,
    scope: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: format!("{viewer}@example.com"),
        platform_role: "user".to_string(),
        scope: scope.to_string(),
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

async fn add_repair(f: &Fixture) {
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        "/api/assets/record-types/catalogue/repair",
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
}

/// A repair record on `old`, ended with `outcome`, naming `replacement`;
/// its id.
async fn record(f: &Fixture, old: i32, outcome: &str, replacement: Option<i32>) -> i32 {
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{old}/records"),
        Some(json!({
            "record_type": "repair",
            "summary": "Swapped under warranty",
            "occurred_on": "2025-03-04",
            "completed_on": "2025-03-04",
            "outcome": outcome,
            "replacement_asset_id": replacement,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["record"]["id"].as_i64().expect("record id") as i32
}

async fn carry(f: &Fixture, old: i32, record: i32, body: Value) -> (StatusCode, Value) {
    let uri = format!("/api/assets/{old}/records/{record}/replacement");
    call(f, f.agent, Method::POST, &uri, Some(body)).await
}

/// A ticket in a state of `category`, linked to each of `assets`.
fn ticket(
    f: &Fixture,
    title: &str,
    category: WorkflowStateCategory,
    pending: bool,
    assets: &[i32],
) -> i32 {
    in_ws(f, |c| {
        let state = workflow_states::first_in_category(c, category)?;
        let id: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: state.id,
                verification_state: pending.then(|| "pending".to_string()),
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)?;
        for asset in assets {
            diesel::insert_into(ticket_assets::table)
                .values((
                    ticket_assets::ticket_id.eq(id),
                    ticket_assets::asset_id.eq(asset),
                ))
                .execute(c)?;
        }
        Ok(id)
    })
}

fn tickets_of(f: &Fixture, asset: i32) -> Vec<i32> {
    let mut ids: Vec<i32> = in_ws(f, |c| {
        ticket_assets::table
            .filter(ticket_assets::asset_id.eq(asset))
            .select(ticket_assets::ticket_id)
            .load(c)
    });
    ids.sort_unstable();
    ids
}

fn group(f: &Fixture, name: &str, assets: &[i32]) -> i32 {
    in_ws(f, |c| {
        let g = asset_groups::create_group(
            c,
            NewAssetGroup {
                name: name.to_string(),
                description: None,
                color: None,
                display_order: 0,
                created_by: None,
            },
        )?;
        for a in assets {
            let mut ids: Vec<i32> = asset_groups::groups_for_asset(c, *a)?
                .into_iter()
                .map(|g| g.id)
                .collect();
            ids.push(g.id);
            asset_groups::set_groups_for_asset(c, *a, &ids, None)?;
        }
        Ok(g.id)
    })
}

fn groups_of(f: &Fixture, asset: i32) -> Vec<i32> {
    let mut ids: Vec<i32> = in_ws(f, |c| asset_groups::groups_for_asset(c, asset))
        .into_iter()
        .map(|g| g.id)
        .collect();
    ids.sort_unstable();
    ids
}

fn events_of(f: &Fixture, asset: i32) -> Vec<(Option<String>, String)> {
    in_ws(f, |c| {
        asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq(asset))
            .order(asset_lifecycle_events::id.asc())
            .select((
                asset_lifecycle_events::from_status,
                asset_lifecycle_events::to_status,
            ))
            .load(c)
    })
}

#[actix_web::test]
async fn nothing_is_carried_unless_asked() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset_in(
        &f.pool,
        f.ws.workspace_id,
        "CAM-014",
        Some(f.ws.member_uuid),
        Some("Studio A"),
    );
    let new = asset(&f, "CAM-031");
    group(&f, "Studio cameras", &[old]);
    let open = ticket(&f, "No SDI", WorkflowStateCategory::Active, false, &[old]);
    let rid = record(&f, old, "replaced", Some(new)).await;

    let (status, body) = carry(&f, old, rid, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["replacement_asset_id"], new, "{body}");
    assert!(body["tickets"].is_null(), "{body}");
    assert!(body["assignment"].is_null(), "{body}");
    assert!(body["old_asset"].is_null(), "{body}");

    let explicit =
        json!({ "link_open_tickets": false, "copy_assignment": false, "move_old_to": null });
    let (status, body) = carry(&f, old, rid, explicit).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(tickets_of(&f, new), Vec::<i32>::new());
    assert_eq!(tickets_of(&f, old), vec![open]);
    let after = read_asset(&f, new);
    assert_eq!(after.primary_user_uuid, None);
    assert_eq!(after.location, None);
    assert_eq!(groups_of(&f, new), Vec::<i32>::new());
    assert_eq!(read_asset(&f, old).status, "in_service");
    assert!(events_of(&f, old).is_empty());
}

#[actix_web::test]
async fn open_tickets_are_linked_to_both_and_a_retry_adds_none() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    let open = ticket(&f, "No SDI", WorkflowStateCategory::Active, false, &[old]);
    let triage = ticket(&f, "Flicker", WorkflowStateCategory::Triage, false, &[old]);
    let done = ticket(&f, "Lens cap", WorkflowStateCategory::Done, false, &[old]);
    let both = ticket(
        &f,
        "Swap",
        WorkflowStateCategory::Active,
        false,
        &[old, new],
    );
    // A guest's ticket still waiting on email confirmation isn't one anybody
    // can see yet.
    let pending = ticket(
        &f,
        "Unconfirmed",
        WorkflowStateCategory::Triage,
        true,
        &[old],
    );
    let rid = record(&f, old, "replaced", Some(new)).await;

    let (status, body) = carry(&f, old, rid, json!({ "link_open_tickets": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut linked = vec![open, triage];
    linked.sort_unstable();
    assert_eq!(body["tickets"]["linked"], json!(linked), "{body}");
    assert_eq!(body["tickets"]["already_linked"], json!([both]), "{body}");

    let mut on_new = vec![open, triage, both];
    on_new.sort_unstable();
    assert_eq!(tickets_of(&f, new), on_new);
    let mut on_old = vec![open, triage, done, both, pending];
    on_old.sort_unstable();
    assert_eq!(tickets_of(&f, old), on_old, "the old links stay");

    let (status, body) = carry(&f, old, rid, json!({ "link_open_tickets": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["tickets"]["linked"], json!([]), "{body}");
    assert_eq!(
        body["tickets"]["already_linked"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(tickets_of(&f, new), on_new, "a retry adds no second link");
}

#[actix_web::test]
async fn holder_managed_by_location_and_groups_copy() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset_in(
        &f.pool,
        f.ws.workspace_id,
        "CAM-014",
        Some(f.ws.member_uuid),
        Some("Studio A"),
    );
    in_ws(&f, |c| {
        asset_repo::update_device(
            c,
            old,
            backend::models::AssetUpdate {
                managed_by_user_uuid: Some(Some(f.agent)),
                ..Default::default()
            },
        )
    });
    let new = asset(&f, "CAM-031");
    let studio = group(&f, "Studio cameras", &[old]);
    let spare = group(&f, "Spares", &[new]);
    let rid = record(&f, old, "replaced", Some(new)).await;

    let (status, body) = carry(&f, old, rid, json!({ "copy_assignment": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assignment"]["copied"],
        json!(["holder", "managed_by", "location"]),
        "{body}"
    );
    assert_eq!(
        body["assignment"]["groups_added"],
        json!([studio]),
        "{body}"
    );

    let after = read_asset(&f, new);
    assert_eq!(after.primary_user_uuid, Some(f.ws.member_uuid));
    assert_eq!(after.managed_by_user_uuid, Some(f.agent));
    assert_eq!(after.location.as_deref(), Some("Studio A"));
    let mut groups = vec![studio, spare];
    groups.sort_unstable();
    assert_eq!(
        groups_of(&f, new),
        groups,
        "the new asset's own group stays"
    );
    // The old asset keeps its own.
    assert_eq!(read_asset(&f, old).location.as_deref(), Some("Studio A"));
    assert_eq!(groups_of(&f, old), vec![studio]);

    let (status, body) = carry(&f, old, rid, json!({ "copy_assignment": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["assignment"]["copied"], json!([]), "{body}");
    assert_eq!(body["assignment"]["groups_added"], json!([]), "{body}");

    // A synced asset's holder and location are the sync's: only managed by
    // and groups copy to it.
    let synced = asset(&f, "LAPTOP-7");
    in_ws(&f, |c| {
        asset_repo::update_device(
            c,
            synced,
            backend::models::AssetUpdate {
                external_sync_source: Some(Some("microsoft".to_string())),
                ..Default::default()
            },
        )
    });
    let rid_synced = record(&f, old, "replaced", Some(synced)).await;
    let (status, body) = carry(&f, old, rid_synced, json!({ "copy_assignment": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assignment"]["copied"],
        json!(["managed_by"]),
        "{body}"
    );
    assert_eq!(
        body["assignment"]["skipped"],
        json!(["holder", "location"]),
        "{body}"
    );
    let after = read_asset(&f, synced);
    assert_eq!(after.primary_user_uuid, None);
    assert_eq!(after.location, None);
    assert_eq!(after.managed_by_user_uuid, Some(f.agent));
    assert_eq!(groups_of(&f, synced), vec![studio]);

    // An old asset with nothing to copy clears nothing on the new one.
    let bare = asset(&f, "CAM-040");
    let rid = record(&f, bare, "replaced", Some(new)).await;
    let (status, body) = carry(&f, bare, rid, json!({ "copy_assignment": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let after = read_asset(&f, new);
    assert_eq!(after.primary_user_uuid, Some(f.ws.member_uuid));
    assert_eq!(after.location.as_deref(), Some("Studio A"));
    assert_eq!(groups_of(&f, new), groups);
}

#[actix_web::test]
async fn the_old_asset_moves_through_the_transition() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let new = asset(&f, "CAM-031");
    for (name, status) in [("CAM-014", "retired"), ("CAM-015", "in_stock")] {
        let old = asset(&f, name);
        let rid = record(&f, old, "replaced", Some(new)).await;
        let (code, body) = carry(&f, old, rid, json!({ "move_old_to": status })).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["old_asset"]["status"], status, "{body}");
        assert!(body["old_asset"]["event_id"].is_i64(), "{body}");
        assert_eq!(read_asset(&f, old).status, status);
        assert_eq!(
            events_of(&f, old),
            vec![(Some("in_service".to_string()), status.to_string())]
        );

        // Already there: no second event.
        let (code, body) = carry(&f, old, rid, json!({ "move_old_to": status })).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert!(body["old_asset"]["event_id"].is_null(), "{body}");
        assert_eq!(events_of(&f, old).len(), 1);
    }

    let old = asset(&f, "CAM-016");
    let rid = record(&f, old, "replaced", Some(new)).await;
    for status in ["in_service", "in_repair", "lost", "nonsense"] {
        let (code, body) = carry(&f, old, rid, json!({ "move_old_to": status })).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{status}: {body}");
    }
    let (code, body) = carry(
        &f,
        old,
        rid,
        json!({ "disposal": { "sanitization_method": "clear", "data_bearing": true } }),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
    assert!(events_of(&f, old).is_empty());
}

#[actix_web::test]
async fn disposal_needs_its_record_and_a_refusal_undoes_every_part() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset_in(
        &f.pool,
        f.ws.workspace_id,
        "CAM-014",
        None,
        Some("Studio A"),
    );
    let new = asset(&f, "CAM-031");
    let open = ticket(&f, "No SDI", WorkflowStateCategory::Active, false, &[old]);
    let rid = record(&f, old, "replaced", Some(new)).await;

    let everything = |disposal: Value| {
        json!({
            "link_open_tickets": true,
            "copy_assignment": true,
            "move_old_to": "disposed",
            "disposal": disposal,
        })
    };
    let (status, body) = carry(&f, old, rid, everything(Value::Null)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = carry(
        &f,
        old,
        rid,
        everything(json!({ "sanitization_method": "shred", "data_bearing": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        tickets_of(&f, new),
        Vec::<i32>::new(),
        "no link outlives a refusal"
    );
    assert_eq!(read_asset(&f, new).location, None);
    assert_eq!(read_asset(&f, old).status, "in_service");

    let (status, body) = carry(
        &f,
        old,
        rid,
        everything(json!({
            "sanitization_method": "destroy",
            "data_bearing": true,
            "itad_vendor": "Shred Co",
            "notes": "Certificate to follow",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["old_asset"]["status"], "disposed", "{body}");
    assert_eq!(body["tickets"]["linked"], json!([open]), "{body}");
    assert_eq!(read_asset(&f, old).status, "disposed");
    assert_eq!(read_asset(&f, new).location.as_deref(), Some("Studio A"));
    let (method, vendor): (String, Option<String>) = in_ws(&f, |c| {
        asset_disposals::table
            .filter(asset_disposals::asset_id.eq(old))
            .select((
                asset_disposals::sanitization_method,
                asset_disposals::itad_vendor,
            ))
            .first(c)
    });
    assert_eq!(method, "destroy");
    assert_eq!(vendor.as_deref(), Some("Shred Co"));
}

#[actix_web::test]
async fn only_a_replaced_record_naming_a_replacement_runs() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    ticket(&f, "No SDI", WorkflowStateCategory::Active, false, &[old]);
    let ask = json!({ "link_open_tickets": true, "move_old_to": "retired" });

    let repaired = record(&f, old, "repaired", None).await;
    let (status, body) = carry(&f, old, repaired, ask.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "RECORD_NOT_REPLACED", "{body}");

    let unnamed = record(&f, old, "replaced", None).await;
    let (status, body) = carry(&f, old, unnamed, ask.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "NO_REPLACEMENT_ASSET", "{body}");

    // A record of another asset isn't one of this asset's.
    let rid = record(&f, old, "replaced", Some(new)).await;
    let (status, body) = carry(&f, new, rid, ask.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    assert_eq!(tickets_of(&f, new), Vec::<i32>::new());
    assert_eq!(read_asset(&f, old).status, "in_service");
}

#[actix_web::test]
async fn requesters_and_other_workspaces_get_404() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    ticket(&f, "No SDI", WorkflowStateCategory::Active, false, &[old]);
    let rid = record(&f, old, "replaced", Some(new)).await;
    let ask = json!({ "link_open_tickets": true, "move_old_to": "retired" });

    let uri = format!("/api/assets/{old}/records/{rid}/replacement");
    let (status, body) = call(&f, f.ws.member_uuid, Method::POST, &uri, Some(ask.clone())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(tickets_of(&f, new), Vec::<i32>::new());
    assert_eq!(read_asset(&f, old).status, "in_service");

    // Workspace B's replaced asset and its record, asked for from A.
    let theirs = asset_in(&f.pool, f.other.workspace_id, "B-001", None, None);
    let their_new = asset_in(&f.pool, f.other.workspace_id, "B-002", None, None);
    let their_record = run_in_workspace(&f.pool, REF, f.other.workspace_id, |c| {
        let repair = asset_record_types::add_from_catalogue(c, "repair", None)
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        let day = WorkDate::Day("2025-03-04".parse().expect("a day"));
        records_repo::create(
            c,
            NewRecord {
                asset_id: theirs,
                record_type_id: repair.id,
                summary: None,
                notes: None,
                occurred: day,
                completed: Some(day),
                outcome: Some("replaced".to_string()),
                vendor: None,
                performed_by_uuid: None,
                reference: None,
                cost: None,
                ticket_id: None,
                replacement_asset_id: Some(their_new),
                snapshot: Some(Default::default()),
                attributes: json!({}),
                source: RecordSource::Manual,
                source_ref: None,
                import_job_id: None,
                rule_id: None,
                lifecycle_event_id: None,
                recorded_by: None,
            },
        )
        .map(|w| w.record.id)
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e}").into()))
    })
    .expect("workspace B's record");
    let (status, body) = carry(&f, theirs, their_record, ask.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let still: String = run_in_workspace(&f.pool, REF, f.other.workspace_id, |c| {
        asset_repo::get_device_by_id(c, theirs).map(|a| a.status)
    })
    .expect("workspace B's asset");
    assert_eq!(still, "in_service");
}

#[actix_web::test]
async fn the_new_assets_timeline_reads_the_old_ones_history() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    let (status, body) = call(
        &f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{old}/records"),
        Some(json!({
            "record_type": "repair",
            "summary": "Lens mount reseated",
            "occurred_on": "2024-06-01",
            "completed_on": "2024-06-01",
            "outcome": "repaired",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let earlier = body["record"]["id"].as_i64().expect("record id");
    let rid = record(&f, old, "replaced", Some(new)).await;
    let (status, body) = carry(&f, old, rid, json!({ "move_old_to": "retired" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let records_from = |page: &Value, asset: i32| -> Vec<i64> {
        page["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .filter(|e| e["source"] == "record" && e["asset_id"] == asset)
            .filter_map(|e| e["data"]["id"].as_i64())
            .collect()
    };
    let uri = format!("/api/assets/{new}/timeline");
    let (status, alone) = call(&f, f.agent, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{alone}");
    assert!(records_from(&alone, old).is_empty(), "{alone}");

    let uri = format!("/api/assets/{new}/timeline?include_predecessors=true");
    let (status, merged) = call(&f, f.agent, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    let from_old = records_from(&merged, old);
    assert!(from_old.contains(&earlier), "{merged}");
    assert!(from_old.contains(&(rid as i64)), "{merged}");
    let moved = merged["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .any(|e| e["source"] == "status" && e["asset_id"] == old);
    assert!(moved, "the old asset's move shows too: {merged}");
}

#[actix_web::test]
async fn a_token_that_cant_write_tickets_links_none() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    ticket(&f, "No SDI", WorkflowStateCategory::Active, false, &[old]);
    let rid = record(&f, old, "replaced", Some(new)).await;
    let uri = format!("/api/assets/{old}/records/{rid}/replacement");

    let ask = json!({ "link_open_tickets": true, "move_old_to": "retired" });
    let (status, body) =
        call_scoped(&f, f.agent, "assets:write", Method::POST, &uri, Some(ask)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(tickets_of(&f, new), Vec::<i32>::new());
    assert_eq!(read_asset(&f, old).status, "in_service");

    // What is the assets' own runs.
    let ask = json!({ "move_old_to": "retired" });
    let (status, body) =
        call_scoped(&f, f.agent, "assets:write", Method::POST, &uri, Some(ask)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read_asset(&f, old).status, "retired");
}

#[actix_web::test]
async fn the_copy_keeps_the_new_assets_archived_group_memberships() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    let studio = group(&f, "Studio cameras", &[old]);
    let retired_group = group(&f, "Old fleet", &[new]);
    in_ws(&f, |c| asset_groups::archive_group(c, retired_group));
    let rid = record(&f, old, "replaced", Some(new)).await;

    let (status, body) = carry(&f, old, rid, json!({ "copy_assignment": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assignment"]["groups_added"],
        json!([studio]),
        "{body}"
    );
    let mut memberships: Vec<i32> = in_ws(&f, |c| {
        asset_group_assignments::table
            .filter(asset_group_assignments::asset_id.eq(new))
            .select(asset_group_assignments::group_id)
            .load(c)
    });
    memberships.sort_unstable();
    let mut expected = vec![studio, retired_group];
    expected.sort_unstable();
    assert_eq!(
        memberships, expected,
        "the archived group's membership stays"
    );
}

/// Move `asset` into repair through the status change, opening a repair
/// record; that record's id.
async fn into_repair(f: &Fixture, asset: i32) -> i32 {
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{asset}/lifecycle"),
        Some(json!({ "to_status": "in_repair", "record": { "summary": "Sent to Camera Clinic" } })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["metadata"]["record_id"].as_i64().expect("record id") as i32
}

fn completed_on(f: &Fixture, record: i32) -> Option<chrono::NaiveDate> {
    in_ws(f, |c| {
        asset_records::table
            .find(record)
            .select(asset_records::completed_on)
            .first(c)
    })
}

#[actix_web::test]
async fn leaving_repair_completes_the_one_open_repair_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    let open = into_repair(&f, old).await;
    let rid = record(&f, old, "replaced", Some(new)).await;

    let (status, body) = carry(&f, old, rid, json!({ "move_old_to": "retired" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["old_asset"]["completed_record_id"], open, "{body}");
    assert!(completed_on(&f, open).is_some());
    assert_eq!(read_asset(&f, old).status, "retired");
}

#[actix_web::test]
async fn leaving_repair_with_several_open_records_completes_none() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let old = asset(&f, "CAM-014");
    let new = asset(&f, "CAM-031");
    let first = into_repair(&f, old).await;
    let (status, body) = call(
        &f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{old}/records"),
        Some(json!({ "record_type": "repair", "summary": "Second fault", "occurred_on": "2025-03-04" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let second = body["record"]["id"].as_i64().expect("record id") as i32;
    let rid = record(&f, old, "replaced", Some(new)).await;

    let (status, body) = carry(&f, old, rid, json!({ "move_old_to": "in_stock" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["old_asset"]["completed_record_id"].is_null(), "{body}");
    assert!(completed_on(&f, first).is_none());
    assert!(completed_on(&f, second).is_none());
    assert_eq!(read_asset(&f, old).status, "in_stock");
}
