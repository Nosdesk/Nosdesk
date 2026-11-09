//! Hosted `GET /login` starts single sign-on on the server: a 302 to the
//! identity provider with the `oauth_state` cookie and `no-store`, carrying a
//! valid `?redirect=` in the signed state. It serves the page instead for a
//! sign-in page message, a tenant origin (the portal's sign-in), a browser
//! with a live session cookie, and an identity provider it can't reach. A
//! stale session cookie redirects without deleting anything, and a refused
//! refresh still sets no cookies.
//!
//! The PKCE verifier and nonce ride in the `oauth_state` cookie, not the
//! state, and the callback exchanges the code with the cookie's verifier; a
//! state minted before 1.1.6 still works with the verifier it carries.
//!
//! Its own binary: the deployment mode, the OIDC settings and the cached OIDC
//! client are process-wide, and `tests/hosted` sets a different issuer. One
//! test drives every case in turn for the same reason.

#![allow(clippy::expect_used)]

mod common;

use actix_web::cookie::Cookie;
use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::{header, StatusCode};
use actix_web::{test, web, App, HttpResponse};
use serde_json::json;

use backend::middleware::{RequestLimiter, WorkspaceContextConfig, WorkspaceContextMiddleware};
use backend::models::OAuthState;
use backend::utils::cookies::{cookie_name, OAUTH_STATE_COOKIE, REFRESH_TOKEN_COOKIE};

/// The `code_verifier` of each token request the fake identity provider got.
type TokenRequests = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// An identity provider that serves discovery for `issuer` and an empty key
/// set. The issuer is the server's own address, which discovery requires. Its
/// token endpoint records the `code_verifier` it is sent and refuses the code.
fn fake_idp() -> (actix_test::TestServer, String, TokenRequests) {
    let token_requests = TokenRequests::default();
    let recorded = token_requests.clone();
    let srv = actix_test::start(move || {
        let recorded = recorded.clone();
        App::new()
            .route(
                "/oauth2/token",
                web::post().to(move |form: web::Form<Vec<(String, String)>>| {
                    let recorded = recorded.clone();
                    async move {
                        let verifier = form
                            .iter()
                            .find(|(k, _)| k == "code_verifier")
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        recorded.lock().expect("lock").push(verifier);
                        HttpResponse::BadRequest().json(json!({ "error": "invalid_grant" }))
                    }
                }),
            )
            .route(
                "/.well-known/openid-configuration",
                web::get().to(|req: actix_web::HttpRequest| async move {
                    let host = req.connection_info().host().to_string();
                    let issuer = format!("http://{host}");
                    HttpResponse::Ok().json(json!({
                        "issuer": issuer,
                        "authorization_endpoint": format!("{issuer}/oauth2/auth"),
                        "token_endpoint": format!("{issuer}/oauth2/token"),
                        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
                        "response_types_supported": ["code"],
                        "subject_types_supported": ["public"],
                        "id_token_signing_alg_values_supported": ["RS256"],
                    }))
                }),
            )
            .route(
                "/.well-known/jwks.json",
                web::get().to(|| async { HttpResponse::Ok().json(json!({ "keys": [] })) }),
            )
    });
    let issuer = format!("http://{}", srv.addr());
    (srv, issuer, token_requests)
}

/// A limiter that never reaches Redis, so every request passes.
fn open_limiter() -> RequestLimiter {
    RequestLimiter::build(
        "auth",
        "redis://127.0.0.1:1",
        1_000,
        std::time::Duration::from_secs(60),
        |_| None,
    )
    .expect("limiter builds")
}

struct Answer {
    status: StatusCode,
    location: Option<String>,
    cache_control: Option<String>,
    content_type: Option<String>,
    server_timing: Option<String>,
    cookies: Vec<Cookie<'static>>,
}

fn answer(resp: ServiceResponse) -> Answer {
    let headers = resp.headers();
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    Answer {
        status: resp.status(),
        location: get(header::LOCATION.as_str()),
        cache_control: get(header::CACHE_CONTROL.as_str()),
        content_type: get(header::CONTENT_TYPE.as_str()),
        server_timing: get("server-timing"),
        cookies: resp.response().cookies().map(|c| c.into_owned()).collect(),
    }
}

impl Answer {
    /// The page, not a redirect: the shell (a 404 "rebuilding" page here,
    /// with no frontend built) and no sign-in started.
    fn assert_page(&self, case: &str) {
        assert!(
            self.location.is_none(),
            "{case}: no redirect, got {:?}",
            self.location
        );
        assert!(
            self.content_type
                .as_deref()
                .is_some_and(|t| t.starts_with("text/html")),
            "{case}: HTML, got {:?}",
            self.content_type
        );
        assert!(
            !self
                .cookies
                .iter()
                .any(|c| c.name() == cookie_name(OAUTH_STATE_COOKIE)),
            "{case}: no sign-in flow started"
        );
    }

