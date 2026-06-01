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

use std::sync::Once;

use actix_web::{web, App, HttpResponse};
use diesel::prelude::*;

use backend::middleware::api_token::{dual_auth_middleware, PlatformScope};
use backend::models::{NewApiToken, User};
use backend::repository::api_tokens::{get_token_prefix, hash_token};

mod common;

/// Set JWT_SECRET before the lazy_static resolves. The crypto
/// keyring isn't touched by these tests (no MFA / no at-rest
/// encryption code path), so the legacy collaboration_ws-style
/// MFA_KEK_V1 setup isn't needed here.
fn install_env() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-32-characters-min-for-tests");
        }
        std::env::set_var("ENVIRONMENT", "test");
    });
}

/// Mint an API token row directly via Diesel so the test can set
/// `is_platform_scoped` explicitly without going through the (user-
/// scope-only) `create_api_token` repository helper. Returns the raw
/// token string so the test can put it in the Authorization header.
fn mint_token(
    conn: &mut diesel::pg::PgConnection,
    user: &User,
    name: &str,
    is_platform_scoped: bool,
) -> String {
    use backend::schema::api_tokens;
    // Token format matches the repo's `generate_api_token`: nsk_ prefix
    // + opaque random body. The exact body doesn't matter — only that
    // hashing it matches what get_valid_api_token looks up.
    let raw = format!("nsk_test_{}", uuid::Uuid::new_v4().simple());
    let token_hash = hash_token(&raw);
    let token_prefix = get_token_prefix(&raw);

    let new_token = NewApiToken {
        token_hash,
        token_prefix,
        user_uuid: user.uuid,
        name: name.to_string(),
        scopes: Some(vec![Some("full".to_string())]),
        created_by: user.uuid,
        expires_at: None,
        is_platform_scoped,
    };

    diesel::insert_into(api_tokens::table)
        .values(&new_token)
        .execute(conn)
        .expect("insert api_token");
    raw
}

/// Pins `app.workspace_id = 1` on every newly-acquired connection so
/// audit triggers don't trip the NOT-NULL on `audit_log.workspace_id`.
/// Mirrors the customizer in `tests/common/mod.rs` (private there).
#[derive(Debug)]
struct WorkspaceGuc;

impl diesel::r2d2::CustomizeConnection<diesel::pg::PgConnection, diesel::r2d2::Error>
    for WorkspaceGuc
{
    fn on_acquire(&self, conn: &mut diesel::pg::PgConnection) -> Result<(), diesel::r2d2::Error> {
        use diesel::RunQueryDsl;
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(diesel::r2d2::Error::QueryError)?;
        Ok(())
    }
}

fn build_pool(url: &str) -> backend::db::Pool {
    use diesel::r2d2::{ConnectionManager, Pool};
    let mgr = ConnectionManager::<diesel::pg::PgConnection>::new(url);
    Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(WorkspaceGuc))
        .build(mgr)
        .expect("build backend Pool")
}

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
    install_env();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let user = common::insert_user(&mut pool.get().expect("conn"), "PlatformProbe");

    let platform_token = mint_token(&mut pool.get().expect("conn"), &user, "platform", true);
    let user_token = mint_token(&mut pool.get().expect("conn"), &user, "user", false);

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
