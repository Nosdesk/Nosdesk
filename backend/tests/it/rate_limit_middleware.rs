//! The request rate limiter: a spent quota is a 429 logged by its bucket,
//! never by the client's address, and an unreachable Redis lets requests
//! through with one throttled warning. Tests that count need Redis
//! (`TEST_REDIS_URL`) and are skipped without it.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use actix_web::{test, web, App, HttpResponse};
use backend::middleware::{RateLimit, RequestLimiter};
use backend::utils::client_ip::{from_service_request, limit_bucket};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

/// The middleware's log events, as field maps.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

struct Fields<'a>(&'a mut BTreeMap<String, String>);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if !event
            .metadata()
            .target()
            .ends_with("middleware::rate_limit")
        {
            return;
        }
        let mut fields = BTreeMap::new();
        event.record(&mut Fields(&mut fields));
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(fields);
    }
}

impl Capture {
    /// Captures this thread's events until the guard drops. The actix test
    /// runtime is single-threaded, so the requests run under it.
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(self.clone()));
        tracing::callsite::rebuild_interest_cache();
        guard
    }

    fn events(&self) -> Vec<BTreeMap<String, String>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

fn keyed_limiter(name: &'static str, redis: &str, prefix: String, limit: usize) -> RequestLimiter {
    RequestLimiter::build(name, redis, limit, Duration::from_secs(60), move |req| {
        from_service_request(req).map(|ip| format!("{prefix}:{}", limit_bucket(ip)))
    })
    .expect("limiter")
}

fn post() -> test::TestRequest {
    test::TestRequest::post()
        .uri("/")
        .peer_addr("203.0.113.77:4000".parse().expect("addr"))
}

#[actix_web::test]
async fn a_spent_quota_is_429_logged_without_the_client_address() {
    let Ok(url) = std::env::var("TEST_REDIS_URL") else {
        return;
    };
    let capture = Capture::default();
    let _guard = capture.install();
    // A fresh bucket each run, keyed the way startup keys the real ones.
    let bucket = format!("logtest{}", uuid::Uuid::new_v4().simple());
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(keyed_limiter(
                "test",
                &url,
                bucket.clone(),
                1,
            )))
            .wrap(RateLimit)
            .route("/", web::post().to(HttpResponse::Ok)),
    )
    .await;

    assert_eq!(
        test::call_service(&app, post().to_request()).await.status(),
        200
    );
    let refused = test::call_service(&app, post().to_request()).await;
    assert_eq!(refused.status(), 429);
    let retry_after: u64 = refused
        .headers()
        .get("Retry-After")
        .expect("Retry-After")
        .to_str()
        .expect("ascii")
        .parse()
        .expect("seconds");
    assert!((1..=61).contains(&retry_after), "{retry_after}");
    let body: serde_json::Value = test::read_body_json(refused).await;
    assert_eq!(body["code"], "RATE_LIMITED");
    assert_eq!(body["retry_after"], retry_after);

    let lines: Vec<String> = capture
        .events()
        .into_iter()
        .filter_map(|f| f.get("message").cloned())
        .filter(|m| m.contains(&bucket))
        .collect();
    assert_eq!(lines.len(), 1, "one 429 line: {lines:?}");
    let line = &lines[0];
    assert!(!line.contains("203.0.113.77"), "{line}");
    let hashed = limit_bucket("203.0.113.77".parse().expect("ip"));
    assert!(line.ends_with(&format!("{bucket}:{hashed}")), "{line}");
}

#[actix_web::test]
async fn an_unreachable_redis_lets_requests_through_with_one_warning() {
    let capture = Capture::default();
    let _guard = capture.install();
    // Nothing listens on port 1, so every count fails to connect.
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(keyed_limiter(
                "unreachable-test",
                "redis://127.0.0.1:1",
                "unreachable".into(),
                1,
            )))
            .wrap(RateLimit)
            .route("/", web::post().to(HttpResponse::Ok)),
    )
    .await;

    for _ in 0..3 {
        assert_eq!(
            test::call_service(&app, post().to_request()).await.status(),
            200
        );
    }

    let warnings: Vec<_> = capture
        .events()
        .into_iter()
        .filter(|f| {
            f.get("message")
                .is_some_and(|m| m.contains("unreachable-test"))
        })
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].get("count").map(String::as_str), Some("1"));
}

#[actix_web::test]
async fn a_request_with_no_key_is_not_counted() {
    let Ok(url) = std::env::var("TEST_REDIS_URL") else {
        return;
    };
    // A zero quota refuses any request that is counted.
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(
                RequestLimiter::build("nokey-test", &url, 0, Duration::from_secs(60), |_| None)
                    .expect("limiter"),
            ))
            .wrap(RateLimit)
            .route("/", web::post().to(HttpResponse::Ok)),
    )
    .await;
    assert_eq!(
        test::call_service(&app, post().to_request()).await.status(),
        200
    );
}

/// The public limiter's shape: a bucket per workspace and client, and a
/// looser one per client across every workspace. One client spending its
/// quota at one tenant still reaches another, until the shared bucket is
/// spent too.
#[actix_web::test]
async fn one_workspace_spending_its_quota_leaves_another_reachable() {
    use backend::middleware::rate_limit::{client_key, workspace_client_key};

    let Ok(url) = std::env::var("TEST_REDIS_URL") else {
        return;
    };
    let prefix = format!("wstest{}", uuid::Uuid::new_v4().simple());
    let (ws_prefix, ip_prefix) = (prefix.clone(), prefix);
    let limiter = RequestLimiter::build("ws-test", &url, 1, Duration::from_secs(60), move |req| {
        workspace_client_key(&ws_prefix, req)
    })
    .and_then(|l| {
        l.with_bucket(&url, 3, Duration::from_secs(60), move |req| {
            client_key(&ip_prefix, req)
        })
    })
    .expect("limiter");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(limiter))
            .wrap(RateLimit)
            .wrap(actix_web::middleware::from_fn(resolve_workspace))
            .route("/", web::post().to(HttpResponse::Ok)),
    )
    .await;
    let to = |ws: Option<i32>| {
        let req = post();
        match ws {
            Some(ws) => req.insert_header(("x-ws", ws.to_string())),
            None => req,
        }
        .to_request()
    };

    assert_eq!(test::call_service(&app, to(Some(1))).await.status(), 200);
    assert_eq!(
        test::call_service(&app, to(Some(1))).await.status(),
        429,
        "workspace 1's quota is spent"
    );
    assert_eq!(
        test::call_service(&app, to(Some(2))).await.status(),
        200,
        "workspace 2 has its own"
    );
    assert_eq!(
        test::call_service(&app, to(None)).await.status(),
        200,
        "no workspace: a bucket of its own"
    );
    assert_eq!(
        test::call_service(&app, to(None)).await.status(),
        429,
        "no workspace is still counted at the per-workspace limit"
    );
    assert_eq!(
        test::call_service(&app, to(Some(3))).await.status(),
        429,
        "the shared bucket is spent"
    );
}

/// Stands in for the workspace-context middleware: the workspace a request is
/// for, named by a header.
async fn resolve_workspace(
    req: actix_web::dev::ServiceRequest,
    next: actix_web::middleware::Next<impl actix_web::body::MessageBody>,
) -> Result<actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>, actix_web::Error> {
    use actix_web::HttpMessage as _;
    if let Some(id) = req
        .headers()
        .get("x-ws")
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
    next.call(req).await
}
