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
            .route("/auth/recovery-login", web::post().to(ok))
            .route("/csp-report", web::post().to(ok))
            .route("/tickets", web::post().to(ok))
            .route("/portal/tickets", web::post().to(ok)),
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

/// `Cookie` header carrying an access cookie for the agent realm.
fn agent_session() -> String {
    format!(
        "{}=a-session",
        backend::utils::cookies::cookie_name(backend::utils::cookies::ACCESS_TOKEN_COOKIE)
    )
}

#[actix_web::test]
async fn same_host_request_without_csrf_token_still_fails_double_submit() {
    let session = agent_session();
    let (status, body) = post(
        "/api/tickets",
        &[
            ("Host", "app.test"),
            ("Origin", "https://app.test"),
            ("Cookie", &session),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_missing");
}

/// With no session cookie there is nothing to forge: the write goes on to
/// auth (here, straight to the handler), which answers 401 in the real app.
#[actix_web::test]
async fn a_write_without_a_session_is_left_to_auth() {
    let (status, _) = post(
        "/api/tickets",
        &[("Host", "app.test"), ("Origin", "https://app.test")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// A CSRF cookie alone is not a session either: an idle tab's access cookie
/// has expired while its CSRF cookie lives on.
#[actix_web::test]
async fn a_csrf_cookie_without_an_access_cookie_is_left_to_auth() {
    let csrf = format!(
        "{}=t",
        backend::utils::cookies::cookie_name(backend::utils::cookies::CSRF_TOKEN_COOKIE)
    );
    let (status, _) = post(
        "/api/tickets",
        &[
            ("Host", "app.test"),
            ("Origin", "https://app.test"),
            ("Cookie", &csrf),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[actix_web::test]
async fn the_origin_check_runs_without_a_session() {
    let (status, body) = post(
        "/api/tickets",
        &[("Host", "app.test"), ("Origin", "https://evil.example")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "origin_not_allowed");
}

/// `Cookie` header value: `base=value` under its wire name.
fn cookie(base: &'static str, value: &str) -> String {
    format!("{}={value}", backend::utils::cookies::cookie_name(base))
}

/// The router matches the decoded path, so `/api/%70ortal/...` is a portal
/// request. A portal session without a portal CSRF token is refused, however
/// the path is spelled.
#[actix_web::test]
async fn an_encoded_portal_path_is_checked_as_the_portal() {
    use backend::utils::cookies::PORTAL_ACCESS_TOKEN_COOKIE;
    let session = cookie(PORTAL_ACCESS_TOKEN_COOKIE, "a-session");
    let (status, body) = post(
        "/api/%70ortal/tickets",
        &[
            ("Host", "help.acme.test"),
            ("Origin", "https://help.acme.test"),
            ("Cookie", &session),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_missing");
}

/// Nor does an agent CSRF token stand in for the portal's on that path.
#[actix_web::test]
async fn an_encoded_portal_path_takes_the_portal_csrf_cookie() {
    use backend::utils::cookies::{CSRF_TOKEN_COOKIE, PORTAL_ACCESS_TOKEN_COOKIE};
    let cookies = format!(
        "{}; {}",
        cookie(PORTAL_ACCESS_TOKEN_COOKIE, "a-session"),
        cookie(CSRF_TOKEN_COOKIE, "agent-token")
    );
    let (status, body) = post(
        "/api/%70ortal/tickets",
        &[
            ("Host", "help.acme.test"),
            ("Origin", "https://help.acme.test"),
            ("Cookie", &cookies),
            ("X-CSRF-Token", "agent-token"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_missing");
}

/// A session of either realm is a session: a portal cookie on an agent path
/// still needs the double-submit token.
#[actix_web::test]
async fn a_portal_session_on_an_agent_path_still_needs_a_csrf_token() {
    use backend::utils::cookies::PORTAL_ACCESS_TOKEN_COOKIE;
    let session = cookie(PORTAL_ACCESS_TOKEN_COOKIE, "a-session");
    let (status, body) = post(
        "/api/tickets",
        &[
            ("Host", "app.test"),
            ("Origin", "https://app.test"),
            ("Cookie", &session),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_missing");
}

/// The agent realm spelled with an escape is still the agent realm.
#[actix_web::test]
async fn an_encoded_agent_path_is_checked_as_the_agent() {
    let session = agent_session();
    let (status, body) = post(
        "/api/%74ickets",
        &[
            ("Host", "app.test"),
            ("Origin", "https://app.test"),
            ("Cookie", &session),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_missing");
}

/// Recovery-code sign-in is a login: exempt from the double-submit check even
/// when the browser holds a session cookie (here, another realm's).
#[actix_web::test]
async fn recovery_login_is_exempt_from_the_double_submit() {
    use backend::utils::cookies::PORTAL_ACCESS_TOKEN_COOKIE;
    let session = cookie(PORTAL_ACCESS_TOKEN_COOKIE, "a-session");
    let (status, _) = post(
        "/api/auth/recovery-login",
        &[
            ("Host", "app.test"),
            ("Origin", "https://app.test"),
            ("Cookie", &session),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
