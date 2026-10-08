//! On hosted, the scheduler status is the instance's, shared by every
//! workspace on it, so only a platform admin reads it; a workspace admin gets
//! 403. Self-hosted is unchanged (`tests/it/scheduler_status.rs`).
//!
//! A standalone binary rather than part of `tests/it`: the deployment mode is
//! process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::models::Claims;

fn claims(user: Uuid, platform_role: &str) -> Claims {
    Claims {
        sub: user.to_string(),
        name: "Someone".to_string(),
        email: "someone@example.com".to_string(),
        platform_role: platform_role.to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

#[actix_web::test]
async fn on_hosted_only_a_platform_admin_reads_the_scheduler_status() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let operator = common::insert_plain_user(&mut pool.get().expect("conn"), "Operator");
    let workspace = WorkspaceContext {
        workspace_id: seeded.workspace_id,
        workspace_uuid: seeded.workspace_uuid,
        slug: seeded.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };

    for (who, as_claims, expected) in [
        ("workspace admin", claims(seeded.admin_uuid, "user"), 403),
        ("platform admin", claims(operator, "platform_admin"), 200),
    ] {
        let workspace = workspace.clone();
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(
                    backend::services::scheduler::status_registry(),
                ))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(as_claims.clone());
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::scheduler::config)),
        )
        .await;
        let resp = http_test::call_service(
            &app,
            http_test::TestRequest::get()
                .uri("/api/admin/scheduler/status")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), expected, "{who}");
    }
}
