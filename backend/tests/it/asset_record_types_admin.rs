//! Asset record types over REST: workspace admins create, edit, archive and
//! restore them; a field change some records no longer fit is refused with
//! their count unless forced; a built-in renamed goes back to its catalogue
//! name; any staff member adds a built-in from the catalogue, which is
//! marked added once it is; and the record form's list carries the currency
//! its cost field offers.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, UpdateSiteSettings};
use backend::repository::site_settings;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::schema::assets;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_record_types_admin";
const ADMIN: &str = "/api/admin/asset-record-types";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    asset: i32,
}

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
        let asset = common::insert_stock_asset(c, "Forklift");
        diesel::update(assets::table.find(asset))
            .set(assets::asset_tag.eq("FL-14"))
            .execute(c)?;
        Ok(asset)
    })
    .expect("seed");
    Fixture {
        pool,
        ws,
        agent,
        asset,
    }
}

async fn call(
    f: &Fixture,
    viewer: Uuid,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
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
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::asset_record_types::config)
                    .configure(backend::handlers::assets::config),
            ),
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

fn calibration() -> Value {
    json!({
        "slug": "calibration",
        "label": "Calibration",
        "field_schema": {
            "type": "object",
            "properties": { "drift": { "type": "string" } }
        },
        "outcomes": [{ "key": "in_tolerance", "category": "done", "label": "In tolerance" }],
    })
}

