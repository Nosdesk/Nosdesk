//! Field order for asset kinds and the user field schema over REST: a kind
//! keeps the order it is saved with, a schema saved without an order keeps
//! the stored one, and the user field schema carries its order as a root
//! `field_order` beside the schema a 1.1 client reads and sends unchanged.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

async fn call(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let viewer = ws.admin_uuid;
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Admin".to_string(),
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
            .service(
                web::scope("/api")
                    .configure(backend::handlers::asset_kinds::config)
                    .configure(backend::handlers::assets::config)
                    .configure(backend::handlers::users::config),
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

fn props(keys: &[&str]) -> Value {
    let properties: serde_json::Map<String, Value> = keys
        .iter()
        .map(|k| (k.to_string(), json!({ "type": "string" })))
        .collect();
    json!({ "type": "object", "properties": properties })
}

#[actix_web::test]
async fn a_kind_keeps_its_field_order() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, kind) = call(
        &pool,
        &ws,
        Method::POST,
        "/api/admin/asset-kinds",
        Some(json!({
            "slug": "camera",
            "label": "Camera",
            "attribute_schema": props(&["mount", "lens", "body", "zoom"]),
            "field_order": ["zoom", "mount", "gone", "zoom", "body"],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{kind}");
    assert_eq!(
        kind["field_order"],
        json!(["zoom", "mount", "body", "lens"])
    );
    let id = kind["id"].as_i64().expect("id");
    let uri = format!("/api/admin/asset-kinds/{id}");

    let (_, read) = call(&pool, &ws, Method::GET, &uri, None).await;
    assert_eq!(
        read["field_order"],
        json!(["zoom", "mount", "body", "lens"])
    );

    // A new order alone.
    let (status, kind) = call(
        &pool,
        &ws,
        Method::PUT,
        &uri,
        Some(json!({ "field_order": ["lens", "body", "mount", "zoom"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kind}");
    assert_eq!(
        kind["field_order"],
        json!(["lens", "body", "mount", "zoom"])
    );

    // A schema sent without an order (a 1.1 client) keeps the stored one:
    // a removed key leaves it, a new one follows.
    let (status, kind) = call(
        &pool,
        &ws,
        Method::PUT,
        &uri,
        Some(json!({ "attribute_schema": props(&["mount", "lens", "zoom", "flash"]) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{kind}");
    assert_eq!(
        kind["field_order"],
        json!(["lens", "mount", "zoom", "flash"])
    );

    // A label edit leaves the order alone.
    let (_, kind) = call(
        &pool,
        &ws,
        Method::PUT,
        &uri,
        Some(json!({ "label": "Cinema camera" })),
    )
    .await;
    assert_eq!(
        kind["field_order"],
        json!(["lens", "mount", "zoom", "flash"])
    );

    // The picker list carries it too.
    let (_, kinds) = call(&pool, &ws, Method::GET, "/api/asset-kinds", None).await;
    let listed = kinds
        .as_array()
        .expect("kinds")
        .iter()
        .find(|k| k["slug"] == "camera")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        listed["field_order"],
        json!(["lens", "mount", "zoom", "flash"])
    );
}

#[actix_web::test]
async fn the_default_user_field_schema_saves_unchanged() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, default) = call(&pool, &ws, Method::GET, "/api/admin/user-fields", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        default["properties"]["gender"]["x-allow-custom"],
        json!(true)
    );
    assert_eq!(
        default["field_order"],
        json!(["birthday", "gender", "office_location"])
    );

    // Sent back as read, field_order included.
    let (status, saved) = call(
        &pool,
        &ws,
        Method::PUT,
        "/api/admin/user-fields",
        Some(default.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved, default);

    // Sent back as a 1.1 client would, without field_order.
    let mut bare = default.clone();
    bare.as_object_mut().expect("object").remove("field_order");
    let (status, saved) = call(
        &pool,
        &ws,
        Method::PUT,
        "/api/admin/user-fields",
        Some(bare),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved, default);
}

#[actix_web::test]
async fn the_user_field_schema_keeps_its_order() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let mut schema = props(&["year_level", "house", "campus"]);
    schema["field_order"] = json!(["campus", "year_level", "house"]);
    let (status, saved) = call(
        &pool,
        &ws,
        Method::PUT,
        "/api/admin/user-fields",
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["field_order"],
        json!(["campus", "year_level", "house"])
    );

    let (_, read) = call(&pool, &ws, Method::GET, "/api/admin/user-fields", None).await;
    assert_eq!(
        read["field_order"],
        json!(["campus", "year_level", "house"])
    );
    assert!(
        read["properties"].get("field_order").is_none(),
        "the order isn't a field"
    );

    // Without an order the stored one stays; a new field follows.
    let (_, saved) = call(
        &pool,
        &ws,
        Method::PUT,
        "/api/admin/user-fields",
        Some(props(&["year_level", "house", "campus", "bus_route"])),
    )
    .await;
    assert_eq!(
        saved["field_order"],
        json!(["campus", "year_level", "house", "bus_route"])
    );

    let mut bad = props(&["house"]);
    bad["field_order"] = json!("house");
    let (status, _) = call(&pool, &ws, Method::PUT, "/api/admin/user-fields", Some(bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
