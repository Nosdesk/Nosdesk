//! A session outlives its 15-minute access cookie: after an idle spell the
//! next write gets auth's 401, the client refreshes, and the retry succeeds.
//! A write must never be refused for CSRF just because time passed.
//!
//! The browser is modelled by [`Jar`]: it keeps what `Set-Cookie` hands it,
//! sends it back, echoes the CSRF cookie as `X-CSRF-Token` the way both web
//! clients do, and drops a cookie once its max-age has run out.
//!
//! Each realm runs through the real CSRF middleware, the real auth middleware
//! for that realm and the real refresh handler.

#![allow(clippy::expect_used)]

use actix_web::cookie::{time::Duration, Cookie};
use actix_web::dev::Service as _;
use actix_web::http::StatusCode;
use actix_web::middleware::from_fn;
use actix_web::{test, web, App, HttpMessage as _, HttpResponse};

use backend::extractors::WorkspaceContext;
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::cookies::{
    cookie_name, ACCESS_TOKEN_COOKIE, CSRF_TOKEN_COOKIE, PORTAL_ACCESS_TOKEN_COOKIE,
    PORTAL_CSRF_TOKEN_COOKIE, PORTAL_REFRESH_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE,
};
use backend::utils::csrf::CsrfProtection;

/// A browser's cookie store for one origin.
#[derive(Default)]
struct Jar(Vec<Cookie<'static>>);

impl Jar {
    /// Take in a response's `Set-Cookie`s: replace by name, drop on max-age 0.
    fn absorb<'a>(&mut self, cookies: impl Iterator<Item = Cookie<'a>>) {
        for c in cookies {
            let c = c.into_owned();
            self.0.retain(|have| have.name() != c.name());
            if c.max_age() != Some(Duration::ZERO) {
                self.0.push(c);
            }
        }
    }

    /// Let `idle` pass: a cookie whose max-age is up is gone.
    fn idle_for(&mut self, idle: Duration) {
        self.0.retain(|c| c.max_age().is_some_and(|age| age > idle));
    }

    fn value(&self, base: &'static str) -> Option<String> {
        let name = cookie_name(base);
        self.0
            .iter()
            .find(|c| c.name() == name)
            .map(|c| c.value().to_string())
    }

    fn has(&self, base: &'static str) -> bool {
        self.value(base).is_some()
    }

    /// A POST as the web client sends it: every cookie, and the CSRF cookie
    /// echoed in the header when there is one.
    fn post(&self, uri: &str, csrf: &'static str) -> test::TestRequest {
        let mut req = test::TestRequest::post().uri(uri);
        for c in &self.0 {
            req = req.cookie(Cookie::new(c.name().to_string(), c.value().to_string()));
        }
        if let Some(token) = self.value(csrf) {
            req = req.insert_header(("X-CSRF-Token", token));
        }
        req
    }
}

/// What came back: status, the JSON `code` if any, and the response's cookies.
struct Reply {
    status: StatusCode,
    code: Option<String>,
    cookies: Vec<Cookie<'static>>,
}

/// Send `req` to `srv` and read the reply. A middleware refusal surfaces as
/// `Err`; over the wire it is the error's response, which is what the
/// assertions are about.
macro_rules! send {
    ($srv:expr, $req:expr $(,)?) => {{
        let res: HttpResponse = match test::try_call_service(&$srv, $req.to_request()).await {
            Ok(res) => res.map_into_boxed_body().into_parts().1,
            Err(e) => e.error_response(),
        };
        reply_of(res).await
    }};
}

async fn reply_of(res: HttpResponse) -> Reply {
    let status = res.status();
    let cookies = res.cookies().map(|c| c.into_owned()).collect();
    let body = actix_web::body::to_bytes(res.into_body())
        .await
        .unwrap_or_default();
    let code = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["code"].as_str().map(str::to_string));
    Reply {
        status,
        code,
        cookies,
    }
}

/// Longer than the access cookie's 15 minutes, far shorter than the session.
fn idle_spell() -> Duration {
    Duration::minutes(16)
}

async fn wrote() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

fn expired(reply: &Reply, base: &'static str) -> bool {
    let name = cookie_name(base);
    reply
        .cookies
        .iter()
        .any(|c| c.name() == name && c.max_age() == Some(Duration::ZERO))
}

// ---- Agent ----------------------------------------------------------------

macro_rules! agent_app {
    ($pool:expr) => {
        test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .wrap(CsrfProtection)
                .route(
                    "/api/auth/refresh",
                    web::post().to(backend::handlers::refresh_token),
                )
                .service(
                    web::resource("/api/notes")
                        .wrap(from_fn(backend::middleware::cookie_auth_middleware))
                        .route(web::post().to(wrote)),
                ),
        )
        .await
    };
}

