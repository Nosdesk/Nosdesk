//! On self-hosted the agent app is for staff. A requester signing in there is
//! signed in to the portal instead: the web app gets the portal's cookies, and
//! the native app gets no session, only a one-time address that its system
//! browser redeems at `/api/portal/auth/handoff` for a portal session.

#![allow(clippy::expect_used)]

use actix_web::dev::Service as _;
use actix_web::{test, web, App, HttpMessage as _};
use diesel::prelude::*;

use backend::extractors::WorkspaceContext;
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

const PASSWORD: &str = "Handoff-pass-4012!";

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

/// A person with no platform role, a password and `email`, holding `role` in
/// workspace `ws`.
fn person(conn: &mut backend::db::DbConnection, ws: i32, role: &str, email: &str) -> uuid::Uuid {
    use backend::schema::user_emails;
    let user = crate::common::insert_plain_user(conn, email);
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user),
            user_emails::email.eq(email),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(conn)
        .expect("email");
    backend::repository::user_auth_identities::create_local_identity(
        backend::models::NewUserAuthIdentity {
            user_uuid: user,
            provider_type: "local".to_string(),
            external_id: user.to_string(),
            email: None,
            metadata: None,
            password_hash: Some(bcrypt::hash(PASSWORD, 4).expect("hash")),
            workspace_id: None,
        },
        conn,
    )
    .expect("local identity");
    with_actor_context(
        conn,
        &ActorContext::system("test:seed").with_workspace(ws),
        |c| add_membership(c, ws, user, role, SeatWriteAuthority::ControlPlane),
    )
    .expect("membership");
    user
}

/// The app's sign-in and the portal's hand-off redemption, both served for
/// workspace `ctx` (as its Host would resolve).
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
                .route("/api/auth/login", web::post().to(backend::handlers::login)),
        )
        .await
    }};
}

/// Sign in at the app as `email`, from the native app when `native`.
fn sign_in(email: &str, native: bool) -> test::TestRequest {
    let req = test::TestRequest::post()
        .uri("/api/auth/login")
        .set_json(serde_json::json!({ "email": email, "password": PASSWORD }));
    if native {
        req.insert_header(("X-Auth-Mode", "bearer"))
    } else {
        req
    }
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
async fn a_requester_signing_in_at_the_native_app_gets_the_portal_not_a_session() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "handoffws", "Handoff WS");
    person(&mut conn, ws, "member", "requester@handoff.test");
    let ctx = context_of(&mut conn, ws);
    let app = handoff_app!(pool, ctx);

    let resp = test::call_service(&app, sign_in("requester@handoff.test", true).to_request()).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.response().cookies().count(), 0, "no session cookies");
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert!(body.get("access_token").is_none(), "no app session: {body}");
    assert!(
        body.get("refresh_token").is_none(),
        "no app session: {body}"
    );
    assert!(
        body["portal_url"]
            .as_str()
            .expect("portal_url")
            .ends_with("/portal/tickets"),
        "{body}"
    );
    let url = body["portal_handoff"]
        .as_str()
        .expect("portal_handoff")
        .to_string();
    assert!(url.contains("/api/portal/auth/handoff?t="), "{url}");
    let t = param(&url, "t");

    // The system browser follows it: signed in to the portal.
    let redeem = |t: &str| {
        test::TestRequest::get()
            .uri(&format!("/api/portal/auth/handoff?t={t}"))
            .to_request()
    };
    let resp = test::call_service(&app, redeem(&t)).await;
    assert_eq!(resp.status(), 302);
    assert_eq!(
        resp.headers().get("location").expect("location"),
        "/portal/tickets"
    );
    let cookies: Vec<String> = resp
        .response()
        .cookies()
        .map(|c| c.name().to_string())
        .collect();
    assert!(cookies.iter().any(|c| c == "portal_access"), "{cookies:?}");

    // Once only.
    let resp = test::call_service(&app, redeem(&t)).await;
    assert_eq!(
        resp.headers().get("location").expect("location"),
        "/portal/login?signin_error=1"
    );
}

#[actix_web::test]
async fn the_web_app_signs_a_requester_in_to_the_portal_and_staff_to_the_app() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "landingws", "Landing WS");
    person(&mut conn, ws, "member", "requester@landing.test");
    person(&mut conn, ws, "agent", "agent@landing.test");
    let ctx = context_of(&mut conn, ws);
    let app = handoff_app!(pool, ctx);

    // A requester in the browser: the portal's cookies, and where to go.
    let resp =
        test::call_service(&app, sign_in("requester@landing.test", false).to_request()).await;
    assert_eq!(resp.status(), 200);
    let cookies: Vec<String> = resp
        .response()
        .cookies()
        .map(|c| c.name().to_string())
        .collect();
    assert!(cookies.iter().any(|c| c == "portal_access"), "{cookies:?}");
    assert!(!cookies.iter().any(|c| c == "access_token"), "{cookies:?}");
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["portal"], "/portal/tickets");

    // Staff stay in the app, in the browser and the native app alike: its own
    // sign-in goes on (here to the two-factor setup staff must complete).
    for native in [false, true] {
        let resp =
            test::call_service(&app, sign_in("agent@landing.test", native).to_request()).await;
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["mfa_setup_required"], true, "{body}");
        assert!(body.get("portal").is_none(), "{body}");
        assert!(body.get("portal_handoff").is_none(), "{body}");
    }
}

#[actix_web::test]
async fn a_handoff_opens_only_its_own_workspace_and_ends_with_sign_out_everywhere() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let home = crate::common::mint_workspace(&mut conn, "handoffhome", "Handoff Home");
    let elsewhere = crate::common::mint_workspace(&mut conn, "handoffaway", "Handoff Away");
    let requester = person(&mut conn, home, "member", "roaming@handoff.test");
    let mint = |conn: &mut backend::db::DbConnection| {
        use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};
        let token = ResetTokenUtils::create_reset_token(requester, TokenType::PortalHandoff);
        backend::repository::reset_tokens::create_reset_token(
            conn,
            &token.token_hash,
            requester,
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
    backend::repository::active_sessions::revoke_all_sessions(&mut conn, &requester)
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
