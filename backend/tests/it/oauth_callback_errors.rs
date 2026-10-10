//! `GET /api/auth/oauth/callback` is a top-level browser load. A sign-in that
//! can't finish lands on the sign-in page with a code it can explain, never on
//! a JSON error body, always on the fixed `/login` target whatever return path
//! the state carries, and clears the flow's `oauth_state` cookie.

use actix_web::cookie::{time::Duration, Cookie};
use actix_web::http::{header, StatusCode};
use actix_web::{test, web, App};

use backend::models::OAuthState;
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
    let cookie = landing
        .state_cookie
        .as_ref()
        .expect("the response clears the oauth_state cookie");
    assert_eq!(cookie.value(), "");
    assert_eq!(cookie.max_age(), Some(Duration::ZERO));
}

#[actix_web::test]
async fn a_state_that_does_not_verify_lands_on_login_as_expired() {
    let landing = callback("code=abc&state=not-a-jwt", "binding").await;
    assert_lands_on_login(&landing, "state_expired");
}

#[actix_web::test]
async fn an_expired_state_lands_on_login_as_expired() {
    let state = signed_state("binding", -3600);
    let landing = callback(&format!("code=abc&state={state}"), "binding").await;
    assert_lands_on_login(&landing, "state_expired");
}

#[actix_web::test]
async fn a_state_from_another_browser_lands_on_login_as_expired() {
    let state = signed_state("binding", 600);
    let landing = callback(&format!("code=abc&state={state}"), "some-other-flow").await;
    assert_lands_on_login(&landing, "state_expired");
}

#[actix_web::test]
async fn a_cancelled_sign_in_lands_on_login_as_denied_by_the_provider() {
    let state = signed_state("binding", 600);
    let landing = callback(
        &format!("error=access_denied&error_description=User+cancelled&state={state}"),
        "binding",
    )
    .await;
    assert_lands_on_login(&landing, "provider_denied");
}

#[actix_web::test]
async fn a_callback_with_no_code_lands_on_login_as_failed() {
    let state = signed_state("binding", 600);
    let landing = callback(&format!("state={state}"), "binding").await;
    assert_lands_on_login(&landing, "signin_failed");
}
