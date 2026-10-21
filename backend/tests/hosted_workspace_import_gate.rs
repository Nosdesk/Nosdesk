//! Hosted: the owner's workspace import routes aren't served. Imports into a
//! hosted workspace come from the control plane, so every
//! `/api/workspace/imports` route answers 404 to the workspace's own owner,
//! and nothing is created. The platform-admin routes still answer.
//!
//! A standalone binary rather than part of `tests/it`: the deployment mode
//! is process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test::{self as http_test, TestRequest};
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

async fn call(
    pool: &common::TestPool,
    workspace_id: i32,
    user: Uuid,
    platform_role: &str,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Owner".to_string(),
        email: "owner@example.com".to_string(),
        platform_role: platform_role.to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id,
        workspace_uuid: Uuid::now_v7(),
        slug: "hosted-import".into(),
        name: "Hosted Import".into(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(user, Some(corr)).with_workspace(workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::workspace_import::config)),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

#[actix_web::test]
async fn hosted_owners_get_404_from_the_import_routes() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let (workspace_id, owner) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "hosted-import", "Hosted Import");
        let owner = common::insert_plain_user(&mut conn, "Hosted Owner");
        (ws, owner)
    };
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::system("test:hosted_import_gate").with_workspace(workspace_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                workspace_id,
                owner,
                "owner",
                SeatWriteAuthority::ControlPlane,
            )
        })
        .expect("owner seat");
    }

    let job = Uuid::now_v7();
    let requests = [
        TestRequest::get().uri("/api/workspace/imports"),
        TestRequest::get().uri("/api/workspace/imports/latest"),
        TestRequest::post()
            .uri("/api/workspace/imports")
            .insert_header(("content-type", "multipart/form-data; boundary=x"))
            .set_payload("--x--\r\n"),
        TestRequest::get().uri(&format!("/api/workspace/imports/{job}")),
        TestRequest::post().uri(&format!("/api/workspace/imports/{job}/confirm")),
        TestRequest::post().uri(&format!("/api/workspace/imports/{job}/reset")),
        TestRequest::post().uri(&format!("/api/workspace/imports/{job}/invite")),
    ];
    for req in requests {
        let (status, body) = call(&pool, workspace_id, owner, "user", req).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["code"], "workspace_import.not_on_cloud");
    }

    let jobs: i64 = backend::schema::workspace_import_jobs::table
        .count()
        .get_result(&mut pool.get().expect("conn"))
        .expect("count");
    assert_eq!(jobs, 0);

    // The operator's routes answer on hosted.
    let (status, body) = call(
        &pool,
        workspace_id,
        Uuid::now_v7(),
        "platform_admin",
        TestRequest::get().uri(&format!("/api/admin/workspaces/{workspace_id}/imports")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, serde_json::json!([]));
}
