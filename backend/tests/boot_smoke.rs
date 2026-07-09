//! Boot-path smoke test.
//!
//! Builds the *real* application state (`startup::build_state`) and the *real*
//! route tree (`startup::configure_app`), then drives requests through them via
//! `init_service`. This is the automated form of the manual boot check done
//! during the bootstrap refactor: it proves the composition wires up — every
//! `app_data` is present and every scope is mounted — which the compiler can't
//! (a dropped `app_data` is a runtime 500, not a type error).
//!
//! Needs a sandbox Postgres (via `common::TestDb`) and `TEST_REDIS_URL` (the
//! Yjs cache + `/readiness` require Redis). The sandbox DB isolates the
//! background listeners/scheduler `build_state` spawns; the scheduler is also
//! cancelled immediately so no periodic job fires.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::ServiceRequest;
use actix_web::{http::StatusCode, test, App};
use backend::config::Config;
use backend::startup::{build_state, configure_app};

fn test_redis() -> String {
    std::env::var("TEST_REDIS_URL").expect(
        "TEST_REDIS_URL not set. Bring up the dev stack (redis published on \
         127.0.0.1:63799) or set TEST_REDIS_URL; CI provides it.",
    )
}

/// A permissive limiter for the smoke test; the real per-IP keying is exercised
/// elsewhere. Just needs to be a live `Limiter` so `configure_app` can register
/// the `web::Data<Limiter>` the scopes resolve by type.
fn limiter(redis: &str, prefix: &'static str) -> actix_limitation::Limiter {
    actix_limitation::Limiter::builder(redis)
        .key_by(move |_: &ServiceRequest| Some(format!("{prefix}:smoke")))
        .limit(10_000)
        .period(std::time::Duration::from_secs(60))
        .build()
        .expect("build test limiter")
}

#[actix_web::test]
async fn boot_wires_state_and_routes() {
    common::ensure_test_keyring();

    let db = common::TestDb::new();
    let pool = db.pool();
    let redis = test_redis();
    // The /readiness handler pings Redis via `get_redis_url()`, which reads the
    // REDIS_URL process env — point it at the test Redis for this binary.
    std::env::set_var("REDIS_URL", &redis);

    // A development config pointed at the test Redis. Reuses the injectable
    // `from_source` so this doesn't touch process env — which means the explicit
    // ENVIRONMENT=development is required here, since an unset value now assumes
    // production (fail-closed) and would demand FRONTEND_URL etc.
    let config = Config::from_source(
        &|k| match k {
            "ENVIRONMENT" => Some("development".to_string()),
            "JWT_SECRET" => Some("0123456789abcdef0123456789abcdef01".to_string()),
            "REDIS_URL" => Some(redis.clone()),
            _ => None,
        },
        true,
    )
    .expect("test config builds");

    // Keep the Tantivy index out of the source tree.
    let search_dir =
        std::env::temp_dir().join(format!("nosdesk-smoke-search-{}", std::process::id()));
    std::env::set_var("SEARCH_INDEX_PATH", &search_dir);

    let (state, _background_tasks) = build_state(
        &config,
        pool.clone(),
        limiter(&redis, "public"),
        limiter(&redis, "auth"),
        limiter(&redis, "felogs"),
    )
    .expect("build_state succeeds");

    // Stop the periodic scheduler before any job's first tick so nothing runs
    // against the sandbox DB for the life of the test.
    state.scheduler_shutdown.cancel();

    let ws = backend::middleware::WorkspaceContextConfig::initialise(&pool)
        .expect("workspace context initialises (sandbox has the bootstrap workspace)");

    let app = test::init_service(
        App::new().configure(|cfg| configure_app(cfg, &state, &pool, &ws, &config)),
    )
    .await;

    // Public liveness.
    let health =
        test::call_service(&app, test::TestRequest::get().uri("/health").to_request()).await;
    assert!(
        health.status().is_success(),
        "GET /health => {}",
        health.status()
    );

    // Readiness (checks Redis + DB pool).
    let ready = test::call_service(
        &app,
        test::TestRequest::get().uri("/readiness").to_request(),
    )
    .await;
    assert!(
        ready.status().is_success(),
        "GET /readiness => {}",
        ready.status()
    );

    // An authenticated /api route must be mounted and its app_data present: with
    // no credentials the auth middleware rejects with 401. A missing app_data
    // would instead surface as a 500 here, which is exactly the wiring bug this
    // test guards against. (The middleware short-circuits with an Err, so use
    // try_call_service and read the response-error status.)
    let resp = test::try_call_service(
        &app,
        test::TestRequest::get().uri("/api/users").to_request(),
    )
    .await;
    let status = match resp {
        Ok(r) => r.status(),
        Err(e) => e.as_response_error().status_code(),
    };
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET /api/users => {status}"
    );

    // Tenant file serving must sit inside the scope-enforced tree: a
    // deliberately-narrowed API token maps to `Full` on /api/files/* and must
    // be denied (403) by token_scope, not slip through to the file handler.
    // This drives the real route tree, so it guards the /api/files scope wiring
    // itself — the bug was these routes living outside any scope-wrapped scope.
    let user = common::insert_user(&mut pool.get().expect("conn"), "Files Scope Probe");
    let narrowed = common::mint_scoped_api_token(
        &mut pool.get().expect("conn"),
        &user,
        "probe",
        &["tickets:read"],
    );
    let resp = test::try_call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/files/tickets/does-not-exist.png")
            .insert_header(("Authorization", format!("Bearer {narrowed}")))
            .to_request(),
    )
    .await;
    let status = match resp {
        Ok(r) => r.status(),
        Err(e) => e.as_response_error().status_code(),
    };
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "narrowed token on /api/files/* must be scope-denied => {status}"
    );
}
