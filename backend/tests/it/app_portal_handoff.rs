//! The native app hands its signed-in person to the portal in the system
//! browser: `POST /api/me/portal-handoff` gives a one-time address, and
//! `/api/portal/auth/handoff` redeems it for a portal session.

#![allow(clippy::expect_used)]

use actix_web::dev::Service as _;
use actix_web::middleware::from_fn;
use actix_web::{test, web, App, HttpMessage as _};

use backend::extractors::WorkspaceContext;
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

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

fn member(conn: &mut backend::db::DbConnection, ws: i32, name: &str) -> backend::models::User {
    let user = crate::common::insert_user(conn, name);
    with_actor_context(
        conn,
        &ActorContext::system("test:seed").with_workspace(ws),
        |c| add_membership(c, ws, user.uuid, "member", SeatWriteAuthority::ControlPlane),
    )
    .expect("membership");
    user
}

/// The native app's bearer token for a session of `user`.
fn app_bearer(conn: &mut backend::db::DbConnection, user: &backend::models::User) -> String {
    let sid = backend::repository::active_sessions::create_session(
        conn,
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("iPhone".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::days(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
            provider_amr: None,
        },
    )
    .expect("session")
    .session_id;
    let (_, tokens) = backend::utils::jwt::helpers::create_login_response(
        user.clone(),
        &sid,
        &uuid::Uuid::new_v4(),
        conn,
    )
    .expect("tokens");
    tokens.access_token
}

macro_rules! handoff_app {
    ($pool:expr, $ctx:expr) => {{
        let pool = $pool.clone();
        let ctx = $ctx.clone();
        test::init_service(
            App::new()
                .app_data(web::Data::new(pool))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(ctx.clone());
                    srv.call(req)
                })
                .service(
                    web::scope("/api/portal/auth")
                        .configure(backend::handlers::portal::auth_config),
                )
                .service(
                    web::scope("/api")
                        .wrap(from_fn(backend::middleware::dual_auth_middleware))
                        .configure(backend::handlers::portal::app_config),
                ),
        )
        .await
    }};
}

/// The query parameter `name` of `url`.
fn param(url: &str, name: &str) -> String {
    url::Url::parse(url)
        .expect("url")
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .expect("param")
}

#[actix_web::test]
async fn the_app_opens_the_portal_signed_in_once() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "handoffws", "Handoff WS");
    let requester = member(&mut conn, ws, "App Requester");
    let ctx = context_of(&mut conn, ws);
    let app = handoff_app!(pool, ctx);
    let bearer = app_bearer(&mut conn, &requester);

    let resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/me/portal-handoff")
            .insert_header(("Authorization", format!("Bearer {bearer}")))
            .insert_header(("X-Auth-Mode", "bearer"))
            .set_json(serde_json::json!({ "next": "/tickets/12" }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let url = body["url"].as_str().expect("url").to_string();
    assert!(url.contains("/api/portal/auth/handoff?t="), "{url}");
    let t = param(&url, "t");
    assert_eq!(param(&url, "next"), "/tickets/12");

    // The browser follows it: signed in to the portal, on the request.
    let redeem = |t: &str| {
        test::TestRequest::get()
            .uri(&format!(
                "/api/portal/auth/handoff?t={t}&next=%2Ftickets%2F12"
            ))
            .to_request()
    };
    let resp = test::call_service(&app, redeem(&t)).await;
    assert_eq!(resp.status(), 302);
    assert_eq!(
        resp.headers().get("location").expect("location"),
        "/portal/tickets/12"
    );
    let cookies: Vec<String> = resp
        .response()
        .cookies()
        .map(|c| c.name().to_string())
        .collect();
    assert!(cookies.iter().any(|c| c == "portal_access"), "{cookies:?}");

    // Once only.
    let resp = test::call_service(&app, redeem(&t)).await;
    assert_eq!(resp.status(), 302);
    assert_eq!(
        resp.headers().get("location").expect("location"),
        "/portal/login?signin_error=1"
    );
}

#[actix_web::test]
async fn a_handoff_opens_only_its_own_workspace_and_ends_with_sign_out_everywhere() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let home = crate::common::mint_workspace(&mut conn, "handoffhome", "Handoff Home");
    let elsewhere = crate::common::mint_workspace(&mut conn, "handoffaway", "Handoff Away");
    let requester = member(&mut conn, home, "Roaming Requester");
    let mint = |conn: &mut backend::db::DbConnection| {
        use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};
        let token = ResetTokenUtils::create_reset_token(requester.uuid, TokenType::PortalHandoff);
        backend::repository::reset_tokens::create_reset_token(
            conn,
            &token.token_hash,
            requester.uuid,
            TokenType::PortalHandoff.as_str(),
            None,
            None,
            token.expires_at,
            None,
        )
        .expect("token");
        token.raw_token
    };
    let refused = "/portal/login?signin_error=1";

    // Another workspace's portal doesn't take it.
    let away = context_of(&mut conn, elsewhere);
    let app = handoff_app!(pool, away);
    let t = mint(&mut conn);
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/portal/auth/handoff?t={t}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.headers().get("location").expect("location"), refused);

    // Signing out everywhere spends an unused one.
    let ctx = context_of(&mut conn, home);
    let app = handoff_app!(pool, ctx);
    let t = mint(&mut conn);
    backend::repository::active_sessions::revoke_all_sessions(&mut conn, &requester.uuid)
        .expect("sign out everywhere");
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/portal/auth/handoff?t={t}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.headers().get("location").expect("location"), refused);
}
