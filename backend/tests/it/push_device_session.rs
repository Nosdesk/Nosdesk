//! A push registration belongs to the session that made it and goes with it,
//! however the session ends. Signing out on the device can't be relied on to
//! release it: that request needs the device online and the session still
//! live, and every other way a session ends (signing out elsewhere, a password
//! reset, eviction, expiry) never asks the device at all.
//!
//! Sessions are ended here on a connection with no workspace set, as the CLI
//! and the control plane end them, so the release can't depend on the
//! device's workspace being in scope.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::json;
use tokio::sync::RwLock;
use uuid::Uuid;

use backend::db::DbConnection;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewActiveSession};
use backend::repository::{active_sessions, push_devices};
use backend::services::notifications::NotificationService;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_bypass_context;

fn session(conn: &mut DbConnection, user: Uuid) -> Uuid {
    active_sessions::create_session(
        conn,
        NewActiveSession {
            user_uuid: user,
            device_name: Some("iPhone (18.2)".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
            provider_amr: None,
        },
    )
    .expect("session")
    .session_id
}

fn register(conn: &mut DbConnection, user: Uuid, sid: Uuid, workspace: i32, token: &str) {
    let actor = ActorContext::system("test:register_device").with_workspace(workspace);
    with_actor_bypass_context::<_, diesel::result::Error>(conn, &actor, |c| {
        push_devices::register(c, user, sid, workspace, "ios", token, None)
    })
    .expect("register device");
}

/// The tokens the push channel would send this user's notifications to.
fn send_list(conn: &mut DbConnection, user: Uuid, workspace: i32) -> Vec<String> {
    let actor = ActorContext::system("test:send_list").with_workspace(workspace);
    let mut tokens = with_actor_bypass_context::<_, diesel::result::Error>(conn, &actor, |c| {
        push_devices::active_tokens_for_user(c, user)
    })
    .expect("send list")
    .into_iter()
    .map(|(_, token)| token)
    .collect::<Vec<_>>();
    tokens.sort();
    tokens
}

#[test]
fn signing_out_stops_that_device_receiving_notifications() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);
    let phone = session(&mut conn, user);
    register(&mut conn, user, phone, workspace, "phone");

    active_sessions::revoke_session_by_uuid(&mut conn, &phone).expect("sign out");

    assert!(send_list(&mut conn, user, workspace).is_empty());
    let actor = ActorContext::system("test:has_device").with_workspace(workspace);
    let has_device =
        with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            push_devices::has_active_device(c, user)
        })
        .expect("has device");
    assert!(
        !has_device,
        "push's default falls back to off with the device gone"
    );
}

#[test]
fn signing_out_other_sessions_keeps_this_device() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);
    let here = session(&mut conn, user);
    let elsewhere = session(&mut conn, user);
    register(&mut conn, user, here, workspace, "here");
    register(&mut conn, user, elsewhere, workspace, "elsewhere");

    active_sessions::revoke_other_sessions_by_uuid(&mut conn, &user, &here)
        .expect("sign out others");

    assert_eq!(send_list(&mut conn, user, workspace), ["here"]);
}

#[test]
fn signing_out_everywhere_releases_every_device_in_every_workspace() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let user = ws.a.member_uuid;
    let phone = session(&mut conn, user);
    let tablet = session(&mut conn, user);
    register(&mut conn, user, phone, ws.a.workspace_id, "phone");
    register(&mut conn, user, tablet, ws.b.workspace_id, "tablet");

    active_sessions::revoke_all_sessions(&mut conn, &user).expect("sign out everywhere");

    assert!(send_list(&mut conn, user, ws.a.workspace_id).is_empty());
}

#[test]
fn an_expired_session_releases_its_device_when_it_is_cleaned_up() {
    use backend::schema::active_sessions as s;
    use diesel::prelude::*;

    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);
    let phone = session(&mut conn, user);
    register(&mut conn, user, phone, workspace, "phone");
    let long_ago = (chrono::Utc::now() - chrono::Duration::days(10)).naive_utc();
    diesel::update(s::table.filter(s::session_id.eq(phone)))
        .set((
            s::created_at.eq(long_ago),
            s::last_active.eq(long_ago),
            s::expires_at.eq(long_ago + chrono::Duration::days(7)),
        ))
        .execute(&mut conn)
        .expect("expire session");

    active_sessions::cleanup_expired(&mut conn).expect("cleanup");

    assert!(send_list(&mut conn, user, workspace).is_empty());
}

/// Signing in again on the same phone registers the same token from the new
/// session. The token moves to it, so ending the old one no longer touches it.
#[test]
fn a_new_sign_in_on_the_same_phone_takes_the_token_over() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);
    let before = session(&mut conn, user);
    let after = session(&mut conn, user);
    register(&mut conn, user, before, workspace, "phone");
    register(&mut conn, user, after, workspace, "phone");

    active_sessions::revoke_session_by_uuid(&mut conn, &before).expect("end old session");
    assert_eq!(send_list(&mut conn, user, workspace), ["phone"]);

    active_sessions::revoke_session_by_uuid(&mut conn, &after).expect("sign out");
    assert!(send_list(&mut conn, user, workspace).is_empty());
}

/// POST /api/notifications/devices as `user` with `sid` (None: an API token).
async fn register_over_http(
    pool: &backend::db::Pool,
    user: Uuid,
    workspace: i32,
    sid: Option<Uuid>,
    token: &str,
) -> u16 {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Member".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: sid.map(|s| s.to_string()),
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(user, Some(corr)).with_workspace(workspace);
    let notifications =
        NotificationService::new(pool.clone(), Arc::new(RwLock::new(HashMap::new())));
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(notifications))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::notifications::config)),
    )
    .await;
    let req = http_test::TestRequest::post()
        .uri("/api/notifications/devices")
        .set_json(json!({ "platform": "ios", "token": token }))
        .to_request();
    http_test::call_service(&app, req).await.status().as_u16()
}

#[actix_web::test]
async fn the_endpoint_binds_the_device_to_the_calling_session() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let conn = &mut pool.get().expect("conn");
    let ws = crate::common::seed_two_workspaces(conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);
    let phone = session(conn, user);

    assert_eq!(
        register_over_http(&pool, user, workspace, Some(phone), "phone").await,
        200
    );
    assert_eq!(send_list(conn, user, workspace), ["phone"]);

    active_sessions::revoke_session_by_uuid(conn, &phone).expect("sign out");
    assert!(send_list(conn, user, workspace).is_empty());
}

/// An API token has no session, so a device it registered would never be
/// released.
#[actix_web::test]
async fn the_endpoint_refuses_a_caller_without_a_session() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let conn = &mut pool.get().expect("conn");
    let ws = crate::common::seed_two_workspaces(conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);

    assert_eq!(
        register_over_http(&pool, user, workspace, None, "api").await,
        403
    );
    assert!(send_list(conn, user, workspace).is_empty());
}

/// The session ended between authenticating the request and the insert.
#[actix_web::test]
async fn the_endpoint_refuses_a_session_that_has_ended() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let conn = &mut pool.get().expect("conn");
    let ws = crate::common::seed_two_workspaces(conn);
    let (user, workspace) = (ws.a.member_uuid, ws.a.workspace_id);
    let gone = session(conn, user);
    active_sessions::revoke_session_by_uuid(conn, &gone).expect("sign out");

    assert_eq!(
        register_over_http(&pool, user, workspace, Some(gone), "phone").await,
        401
    );
    assert!(send_list(conn, user, workspace).is_empty());
}
