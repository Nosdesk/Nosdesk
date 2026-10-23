//! A workspace admin deletes and restores the people of their own workspace.
//!
//! The user row is shared by every workspace the person is in, so a workspace
//! admin may delete only someone who belongs to no other workspace; a platform
//! admin may delete anyone (see `user_delete_reauth`). Refusals come before
//! the step-up, so nobody confirms it's them for a delete that can't happen.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;

const WS: i32 = 1;
const PASSWORD: &str = "correct horse battery staple";

fn user(conn: &mut PgConnection, name: &str) -> User {
    use backend::schema::users;
    diesel::insert_into(users::table)
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
        .expect("insert user")
}

fn join(conn: &mut PgConnection, workspace_id: i32, user: &User, role: &str) {
    use backend::schema::workspace_members;
    diesel::insert_into(workspace_members::table)
        .values((
            workspace_members::workspace_id.eq(workspace_id),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert member");
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

/// A test server that runs every request as `caller`, who holds no platform
/// role, in workspace 1.
fn spawn(pool: &crate::common::TestPool, caller: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = Claims {
        sub: caller.uuid.to_string(),
        name: caller.name.clone(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let caller_uuid = caller.uuid;
    let search_dir = Arc::new(tempfile::tempdir().expect("search dir"));
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    actix_test::start(move || {
        let _keep = &search_dir;
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(caller_uuid, Some(corr)).with_workspace(WS);
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
            .app_data(web::Data::new(search.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/users/bulk",
                web::post().to(backend::handlers::users::bulk_users),
            )
            .route(
                "/users/{uuid}",
                web::delete().to(backend::handlers::users::delete_user),
            )
            .route(
                "/users/{uuid}/restore",
                web::post().to(backend::handlers::users::restore_user),
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
async fn a_workspace_admin_deletes_and_restores_their_own_requester() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = user(&mut conn, "Admin");
    join(&mut conn, WS, &admin, "admin");
    give_local_password(&mut conn, &admin);
    let requester = user(&mut conn, "Requester");
    join(&mut conn, WS, &requester, "member");
    let srv = spawn(&pool, &admin);

    let resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", requester.uuid)))
        .send_json(&json!({ "password": PASSWORD }))
        .await
        .expect("send");
    assert_eq!(
        resp.status(),
        200,
        "a workspace admin deletes their requester"
    );
    assert!(deleted_at(&mut conn, requester.uuid).is_some());

    let resp = awc::Client::new()
        .post(srv.url(&format!("/users/{}/restore", requester.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "and can undo it");
    assert!(deleted_at(&mut conn, requester.uuid).is_none());
}

#[actix_web::test]
async fn a_workspace_admin_cannot_reach_other_workspaces_people() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let theirs = crate::common::mint_workspace(&mut conn, "deluser-theirs", "Theirs");
    let admin = user(&mut conn, "Admin");
    join(&mut conn, WS, &admin, "admin");
    give_local_password(&mut conn, &admin);
    let stranger = user(&mut conn, "Stranger");
    join(&mut conn, theirs, &stranger, "member");
    let shared = user(&mut conn, "Shared");
    join(&mut conn, WS, &shared, "member");
    join(&mut conn, theirs, &shared, "member");
    let srv = spawn(&pool, &admin);

    let resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", stranger.uuid)))
        .send_json(&json!({ "password": PASSWORD }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 404, "someone only in another workspace");
    assert!(deleted_at(&mut conn, stranger.uuid).is_none());

    // No credential: the refusal must come first, not the step-up's 400.
    let mut resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", shared.uuid)))
        .send_json(&json!({}))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403, "someone also in another workspace");
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["code"], "member_elsewhere");
    assert!(deleted_at(&mut conn, shared.uuid).is_none());

    // Restoring follows the same rule.
    diesel::update(backend::schema::users::table.find(stranger.uuid))
        .set(backend::schema::users::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
        .execute(&mut conn)
        .expect("soft delete");
    let resp = awc::Client::new()
        .post(srv.url(&format!("/users/{}/restore", stranger.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp.status(),
        404,
        "restoring someone only in another workspace"
    );
    assert!(deleted_at(&mut conn, stranger.uuid).is_some());
}

#[actix_web::test]
async fn an_agent_cannot_delete() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let agent = user(&mut conn, "Agent");
    join(&mut conn, WS, &agent, "agent");
    give_local_password(&mut conn, &agent);
    let requester = user(&mut conn, "Requester");
    join(&mut conn, WS, &requester, "member");
    let srv = spawn(&pool, &agent);

    let resp = awc::Client::new()
        .delete(srv.url(&format!("/users/{}", requester.uuid)))
        .send_json(&json!({ "password": PASSWORD }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
    let resp = awc::Client::new()
        .post(srv.url("/users/bulk"))
        .send_json(&json!({ "action": "delete", "ids": [requester.uuid], "password": PASSWORD }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
    assert!(deleted_at(&mut conn, requester.uuid).is_none());
}

#[actix_web::test]
async fn a_workspace_admin_bulk_deletes_only_their_own_people() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let theirs = crate::common::mint_workspace(&mut conn, "deluser-bulk", "Theirs");
    let admin = user(&mut conn, "Admin");
    join(&mut conn, WS, &admin, "admin");
    give_local_password(&mut conn, &admin);
    let ours = user(&mut conn, "Ours");
    join(&mut conn, WS, &ours, "member");
    let shared = user(&mut conn, "Shared");
    join(&mut conn, WS, &shared, "member");
    join(&mut conn, theirs, &shared, "member");
    let stranger = user(&mut conn, "Stranger");
    join(&mut conn, theirs, &stranger, "member");
    let srv = spawn(&pool, &admin);

    let mut resp = awc::Client::new()
        .post(srv.url("/users/bulk"))
        .send_json(&json!({
            "action": "delete",
            "ids": [ours.uuid, shared.uuid, stranger.uuid],
            "password": PASSWORD,
        }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["affected"], 1);
    assert_eq!(body["skipped_elsewhere"], 1);
    assert_eq!(body["failed"], 1, "the stranger is not ours to delete");
    assert!(deleted_at(&mut conn, ours.uuid).is_some());
    assert!(deleted_at(&mut conn, shared.uuid).is_none());
    assert!(deleted_at(&mut conn, stranger.uuid).is_none());

    // Every other bulk action stays with platform admins.
    let resp = awc::Client::new()
        .post(srv.url("/users/bulk"))
        .send_json(&json!({ "action": "set-role", "ids": [shared.uuid], "value": "admin" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
}
