//! `GET /api/auth/oauth/callback` is a top-level browser load. A sign-in that
//! can't finish lands on the sign-in page with a code it can explain, never on
//! a JSON error body, always on the fixed `/login` target whatever return path
//! the state carries. It clears the flow's `oauth_state` cookie once the
//! state has verified against it, and leaves a cookie that binds another
//! flow (a sign-in started later in another tab) for that flow. A step-up
//! confirmation (in a popup) instead lands on `/auth/confirmed` with a result
//! code, and never signs anyone in.

use actix_web::cookie::{time::Duration, Cookie};
use actix_web::http::{header, StatusCode};
use actix_web::{test, web, App};

use backend::models::{OAuthState, StepUpFlow};
use backend::utils::cookies::{cookie_name, OAUTH_STATE_COOKIE};

use crate::common;

struct Landing {
    status: StatusCode,
    location: Option<String>,
    /// The `oauth_state` cookie the response sets, if any.
    state_cookie: Option<Cookie<'static>>,
}

/// Run the callback with `query`, from a browser holding `binding` in its
/// `oauth_state` cookie.
async fn callback(query: &str, binding: &str) -> Landing {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let app = test::init_service(App::new().app_data(web::Data::new(db.pool())).route(
        "/api/auth/oauth/callback",
        web::get().to(backend::handlers::oauth_callback),
    ))
    .await;
    let req = test::TestRequest::get()
        .uri(&format!("/api/auth/oauth/callback?{query}"))
        .cookie(Cookie::new(
            cookie_name(OAUTH_STATE_COOKIE),
            binding.to_string(),
        ))
        .to_request();
    let resp = test::call_service(&app, req).await;
    Landing {
        status: resp.status(),
        location: resp
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
        state_cookie: resp
            .response()
            .cookies()
            .find(|c| c.name() == cookie_name(OAUTH_STATE_COOKIE))
            .map(Cookie::into_owned),
    }
}

/// A state signed as the server signs one, `exp_offset` seconds from now,
/// asking to come back to an address outside the app.
fn signed_state(binding: &str, exp_offset: i64) -> String {
    sign(binding, exp_offset, None)
}

/// A step-up confirmation's state.
fn signed_step_up_state(binding: &str) -> String {
    sign(
        binding,
        600,
        Some(StepUpFlow {
            user_uuid: uuid::Uuid::new_v4(),
            session_id: uuid::Uuid::new_v4(),
            started_at: chrono::Utc::now().timestamp(),
        }),
    )
}

fn sign(binding: &str, exp_offset: i64, step_up: Option<StepUpFlow>) -> String {
    common::ensure_test_keyring();
    let claims = OAuthState {
        state: binding.to_string(),
        redirect_uri: "https://elsewhere.example/landing".to_string(),
        provider_type: "oidc".to_string(),
        exp: (chrono::Utc::now().timestamp() + exp_offset) as usize,
        user_connection: None,
        pkce_verifier: Some("verifier".to_string()),
        nonce: Some("nonce".to_string()),
        callback_redirect_uri: None,
        binding: Some(binding.to_string()),
        step_up,
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(backend::utils::jwt::JWT_SECRET.as_bytes()),
    )
    .expect("sign state")
}

fn assert_lands_on_login(landing: &Landing, code: &str) {
    assert_eq!(
        landing.status,
        StatusCode::FOUND,
        "a redirect, not an error body"
    );
    assert_eq!(
        landing.location.as_deref(),
        Some(format!("/login?auth_error={code}").as_str())
    );
}

/// Lands on the sign-in page with `code` and clears the flow's cookie.
fn assert_ends_the_flow(landing: &Landing, code: &str) {
    assert_lands_on_login(landing, code);
    let cookie = landing
        .state_cookie
        .as_ref()
        .expect("the response clears the oauth_state cookie");
    assert_eq!(cookie.value(), "");
    assert_eq!(cookie.max_age(), Some(Duration::ZERO));
}

/// Lands on the sign-in page with `code` and leaves the browser's cookie,
/// which binds some other flow, as it is.
fn assert_keeps_the_cookie(landing: &Landing, code: &str) {
    assert_lands_on_login(landing, code);
    assert!(
        landing.state_cookie.is_none(),
        "the response leaves the oauth_state cookie alone, got {:?}",
        landing.state_cookie
    );
}

