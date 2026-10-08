//! A workspace has one default SLA policy and one default working calendar.
//! Making another the default, when created or later, moves the default to it
//! instead of failing on the one-default rule.

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

async fn as_admin(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: http_test::TestRequest,
) -> ServiceResponse {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
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
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::sla::config)),
    )
    .await;
    http_test::call_service(&app, req.to_request()).await
}

/// The ids of the listed rows marked default.
async fn defaults(pool: &TestPool, ws: &WorkspaceSeed, kind: &str) -> Vec<i64> {
    let resp = as_admin(
        pool,
        ws,
        http_test::TestRequest::get().uri(&format!("/api/admin/sla/{kind}")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rows: Value = http_test::read_body_json(resp).await;
    rows.as_array()
        .expect("a list")
        .iter()
        .filter(|r| r["is_default"] == json!(true))
        .map(|r| r["id"].as_i64().expect("id"))
        .collect()
}

fn policy(name: &str, is_default: bool) -> Value {
    json!({
        "name": name,
        "target_response_minutes": 30,
        "target_resolution_minutes": 240,
        "is_default": is_default,
    })
}

fn calendar(name: &str, is_default: bool) -> Value {
    json!({
        "name": name,
        "timezone": "UTC",
        "schedule": { "mon": [["09:00", "17:00"]] },
        "is_default": is_default,
    })
}

#[actix_web::test]
async fn making_a_policy_the_default_moves_the_default() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let pool = db.runtime_pool(4);
    assert_eq!(
        defaults(&pool, a, "policies").await.len(),
        1,
        "the seeded default"
    );

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/policies")
            .set_json(policy("Fast lane", false)),
    )
    .await;
    let id = http_test::read_body_json::<Value, _>(created).await["id"]
        .as_i64()
        .expect("policy id");
    let made_default = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{id}"))
            .set_json(policy("Fast lane", true)),
    )
    .await;
    assert_eq!(made_default.status(), StatusCode::OK, "made the default");
    assert_eq!(defaults(&pool, a, "policies").await, [id]);

    let created_default = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/policies")
            .set_json(policy("Night shift", true)),
    )
    .await;
    assert_eq!(
        created_default.status(),
        StatusCode::CREATED,
        "created as the default"
    );
    let night = http_test::read_body_json::<Value, _>(created_default).await["id"]
        .as_i64()
        .expect("policy id");
    assert_eq!(defaults(&pool, a, "policies").await, [night]);
}

#[actix_web::test]
async fn making_a_calendar_the_default_moves_the_default() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let pool = db.runtime_pool(4);
    assert_eq!(
        defaults(&pool, a, "calendars").await.len(),
        1,
        "the seeded default"
    );

    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/calendars")
            .set_json(calendar("Support hours", false)),
    )
    .await;
    let id = http_test::read_body_json::<Value, _>(created).await["id"]
        .as_i64()
        .expect("calendar id");
    let made_default = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/calendars/{id}"))
            .set_json(calendar("Support hours", true)),
    )
    .await;
    assert_eq!(made_default.status(), StatusCode::OK, "made the default");
    assert_eq!(defaults(&pool, a, "calendars").await, [id]);

    let created_default = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/calendars")
            .set_json(calendar("Weekend cover", true)),
    )
    .await;
    assert_eq!(
        created_default.status(),
        StatusCode::CREATED,
        "created as the default"
    );
    let weekend = http_test::read_body_json::<Value, _>(created_default).await["id"]
        .as_i64()
        .expect("calendar id");
    assert_eq!(defaults(&pool, a, "calendars").await, [weekend]);
}
