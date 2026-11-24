//! What an asset's records add up to: counts, costs summed per currency and
//! never converted, the types used and the completed work per year. Staff
//! only, like the records it sums, and blind to other workspaces.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::NaiveDate;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, UpdateSiteSettings};
use backend::repository::{assets as asset_repo, site_settings};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_records_summary";

fn new_asset(pool: &TestPool, ws: i32, name: &str) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
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
                purchase_date: Some(NaiveDate::from_ymd_opt(2020, 1, 1).expect("a date")),
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
    .expect("create an asset")
    .id
}

/// Send `uri` as `viewer` in `ws`, signed in.
async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
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
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
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

async fn add_type(pool: &TestPool, ws: &WorkspaceSeed, slug: &str) {
    let uri = format!("/api/assets/record-types/catalogue/{slug}");
    let (status, body) = call(pool, ws, ws.admin_uuid, Method::POST, &uri, None).await;
    assert!(status.is_success(), "{status}: {body}");
}

/// A record of `slug` on `asset`, completed on its day unless `open`.
async fn record(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    asset: i32,
    slug: &str,
    day: &str,
    cost: Option<(&str, &str)>,
    open: bool,
) {
    let mut body = json!({ "record_type": slug, "occurred_on": day });
    if !open {
        body["completed_on"] = json!(day);
    }
    if let Some((amount, currency)) = cost {
        body["cost"] = json!({ "amount": amount, "currency": currency });
    }
    let uri = format!("/api/assets/{asset}/records");
    let (status, created) = call(pool, ws, ws.admin_uuid, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

struct Fixture {
    pool: TestPool,
    a: WorkspaceSeed,
    b: WorkspaceSeed,
    asset: i32,
}

/// Workspace A's asset with two years of work in three currencies and one
/// repair still open; workspace B with a record of its own.
async fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a, seeded.b);
    run_in_workspace(&pool, REF, a.workspace_id, |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                // Members see every asset, and still never see records.
                requester_asset_visibility: Some("all".to_string()),
                default_timezone: Some("UTC".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the settings");
    let asset = new_asset(&pool, a.workspace_id, "Camera 14");
    add_type(&pool, &a, "repair").await;
    add_type(&pool, &a, "inspection").await;
    record(
        &pool,
        &a,
        asset,
        "repair",
        "2023-05-03",
        Some(("1250.50", "AUD")),
        false,
    )
    .await;
    record(
        &pool,
        &a,
        asset,
        "repair",
        "2023-08-10",
        Some(("100", "AUD")),
        false,
    )
    .await;
    record(&pool, &a, asset, "inspection", "2023-11-01", None, false).await;
    record(
        &pool,
        &a,
        asset,
        "inspection",
        "2024-02-14",
        Some(("38.50", "USD")),
        false,
    )
    .await;
    record(
        &pool,
        &a,
        asset,
        "inspection",
        "2024-06-09",
        Some(("1250", "JPY")),
        false,
    )
    .await;
    record(
        &pool,
        &a,
        asset,
        "repair",
        "2025-01-20",
        Some(("10", "AUD")),
        true,
    )
    .await;

    let b_asset = new_asset(&pool, b.workspace_id, "Camera 99");
    add_type(&pool, &b, "repair").await;
    record(
        &pool,
        &b,
        b_asset,
        "repair",
        "2023-05-03",
        Some(("999", "AUD")),
        false,
    )
    .await;
    Fixture { pool, a, b, asset }
}

#[actix_web::test]
async fn costs_sum_per_currency_and_years_count_completed_work() {
    let db = common::TestDb::new();
    let f = seed(&db).await;
    let uri = format!("/api/assets/{}/records/summary", f.asset);
    let (status, body) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Every record, the open one too; JPY keeps no decimals.
    assert_eq!(body["count"], 6);
    assert_eq!(body["open"], 1);
    assert_eq!(
        body["costs"],
        json!([
            { "currency": "AUD", "amount": "1360.50", "amount_minor": 136050 },
            { "currency": "USD", "amount": "38.50", "amount_minor": 3850 },
            { "currency": "JPY", "amount": "1250", "amount_minor": 1250 },
        ])
    );
    assert_eq!(body["last_on"], "2024-06-09");
    assert_eq!(
        body["by_type"],
        json!([
            { "record_type": "inspection", "count": 3 },
            { "record_type": "repair", "count": 3 },
        ])
    );
    // The open 2025 repair sits above the years.
    assert_eq!(
        body["by_year"],
        json!({
            "2023": {
                "count": 3,
                "costs": [{ "currency": "AUD", "amount": "1350.50", "amount_minor": 135050 }],
            },
            "2024": {
                "count": 2,
                "costs": [
                    { "currency": "USD", "amount": "38.50", "amount_minor": 3850 },
                    { "currency": "JPY", "amount": "1250", "amount_minor": 1250 },
                ],
            },
        })
    );
}

#[actix_web::test]
async fn a_record_type_narrows_the_sums() {
    let db = common::TestDb::new();
    let f = seed(&db).await;
    let uri = format!("/api/assets/{}/records/summary?record_type=repair", f.asset);
    let (status, body) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 3);
    assert_eq!(body["open"], 1);
    assert_eq!(body["costs"][0]["amount"], "1360.50");
    assert_eq!(body["last_on"], "2023-08-10");
    assert_eq!(
        body["by_type"],
        json!([{ "record_type": "repair", "count": 3 }])
    );
    assert_eq!(body["by_year"]["2023"]["count"], 2);
    assert!(body["by_year"].get("2024").is_none(), "{body}");

    // A type nobody has: nothing to sum.
    let uri = format!(
        "/api/assets/{}/records/summary?record_type=calibration",
        f.asset
    );
    let (status, body) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 0);
    assert_eq!(body["costs"], json!([]));
    assert_eq!(body["by_year"], json!({}));
}

#[actix_web::test]
async fn requesters_and_other_workspaces_get_404() {
    let db = common::TestDb::new();
    let f = seed(&db).await;
    let uri = format!("/api/assets/{}/records/summary", f.asset);

    // A member shown every asset: records don't exist for them.
    let (status, body) = call(&f.pool, &f.a, f.a.member_uuid, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // Workspace B's admin can't reach A's asset, and B's own record never
    // counts toward A's.
    let (status, body) = call(&f.pool, &f.b, f.b.admin_uuid, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (_, body) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &uri, None).await;
    assert_eq!(body["costs"][0]["amount"], "1360.50");
}

#[actix_web::test]
async fn summary_is_not_taken_for_a_record_id() {
    let db = common::TestDb::new();
    let f = seed(&db).await;
    // The literal resolves to the summary rather than a record lookup.
    let uri = format!("/api/assets/{}/records/summary", f.asset);
    let (status, body) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("by_year").is_some(), "{body}");
    // And a record id still reaches its record.
    let list = format!("/api/assets/{}/records", f.asset);
    let (_, page) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &list, None).await;
    let id = page["records"][0]["id"].as_i64().expect("an id");
    let one = format!("/api/assets/{}/records/{id}", f.asset);
    let (status, record) = call(&f.pool, &f.a, f.a.admin_uuid, Method::GET, &one, None).await;
    assert_eq!(status, StatusCode::OK, "{record}");
    assert_eq!(record["id"], id);
}