#[actix_web::test]
async fn a_state_that_does_not_verify_lands_on_login_as_expired() {
    let landing = callback("code=abc&state=not-a-jwt", "binding").await;
    assert_keeps_the_cookie(&landing, "state_expired");
}

#[actix_web::test]
async fn an_expired_state_lands_on_login_as_expired() {
    let state = signed_state("binding", -3600);
    let landing = callback(&format!("code=abc&state={state}"), "binding").await;
    assert_keeps_the_cookie(&landing, "state_expired");
}

#[actix_web::test]
async fn a_state_from_another_browser_lands_on_login_as_expired() {
    let state = signed_state("binding", 600);
    let landing = callback(&format!("code=abc&state={state}"), "some-other-flow").await;
    assert_keeps_the_cookie(&landing, "state_expired");
}

/// Two tabs of one browser start signing in, so the cookie binds the second
/// tab's flow. The identity provider sends the first tab back with an error
/// (Hydra refuses a login whose CSRF cookie the second tab replaced): that
/// tab's sign-in took too long, not "cancelled", and the second tab's
/// cookie stays for its own callback.
#[actix_web::test]
async fn a_provider_error_for_a_replaced_flow_lands_as_expired_and_keeps_the_newer_cookie() {
    let first = signed_state("first-tab", 600);
    let landing = callback(
        &format!("error=request_forbidden&state={first}"),
        "second-tab",
    )
    .await;
    assert_keeps_the_cookie(&landing, "state_expired");
}

/// The first tab's callback, coming back with a code after the second tab
/// started, fails without clearing the cookie, so the second tab's callback
/// still gets past the state check. Here it then fails at the code exchange,
/// as no provider is set up, which is `signin_failed`, not `state_expired`.
#[actix_web::test]
async fn the_first_tab_failing_leaves_the_second_tab_able_to_finish() {
    let first = signed_state("first-tab", 600);
    let landing = callback(&format!("code=abc&state={first}"), "second-tab").await;
    assert_keeps_the_cookie(&landing, "state_expired");

    let second = signed_state("second-tab", 600);
    let landing = callback(&format!("code=abc&state={second}"), "second-tab").await;
    assert_ends_the_flow(&landing, "signin_failed");
}

#[actix_web::test]
async fn a_cancelled_sign_in_lands_on_login_as_denied_by_the_provider() {
    let state = signed_state("binding", 600);
    let landing = callback(
        &format!("error=access_denied&error_description=User+cancelled&state={state}"),
        "binding",
    )
    .await;
    assert_ends_the_flow(&landing, "provider_denied");
}

#[actix_web::test]
async fn a_callback_with_no_code_lands_on_login_as_failed() {
    let state = signed_state("binding", 600);
    let landing = callback(&format!("state={state}"), "binding").await;
    assert_ends_the_flow(&landing, "signin_failed");
}

fn assert_step_up_lands_on(landing: &Landing, result: &str) {
    assert_eq!(landing.status, StatusCode::FOUND);
    assert_eq!(
        landing.location.as_deref(),
        Some(format!("/auth/confirmed?result={result}").as_str())
    );
    let cookie = landing
        .state_cookie
        .as_ref()
        .expect("the response clears the oauth_state cookie");
    assert_eq!(cookie.value(), "");
}

#[actix_web::test]
async fn a_cancelled_step_up_tells_the_dialog_it_was_cancelled() {
    let state = signed_step_up_state("binding");
    let landing = callback(&format!("error=access_denied&state={state}"), "binding").await;
    assert_step_up_lands_on(&landing, "cancelled");
}

#[actix_web::test]
async fn a_step_up_with_no_code_fails() {
    let state = signed_step_up_state("binding");
    let landing = callback(&format!("state={state}"), "binding").await;
    assert_step_up_lands_on(&landing, "failed");
}

#[actix_web::test]
async fn a_step_up_whose_code_does_not_exchange_fails_without_signing_in() {
    let state = signed_step_up_state("binding");
    let landing = callback(&format!("code=forged&state={state}"), "binding").await;
    assert_step_up_lands_on(&landing, "failed");
}
