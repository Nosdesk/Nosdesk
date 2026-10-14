//! A request the rate limiter refuses is logged by its bucket, never by the
//! client's address. actix-limitation writes its key into the 429 warning,
//! which lands in the always-allowlisted `message`, so the key itself must
//! carry no address. Needs Redis (`TEST_REDIS_URL`); skipped without it.

#![allow(clippy::expect_used)]

use std::sync::{Mutex, Once};
use std::time::Duration;

use actix_limitation::{Limiter, RateLimiter};
use actix_web::{test, web, App, HttpResponse};
use backend::utils::client_ip::{from_service_request, limit_bucket};

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps actix-limitation's log records. It logs through the `log` facade,
/// which the server bridges into tracing.
struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if record.target().starts_with("actix_limitation") {
            LINES
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

fn capture_log() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        log::set_logger(&Capture).expect("no other logger in the test binary");
        log::set_max_level(log::LevelFilter::Warn);
    });
}

#[actix_web::test]
async fn a_refused_request_is_logged_without_the_client_address() {
    let Ok(url) = std::env::var("TEST_REDIS_URL") else {
        return;
    };
    capture_log();
    // A fresh bucket each run, keyed the way startup keys the real ones.
    let bucket = format!("logtest{}", uuid::Uuid::new_v4().simple());
    let prefix = bucket.clone();
    let limiter = Limiter::builder(url)
        .key_by(move |req| {
            from_service_request(req).map(|ip| format!("{prefix}:{}", limit_bucket(ip)))
        })
        .limit(1)
        .period(Duration::from_secs(60))
        .build()
        .expect("limiter");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(limiter))
            .wrap(RateLimiter::default())
            .route("/", web::post().to(HttpResponse::Ok)),
    )
    .await;

    for expected in [200, 429] {
        let req = test::TestRequest::post()
            .uri("/")
            .peer_addr("203.0.113.77:4000".parse().expect("addr"))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), expected);
    }

    let lines: Vec<String> = LINES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .filter(|l| l.contains(&bucket))
        .cloned()
        .collect();
    assert_eq!(lines.len(), 1, "one 429 line: {lines:?}");
    let line = &lines[0];
    assert!(!line.contains("203.0.113.77"), "{line}");
    let hashed = limit_bucket("203.0.113.77".parse().expect("ip"));
    assert!(line.ends_with(&format!("{bucket}:{hashed}")), "{line}");
}
