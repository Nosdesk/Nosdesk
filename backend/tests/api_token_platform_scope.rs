//! Integration test for `api_tokens.is_platform_scoped` + the
//! `PlatformScope` extractor (M5 product-side handoff Task 1).
//!
//! Spins up an actix-test server with one platform-scoped handler
//! and one regular handler, mints both kinds of API token against
//! a sandbox DB, and exercises:
//!
//!   - existing user-bound token still authenticates (default false)
//!   - platform-scoped token is accepted on a platform-gated route
//!   - user-bound token on the same route returns 403
//!   - missing auth on the platform-gated route returns 401
//!
//! The point isn't to test platform-token-mint plumbing (that's the
//! operator CLI's job, future control-plane work); it's to verify
//! the middleware faithfully surfaces the column and the extractor
//! enforces it.

#![allow(clippy::expect_used)]

use actix_web::{web, App, HttpResponse};

use backend::middleware::api_token::{dual_auth_middleware, PlatformScope};

mod common;

/// Handler gated on platform scope. Successful extraction means the
/// caller's API token had `is_platform_scoped = true`. The body just
/// echoes a marker so the test can assert "we reached the handler"
/// distinct from a 403 short-circuit.
async fn platform_only_handler(_: PlatformScope) -> HttpResponse {
    HttpResponse::Ok().body("platform-ok")
}

/// Open handler — succeeds for any authenticated request, used to
/// confirm user-bound tokens still authenticate (the
/// "existing behaviour unchanged" acceptance).
async fn any_auth_handler() -> HttpResponse {
    HttpResponse::Ok().body("any-ok")
}

#[actix_web::test]
async fn platform_scope_extractor_gates_routes_correctly() {
    common::ensure_test_keyring();
    std::env::set_var("ENVIRONMENT", "test");

    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let user = common::insert_user(&mut pool.get().expect("conn"), "PlatformProbe");

    let platform_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &user, "platform", true);
    let user_token = common::mint_api_token(&mut pool.get().expect("conn"), &user, "user", false);

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/x")
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route("/platform", web::get().to(platform_only_handler))
                    .route("/open", web::get().to(any_auth_handler)),
            )
    });

    let client = awc::Client::new();

    // 1. Platform token + platform route: 200
    let resp = client
        .get(srv.url("/x/platform"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .send()
        .await
        .expect("send platform→platform");
    assert_eq!(
        resp.status(),
        200,
        "platform token should reach platform route"
    );

    // 2. User token + platform route: 403
    let resp = client
        .get(srv.url("/x/platform"))
        .insert_header(("Authorization", format!("Bearer {user_token}")))
        .send()
        .await
        .expect("send user→platform");
    assert_eq!(
        resp.status(),
        403,
        "user-bound token must be rejected by PlatformScope (got {:?})",
        resp.status()
    );

    // 3. No auth on platform route: 401 (dual_auth_middleware short-
    //    circuits before the extractor ever runs).
    let resp = client
        .get(srv.url("/x/platform"))
        .send()
        .await
        .expect("send unauth→platform");
    assert_eq!(
        resp.status(),
        401,
        "unauth on platform route should be 401 from dual_auth_middleware (got {:?})",
        resp.status()
    );

    // 4. User token + open route: 200 (existing-behaviour-unchanged check).
    let resp = client
        .get(srv.url("/x/open"))
        .insert_header(("Authorization", format!("Bearer {user_token}")))
        .send()
        .await
        .expect("send user→open");
    assert_eq!(
        resp.status(),
        200,
        "user-bound token must still authenticate on a non-platform route"
    );

    // 5. Platform token + open route: 200. Platform tokens are
    //    cross-workspace by design; they should still pass auth on
    //    non-platform endpoints (the extractor decides gating, not
    //    the middleware itself).
    let resp = client
        .get(srv.url("/x/open"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .send()
        .await
        .expect("send platform→open");
    assert_eq!(
        resp.status(),
        200,
        "platform token should also authenticate on non-platform routes"
    );
}
