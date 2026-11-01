//! `/account`: one set of self-service routes, mounted for the agent app at
//! `/api/account` and for the portal at `/api/portal/account`. Each mount
//! answers only its own app's session, and a portal session opened from a
//! ticket's link reaches none of it.

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

fn member(
    conn: &mut backend::db::DbConnection,
    ws: i32,
    name: &str,
    role: &str,
) -> backend::models::User {
    let user = crate::common::insert_user(conn, name);
    with_actor_context(
        conn,
        &ActorContext::system("test:seed").with_workspace(ws),
        |c| add_membership(c, ws, user.uuid, role, SeatWriteAuthority::ControlPlane),
    )
    .expect("membership");
    user
}

/// A session row for `user`, confirmed or not.
fn session(
    conn: &mut backend::db::DbConnection,
    user: &backend::models::User,
    confirmed: bool,
) -> uuid::Uuid {
    backend::repository::active_sessions::create_session(
        conn,
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("account-surface-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::days(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: confirmed.then(|| chrono::Utc::now().naive_utc()),
            provider_amr: None,
        },
    )
    .expect("session")
    .session_id
}

/// Portal cookies for a session of `user`.
fn portal_cookies(
    conn: &mut backend::db::DbConnection,
    user: &backend::models::User,
    ctx: &WorkspaceContext,
    confirmed: bool,
) -> (uuid::Uuid, String) {
    let sid = session(conn, user, confirmed);
    let tokens = backend::utils::jwt::helpers::create_portal_tokens(
        user,
        ctx.workspace_uuid,
        &sid,
        &uuid::Uuid::new_v4(),
        conn,
    )
    .expect("portal tokens");
    (sid, format!("portal_access={}", tokens.access_token))
}

/// Agent-app cookies for a session of `user`.
fn agent_cookies(conn: &mut backend::db::DbConnection, user: &backend::models::User) -> String {
    let sid = session(conn, user, true);
    let (_, tokens) = backend::utils::jwt::helpers::create_login_response(
        user.clone(),
        &sid,
        &uuid::Uuid::new_v4(),
        conn,
    )
    .expect("login tokens");
    format!("access_token={}", tokens.access_token)
}

macro_rules! account_app {
    ($pool:expr, $ctx:expr, $dir:expr) => {{
        let pool = $pool.clone();
        let ctx = $ctx.clone();
        let search = std::sync::Arc::new(
            backend::services::search::SearchService::new(&$dir.join("search"), &pool)
                .expect("init search"),
        );
        test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(search))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(ctx.clone());
                    srv.call(req)
                })
                .route(
                    "/api/portal/auth/ticket",
                    web::get().to(backend::handlers::portal::ticket_link_callback),
                )
                .service(
                    web::scope("/api/portal")
                        .wrap(from_fn(backend::handlers::portal::portal_auth_middleware))
                        .service(
                            web::scope("/account")
                                .wrap(from_fn(backend::handlers::account::full_portal_session))
                                .configure(backend::handlers::account::config),
                        )
                        .configure(backend::handlers::portal::config),
                )
                .service(
                    web::scope("/api/account")
                        .wrap(from_fn(backend::middleware::cookie_auth_middleware))
                        .configure(backend::handlers::account::config),
                ),
        )
        .await
    }};
}

/// The status a request ends with, whether a handler answered or a middleware
/// refused it.
macro_rules! status_of {
    ($app:expr, $req:expr) => {
        match test::try_call_service($app, $req).await {
            Ok(resp) => resp.status().as_u16(),
            Err(e) => e.as_response_error().status_code().as_u16(),
        }
    };
}

fn get(path: &str, cookies: &str) -> test::TestRequest {
    test::TestRequest::get()
        .uri(path)
        .insert_header(("Cookie", cookies.to_string()))
}

#[actix_web::test]
async fn a_portal_sign_in_manages_its_own_profile() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "acctprofile", "Account Profile");
    let customer = member(&mut conn, ws, "Portal Person", "member");
    let ctx = context_of(&mut conn, ws);
    let dir = tempfile::tempdir().expect("dir");
    let app = account_app!(pool, ctx, dir.path());
    let (_, cookies) = portal_cookies(&mut conn, &customer, &ctx, true);

    let resp = test::call_service(&app, get("/api/portal/account", &cookies).to_request()).await;
    assert_eq!(resp.status(), 200);
    let me: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(me["uuid"], customer.uuid.to_string(), "{me}");
    assert_eq!(me["sections"]["name"], true, "{me}");
    assert_eq!(
        me["sections"]["password"], false,
        "no password to change: {me}"
    );
    assert_eq!(
        me["sections"]["passkeys"], false,
        "no passkey they couldn't remove: {me}"
    );

    let resp = test::call_service(
        &app,
        test::TestRequest::patch()
            .uri("/api/portal/account")
            .insert_header(("Cookie", cookies.clone()))
            .set_json(
                serde_json::json!({ "name": "Renamed Person", "timezone": "Australia/Sydney" }),
            )
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let resp = test::call_service(&app, get("/api/portal/account", &cookies).to_request()).await;
    let me: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(me["name"], "Renamed Person", "{me}");
    assert_eq!(me["effective_timezone"], "Australia/Sydney", "{me}");

    // The profile only: no role, password or signature through here.
    for body in [
        serde_json::json!({ "role": "admin" }),
        serde_json::json!({ "password": "a-new-password" }),
        serde_json::json!({ "signature": "Hi" }),
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::patch()
                .uri("/api/portal/account")
                .insert_header(("Cookie", cookies.clone()))
                .set_json(body.clone())
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 400, "{body}");
    }
}

