//! The single-origin agent app names its workspace in the selection header,
//! which only the authenticated routes resolve. Its branding comes from
//! `GET /api/workspace/branding` under the request's workspace; the public
//! route, with no workspace resolved, reads without creating a row.
//!
//! The public and member routes return only what pages display; the admin
//! route returns the full settings, to admins only.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
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

/// What `/api/branding` and `/api/workspace/branding` may return. Written out
/// here, not imported, so a field added to the response fails this test.
const PUBLIC_KEYS: [&str; 6] = [
    "app_name",
    "favicon_url",
    "logo_light_url",
    "logo_url",
    "primary_color",
    "updated_at",
];

fn keys(body: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = body
        .as_object()
        .unwrap_or_else(|| panic!("expected a JSON object, got {body}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn workspace_context(seed: &common::WorkspaceSeed) -> WorkspaceContext {
    WorkspaceContext {
        workspace_id: seed.workspace_id,
        workspace_uuid: seed.workspace_uuid,
        slug: seed.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    }
}

/// GET `uri` with the workspace the Host resolved (if any) and the signed-in
/// user (if any) in the request, as the middleware leaves them.
async fn get(
    pool: &common::TestPool,
    workspace: Option<WorkspaceContext>,
    user: Option<Uuid>,
    uri: &str,
) -> (StatusCode, serde_json::Value) {
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                if let Some(ws) = &workspace {
                    req.extensions_mut().insert(ws.clone());
                }
                if let Some(uuid) = user {
                    let now = chrono::Utc::now().timestamp();
                    req.extensions_mut().insert(Claims {
                        sub: uuid.to_string(),
                        name: "User".to_string(),
                        email: "user@example.com".to_string(),
                        platform_role: "user".to_string(),
                        scope: "full".to_string(),
                        sid: None,
                        workspace_uuid: None,
                        exp: (now + 3600) as usize,
                        iat: now as usize,
                    });
                    let corr = Uuid::now_v7();
                    let mut actor = ActorContext::user(uuid, Some(corr));
                    if let Some(ws) = &workspace {
                        actor = actor.with_workspace(ws.workspace_id);
                    }
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor));
                }
                srv.call(req)
            })
            .route(
                "/api/branding",
                web::get().to(backend::handlers::branding::get_public_branding),
            )
            .service(web::scope("/api").configure(backend::handlers::branding::config)),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

#[actix_web::test]
async fn the_public_route_returns_only_what_pages_display() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    set_primary_color(&pool, seeded.a.workspace_id, "#2563eb");

    let (status, body) = get(
        &pool,
        Some(workspace_context(&seeded.a)),
        None,
        "/api/branding",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["primary_color"], "#2563eb");
    assert_eq!(keys(&body), PUBLIC_KEYS);
}

#[actix_web::test]
async fn the_public_route_with_no_workspace_returns_only_what_pages_display() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.runtime_pool(2);

    // No workspace resolved, as at an origin that names none: the built-in
    // branding.
    let (status, body) = get(&pool, None, None, "/api/branding").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["app_name"], "Nosdesk");
    assert_eq!(keys(&body), PUBLIC_KEYS);
}

#[actix_web::test]
async fn the_member_route_returns_only_what_pages_display() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);

    let (status, body) = get(
        &pool,
        Some(workspace_context(&seeded.a)),
        Some(seeded.a.member_uuid),
        "/api/workspace/branding",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(keys(&body), PUBLIC_KEYS);
}

#[actix_web::test]
async fn the_admin_route_returns_the_full_settings_to_admins_only() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    let workspace = workspace_context(&seeded.a);

    let (status, body) = get(
        &pool,
        Some(workspace.clone()),
        Some(seeded.a.admin_uuid),
        "/api/admin/branding/config",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let admin_keys = keys(&body);
    for key in PUBLIC_KEYS.iter().chain(&[
        "signature_default",
        "channel_auto_ack_enabled",
        "channel_auto_ack_template",
        "email_security_note_enabled",
        "email_security_note_template",
    ]) {
        assert!(admin_keys.iter().any(|k| k == key), "missing {key}");
    }

    let (status, _) = get(
        &pool,
        Some(workspace),
        Some(seeded.a.member_uuid),
        "/api/admin/branding/config",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
