//! On hosted, a requester signed in to the portal can use it. The agent app
//! admits staff seats only there; the portal admits any member of the
//! workspace its origin serves.
//!
//! A standalone binary rather than part of `tests/it`: deployment mode is
//! process-wide environment.

#![allow(clippy::expect_used)]

mod common;

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
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
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
        let claims = Claims {
            sub: user.to_string(),
            name: "Customer".to_string(),
            email: String::new(),
            platform_role: "user".to_string(),
            scope: PORTAL_SCOPE.to_string(),
            sid: Some(uuid::Uuid::new_v4().to_string()),
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
