//! `PUT /assets/{id}` keeps a model-linked asset's kind: the model sets
//! it, and this route never changes the model. The kind changes once the
//! model is cleared on `DELETE /assets/{id}/model`.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    search: Arc<SearchService>,
    _search_dir: tempfile::TempDir,
}

#[derive(diesel::QueryableByName)]
struct IdRow {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    id: i32,
}

#[derive(diesel::QueryableByName)]
struct KindRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    name: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    kind: String,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Integer>)]
    model_id: Option<i32>,
}

fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(
        "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
         VALUES ('laptop', 'Laptop', '{\"type\":\"object\",\"properties\":{}}', 20, true, 'it', $1),
                ('desktop', 'Desktop', '{\"type\":\"object\",\"properties\":{}}', 21, true, 'it', $1)
         ON CONFLICT (workspace_id, slug) DO NOTHING",
    )
    .bind::<diesel::sql_types::Integer, _>(ws.workspace_id)
    .execute(&mut conn)
    .expect("seed kinds");
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    Fixture {
        pool,
        ws,
        search,
        _search_dir: search_dir,
    }
}

/// A laptop in the fixture's workspace, linked to a new model when `linked`.
fn laptop(f: &Fixture, name: &str, linked: bool) -> i32 {
    let mut conn = f.pool.get().expect("conn");
    let model_id: Option<i32> = linked.then(|| {
        diesel::sql_query(
            "WITH m AS (INSERT INTO manufacturers (name, workspace_id) VALUES ('Lenovo', $1) RETURNING id)
             INSERT INTO asset_models (manufacturer_id, name, kind, workspace_id)
             SELECT id, 'ThinkPad T14', 'laptop', $1 FROM m RETURNING id",
        )
        .bind::<diesel::sql_types::Integer, _>(f.ws.workspace_id)
        .get_result::<IdRow>(&mut conn)
        .expect("seed model")
        .id
    });
    diesel::sql_query(
        "INSERT INTO assets (name, kind, model_id, workspace_id)
         VALUES ($1, 'laptop', $2, $3) RETURNING id",
    )
    .bind::<diesel::sql_types::Text, _>(name)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Integer>, _>(model_id)
    .bind::<diesel::sql_types::Integer, _>(f.ws.workspace_id)
    .get_result::<IdRow>(&mut conn)
    .expect("seed asset")
    .id
}

fn stored(f: &Fixture, id: i32) -> KindRow {
    diesel::sql_query("SELECT name, kind, model_id FROM assets WHERE id = $1")
        .bind::<diesel::sql_types::Integer, _>(id)
        .get_result(&mut f.pool.get().expect("conn"))
        .expect("read asset")
}

/// Call `uri` as the workspace admin.
async fn call(f: &Fixture, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let admin = f.ws.admin_uuid;
    let claims = Claims {
        sub: admin.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
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
    let actor = ActorContext::user(admin, Some(corr)).with_workspace(f.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(f.pool.clone()))
            .app_data(web::Data::new(f.search.clone()))
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
    let body = serde_json::from_slice(&http_test::read_body(resp).await).unwrap_or_default();
    (status, body)
}

#[actix_web::test]
async fn a_model_linked_asset_keeps_its_kind_until_the_model_is_cleared() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let linked = laptop(&f, "Laptop-01", true);
    let uri = format!("/api/assets/{linked}");

    // A different kind is refused, even with model_id: null in the body,
    // since this route never changes the model.
    for body in [
        json!({ "kind": "desktop" }),
        json!({ "kind": "desktop", "model_id": null }),
    ] {
        let (status, resp) = call(&f, Method::PUT, &uri, Some(body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{resp}");
        assert_eq!(
            resp["error"].as_str(),
            Some("This asset takes its kind from its model; change or clear the model instead"),
            "{resp}"
        );
    }
    let row = stored(&f, linked);
    assert_eq!(row.kind, "laptop");
    assert!(row.model_id.is_some());

    // The same kind alongside another change goes through.
    let (status, resp) = call(
        &f,
        Method::PUT,
        &uri,
        Some(json!({ "kind": "laptop", "name": "Laptop-01 renamed" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(stored(&f, linked).name, "Laptop-01 renamed");

    // Once the model is cleared, the kind can change.
    let (status, resp) = call(&f, Method::DELETE, &format!("{uri}/model"), None).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    let (status, resp) = call(&f, Method::PUT, &uri, Some(json!({ "kind": "desktop" }))).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(stored(&f, linked).kind, "desktop");
}

#[actix_web::test]
async fn an_unlinked_asset_changes_kind() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let plain = laptop(&f, "Laptop-02", false);
    let (status, resp) = call(
        &f,
        Method::PUT,
        &format!("/api/assets/{plain}"),
        Some(json!({ "kind": "desktop" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(stored(&f, plain).kind, "desktop");
}