    fn cookie(&self, base: &'static str) -> Option<&Cookie<'static>> {
        self.cookies.iter().find(|c| c.name() == cookie_name(base))
    }
}

/// The signed state in an authorize URL's `state` parameter.
fn state_in(location: &str) -> OAuthState {
    let state = query_param(location, "state");
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");
    jsonwebtoken::decode::<OAuthState>(
        &state,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .expect("state verifies")
    .claims
}

/// The value of `name` in an absolute URL's query.
fn query_param(location: &str, name: &str) -> String {
    url::Url::parse(location)
        .expect("absolute URL")
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{name} parameter in {location}"))
}

/// The PKCE S256 challenge for `verifier` (RFC 7636 §4.2).
fn s256(verifier: &str) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier))
}

/// Sign `user` in as the agent app does: an active session and its tokens.
/// Returns the session id and the raw refresh token.
fn agent_sign_in(pool: &common::TestPool, user: &backend::models::User) -> (uuid::Uuid, String) {
    let mut conn = pool.get().expect("conn");
    let session = backend::repository::active_sessions::create_session(
        &mut conn,
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("hosted-login-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::days(7)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
            provider_amr: None,
        },
    )
    .expect("session");
    let (_, tokens) = backend::utils::jwt::helpers::create_login_response(
        user.clone(),
        &session.session_id,
        &uuid::Uuid::new_v4(),
        &mut conn,
    )
    .expect("login tokens");
    (session.session_id, tokens.refresh_token)
}

