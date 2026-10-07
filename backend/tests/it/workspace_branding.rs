//! The single-origin agent app names its workspace in the selection header,
//! which only the authenticated routes resolve. Its branding comes from
//! `GET /api/workspace/branding` under the request's workspace; the public
//! route, with no workspace resolved, reads without creating a row.

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, UpdateSiteSettings};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common;

const REF: &str = "test:workspace_branding";

fn set_primary_color(pool: &common::TestPool, ws: i32, color: &str) {
    run_in_workspace(pool, REF, ws, |c| {
        backend::repository::site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                primary_color: Some(Some(color.to_string())),
                ..Default::default()
            },
        )
    })
    .expect("set primary colour");
}

#[actix_web::test]
async fn a_member_gets_the_selected_workspaces_branding() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    set_primary_color(&pool, seeded.a.workspace_id, "#2563eb");
    set_primary_color(&pool, seeded.b.workspace_id, "#16a34a");

    let member = seeded.a.member_uuid;
    let claims = Claims {
        sub: member.to_string(),
        name: "Member".to_string(),
        email: "member@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: seeded.a.workspace_id,
        workspace_uuid: seeded.a.workspace_uuid,
        slug: seeded.a.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(member, Some(corr)).with_workspace(seeded.a.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::branding::config)),
    )
    .await;

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::get()
            .uri("/api/workspace/branding")
            .to_request(),
    )
    .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let body: serde_json::Value = http_test::read_body_json(resp).await;
    assert_eq!(body["primary_color"], "#2563eb");
}

#[test]
fn a_read_with_no_workspace_finds_nothing_and_creates_nothing() {
    let db = common::TestDb::new();
    let pool = db.runtime_pool(1);
    let mut conn = pool.get().expect("conn");
    // No workspace pinned, as on the public route at an origin that names none.
    assert!(matches!(
        backend::repository::site_settings::find_site_settings(&mut conn),
        Ok(None)
    ));
    // Creating the row on read is what failed there.
    assert!(backend::repository::site_settings::get_site_settings(&mut conn).is_err());
}