/// Sign `user` in: an active session and the three cookies login sets.
fn agent_sign_in(
    pool: &crate::common::TestPool,
    user: &backend::models::User,
) -> (uuid::Uuid, Jar) {
    let mut conn = pool.get().expect("conn");
    let session = backend::repository::active_sessions::create_session(
        &mut conn,
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("session-csrf-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::days(7)).naive_utc(),
            oidc_id_token: None,
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
    let mut jar = Jar::default();
    jar.absorb(
        [
            backend::utils::cookies::create_access_token_cookie(&tokens.access_token),
            backend::utils::cookies::create_refresh_token_cookie(
                &tokens.refresh_token,
                tokens.expires_at,
            ),
            backend::utils::cookies::create_csrf_token_cookie(
                &tokens.csrf_token,
                tokens.expires_at,
            ),
        ]
        .into_iter(),
    );
    (session.session_id, jar)
}

#[actix_web::test]
async fn an_agent_write_after_an_idle_spell_refreshes_and_retries() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let user = crate::common::insert_user(&mut pool.get().expect("conn"), "Idle Agent");
    let (_sid, mut jar) = agent_sign_in(&pool, &user);
    let app = agent_app!(pool);

    assert_eq!(
        send!(app, jar.post("/api/notes", CSRF_TOKEN_COOKIE)).status,
        StatusCode::OK,
        "a fresh session writes"
    );

    jar.idle_for(idle_spell());
    assert!(
        !jar.has(ACCESS_TOKEN_COOKIE),
        "the access cookie has expired"
    );
    assert!(jar.has(REFRESH_TOKEN_COOKIE), "the session has not");

    let write = send!(app, jar.post("/api/notes", CSRF_TOKEN_COOKIE));
    assert_eq!(
        write.status,
        StatusCode::UNAUTHORIZED,
        "an idle session's write is auth's 401, not a CSRF refusal (code {:?})",
        write.code
    );

    let refresh = send!(app, jar.post("/api/auth/refresh", CSRF_TOKEN_COOKIE));
    assert_eq!(refresh.status, StatusCode::OK, "the session refreshes");
    jar.absorb(refresh.cookies.into_iter());

    assert_eq!(
        send!(app, jar.post("/api/notes", CSRF_TOKEN_COOKIE)).status,
        StatusCode::OK,
        "the retried write goes through"
    );
}

#[actix_web::test]
async fn an_agent_write_with_no_session_is_a_401() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let app = agent_app!(pool);

    let write = send!(app, Jar::default().post("/api/notes", CSRF_TOKEN_COOKIE));
    assert_eq!(write.status, StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn a_refused_agent_refresh_expires_the_session_cookies() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let user = crate::common::insert_user(&mut pool.get().expect("conn"), "Revoked Agent");
    let (sid, jar) = agent_sign_in(&pool, &user);
    backend::repository::active_sessions::revoke_session_by_uuid(
        &mut pool.get().expect("conn"),
        &sid,
    )
    .expect("revoke");
    let app = agent_app!(pool);

    let refresh = send!(app, jar.post("/api/auth/refresh", CSRF_TOKEN_COOKIE));
    assert_eq!(refresh.status, StatusCode::UNAUTHORIZED);
    for base in [ACCESS_TOKEN_COOKIE, REFRESH_TOKEN_COOKIE, CSRF_TOKEN_COOKIE] {
        assert!(
            expired(&refresh, base),
            "a refused refresh expires {base}; got {:?}",
            refresh.cookies
        );
    }

    // A native client sent its token in the body and holds no cookies.
    let bearer = send!(
        app,
        test::TestRequest::post()
            .uri("/api/auth/refresh")
            .insert_header(("X-Auth-Mode", "bearer"))
            .set_json(serde_json::json!({ "refresh_token": "not-a-token" })),
    );
    assert_eq!(bearer.status, StatusCode::UNAUTHORIZED);
    assert!(
        bearer.cookies.is_empty(),
        "no Set-Cookie for a native client"
    );
}

// ---- Portal ---------------------------------------------------------------

fn context_of(conn: &mut backend::db::DbConnection, workspace_id: i32) -> WorkspaceContext {
    let ws = find_by_id(conn, workspace_id)
        .expect("workspace lookup")
        .expect("workspace exists");
    WorkspaceContext {
        workspace_id: ws.id,
        workspace_uuid: ws.uuid,
        slug: ws.slug,
        name: ws.name,
        custom_domain: ws.custom_domain,
        organisation_id: ws.organisation_id,
    }
}

macro_rules! portal_app {
    ($pool:expr, $ctx:expr) => {{
        let ctx: WorkspaceContext = $ctx.clone();
        test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .wrap(CsrfProtection)
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(ctx.clone());
                    srv.call(req)
                })
                .route(
                    "/api/portal/auth/refresh",
                    web::post().to(backend::handlers::portal::refresh_portal_session),
                )
                .service(
                    web::resource("/api/portal/notes")
                        .wrap(from_fn(backend::handlers::portal::portal_auth_middleware))
                        .route(web::post().to(wrote)),
                ),
        )
        .await
    }};
}

