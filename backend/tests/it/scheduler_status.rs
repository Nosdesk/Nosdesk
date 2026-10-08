//! Self-hosted: a workspace admin reads the scheduler status (the instance is
//! theirs); a plain member doesn't. Hosted is `tests/hosted_scheduler_status.rs`.

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::models::Claims;

use crate::common;

fn claims(user: Uuid) -> Claims {
    Claims {
        sub: user.to_string(),
        name: "Someone".to_string(),
        email: "someone@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

#[actix_web::test]
async fn self_hosted_a_workspace_admin_reads_the_scheduler_status() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let workspace = WorkspaceContext {
        workspace_id: seeded.workspace_id,
        workspace_uuid: seeded.workspace_uuid,
        slug: seeded.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };

    for (who, user, expected) in [
        ("workspace admin", seeded.admin_uuid, 200),
        ("member", seeded.member_uuid, 403),
    ] {
        let workspace = workspace.clone();
        let as_claims = claims(user);
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
