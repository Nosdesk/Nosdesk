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
use backend::startup::{build_request_limiters, build_state, configure_app};

fn test_redis() -> String {
    std::env::var("TEST_REDIS_URL").expect(
        "TEST_REDIS_URL not set. Bring up the dev stack (redis published on \
         127.0.0.1:63799) or set TEST_REDIS_URL; CI provides it.",
    )
}

/// Stands in for the app-level workspace-context middleware, which
/// `init_service` on the route tree skips: the workspace a request's origin
/// resolved, named by a header.
async fn resolve_workspace(
    req: ServiceRequest,
    next: actix_web::middleware::Next<impl actix_web::body::MessageBody + 'static>,
) -> Result<actix_web::dev::ServiceResponse, actix_web::Error> {
    use actix_web::HttpMessage as _;
    if let Some(id) = req
        .headers()
        .get("x-test-ws")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i32>().ok())
    {
        req.extensions_mut()
            .insert(backend::extractors::WorkspaceContext {
                workspace_id: id,
                workspace_uuid: uuid::Uuid::new_v4(),
                slug: format!("ws{id}"),
                name: format!("Workspace {id}"),
                custom_domain: None,
                organisation_id: None,
            });
    }
    next.call(req)
        .await
        .map(actix_web::dev::ServiceResponse::map_into_boxed_body)
}

/// A client address no earlier run has counted: a fresh IPv6 /64.
fn fresh_client() -> std::net::SocketAddr {
    let n = uuid::Uuid::new_v4().as_u128();
    let ip = std::net::Ipv6Addr::new(0x2001, 0x0db8, (n >> 16) as u16, n as u16, 0, 0, 0, 1);
    std::net::SocketAddr::new(ip.into(), 4000)
}

