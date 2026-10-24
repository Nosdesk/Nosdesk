//! A workspace admin manages the people of their own workspace from their
//! settings page.
//!
//! Workspace-scoped data (groups) is the workspace's to manage for any member.
//! The account itself (name, photo, email addresses, language) is the user row
//! every workspace the person is in shares, so a workspace admin may change it
//! only for someone who belongs to no other workspace, the rule delete follows.
//! A password, signature or role never goes through the profile update.

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

fn email(conn: &mut PgConnection, user: &User, address: &str) {
    use backend::schema::user_emails;
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user.uuid),
            user_emails::email.eq(address),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(conn)
        .expect("insert email");
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
                "/users/{uuid}/emails",
                web::get().to(backend::handlers::users::get_user_emails),
            )
            .route(
                "/users/{uuid}/emails",
                web::post().to(backend::handlers::users::add_user_email),
            )
            .route(
                "/users/{uuid}/groups",
                web::get().to(backend::handlers::groups::get_user_groups),
            )
            .route(
                "/users/{uuid}/profile",
                web::get().to(backend::handlers::users::get_user_profile_bundle),
            )
            .route(
                "/users/{uuid}/auth-identities",
                web::get().to(backend::handlers::users::get_user_auth_identities_by_uuid),
            )
            .route(
                "/users/{uuid}",
                web::put().to(backend::handlers::users::update_user_by_uuid),
            )
    })
}

fn name_of(conn: &mut PgConnection, user: Uuid) -> String {
    use backend::schema::users;
    users::table
        .find(user)
        .select(users::name)
        .first(conn)
        .expect("user row")
}

#[actix_web::test]
async fn a_workspace_admin_manages_the_account_of_their_own_requester() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = user(&mut conn, "Admin");
    join(&mut conn, WS, &admin, "admin");
    let requester = user(&mut conn, "Requester");
    join(&mut conn, WS, &requester, "member");
    email(&mut conn, &requester, "requester@example.test");
    let srv = spawn(&pool, &admin);
    let client = awc::Client::new();

    let mut resp = client
        .get(srv.url(&format!("/users/{}/emails", requester.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "reads their addresses");
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["emails"][0]["email"], "requester@example.test");

    let resp = client
        .post(srv.url(&format!("/users/{}/emails", requester.uuid)))
        .send_json(&json!({ "email": "requester.new@example.test" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 201, "adds one, unverified");

    let resp = client
        .put(srv.url(&format!("/users/{}", requester.uuid)))
        .send_json(&json!({ "name": "Renamed", "pronouns": "they/them" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "renames them");
    assert_eq!(name_of(&mut conn, requester.uuid), "Renamed");

    let resp = client
        .get(srv.url(&format!("/users/{}/groups", requester.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "reads their groups");

    let resp = client
        .get(srv.url(&format!("/users/{}/auth-identities", requester.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(
        resp.status(),
        200,
        "reads their sign-in methods (self-hosted)"
    );

    let mut resp = client
        .get(srv.url(&format!("/users/{}/profile", requester.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let bundle: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(bundle["editable"]["name"], true);
    assert_eq!(bundle["editable"]["emails"], true);
    assert_eq!(bundle["editable"]["groups"], true);
    assert!(
        bundle["emails"].is_array(),
        "the bundle carries their addresses"
    );
}

#[actix_web::test]
async fn the_profile_update_takes_no_role_password_or_signature_from_an_admin() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let admin = user(&mut conn, "Admin");
    join(&mut conn, WS, &admin, "admin");
    let requester = user(&mut conn, "Requester");
    join(&mut conn, WS, &requester, "member");
    let srv = spawn(&pool, &admin);
    let client = awc::Client::new();

    for body in [
        json!({ "role": "platform_admin" }),
        json!({ "role": "admin" }),
        json!({ "password": "correct horse battery staple" }),
        json!({ "signature": "Hi" }),
        json!({ "name": "Renamed", "role": "user" }),
    ] {
        let resp = client
            .put(srv.url(&format!("/users/{}", requester.uuid)))
            .send_json(&body)
            .await
            .expect("send");
        assert_eq!(resp.status(), 403, "refuses {body}");
    }

    use backend::schema::users;
    let platform_role: String = users::table
        .find(requester.uuid)
        .select(users::platform_role)
        .first(&mut conn)
        .expect("user row");
    assert_ne!(platform_role, "platform_admin");
    assert_eq!(
        name_of(&mut conn, requester.uuid),
        "Requester",
        "nothing applied"
    );
}

#[actix_web::test]
async fn someone_also_in_another_workspace_keeps_their_account() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let theirs = crate::common::mint_workspace(&mut conn, "settings-theirs", "Theirs");
    let admin = user(&mut conn, "Admin");
    join(&mut conn, WS, &admin, "admin");
    let shared = user(&mut conn, "Shared");
    join(&mut conn, WS, &shared, "member");
    join(&mut conn, theirs, &shared, "member");
    email(&mut conn, &shared, "shared@example.test");
    let stranger = user(&mut conn, "Stranger");
    join(&mut conn, theirs, &stranger, "member");
    let srv = spawn(&pool, &admin);
    let client = awc::Client::new();

    let mut resp = client
        .get(srv.url(&format!("/users/{}/emails", shared.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "member_elsewhere");

    let mut resp = client
        .put(srv.url(&format!("/users/{}", shared.uuid)))
        .send_json(&json!({ "name": "Renamed" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "member_elsewhere");
    assert_eq!(name_of(&mut conn, shared.uuid), "Shared");

    let mut resp = client
        .get(srv.url(&format!("/users/{}/profile", shared.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "their profile still loads");
    let bundle: serde_json::Value = resp.json().await.expect("json");
    assert!(bundle.get("emails").is_none(), "without their addresses");
    assert_eq!(bundle["editable"]["name"], false);
    assert_eq!(bundle["editable"]["emails"], false);
    assert_eq!(bundle["editable"]["account_locked"], "member_elsewhere");
    assert_eq!(
        bundle["editable"]["groups"], true,
        "groups are this workspace's"
    );

    let resp = client
        .get(srv.url(&format!("/users/{}/groups", shared.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);

    let resp = client
        .get(srv.url(&format!("/users/{}/emails", stranger.uuid)))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 404, "someone only in another workspace");
}

#[actix_web::test]
async fn an_agent_manages_no_one_elses_account() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let agent = user(&mut conn, "Agent");
    join(&mut conn, WS, &agent, "agent");
    let requester = user(&mut conn, "Requester");
    join(&mut conn, WS, &requester, "member");
    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();

    for path in ["emails", "groups", "auth-identities"] {
        let resp = client
            .get(srv.url(&format!("/users/{}/{path}", requester.uuid)))
            .send()
            .await
            .expect("send");
        assert_eq!(resp.status(), 403, "{path}");
    }
    let resp = client
        .put(srv.url(&format!("/users/{}", requester.uuid)))
        .send_json(&json!({ "name": "Renamed" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
    assert_eq!(name_of(&mut conn, requester.uuid), "Requester");
}
