//! Layer-3 boot test: boot the *real* HTTP server via `startup::build_server`
//! on an ephemeral port and drive it with a real HTTP client.
//!
//! Unlike `boot_smoke` (which calls `init_service` on `configure_app`), this
//! exercises the full `run` / `build_server` composition — keyring, DB init,
//! partition provisioning, seeds, background workers — AND the app-level
//! middleware wraps (TracingLogger, CORS, SecurityHeaders, CSRF,
//! WorkspaceContext) that `init_service` on the ServiceConfig skips.
//!
//! Needs a sandbox Postgres (via `common::TestDb`, a migrated-template clone,
//! so `build_server`'s migrations are a no-op) and `TEST_REDIS_URL`.

#![allow(clippy::expect_used)]

mod common;

use std::net::TcpListener;

use backend::config::Config;
use backend::startup::build_server;

fn test_redis() -> String {
    std::env::var("TEST_REDIS_URL").expect(
        "TEST_REDIS_URL not set. Bring up the dev stack (redis on 127.0.0.1:63799) \
         or set TEST_REDIS_URL; CI provides it.",
    )
}

#[actix_web::test]
async fn real_server_boots_and_serves() {
    let db = common::TestDb::new();
    let redis = test_redis();

    // `build_server` is the real composition root: it reads these from the
    // process env. Point them at the sandbox DB + test redis. This is the only
    // test in its binary, so the set_var calls can't race other tests.
    std::env::set_var("DATABASE_URL", db.url());
    // The sandbox role is a superuser in CI/dev, so it can run the (idempotent)
    // partition-provisioning DDL that MIGRATION_DATABASE_URL drives.
    std::env::set_var("MIGRATION_DATABASE_URL", db.url());
    std::env::set_var("REDIS_URL", &redis);
    std::env::set_var(
        "MFA_KEK_V1",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    let uploads = std::env::temp_dir().join(format!("nosdesk-l3-uploads-{}", std::process::id()));
    std::env::set_var("NOSDESK_UPLOAD_DIR", &uploads);
    let search = std::env::temp_dir().join(format!("nosdesk-l3-search-{}", std::process::id()));
    std::env::set_var("SEARCH_INDEX_PATH", &search);

    let config = Config::from_source(
        &|k| match k {
            // Explicit dev label: the mock getter bypasses process env, so the
            // `.cargo/config.toml` ENVIRONMENT=development doesn't reach here, and
            // an unset ENVIRONMENT now assumes production (which would demand
            // FRONTEND_URL etc.). This boots a real server for a smoke test.
            "ENVIRONMENT" => Some("development".to_string()),
            "JWT_SECRET" => Some("0123456789abcdef0123456789abcdef01".to_string()),
            "REDIS_URL" => Some(redis.clone()),
            _ => None,
        },
        true,
    )
    .expect("test config builds");

    // Ephemeral port; hand the pre-bound listener to build_server (the whole
    // point of the run/build_server split).
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let built = build_server(config, listener)
        .await
        .expect("build_server boots");

    // Serve in the background; keep the handle + shutdown token for teardown.
    let handle = built.server.handle();
    let scheduler = built.scheduler_shutdown.clone();
    let server_task = actix_web::rt::spawn(built.server);

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // Liveness through the real server + the full middleware stack.
    let health = client
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("GET /health");
    assert_eq!(health.status().as_u16(), 200, "GET /health");
    // The SecurityHeaders app-level wrap ran — this is the coverage init_service
    // can't give, since ServiceConfig can't carry wraps.
    assert_eq!(
        health
            .headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff"),
        "SecurityHeaders wrap should have set X-Content-Type-Options"
    );

    // Readiness (real DB pool + Redis).
    let ready = client
        .get(format!("{base}/readiness"))
        .send()
        .await
        .expect("GET /readiness");
    assert_eq!(ready.status().as_u16(), 200, "GET /readiness");

    // An authenticated /api route is mounted and its app_data is present, so an
    // uncredentialed request is rejected (401), not a missing-app_data 500.
    let users = client
        .get(format!("{base}/api/users"))
        .send()
        .await
        .expect("GET /api/users");
    assert_eq!(users.status().as_u16(), 401, "GET /api/users");

    // Graceful-shutdown drain signal: once begin_shutdown() is called,
    // /readiness must report 503 (so the LB drains this instance) while /health
    // stays up. This is the app-level piece of the shutdown sequence.
    backend::handlers::health::begin_shutdown();
    let draining = client
        .get(format!("{base}/readiness"))
        .send()
        .await
        .expect("GET /readiness (draining)");
    assert_eq!(
        draining.status().as_u16(),
        503,
        "readiness should report draining after begin_shutdown()"
    );
    let alive = client
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("GET /health (draining)");
    assert_eq!(
        alive.status().as_u16(),
        200,
        "liveness stays up while draining"
    );

    // Teardown: stop the background scheduler + the server.
    scheduler.cancel();
    handle.stop(false).await;
    let _ = server_task.await;
}
