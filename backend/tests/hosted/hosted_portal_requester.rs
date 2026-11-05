//! On hosted, a requester signed in to the portal can use it. The agent app
//! admits staff seats only there; the portal admits any member of the
//! workspace its origin serves.
//!
//! Part of the hosted binary rather than `tests/it`: the deployment mode is
//! process-wide environment.

use crate::common;

use actix_web::test::TestRequest;
use actix_web::HttpMessage as _;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::authorize_portal_request;
use backend::middleware::cookie_auth::{require_workspace_membership, PORTAL_SCOPE};
use backend::models::Claims;
use backend::repository::workspaces::find_by_id;

fn status_of(err: &actix_web::Error) -> u16 {
    err.as_response_error().status_code().as_u16()
}

#[test]
fn a_requester_passes_the_portal_gate_on_hosted() {
    crate::hosted_env();
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = &seeded.a;

    let origin = {
        let mut conn = pool.get().expect("conn");
        let w = find_by_id(&mut conn, ws.workspace_id)
            .expect("workspace lookup")
            .expect("workspace exists");
        WorkspaceContext {
            workspace_id: w.id,
            workspace_uuid: w.uuid,
            slug: w.slug,
            name: w.name,
            custom_domain: w.custom_domain,
            organisation_id: w.organisation_id,
        }
    };
    let authorize = |user: uuid::Uuid| {
        let now = chrono::Utc::now();
        // A portal token names its session, whose record holds its scope.
        let sid = backend::repository::active_sessions::create_session(
            &mut pool.get().expect("conn"),
            backend::models::NewActiveSession {
                user_uuid: user,
                device_name: None,
                ip_address: None,
                user_agent: None,
                location: None,
                expires_at: (now + chrono::Duration::hours(1)).naive_utc(),
                oidc_id_token: None,
                confirmed_at: None,
                provider_amr: None,
            },
        )
        .expect("session")
        .session_id;
        let claims = Claims {
            sub: user.to_string(),
            name: "Customer".to_string(),
            email: String::new(),
            platform_role: "user".to_string(),
            scope: PORTAL_SCOPE.to_string(),
            sid: Some(sid.to_string()),
            workspace_uuid: Some(origin.workspace_uuid),
            exp: (now + chrono::Duration::hours(1)).timestamp() as usize,
            iat: now.timestamp() as usize,
        };
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(origin.clone());
        authorize_portal_request(&req, &mut pool.get().expect("conn"), &claims)
    };

    assert!(
        authorize(ws.member_uuid).is_ok(),
        "a requester uses the portal"
    );
    assert!(authorize(ws.admin_uuid).is_ok(), "so does staff");
    let stranger = authorize(seeded.b.member_uuid).expect_err("another workspace's requester");
    assert_eq!(status_of(&stranger), 403);

    // The agent app's gate still admits staff seats only.
    let agent_app = require_workspace_membership(
        &mut pool.get().expect("conn"),
        ws.workspace_id,
        ws.member_uuid,
    )
    .expect_err("a requester on the hosted agent app");
    assert_eq!(status_of(&agent_app), 403);
}

/// Hosted portals have no password sign-in, so their sign-in page doesn't
/// link to one.
#[actix_web::test]
async fn the_hosted_portal_offers_no_password_sign_in() {
    crate::hosted_env();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let w = find_by_id(&mut pool.get().expect("conn"), seeded.a.workspace_id)
        .expect("workspace lookup")
        .expect("workspace exists");
    let req = TestRequest::default().to_http_request();
    req.extensions_mut().insert(WorkspaceContext {
        workspace_id: w.id,
        workspace_uuid: w.uuid,
        slug: w.slug,
        name: w.name,
        custom_domain: w.custom_domain,
        organisation_id: w.organisation_id,
    });
    let resp =
        backend::handlers::portal_sso::sso_info(req, actix_web::web::Data::new(pool.clone())).await;
    let body: serde_json::Value = serde_json::from_slice(
        &actix_web::body::to_bytes(resp.into_body())
            .await
            .expect("body"),
    )
    .expect("json");
    assert_eq!(body["password"], false);
}
