//! Hosted `GET /login` starts single sign-on on the server: a 302 to the
//! identity provider with the `oauth_state` cookie and `no-store`, carrying a
//! valid `?redirect=` in the signed state. It serves the page instead for a
//! sign-in page message, a tenant origin (the portal's sign-in), a browser
//! with a live session cookie, and an identity provider it can't reach. A
//! stale session cookie redirects without deleting anything, and a refused
//! refresh still sets no cookies.
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

/// An identity provider that serves discovery for `issuer` and an empty key
/// set. The issuer is the server's own address, which discovery requires.
fn fake_idp() -> (actix_test::TestServer, String) {
    let srv = actix_test::start(|| {
        App::new()
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
    (srv, issuer)
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
    let url = url::Url::parse(location).expect("absolute authorize URL");
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .expect("state parameter");
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");
    jsonwebtoken::decode::<OAuthState>(
        &state,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .expect("state verifies")
    .claims
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

    let (_idp, issuer) = fake_idp();
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
    let binding = started
        .cookie(OAUTH_STATE_COOKIE)
        .expect("oauth_state cookie")
        .value()
        .to_string();
    let state = state_in(&location);
    assert_eq!(state.binding.as_deref(), Some(binding.as_str()));
    assert_eq!(state.redirect_uri, "/acme/tickets/5");
    assert!(state.pkce_verifier.is_some() && state.nonce.is_some());
    assert_eq!(state.user_connection, None);

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