/// A workspace with one requester signed in to its portal.
fn portal_sign_in(pool: &crate::common::TestPool, slug: &str) -> (WorkspaceContext, Jar) {
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, slug, "Portal WS");
    let customer = crate::common::insert_user(&mut conn, "Idle Requester");
    with_actor_context(
        &mut conn,
        &ActorContext::system("test:seed").with_workspace(ws),
        |c| {
            add_membership(
                c,
                ws,
                customer.uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )
        },
    )
    .expect("membership");
    let ctx = context_of(&mut conn, ws);
    let http_req = test::TestRequest::default().to_http_request();
    let resp = backend::handlers::portal::establish_portal_session(
        &customer,
        ctx.workspace_uuid,
        &http_req,
        &mut conn,
    )
    .expect("portal sign-in");
    let mut jar = Jar::default();
    jar.absorb(resp.cookies());
    (ctx, jar)
}

#[actix_web::test]
async fn a_portal_write_after_an_idle_spell_refreshes_and_retries() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ctx, mut jar) = portal_sign_in(&pool, "idle-portal");
    let app = portal_app!(pool, ctx);

    assert_eq!(
        send!(app, jar.post("/api/portal/notes", PORTAL_CSRF_TOKEN_COOKIE)).status,
        StatusCode::OK,
        "a fresh portal session writes"
    );

    jar.idle_for(idle_spell());
    assert!(
        !jar.has(PORTAL_ACCESS_TOKEN_COOKIE),
        "the access cookie has expired"
    );
    assert!(jar.has(PORTAL_REFRESH_TOKEN_COOKIE), "the session has not");

    let write = send!(app, jar.post("/api/portal/notes", PORTAL_CSRF_TOKEN_COOKIE));
    assert_eq!(
        write.status,
        StatusCode::UNAUTHORIZED,
        "an idle session's write is auth's 401, not a CSRF refusal (code {:?})",
        write.code
    );

    let refresh = send!(
        app,
        jar.post("/api/portal/auth/refresh", PORTAL_CSRF_TOKEN_COOKIE),
    );
    assert_eq!(refresh.status, StatusCode::OK, "the session refreshes");
    jar.absorb(refresh.cookies.into_iter());

    assert_eq!(
        send!(app, jar.post("/api/portal/notes", PORTAL_CSRF_TOKEN_COOKIE)).status,
        StatusCode::OK,
        "the retried write goes through"
    );
}

#[actix_web::test]
async fn a_portal_write_with_no_session_is_a_401() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(2);
    let ctx = {
        let mut conn = pool.get().expect("conn");
        let ws = crate::common::mint_workspace(&mut conn, "nosession-portal", "Portal WS");
        context_of(&mut conn, ws)
    };
    let app = portal_app!(pool, ctx);

    let write = send!(
        app,
        Jar::default().post("/api/portal/notes", PORTAL_CSRF_TOKEN_COOKIE),
    );
    assert_eq!(write.status, StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn a_refused_portal_refresh_expires_the_session_cookies() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let (ctx, mut jar) = portal_sign_in(&pool, "dead-portal");
    // The refresh token no longer names a live session.
    jar.absorb(std::iter::once(
        backend::utils::cookies::create_portal_refresh_cookie(
            "not-a-token",
            chrono::Utc::now().naive_utc() + chrono::Duration::days(7),
        ),
    ));
    let app = portal_app!(pool, ctx);

    let refresh = send!(
        app,
        jar.post("/api/portal/auth/refresh", PORTAL_CSRF_TOKEN_COOKIE),
    );
    assert_eq!(refresh.status, StatusCode::UNAUTHORIZED);
    for base in [
        PORTAL_ACCESS_TOKEN_COOKIE,
        PORTAL_REFRESH_TOKEN_COOKIE,
        PORTAL_CSRF_TOKEN_COOKIE,
    ] {
        assert!(
            expired(&refresh, base),
            "a refused refresh expires {base}; got {:?}",
            refresh.cookies
        );
    }
}
