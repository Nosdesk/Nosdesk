//! The workspace currency (`site_settings.currency_code`) on
//! `/api/admin/asset-settings`: unset by default with a suggestion from the
//! default locale, set and cleared by workspace admins only, refused for a
//! code that isn't a current ISO 4217 currency, and saved apart from the
//! requester visibility on the same endpoint.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_settings_currency";
const URI: &str = "/api/admin/asset-settings";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    other: WorkspaceSeed,
    agent: Uuid,
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
        )
    })
    .expect("add the agent");
    Fixture {
        pool,
        ws: seeded.a,
        other: seeded.b,
        agent,
    }
}

/// Call the asset settings as `viewer` in `ws`.
async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    viewer: Uuid,
    method: Method,
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
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::asset_settings::config)),
    )
    .await;
    let mut req = http_test::TestRequest::default().method(method).uri(URI);
    if let Some(body) = body {
        req = req.set_json(body);
    }
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, body)
}

async fn get(f: &Fixture, ws: &WorkspaceSeed, viewer: Uuid) -> Value {
    let (status, body) = call(&f.pool, ws, viewer, Method::GET, None).await;
    assert_eq!(status, StatusCode::OK, "GET: {body}");
    body
}

async fn put(f: &Fixture, viewer: Uuid, body: Value) -> (StatusCode, Value) {
    call(&f.pool, &f.ws, viewer, Method::PUT, Some(body)).await
}

#[actix_web::test]
async fn admins_set_and_clear_the_workspace_currency() {
    let db = common::TestDb::new();
    let f = seed(&db);

    // Unset by default, with the default locale's suggestion and the codes
    // an admin can pick.
    let initial = get(&f, &f.ws, f.ws.admin_uuid).await;
    assert_eq!(initial["currency"], Value::Null);
    assert_eq!(initial["suggested_currency"], json!("USD"));
    let codes = initial["currencies"].as_array().expect("currencies");
    assert!(codes.contains(&json!("AUD")) && codes.contains(&json!("JPY")));
    assert!(
        !codes.contains(&json!("HRK")),
        "withdrawn codes aren't offered"
    );

    // An admin sets it; the code is read without regard to case.
    let (status, saved) = put(&f, f.ws.admin_uuid, json!({ "currency": "aud" })).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["currency"], json!("AUD"));
    assert_eq!(
        get(&f, &f.ws, f.ws.admin_uuid).await["currency"],
        json!("AUD")
    );

    // Another workspace keeps its own.
    assert_eq!(
        get(&f, &f.other, f.other.admin_uuid).await["currency"],
        Value::Null
    );

    // A body without `currency` leaves it; `null` clears it.
    let (status, _) = put(&f, f.ws.admin_uuid, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        get(&f, &f.ws, f.ws.admin_uuid).await["currency"],
        json!("AUD")
    );
    let (status, cleared) = put(&f, f.ws.admin_uuid, json!({ "currency": null })).await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["currency"], Value::Null);
    assert_eq!(
        get(&f, &f.ws, f.ws.admin_uuid).await["currency"],
        Value::Null
    );
}

#[actix_web::test]
async fn only_admins_change_it_and_only_to_a_current_iso_code() {
    let db = common::TestDb::new();
    let f = seed(&db);

    for viewer in [f.agent, f.ws.member_uuid] {
        let (status, _) = put(&f, viewer, json!({ "currency": "EUR" })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{viewer}");
        let (status, _) = call(&f.pool, &f.ws, viewer, Method::GET, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{viewer}");
    }

    // `ZZZ` passes the column's shape check; the ISO table refuses it. A
    // withdrawn code and a malformed one are refused too.
    for code in ["ZZZ", "HRK", "EURO", "", "12$"] {
        let (status, body) = put(&f, f.ws.admin_uuid, json!({ "currency": code })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code}: {body}");
    }
    assert_eq!(
        get(&f, &f.ws, f.ws.admin_uuid).await["currency"],
        Value::Null
    );
}

#[actix_web::test]
async fn currency_and_requester_visibility_save_apart() {
    let db = common::TestDb::new();
    let f = seed(&db);

    let (status, _) = put(
        &f,
        f.ws.admin_uuid,
        json!({ "requester_visibility": "none" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Setting the currency leaves the visibility.
    let (status, saved) = put(&f, f.ws.admin_uuid, json!({ "currency": "EUR" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["requester_visibility"], json!("none"));
    assert_eq!(saved["currency"], json!("EUR"));

    // Setting the visibility leaves the currency.
    let (status, saved) = put(
        &f,
        f.ws.admin_uuid,
        json!({ "requester_visibility": "all" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["requester_visibility"], json!("all"));
    assert_eq!(saved["currency"], json!("EUR"));

    // A refused currency saves nothing, the valid visibility beside it
    // included.
    let (status, _) = put(
        &f,
        f.ws.admin_uuid,
        json!({ "requester_visibility": "none", "currency": "ZZZ" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let now = get(&f, &f.ws, f.ws.admin_uuid).await;
    assert_eq!(now["requester_visibility"], json!("all"));
    assert_eq!(now["currency"], json!("EUR"));
}
