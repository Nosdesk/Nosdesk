//! `DELETE /users/{uuid}` verifies the caller's re-authentication.
//!
//! The confirmation modal collects the deleting admin's password or 2FA code;
//! the handler must check it, not just the admin role. Same rule as signing
//! out all other sessions: an account with nothing to step up with (no local
//! password, no MFA) passes on a full session alone.

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

fn admin_claims(user: &User) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// A test server that runs every request as `admin` in workspace 1.
fn spawn(pool: &crate::common::TestPool, admin: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = admin_claims(admin);
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

    let srv = spawn(&pool, &admin);
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

#[actix_web::test]
async fn an_sso_only_caller_without_mfa_steps_up_with_the_session_alone() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = member(&mut conn, "Admin", "admin");
    let target = member(&mut conn, "Target", "member");

    let srv = spawn(&pool, &admin);
    let resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", target.uuid)))
        .send_json(&json!({}))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    assert!(deleted_at(&mut conn, target.uuid).is_some());
}
