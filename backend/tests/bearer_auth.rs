//! Bearer-token (native/mobile) auth on the dual-auth `/api` surface.
//!
//! The mobile app authenticates with a session JWT in `Authorization: Bearer`
//! instead of the httpOnly `access_token` cookie. These prove the additive
//! bearer path holds the security invariants:
//!  - a genuine agent session JWT (scope "full") authenticates,
//!  - revoking the session revokes bearer access (the session check applies),
//!  - an SSE-scope JWT (which skips the session check) is rejected — it must
//!    not over-authorize on the agent API,
//!  - a missing credential yields a bare `WWW-Authenticate: Bearer` (RFC 6750),
//!    an invalid one yields `error="invalid_token"`,
//!  - `nsk_` personal API tokens still work (regression guard).
//!
//! The membership gate self-skips when no workspace context is resolved
//! (`enforce_workspace_membership` returns Ok with no Host/selection workspace),
//! so these exercise the auth path without tenant fixturing.

#![allow(clippy::expect_used)]

use actix_web::middleware::from_fn;
use actix_web::{test, web, App, HttpResponse};
use backend::middleware::api_token::dual_auth_middleware;
use backend::models::NewActiveSession;
use backend::utils::jwt::JwtUtils;

mod common;

async fn ping() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

/// App with a single protected `/api/ping` behind the real dual-auth middleware.
macro_rules! protected_app {
    ($pool:expr) => {
        test::init_service(
            App::new().app_data(web::Data::new($pool.clone())).service(
                web::scope("/api")
                    .wrap(from_fn(dual_auth_middleware))
                    .route("/ping", web::get().to(ping)),
            ),
        )
        .await
    };
}

/// Create an active session for `user` and mint the matching session JWT.
fn session_jwt(pool: &backend::db::Pool, user: &backend::models::User) -> (uuid::Uuid, String) {
    let session = backend::repository::active_sessions::create_session(
        &mut pool.get().expect("conn"),
        NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("bearer-test".into()),
            ip_address: None,
            user_agent: Some("nosdesk-mobile-test".into()),
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
        },
    )
    .expect("create active session");
    let token = JwtUtils::create_token(user, &session.session_id).expect("mint JWT");
    (session.session_id, token)
}

#[actix_web::test]
async fn session_jwt_authenticates_as_bearer() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let user = common::insert_user(&mut pool.get().expect("conn"), "Bearer Alice");
    let (_sid, token) = session_jwt(&pool, &user);

    let app = protected_app!(pool);
    let req = test::TestRequest::get()
        .uri("/api/ping")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "valid session JWT bearer should authenticate"
    );
}

#[actix_web::test]
async fn revoked_session_rejects_bearer() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let user = common::insert_user(&mut pool.get().expect("conn"), "Bearer Bob");
    let (sid, token) = session_jwt(&pool, &user);

    // Revoke the session — as logout / reuse-detection would.
    backend::repository::active_sessions::revoke_session_by_uuid(
        &mut pool.get().expect("conn"),
        &sid,
    )
    .expect("revoke session");

    let app = protected_app!(pool);
    let req = test::TestRequest::get()
        .uri("/api/ping")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    // The middleware rejects with an Error (rendered to a 401 at the edge).
    let err = test::try_call_service(&app, req)
        .await
        .err()
        .expect("revoked session must reject the bearer token");
    let resp = err.error_response();
    assert_eq!(resp.status(), 401);
    assert_eq!(
        resp.headers()
            .get("WWW-Authenticate")
            .and_then(|v| v.to_str().ok()),
        Some("Bearer error=\"invalid_token\""),
    );
}

#[actix_web::test]
async fn sse_scope_jwt_rejected_on_bearer() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let user = common::insert_user(&mut pool.get().expect("conn"), "Bearer Carol");
    // An SSE token (scope "sse") skips the session check; it must not ride the
    // agent API as a bearer.
    let sse = JwtUtils::create_sse_token(&user.uuid.to_string(), &user.platform_role, None)
        .expect("mint sse token");

    let app = protected_app!(pool);
    let req = test::TestRequest::get()
        .uri("/api/ping")
        .insert_header(("Authorization", format!("Bearer {sse}")))
        .to_request();
    let err = test::try_call_service(&app, req)
        .await
        .err()
        .expect("SSE-scope JWT must be rejected on the agent bearer path");
    assert_eq!(err.error_response().status(), 401);
}

#[actix_web::test]
async fn missing_credential_returns_bare_challenge() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);

    let app = protected_app!(pool);
    let req = test::TestRequest::get().uri("/api/ping").to_request();
    let err = test::try_call_service(&app, req)
        .await
        .err()
        .expect("missing credential must be rejected");
    let resp = err.error_response();
    assert_eq!(resp.status(), 401);
    assert_eq!(
        resp.headers()
            .get("WWW-Authenticate")
            .and_then(|v| v.to_str().ok()),
        Some("Bearer"),
        "no credential should get a bare Bearer challenge (no error code)",
    );
}

#[actix_web::test]
async fn invalid_jwt_bearer_returns_invalid_token() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);

    let app = protected_app!(pool);
    let req = test::TestRequest::get()
        .uri("/api/ping")
        .insert_header(("Authorization", "Bearer not-a-real-jwt"))
        .to_request();
    let err = test::try_call_service(&app, req)
        .await
        .err()
        .expect("invalid JWT must be rejected");
    let resp = err.error_response();
    assert_eq!(resp.status(), 401);
    assert_eq!(
        resp.headers()
            .get("WWW-Authenticate")
            .and_then(|v| v.to_str().ok()),
        Some("Bearer error=\"invalid_token\""),
    );
}

#[actix_web::test]
async fn nsk_api_token_still_authenticates() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let user = common::insert_user(&mut pool.get().expect("conn"), "Bearer Dave");
    let api_token = common::mint_api_token(&mut pool.get().expect("conn"), &user, "regression");

    let app = protected_app!(pool);
    let req = test::TestRequest::get()
        .uri("/api/ping")
        .insert_header(("Authorization", format!("Bearer {api_token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "nsk_ API tokens must keep working unchanged"
    );
}
