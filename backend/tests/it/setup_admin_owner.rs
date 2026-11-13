//! First-run setup makes its administrator the owner of the bootstrap
//! workspace, so the import its finish screen offers, which only the owner
//! may run, is open to them.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test::{self as http_test, TestRequest};
use actix_web::{web, App, HttpMessage};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::services::admin_setup::{create_initial_admin, InitialAdminInput};
use backend::sync::actor::{ActorContext, BOOTSTRAP_WORKSPACE_ID};

use crate::common::TestDb;

#[actix_web::test]
async fn the_setup_administrator_owns_the_workspace_and_may_import_into_it() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);

    let (admin, email) = create_initial_admin(
        &mut pool.get().expect("conn"),
        InitialAdminInput {
            name: "Ada Admin",
            email: "ada@example.com",
            password_hash: "$2b$12$abcdefghijklmnopqrstuuv0123456789abcdefghijklmnopqrstu",
        },
    )
    .expect("create the first administrator");

    let membership = backend::repository::workspaces::membership(
        &mut pool.get().expect("conn"),
        BOOTSTRAP_WORKSPACE_ID,
        admin.uuid,
    )
    .expect("read membership")
    .expect("a membership");
    assert_eq!(membership.role, "owner");

    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: admin.uuid.to_string(),
        name: admin.name.clone(),
        email: email.email.clone(),
        platform_role: admin.platform_role.clone(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
        file_path: None,
    };
    let workspace = WorkspaceContext {
        workspace_id: BOOTSTRAP_WORKSPACE_ID,
        workspace_uuid: Uuid::now_v7(),
        slug: "default".to_string(),
        name: "Workspace".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(admin.uuid, Some(corr)).with_workspace(BOOTSTRAP_WORKSPACE_ID);
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

    let resp = http_test::call_service(
        &app,
        TestRequest::get()
            .uri("/api/workspace/imports")
            .to_request(),
    )
    .await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the workspace's imports: {}",
        String::from_utf8_lossy(&body)
    );
}