#[actix_web::test]
async fn admins_create_types_and_agents_cannot() {
    let db = common::TestDb::new();
    let f = seed(&db);

    let (status, _) = call(&f, f.agent, Method::POST, ADMIN, Some(calibration())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&f, f.agent, Method::GET, ADMIN, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, created) = call(
        &f,
        f.ws.admin_uuid,
        Method::POST,
        ADMIN,
        Some(calibration()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["label"], "Calibration");
    assert_eq!(created["is_builtin"], false);
    assert_eq!(created["field_order"], json!(["drift"]));

    let (status, taken) = call(
        &f,
        f.ws.admin_uuid,
        Method::POST,
        ADMIN,
        Some(calibration()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{taken}");
    assert_eq!(taken["code"], "SLUG_TAKEN");
    let mut reserved = calibration();
    reserved["slug"] = json!("repair");
    let (status, refused) = call(&f, f.ws.admin_uuid, Method::POST, ADMIN, Some(reserved)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "SLUG_RESERVED");
    let mut bad = calibration();
    bad["slug"] = json!("drift-check");
    bad["color"] = json!("red");
    let (status, refused) = call(&f, f.ws.admin_uuid, Method::POST, ADMIN, Some(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "INVALID_KEY");
}

#[actix_web::test]
async fn staff_add_built_ins_from_the_catalogue_and_admins_rename_them() {
    let db = common::TestDb::new();
    let f = seed(&db);

    let (status, catalogue) = call(
        &f,
        f.agent,
        Method::GET,
        "/api/assets/record-types/catalogue",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{catalogue}");
    let slugs: Vec<&str> = catalogue
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["slug"].as_str().expect("slug"))
        .collect();
    assert_eq!(
        slugs,
        [
            "repair",
            "inspection",
            "preventative_maintenance",
            "firmware_update"
        ]
    );
    assert!(catalogue
        .as_array()
        .expect("entries")
        .iter()
        .all(|e| e["added"] == false));

    // An agent adds one; adding again gives back the same type.
    let add = "/api/assets/record-types/catalogue/repair";
    let (status, added) = call(&f, f.agent, Method::POST, add, None).await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    assert_eq!(added["is_builtin"], true);
    assert_eq!(added["label"], Value::Null);
    assert_eq!(added["opens_with_status"], "in_repair");
    let (status, again) = call(&f, f.agent, Method::POST, add, None).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["id"], added["id"]);
    let (status, _) = call(
        &f,
        f.agent,
        Method::POST,
        "/api/assets/record-types/catalogue/teleport",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, catalogue) = call(
        &f,
        f.agent,
        Method::GET,
        "/api/assets/record-types/catalogue",
        None,
    )
    .await;
    assert_eq!(catalogue[0]["added"], true);
    assert_eq!(catalogue[0]["type"]["id"], added["id"]);
    assert_eq!(catalogue[1]["added"], false);

    // Renamed, then back to the catalogue name.
    let one = format!("{ADMIN}/{}", added["id"]);
    let (status, renamed) = call(
        &f,
        f.ws.admin_uuid,
        Method::PUT,
        &one,
        Some(json!({ "label": "Fix-up" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["label"], "Fix-up");
    let (status, reset) = call(
        &f,
        f.ws.admin_uuid,
        Method::PUT,
        &one,
        Some(json!({ "label": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reset}");
    assert_eq!(reset["label"], Value::Null);
    // A built-in is archived, never deleted.
    let (status, refused) = call(&f, f.ws.admin_uuid, Method::DELETE, &one, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "BUILTIN");
}

#[actix_web::test]
async fn a_used_type_is_archived_not_deleted_and_leaves_the_picker() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (_, created) = call(
        &f,
        f.ws.admin_uuid,
        Method::POST,
        ADMIN,
        Some(calibration()),
    )
    .await;
    let type_id = created["id"].as_i64().expect("type id");
    let records = format!("/api/assets/{}/records", f.asset);
    let record = json!({
        "record_type": "calibration",
        "occurred_on": "2024-02-01",
        "completed_on": "2024-02-01",
        "outcome": "in_tolerance",
        "attributes": { "drift": "0.2%" },
    });
    let (status, body) = call(&f, f.agent, Method::POST, &records, Some(record.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let one = format!("{ADMIN}/{type_id}");
    let (status, refused) = call(&f, f.ws.admin_uuid, Method::DELETE, &one, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "IN_USE");
    assert_eq!(refused["record_count"], 1);
    let (_, listed) = call(&f, f.ws.admin_uuid, Method::GET, ADMIN, None).await;
    assert_eq!(listed[0]["record_count"], 1, "{listed}");

    let (status, archived) = call(
        &f,
        f.ws.admin_uuid,
        Method::POST,
        &format!("{one}/archive"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{archived}");
    assert!(archived["archived_at"].is_string());
    assert_eq!(archived["record_count"], 1, "{archived}");
    let (_, picker) = call(&f, f.agent, Method::GET, "/api/assets/record-types", None).await;
    assert_eq!(picker["types"], json!([]));
    let (_, picker) = call(
        &f,
        f.agent,
        Method::GET,
        "/api/assets/record-types?include_archived=true",
        None,
    )
    .await;
    assert_eq!(picker["types"][0]["id"], json!(type_id));
    let (status, refused) = call(&f, f.agent, Method::POST, &records, Some(record.clone())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "RECORD_TYPE_ARCHIVED");

    let (status, restored) = call(
        &f,
        f.ws.admin_uuid,
        Method::POST,
        &format!("{one}/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["archived_at"], Value::Null);
    assert_eq!(restored["record_count"], 1, "{restored}");
    let (status, _) = call(&f, f.agent, Method::POST, &records, Some(record)).await;
    assert_eq!(status, StatusCode::CREATED);

    // An unused type of the workspace's own is deleted.
    let mut spare = calibration();
    spare["slug"] = json!("spare");
    let (_, spare) = call(&f, f.ws.admin_uuid, Method::POST, ADMIN, Some(spare)).await;
    let (status, _) = call(
        &f,
        f.ws.admin_uuid,
        Method::DELETE,
        &format!("{ADMIN}/{}", spare["id"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[actix_web::test]
async fn a_field_change_records_no_longer_fit_is_refused_with_their_count_unless_forced() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (_, created) = call(
        &f,
        f.ws.admin_uuid,
        Method::POST,
        ADMIN,
        Some(calibration()),
    )
    .await;
    let one = format!("{ADMIN}/{}", created["id"]);
    let mut record_ids = Vec::new();
    for drift in ["0.2%", "0.4%"] {
        let (status, body) = call(
            &f,
            f.agent,
            Method::POST,
            &format!("/api/assets/{}/records", f.asset),
            Some(json!({
                "record_type": "calibration",
                "occurred_on": "2024-02-01",
                "attributes": { "drift": drift },
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        record_ids.push(body["record"]["id"].clone());
    }
    let numeric = json!({
        "field_schema": {
            "type": "object",
            "properties": { "drift": { "type": "number" } }
        }
    });
    let (status, refused) = call(
        &f,
        f.ws.admin_uuid,
        Method::PUT,
        &one,
        Some(numeric.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "RECORDS_WOULD_NOT_FIT");
    assert_eq!(refused["invalid_count"], 2);
    let sample = refused["sample"].as_array().expect("sample");
    assert_eq!(sample.len(), 2);
    // Each names its record and the equipment it is on.
    assert_eq!(sample[0]["id"], record_ids[0], "{refused}");
    assert_eq!(sample[0]["asset_id"], json!(f.asset));
    assert_eq!(sample[0]["asset_name"], "Forklift");
    assert_eq!(sample[0]["asset_tag"], "FL-14");

    let (status, saved) = call(
        &f,
        f.ws.admin_uuid,
        Method::PUT,
        &format!("{one}?force=true"),
        Some(numeric.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["field_schema"]["properties"]["drift"]["type"],
        "number"
    );
    assert_eq!(saved["record_count"], 2, "{saved}");

    // Sending the stored schema again, and the same outcome keys under a new
    // name, changes nothing a record holds, so no check runs: the records
    // that don't fit were let through by the forced save, not by this edit.
    let (status, renamed) = call(
        &f,
        f.ws.admin_uuid,
        Method::PUT,
        &one,
        Some(json!({
            "label": "Calibration check",
            "field_schema": numeric["field_schema"],
            "outcomes": [{ "key": "in_tolerance", "category": "done", "label": "Within tolerance" }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["label"], "Calibration check");

    // A schema that differs is still checked.
    let (status, refused) = call(
        &f,
        f.ws.admin_uuid,
        Method::PUT,
        &one,
        Some(json!({
            "field_schema": {
                "type": "object",
                "properties": { "drift": { "type": "boolean" } }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["invalid_count"], 2);
}

#[actix_web::test]
async fn the_record_form_list_carries_the_workspace_currency_for_staff_only() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let (status, picker) = call(&f, f.agent, Method::GET, "/api/assets/record-types", None).await;
    assert_eq!(status, StatusCode::OK, "{picker}");
    assert_eq!(picker["currency"], Value::Null);
    assert_eq!(picker["suggested_currency"], "USD");

    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                currency_code: Some(Some("AUD".to_string())),
                ..Default::default()
            },
        )
    })
    .expect("set the currency");
    let (_, picker) = call(&f, f.agent, Method::GET, "/api/assets/record-types", None).await;
    assert_eq!(picker["currency"], "AUD");

    let (status, _) = call(
        &f,
        f.ws.member_uuid,
        Method::GET,
        "/api/assets/record-types",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn the_record_form_list_carries_the_workspace_timezone() {
    let db = common::TestDb::new();
    let f = seed(&db);
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                default_timezone: Some("Australia/Sydney".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("set the timezone");
    let (status, picker) = call(&f, f.agent, Method::GET, "/api/assets/record-types", None).await;
    assert_eq!(status, StatusCode::OK, "{picker}");
    // The zone a timed record's day is read in.
    assert_eq!(picker["timezone"], "Australia/Sydney");
}
