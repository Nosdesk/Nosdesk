//! `DELETE /users/{uuid}` verifies the caller's re-authentication.
//!
//! The confirmation modal collects the deleting admin's password or 2FA code,
//! or the admin confirms the session first (passkey, identity provider); the
//! handler must check it, not just the admin role. Same rule as signing out
//! all other sessions: an empty credential passes only on a session confirmed
//! within the step-up window, whatever the account.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::sync::actor::ActorContext;

const WS: i32 = 1;
const PASSWORD: &str = "correct horse battery staple";

fn member(conn: &mut PgConnection, name: &str, role: &str) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(workspace_members::table)
        .values((
            workspace_members::workspace_id.eq(WS),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

fn give_local_password(conn: &mut PgConnection, user: &User) {
    use backend::schema::user_auth_identities;
    let hash = bcrypt::hash(PASSWORD, 4).expect("hash");
    diesel::insert_into(user_auth_identities::table)
        .values((
            user_auth_identities::user_uuid.eq(user.uuid),
            user_auth_identities::provider_type.eq("local"),
            user_auth_identities::external_id.eq(user.uuid.to_string()),
            user_auth_identities::password_hash.eq(hash),
        ))
        .execute(conn)
        .expect("insert local identity");
}

fn admin_claims(user: &User, sid: Option<Uuid>) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: sid.map(|s| s.to_string()),
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// A test server that runs every request as `admin` in workspace 1, on
/// session `sid` when given.
fn spawn(
    pool: &crate::common::TestPool,
    admin: &User,
    sid: Option<Uuid>,
) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = admin_claims(admin, sid);
    let admin_uuid = admin.uuid;
    actix_test::start(move || {
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(admin_uuid, Some(corr)).with_workspace(WS);
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: Uuid::nil(),
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/users/{uuid}",
                web::delete().to(backend::handlers::users::delete_user),
            )
    })
}

fn deleted_at(conn: &mut PgConnection, user: Uuid) -> Option<chrono::NaiveDateTime> {
    use backend::schema::users;
    users::table
        .find(user)
        .select(users::deleted_at)
        .first(conn)
        .expect("user row")
}

#[actix_web::test]
async fn deleting_a_user_requires_the_callers_password() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = member(&mut conn, "Admin", "admin");
    give_local_password(&mut conn, &admin);
    let target = member(&mut conn, "Target", "member");

    let srv = spawn(&pool, &admin, None);
    let url = srv.url(&format!("/users/{}", target.uuid));

    let resp = awc::Client::new()
        .delete(url.clone())
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 400, "no credential is refused");
    assert!(deleted_at(&mut conn, target.uuid).is_none());

    let resp = awc::Client::new()
        .delete(url.clone())
        .send_json(&json!({ "password": "wrong password" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400, "a wrong password is refused");
    assert!(deleted_at(&mut conn, target.uuid).is_none());

    let resp = awc::Client::new()
        .delete(url.clone())
        .send_json(&json!({ "mfa_code": "123456" }))
        .await
        .expect("send");
    assert_eq!(
        resp.status(),
        400,
        "a 2FA code without MFA enrolled is refused"
    );
    assert!(deleted_at(&mut conn, target.uuid).is_none());

    let mut resp = awc::Client::new()
        .delete(url)
        .send_json(&json!({ "password": PASSWORD }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "the correct password succeeds");
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert!(body["purge_at"].is_string(), "response carries purge_at");
    assert!(deleted_at(&mut conn, target.uuid).is_some());
}

/// A live session for `user`, last confirmed `confirmed_ago` (never when
/// `None`).
fn session(
    conn: &mut backend::db::DbConnection,
    user: &User,
    confirmed_ago: Option<chrono::Duration>,
) -> Uuid {
    let now = chrono::Utc::now();
    backend::repository::active_sessions::create_session(
        conn,
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: None,
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (now + chrono::Duration::days(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: confirmed_ago.map(|ago| (now - ago).naive_utc()),
        },
    )
    .expect("session")
    .session_id
}

#[actix_web::test]
async fn an_sso_only_caller_confirms_the_session_first() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = member(&mut conn, "Admin", "admin");
    let target = member(&mut conn, "Target", "member");
    let delete = |srv: &actix_test::TestServer| {
        awc::Client::new()
            .delete(srv.url(&format!("/users/{}", target.uuid)))
            .send_json(&json!({}))
    };

    // Nothing to type and nothing confirmed: refused (the old empty pass).
    let unconfirmed = session(&mut conn, &admin, None);
    let resp = delete(&spawn(&pool, &admin, Some(unconfirmed)))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400, "an unconfirmed session is refused");

    // Another session's confirmation covers only that session.
    let _other = session(&mut conn, &admin, Some(chrono::Duration::zero()));
    let resp = delete(&spawn(&pool, &admin, Some(unconfirmed)))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400, "another session's confirmation");

    let stale = session(
        &mut conn,
        &admin,
        Some(chrono::Duration::minutes(
            backend::services::step_up::WINDOW_MINUTES + 1,
        )),
    );
    let resp = delete(&spawn(&pool, &admin, Some(stale)))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400, "a confirmation past the window");

    // No session at all (a legacy token): refused.
    let resp = delete(&spawn(&pool, &admin, None)).await.expect("send");
    assert_eq!(resp.status(), 400, "no session to have confirmed");
    assert!(deleted_at(&mut conn, target.uuid).is_none());

    let fresh = session(&mut conn, &admin, Some(chrono::Duration::minutes(2)));
    let resp = delete(&spawn(&pool, &admin, Some(fresh)))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "a session confirmed in the window");
    assert!(deleted_at(&mut conn, target.uuid).is_some());
}

#[actix_web::test]
async fn a_correct_password_confirms_the_session_for_the_window() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = member(&mut conn, "Admin", "admin");
    give_local_password(&mut conn, &admin);
    let first = member(&mut conn, "First", "member");
    let second = member(&mut conn, "Second", "member");
    let sid = session(&mut conn, &admin, None);
    let srv = spawn(&pool, &admin, Some(sid));

    let resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", first.uuid)))
        .send_json(&json!({ "password": PASSWORD }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", second.uuid)))
        .send_json(&json!({}))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "the next deletion needs no password");
    assert!(deleted_at(&mut conn, second.uuid).is_some());
}