#[actix_web::test]
async fn a_portal_sign_in_signs_out_its_other_sessions_once_confirmed() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "acctsessions", "Account Sessions");
    let customer = member(&mut conn, ws, "Session Person", "member");
    let ctx = context_of(&mut conn, ws);
    let dir = tempfile::tempdir().expect("dir");
    let app = account_app!(pool, ctx, dir.path());
    let elsewhere = session(&mut conn, &customer, false);
    let still_there = |conn: &mut backend::db::DbConnection| {
        backend::repository::active_sessions::get_session_by_session_id(conn, &elsewhere).is_ok()
    };
    let others = |cookies: &str| {
        test::TestRequest::delete()
            .uri("/api/portal/account/sessions/others")
            .insert_header(("Cookie", cookies.to_string()))
            .set_json(serde_json::json!({}))
            .to_request()
    };

    // A session that never confirmed (the app's hand-off, say) is asked to.
    let (_, unconfirmed) = portal_cookies(&mut conn, &customer, &ctx, false);
    let status = status_of!(&app, others(&unconfirmed));
    assert!((400..500).contains(&status), "{status}");
    assert!(still_there(&mut conn));

    // A code or link sign-in just proved the mailbox, so it may.
    let signed_in = backend::handlers::portal::establish_portal_session(
        &customer,
        ctx.workspace_uuid,
        &test::TestRequest::default().to_http_request(),
        &mut conn,
    )
    .expect("sign in");
    let confirmed = signed_in
        .cookies()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");
    let resp = test::call_service(&app, others(&confirmed)).await;
    assert_eq!(resp.status(), 200);
    assert!(!still_there(&mut conn), "the other session is signed out");
    let resp = test::call_service(&app, get("/api/portal/account", &confirmed).to_request()).await;
    assert_eq!(resp.status(), 200, "this one stays");
    assert_eq!(
        backend::repository::credential_epochs::current(&mut conn, &customer.uuid).expect("epoch"),
        1,
        "and its portal links end"
    );
}

#[actix_web::test]
async fn a_links_session_reaches_no_account_route() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "acctlink", "Account Link");
    let customer = member(&mut conn, ws, "Linked Person", "member");
    let ticket = {
        use diesel::RunQueryDsl as _;
        with_actor_context(
            &mut conn,
            &ActorContext::system("test:seed").with_workspace(ws),
            |c| {
                backend::services::seed::seed_workspace_defaults(c, None)?;
                let state = backend::repository::workflow_states::default_state(c)?;
                diesel::insert_into(backend::schema::tickets::table)
                    .values(&backend::models::NewTicket {
                        title: "Printer jammed".to_string(),
                        workflow_state_id: state.id,
                        requester_uuid: Some(customer.uuid),
                        ..Default::default()
                    })
                    .get_result::<backend::models::Ticket>(c)
            },
        )
        .expect("ticket")
    };
    let ctx = context_of(&mut conn, ws);
    let dir = tempfile::tempdir().expect("dir");
    let app = account_app!(pool, ctx, dir.path());

    let token = backend::utils::portal_ticket_link::sign(&mut conn, ws, customer.uuid, ticket.id)
        .expect("sign");
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/portal/auth/ticket?t={token}"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), 302);
    let cookies = resp
        .response()
        .cookies()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");

    for req in [
        get("/api/portal/account", &cookies),
        get("/api/portal/account/sessions", &cookies),
        get("/api/portal/account/step-up", &cookies),
        test::TestRequest::delete()
            .uri("/api/portal/account/sessions/others")
            .insert_header(("Cookie", cookies.clone()))
            .set_json(serde_json::json!({})),
        test::TestRequest::patch()
            .uri("/api/portal/account")
            .insert_header(("Cookie", cookies.clone()))
            .set_json(serde_json::json!({ "name": "Taken Over" })),
    ] {
        let req = req.to_request();
        let path = req.path().to_string();
        assert_eq!(status_of!(&app, req), 403, "{path}");
    }
    // The request itself still opens.
    let resp = test::call_service(
        &app,
        get(&format!("/api/portal/tickets/{}", ticket.id), &cookies).to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);
}

#[actix_web::test]
async fn each_app_reaches_only_its_own_account_routes() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::mint_workspace(&mut conn, "acctrealm", "Account Realm");
    let agent = member(&mut conn, ws, "Agent Person", "agent");
    let ctx = context_of(&mut conn, ws);
    let dir = tempfile::tempdir().expect("dir");
    let app = account_app!(pool, ctx, dir.path());
    let agent_session = agent_cookies(&mut conn, &agent);
    let (_, portal_session) = portal_cookies(&mut conn, &agent, &ctx, true);

    let resp = test::call_service(&app, get("/api/account", &agent_session).to_request()).await;
    assert_eq!(resp.status(), 200);
    let me: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(me["uuid"], agent.uuid.to_string(), "{me}");
    let resp = test::call_service(
        &app,
        get("/api/portal/account", &portal_session).to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    assert_eq!(
        status_of!(&app, get("/api/account", &portal_session).to_request()),
        401,
        "a portal session isn't the agent app's"
    );
    assert_eq!(
        status_of!(
            &app,
            get("/api/portal/account", &agent_session).to_request()
        ),
        401,
        "an agent session isn't the portal's"
    );
}
