//! An SLA policy's priority filter is one of the five priorities. Saving one
//! with anything else is refused, so a typo can't leave a policy that quietly
//! matches no ticket; a legacy name is saved as the priority it means.

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

fn policy(priority: Value) -> Value {
    json!({
        "name": "Fast lane",
        "target_response_minutes": 30,
        "target_resolution_minutes": 240,
        "priority_filter": priority,
    })
}

#[actix_web::test]
async fn an_sla_policy_targets_only_a_real_priority() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let pool = db.runtime_pool(4);
    let create = |body: Value| {
        http_test::TestRequest::post()
            .uri("/api/admin/sla/policies")
            .set_json(body)
    };

    let refused = as_admin(&pool, a, create(policy(json!("critical")))).await;
    assert_eq!(
        refused.status(),
        StatusCode::BAD_REQUEST,
        "an unknown priority"
    );

    for (sent, saved) in [
        (json!("urgent"), json!("urgent")),
        (json!("none"), json!("none")),
        (json!("normal"), json!("medium")),
        (json!(null), json!(null)),
    ] {
        let resp = as_admin(&pool, a, create(policy(sent.clone()))).await;
        assert_eq!(resp.status(), StatusCode::CREATED, "{sent}");
        let body = http_test::read_body_json::<Value, _>(resp).await;
        assert_eq!(body["priority_filter"], saved, "{sent}");
    }

    let id = {
        let resp = as_admin(&pool, a, create(policy(json!("high")))).await;
        http_test::read_body_json::<Value, _>(resp).await["id"]
            .as_i64()
            .expect("policy id")
    };
    let update = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{id}"))
            .set_json(policy(json!("Hgh"))),
    )
    .await;
    assert_eq!(
        update.status(),
        StatusCode::BAD_REQUEST,
        "update with a typo"
    );
}

/// A policy saved before the check, with a filter naming no priority, can
/// still be saved with that filter unchanged (the app resends the whole policy
/// when it changes one field); changing it to another such value is refused.
#[actix_web::test]
async fn an_unchanged_stored_filter_saves_again() {
    use diesel::prelude::*;

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = &seeded.a;
    let pool = db.runtime_pool(4);
    let created = as_admin(
        &pool,
        a,
        http_test::TestRequest::post()
            .uri("/api/admin/sla/policies")
            .set_json(policy(json!("high"))),
    )
    .await;
    let id = http_test::read_body_json::<Value, _>(created).await["id"]
        .as_i64()
        .expect("policy id") as i32;
    {
        use backend::schema::sla_policies;
        let mut conn = db.pool_with_size(1).get().expect("conn");
        diesel::update(sla_policies::table.find(id))
            .set(sla_policies::priority_filter.eq("critical"))
            .execute(&mut conn)
            .expect("store a legacy filter");
    }
    let mut renamed = policy(json!("critical"));
    renamed["name"] = json!("Faster lane");
    let resp = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{id}"))
            .set_json(renamed),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the stored filter, unchanged"
    );
    let body = http_test::read_body_json::<Value, _>(resp).await;
    assert_eq!(body["priority_filter"], "critical");
    assert_eq!(body["name"], "Faster lane");

    let resp = as_admin(
        &pool,
        a,
        http_test::TestRequest::patch()
            .uri(&format!("/api/admin/sla/policies/{id}"))
            .set_json(policy(json!("severe"))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "a new bad value");
}