#[actix_web::test]
async fn hosted_login_starts_sign_in_on_the_server() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_TENANT_DOMAIN", "nosdesk.test");
    std::env::set_var("OIDC_CLIENT_ID", "nosdesk-product");
    std::env::set_var("OIDC_CLIENT_SECRET", "test-secret");
    std::env::set_var(
        "OIDC_REDIRECT_URI",
        "https://app.nosdesk.test/api/auth/oauth/callback",
    );
    // Nothing listens here, so the first case can't build the client.
    std::env::set_var("OIDC_ISSUER_URL", "http://127.0.0.1:1");
    common::ensure_test_keyring();

    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    common::mint_workspace(&mut pool.get().expect("conn"), "acme", "Acme");

    let workspaces = WorkspaceContextConfig::initialise(&pool).expect("hosted initialises");
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(open_limiter()))
            .wrap(WorkspaceContextMiddleware::new(workspaces))
            .route(
                "/api/auth/refresh",
                web::post().to(backend::handlers::refresh_token),
            )
            .route(
                "/api/auth/oauth/callback",
                web::get().to(backend::handlers::oauth_callback),
            )
            .configure(backend::startup::hosted_login_config),
    )
    .await;
    let get = |uri: &str| {
        test::TestRequest::get()
            .uri(uri)
            .insert_header((header::HOST, "app.nosdesk.test"))
    };
    let call = |req: test::TestRequest| {
        let app = &app;
        async move { answer(app.call(req.to_request()).await.expect("served")) }
    };

    // The identity provider can't be reached: the page, never a 500.
    call(get("/login")).await.assert_page("unreachable issuer");

    let (_idp, issuer, token_requests) = fake_idp();
    std::env::set_var("OIDC_ISSUER_URL", &issuer);

    // The redirect, with the binding cookie, no-store and the timing.
    let started = call(get("/login?redirect=%2Facme%2Ftickets%2F5")).await;
    assert_eq!(started.status, StatusCode::FOUND);
    let location = started.location.clone().expect("Location");
    assert!(
        location.starts_with(&format!("{issuer}/oauth2/auth?")),
        "to the provider's authorize endpoint: {location}"
    );
    assert!(
        location.contains("code_challenge_method=S256"),
        "{location}"
    );
    assert!(location.contains("client_id=nosdesk-product"), "{location}");
    assert!(
        location.contains(&format!(
            "redirect_uri={}",
            urlencoding::encode("https://app.nosdesk.test/api/auth/oauth/callback")
        )),
        "this host's callback: {location}"
    );
    assert_eq!(started.cache_control.as_deref(), Some("no-store"));
    assert!(
        started
            .server_timing
            .as_deref()
            .is_some_and(|t| t.starts_with("oidc;dur=")),
        "Server-Timing: {:?}",
        started.server_timing
    );
    let state_cookie = started
        .cookie(OAUTH_STATE_COOKIE)
        .expect("oauth_state cookie");
    assert!(state_cookie.http_only().unwrap_or(false));
    assert_eq!(
        state_cookie.same_site(),
        Some(actix_web::cookie::SameSite::Lax)
    );
    assert_eq!(state_cookie.path(), Some("/"));
    let state_cookie = state_cookie.value().to_string();
    let state = state_in(&location);
    assert_eq!(state.redirect_uri, "/acme/tickets/5");
    assert_eq!(state.user_connection, None);

    // The cookie holds the binding, then the PKCE verifier and nonce; the
    // state holds the binding only.
    let parts: Vec<&str> = state_cookie.split('.').collect();
    let [binding, verifier, nonce] = parts.as_slice() else {
        panic!("binding.verifier.nonce, got {} parts", parts.len());
    };
    assert_eq!(state.binding.as_deref(), Some(*binding));
    assert_eq!(state.pkce_verifier, None);
    assert_eq!(state.nonce, None);
    let raw_state = query_param(&location, "state");
    assert!(!raw_state.contains(verifier) && !location.contains(verifier));
    assert!(location.contains(&format!("nonce={nonce}")), "{location}");
    assert_eq!(
        query_param(&location, "code_challenge"),
        s256(verifier),
        "the cookie's verifier is the one the challenge commits to"
    );

    // The callback: the browser's cookie supplies the verifier for the code
    // exchange. The fake provider refuses the code, so it ends there.
    let callback = |state: &str, cookie: Option<&str>| {
        let req = get(&format!(
            "/api/auth/oauth/callback?code=abc&state={}",
            urlencoding::encode(state)
        ));
        match cookie {
            Some(value) => req.cookie(Cookie::new(
                cookie_name(OAUTH_STATE_COOKIE).into_owned(),
                value.to_string(),
            )),
            None => req,
        }
    };
    let token_calls = || std::mem::take(&mut *token_requests.lock().expect("lock"));
    let lands_on = |answer: &Answer, code: &str, case: &str| {
        assert_eq!(answer.status, StatusCode::FOUND, "{case}");
        assert_eq!(
            answer.location.as_deref(),
            Some(format!("/login?auth_error={code}").as_str()),
            "{case}"
        );
    };
    let round_trip = call(callback(&raw_state, Some(&state_cookie))).await;
    lands_on(&round_trip, "signin_failed", "fresh flow");
    assert_eq!(token_calls(), [verifier.to_string()], "fresh flow");

    // A missing cookie, or one whose binding or shape was changed, is the
    // expired state it always was, and never reaches the provider.
    let unbound = format!("{}{}", &binding[1..], &state_cookie[..1]);
    let other_binding = state_cookie.replacen(binding, &unbound, 1);
    let bare_binding = binding.to_string();
    let malformed = format!("{binding}.{verifier}");
    for (case, cookie) in [
        ("no cookie", None),
        ("another binding", Some(other_binding.as_str())),
        (
            "the binding without its secrets",
            Some(bare_binding.as_str()),
        ),
        ("two parts", Some(malformed.as_str())),
    ] {
        let answer = call(callback(&raw_state, cookie)).await;
        lands_on(&answer, "state_expired", case);
        assert!(token_calls().is_empty(), "{case}: no token request");
    }

    // Two sign-ins in a row in one browser (#698): one cookie slot, so the
    // second overwrites the first. The first flow's callback is then an
    // expired state and the second completes.
    let first = call(get("/login")).await;
    let second = call(get("/login")).await;
    let first_state = query_param(first.location.as_deref().expect("Location"), "state");
    let second_state = query_param(second.location.as_deref().expect("Location"), "state");
    let latest = second
        .cookie(OAUTH_STATE_COOKIE)
        .expect("oauth_state cookie")
        .value()
        .to_string();
    lands_on(
        &call(callback(&first_state, Some(&latest))).await,
        "state_expired",
        "the overwritten flow",
    );
    assert!(token_calls().is_empty(), "the overwritten flow");
    lands_on(
        &call(callback(&second_state, Some(&latest))).await,
        "signin_failed",
        "the latest flow",
    );
    assert_eq!(
        token_calls(),
        [latest.split('.').nth(1).expect("verifier").to_string()],
        "the latest flow"
    );

    // Fallback until 1.1.8: a state minted before 1.1.6 carries the verifier
    // and nonce itself, beside a cookie holding the bare binding.
    let old_shape = OAuthState {
        state: "0ldb1nd1ng".into(),
        redirect_uri: "/".into(),
        provider_type: "oidc".into(),
        exp: state.exp,
        user_connection: None,
        pkce_verifier: Some("verifier-from-an-old-state".into()),
        nonce: Some("nonce-from-an-old-state".into()),
        callback_redirect_uri: state.callback_redirect_uri.clone(),
        binding: Some("0ldb1nd1ng".into()),
        step_up: None,
    };
    let old_state = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &old_shape,
        &jsonwebtoken::EncodingKey::from_secret(backend::utils::jwt::JWT_SECRET.as_bytes()),
    )
    .expect("sign old-shape state");
    lands_on(
        &call(callback(&old_state, Some("0ldb1nd1ng"))).await,
        "signin_failed",
        "old-shape state",
    );
    assert_eq!(
        token_calls(),
        ["verifier-from-an-old-state".to_string()],
        "old-shape state"
    );
    lands_on(
        &call(callback(&old_state, Some("another"))).await,
        "state_expired",
        "old-shape state, another binding",
    );
    assert!(token_calls().is_empty());

    // A return path off the app is dropped, and the sign-in lands on the root.
    for uri in [
        "/login?redirect=%2F%2Fevil.example",
        "/login?redirect=https%3A%2F%2Fevil.example%2F",
        "/login/",
    ] {
        let off = call(get(uri)).await;
        assert_eq!(off.status, StatusCode::FOUND, "{uri}");
        let state = state_in(off.location.as_deref().expect("Location"));
        assert_eq!(state.redirect_uri, "/", "{uri}");
    }

    // Something for the page to show: the page, which holds its own start.
    for uri in [
        "/login?auth_error=no_seat",
        "/login?auth_error=state_expired&redirect=%2Ftickets",
        "/login?message=Signed%20out",
        "/login?email=a%40example.com",
    ] {
        call(get(uri)).await.assert_page(uri);
    }

    // A tenant origin's /login is its portal's sign-in.
    call(get("/login").insert_header((header::HOST, "acme.nosdesk.test")))
        .await
        .assert_page("tenant origin");

    // HEAD starts nothing.
    let head = call(
        test::TestRequest::default()
            .method(actix_web::http::Method::HEAD)
            .uri("/login")
            .insert_header((header::HOST, "app.nosdesk.test")),
    )
    .await;
    assert!(head.location.is_none(), "HEAD: no redirect");

    // A live session cookie: the page, whose guard sends a signed-in person
    // home.
    let refresh = cookie_name(REFRESH_TOKEN_COOKIE).into_owned();
    let user = common::insert_user(&mut pool.get().expect("conn"), "Hosted Agent");
    let (_, live_token) = agent_sign_in(&pool, &user);
    call(get("/login").cookie(Cookie::new(refresh.clone(), live_token.clone())))
        .await
        .assert_page("live session cookie");

    // A stale one redirects as if there were none, and deletes nothing: the
    // callback sets fresh cookies.
    let (_, revoked_token) = agent_sign_in(&pool, &user);
    backend::repository::refresh_tokens::revoke_refresh_token(
        &mut pool.get().expect("conn"),
        &backend::utils::jwt::JwtUtils::hash_refresh_token(&revoked_token),
    )
    .expect("revoke token");
    let (ended_sid, ended_token) = agent_sign_in(&pool, &user);
    backend::repository::active_sessions::revoke_session_by_uuid(
        &mut pool.get().expect("conn"),
        &ended_sid,
    )
    .expect("end session");
    for (case, token) in [
        ("unknown token", "not-a-token".to_string()),
        ("revoked token", revoked_token),
        ("ended session", ended_token.clone()),
    ] {
        let stale = call(get("/login").cookie(Cookie::new(refresh.clone(), token))).await;
        assert_eq!(stale.status, StatusCode::FOUND, "{case}");
        assert!(
            stale
                .location
                .as_deref()
                .is_some_and(|l| l.starts_with(&format!("{issuer}/oauth2/auth?"))),
            "{case}: {:?}",
            stale.location
        );
        let names: Vec<&str> = stale.cookies.iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            [cookie_name(OAUTH_STATE_COOKIE).as_ref()],
            "{case}: only the flow's binding cookie is set"
        );
    }

    // A refused refresh sets no cookies on hosted either (#698): it may be a
    // stale tab's, sent just before another tab signed in, and a deletion
    // would land on the new session's cookies.
    let refused = call(
        test::TestRequest::post()
            .uri("/api/auth/refresh")
            .insert_header((header::HOST, "app.nosdesk.test"))
            .cookie(Cookie::new(refresh.clone(), ended_token)),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert!(
        refused.cookies.is_empty(),
        "a refused refresh sets no cookies; got {:?}",
        refused.cookies
    );
    // The session signed in alongside is untouched and still keeps the page.
    call(get("/login").cookie(Cookie::new(refresh.clone(), live_token)))
        .await
        .assert_page("the other session after the refusal");

    // The native app sends its token in the body and never gets cookies.
    let native = call(
        test::TestRequest::post()
            .uri("/api/auth/refresh")
            .insert_header((header::HOST, "app.nosdesk.test"))
            .set_json(json!({ "refresh_token": "not-a-token" })),
    )
    .await;
    assert_eq!(native.status, StatusCode::UNAUTHORIZED);
    assert!(
        native.cookies.is_empty(),
        "bearer refresh sets no cookies; got {:?}",
        native.cookies
    );
}
