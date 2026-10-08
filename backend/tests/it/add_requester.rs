//! An agent or admin adds a requester (someone who files tickets) with no
//! password and no invitation: they sign in by a portal link. The rule on
//! passwords is checked before anything is saved, so a refused request leaves
//! no account behind.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
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

use crate::common;

async fn post_user(
    pool: &common::TestPool,
    ws: &common::WorkspaceSeed,
    body: Value,
) -> (StatusCode, Value) {
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
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), pool).expect("init search"));
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api").route("/users", web::post().to(backend::handlers::create_user)),
            ),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/api/users")
            .set_json(body)
            .to_request(),
    )
    .await;
    let status = resp.status();
    let bytes = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn accounts_with(pool: &common::TestPool, email: &str) -> i64 {
    use backend::schema::user_emails;
    user_emails::table
        .filter(user_emails::email.eq(email))
        .count()
        .get_result(&mut pool.get().expect("conn"))
        .expect("count")
}

#[actix_web::test]
async fn a_requester_is_added_without_a_password() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, body) = post_user(
        &pool,
        &ws,
        json!({
            "name": "Pat Caller",
            "email": "pat.caller@example.com",
            "role": "user",
            "send_invitation": false,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["invitation_sent"], json!(false));
    assert_eq!(accounts_with(&pool, "pat.caller@example.com"), 1);
}

/// Staff need a password or an invitation; a request with neither is refused
/// before anything is saved.
#[actix_web::test]
async fn staff_without_a_password_or_invitation_are_refused_and_not_saved() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, body) = post_user(
        &pool,
        &ws,
        json!({
            "name": "Sam Agent",
            "email": "sam.agent@example.com",
            "role": "technician",
            "send_invitation": false,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(accounts_with(&pool, "sam.agent@example.com"), 0);
}