/// Statuses of `count` GETs to `path` from `client`, each for the workspace
/// `ws(i)` names (none when `None`).
macro_rules! statuses {
    ($app:expr, $path:expr, $client:expr, $count:expr, $ws:expr) => {{
        let ws: &dyn Fn(usize) -> Option<i32> = &$ws;
        let mut out: Vec<u16> = Vec::new();
        for i in 0..$count {
            let mut req = test::TestRequest::get().uri($path).peer_addr($client);
            if let Some(id) = ws(i) {
                req = req.insert_header(("x-test-ws", id.to_string()));
            }
            let status = match test::try_call_service($app, req.to_request()).await {
                Ok(r) => r.status(),
                Err(e) => e.as_response_error().status_code(),
            };
            out.push(status.as_u16());
        }
        out
    }};
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
    // No registry sync: it would fetch https://nosdesk.com/registry.
    std::env::set_var("NOSDESK_REGISTRY_URL", "");

    // A development config pointed at the test Redis. Reuses the injectable
    // `from_source` so this doesn't touch process env — which means the explicit
    // ENVIRONMENT=development is required here, since an unset value now assumes
    // production (fail-closed) and would demand FRONTEND_URL etc.
    let config = Config::from_source(&|k| match k {
        "ENVIRONMENT" => Some("development".to_string()),
        "JWT_SECRET" => Some("0123456789abcdef0123456789abcdef01".to_string()),
        "REDIS_URL" => Some(redis.clone()),
        // The floor, so the limiter checks below stay short.
        "RATE_LIMIT_PER_MINUTE" => Some("30".to_string()),
        _ => None,
    })
    .expect("test config builds");

    // Keep the Tantivy index out of the source tree.
    let search_dir =
        std::env::temp_dir().join(format!("nosdesk-smoke-search-{}", std::process::id()));
    std::env::set_var("SEARCH_INDEX_PATH", &search_dir);

    // The real limiters, as `build_server` builds them. A request with no
    // client address (every request below until the limiter checks) is not
    // counted.
    let limiters = build_request_limiters(&config).expect("limiters build");
    let (state, _background_tasks) =
        build_state(&config, pool.clone(), limiters).expect("build_state succeeds");

    // Stop the periodic scheduler before any job's first tick so nothing runs
    // against the sandbox DB for the life of the test.
    state.scheduler_shutdown.cancel();

    let ws = backend::middleware::WorkspaceContextConfig::initialise(&pool)
        .expect("workspace context initialises (sandbox has the bootstrap workspace)");

    let app = test::init_service(
        App::new()
            .configure(|cfg| configure_app(cfg, &state, &pool, &ws, &config))
            .wrap(actix_web::middleware::from_fn(resolve_workspace)),
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
    common::seat(&mut pool.get().expect("conn"), 1, user.uuid, "owner");
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

    // Self-hosted `/login` is the SPA fallback, as any other page: the server
    // starts no sign-in there (hosted's is `tests/hosted_login.rs`).
    for path in ["/login", "/login/", "/login?redirect=%2Ftickets"] {
        let resp = test::call_service(&app, test::TestRequest::get().uri(path).to_request()).await;
        assert!(
            resp.headers().get("location").is_none(),
            "GET {path} => {} with a Location",
            resp.status()
        );
        assert!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|t| t.starts_with("text/html")),
            "GET {path} serves the shell"
        );
    }

    // A request actix can't extract, and a path no API route matches, answer
    // with the `{error, code}` envelope. An unknown path stays behind its
    // scope's auth: without a credential it is still a 401.
    let full = common::mint_api_token(&mut pool.get().expect("conn"), &user, "probe-full");
    let bearer = format!("Bearer {full}");
    for (req, want, code) in [
        (
            test::TestRequest::post()
                .uri("/api/auth/login")
                .insert_header(("Content-Type", "application/json"))
                .set_payload("{not json"),
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
        ),
        (
            test::TestRequest::get().uri("/api/public/no-such-route"),
            StatusCode::NOT_FOUND,
            "RESOURCE_NOT_FOUND",
        ),
        (
            test::TestRequest::get().uri("/api/auth/no-such-route"),
            StatusCode::NOT_FOUND,
            "RESOURCE_NOT_FOUND",
        ),
        (
            test::TestRequest::get().uri("/api/no-such-route"),
            StatusCode::UNAUTHORIZED,
            "AUTH_REQUIRED",
        ),
        (
            test::TestRequest::get()
                .uri("/api/no-such-route")
                .insert_header(("Authorization", bearer.clone())),
            StatusCode::NOT_FOUND,
            "RESOURCE_NOT_FOUND",
        ),
        // Retired: the unattached upload sweep replaced it.
        (
            test::TestRequest::post()
                .uri("/api/files/cleanup-temp")
                .insert_header(("Authorization", bearer.clone())),
            StatusCode::NOT_FOUND,
            "RESOURCE_NOT_FOUND",
        ),
        (
            test::TestRequest::get().uri("/api/collaboration/no-such-route"),
            StatusCode::UNAUTHORIZED,
            "AUTH_REQUIRED",
        ),
        (
            test::TestRequest::get()
                .uri("/api/collaboration/no-such-route")
                .insert_header(("Authorization", bearer.clone())),
            StatusCode::NOT_FOUND,
            "RESOURCE_NOT_FOUND",
        ),
    ] {
        let req = req.to_request();
        let what = format!("{} {}", req.method(), req.path());
        let resp = match test::try_call_service(&app, req).await {
            Ok(r) => r.into_parts().1,
            Err(e) => e.error_response(),
        };
        assert_eq!(resp.status(), want, "{what}");
        let body = actix_web::body::to_bytes(resp.into_body())
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| panic!("{what}: body is not JSON: {body:?}"));
        assert!(json["error"].is_string(), "{what}: {json}");
        assert_eq!(json["code"], code, "{what}: {json}");
    }

    // The limiters each scope is wired to, at the configured 30 a minute (the
    // per client bucket across workspaces is 120).
    let limited = |s: &[u16]| s.iter().filter(|&&c| c == 429).count();

    // `/api/auth` is one bucket per client, whatever workspace a request is
    // for: sign-in targets accounts, which span workspaces.
    let client = fresh_client();
    let auth = statuses!(&app, "/api/auth/no-such-route", client, 31, |i| {
        Some(if i % 2 == 0 { 1 } else { 2 })
    });
    assert_eq!(limited(&auth[..30]), 0, "{auth:?}");
    assert_eq!(auth[30], 429, "the 31st across two workspaces: {auth:?}");

    // So is the agent app's host, which resolves no workspace.
    let client = fresh_client();
    let auth = statuses!(&app, "/api/auth/no-such-route", client, 31, |_| None);
    assert_eq!(auth[30], 429, "{auth:?}");

    // `/api/public` is split per workspace: one tenant spent leaves another.
    let client = fresh_client();
    let form = statuses!(&app, "/api/public/no-such-route", client, 31, |_| Some(1));
    assert_eq!(limited(&form[..30]), 0, "{form:?}");
    assert_eq!(form[30], 429, "{form:?}");
    let other = statuses!(&app, "/api/public/no-such-route", client, 1, |_| Some(2));
    assert_eq!(other, [404], "another workspace has its own quota");

    // With no workspace resolved, `/api/public` still counts at 30, not 120.
    let client = fresh_client();
    let form = statuses!(&app, "/api/public/no-such-route", client, 31, |_| None);
    assert_eq!(limited(&form[..30]), 0, "{form:?}");
    assert_eq!(form[30], 429, "{form:?}");
}
