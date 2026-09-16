//! The CSRF middleware's Origin check: a browser-supplied `Origin` on a
//! state-changing request must be allowlisted or match the request `Host`,
//! including on the login endpoints that are exempt from the double-submit
//! check (login CSRF). Middleware-only, no database.

#![allow(clippy::expect_used)]

use actix_web::{http::StatusCode, test, web, App, HttpResponse};
use backend::utils::csrf::CsrfProtection;

fn app() -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let ok = || async { HttpResponse::Ok().body("handled") };
    App::new().wrap(CsrfProtection).service(
        web::scope("/api")
            .route("/auth/login", web::post().to(ok))
            .route("/csp-report", web::post().to(ok))
            .route("/tickets", web::post().to(ok)),
    )
}

async fn post(path: &str, headers: &[(&str, &str)]) -> (StatusCode, serde_json::Value) {
    let srv = test::init_service(app()).await;
    let mut req = test::TestRequest::post().uri(path);
    for (k, v) in headers {
        req = req.insert_header((*k, *v));
    }
    // A middleware short-circuit surfaces as an `Err` here; over the wire it
    // is the error's response, which is what the assertions are about.
    let (status, body) = match test::try_call_service(&srv, req.to_request()).await {
        Ok(res) => (res.status(), test::read_body(res).await),
        Err(e) => {
            let res = e.error_response();
            (
                res.status(),
                actix_web::body::to_bytes(res.into_body())
                    .await
                    .expect("body"),
            )
        }
    };
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[actix_web::test]
async fn login_from_a_foreign_origin_is_refused() {
    let (status, body) = post(
        "/api/auth/login",
        &[("Host", "app.test"), ("Origin", "https://evil.example")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "origin_not_allowed");
    assert!(body["error"].is_string());
}

#[actix_web::test]
async fn opaque_origin_is_refused() {
    let (status, body) = post(
        "/api/auth/login",
        &[("Host", "app.test"), ("Origin", "null")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "origin_not_allowed");
}

#[actix_web::test]
async fn login_from_the_request_host_reaches_the_handler() {
    let (status, _) = post(
        "/api/auth/login",
        &[
            ("Host", "help.acme.test"),
            ("Origin", "https://help.acme.test"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[actix_web::test]
async fn login_without_an_origin_header_reaches_the_handler() {
    let (status, _) = post("/api/auth/login", &[("Host", "app.test")]).await;
    assert_eq!(status, StatusCode::OK);
}

#[actix_web::test]
async fn bearer_requests_skip_the_origin_check() {
    let (status, _) = post(
        "/api/tickets",
        &[
            ("Host", "app.test"),
            ("Origin", "https://evil.example"),
            ("Authorization", "Bearer not-checked-here"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[actix_web::test]
async fn csp_reports_skip_the_origin_check() {
    let (status, _) = post(
        "/api/csp-report",
        &[("Host", "app.test"), ("Origin", "null")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[actix_web::test]
async fn same_host_request_without_csrf_token_still_fails_double_submit() {
    let (status, body) = post(
        "/api/tickets",
        &[("Host", "app.test"), ("Origin", "https://app.test")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_missing");
}
