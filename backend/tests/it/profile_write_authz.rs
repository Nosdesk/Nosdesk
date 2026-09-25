//! Who may change which part of a person's profile.
//!
//! Handler-level: the auth middleware is replaced by a `wrap_fn` that injects
//! the extensions it would (Claims, WorkspaceContext, RequestContext), so each
//! request runs as a chosen user.

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

fn email(conn: &mut PgConnection, user: Uuid, address: &str, primary: bool, verified: bool) -> i32 {
    use backend::schema::user_emails;
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user),
            user_emails::email.eq(address),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(primary),
            user_emails::is_verified.eq(verified),
        ))
        .returning(user_emails::id)
        .get_result(conn)
        .expect("insert email")
}

fn claims_for(user: &User) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// A test server that runs every request as `user` in workspace 1.
fn spawn(pool: &crate::common::TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = claims_for(user);
    let user_uuid = user.uuid;
    actix_test::start(move || {
        let tmp = tempfile::tempdir().expect("temp search dir");
        let search = Arc::new(SearchService::new(tmp.path(), &pool).expect("init search"));
        std::mem::forget(tmp);
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
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
            .app_data(web::Data::new(search))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/users/{uuid}/emails/{email_id}",
                web::put().to(backend::handlers::users::update_user_email),
            )
            .route(
                "/users/{uuid}/image",
                web::post().to(backend::handlers::users::upload_user_image),
            )
            .route(
                "/users/{uuid}/profile-fields",
                web::put().to(backend::handlers::user_contact::set_user_profile_fields),
            )
            .route(
                "/users/{uuid}/profile-fields",
                web::get().to(backend::handlers::user_contact::get_user_profile_fields),
            )
    })
}

#[actix_web::test]
async fn an_email_id_from_another_user_is_not_found_and_unchanged() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let alice = member(&mut conn, "Alice", "member");
    let bob = member(&mut conn, "Bob", "member");
    email(&mut conn, alice.uuid, "alice@example.com", true, true);
    let bobs = email(&mut conn, bob.uuid, "bob-alt@example.com", false, true);

    let srv = spawn(&pool, &alice);
    let resp = awc::Client::new()
        .put(srv.url(&format!("/users/{}/emails/{bobs}", alice.uuid)))
        .send_json(&json!({ "is_primary": true }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 404);

    use backend::schema::user_emails;
    let (owner, primary): (Uuid, bool) = user_emails::table
        .find(bobs)
        .select((user_emails::user_uuid, user_emails::is_primary))
        .first(&mut conn)
        .expect("row");
    assert_eq!(owner, bob.uuid);
    assert!(!primary, "Bob's address untouched");
    let alice_primary: i64 = user_emails::table
        .filter(user_emails::user_uuid.eq(alice.uuid))
        .filter(user_emails::is_primary.eq(true))
        .count()
        .get_result(&mut conn)
        .expect("count");
    assert_eq!(alice_primary, 1, "Alice keeps her primary");
}

#[actix_web::test]
async fn an_unverified_address_cannot_become_primary() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let alice = member(&mut conn, "Alice", "member");
    email(&mut conn, alice.uuid, "alice@example.com", true, true);
    let unproven = email(
        &mut conn,
        alice.uuid,
        "someone-else@example.com",
        false,
        false,
    );

    let srv = spawn(&pool, &alice);
    let resp = awc::Client::new()
        .put(srv.url(&format!("/users/{}/emails/{unproven}", alice.uuid)))
        .send_json(&json!({ "is_primary": true }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn only_the_person_can_replace_their_images() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let alice = member(&mut conn, "Alice", "member");
    let bob = member(&mut conn, "Bob", "admin");

    let srv = spawn(&pool, &alice);
    let resp = awc::Client::new()
        .post(srv.url(&format!("/users/{}/image?type_=banner", bob.uuid)))
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .send_body("--x--\r\n")
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);
}

#[actix_web::test]
async fn the_workspace_name_is_the_persons_own_and_survives_a_contact_save() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let alice = member(&mut conn, "Alice", "agent");
    let admin = member(&mut conn, "Admin", "admin");
    let url =
        |srv: &actix_test::TestServer| srv.url(&format!("/users/{}/profile-fields", alice.uuid));
    let client = awc::Client::new();

    // Alice sets her name in this workspace.
    let as_alice = spawn(&pool, &alice);
    let resp = client
        .put(url(&as_alice))
        .send_json(&json!({ "job_title": "Engineer", "display_name": "Ali", "custom_fields": {} }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);

    // A contact-card save (no persona fields) keeps it.
    let resp = client
        .put(url(&as_alice))
        .send_json(&json!({ "job_title": "Lead", "custom_fields": {} }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);

    // An admin editing her contact card can't change it either.
    let as_admin = spawn(&pool, &admin);
    let resp = client
        .put(url(&as_admin))
        .send_json(&json!({ "job_title": "Lead", "display_name": "Renamed", "custom_fields": {} }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);

    use backend::schema::user_profiles;
    let (name, title): (Option<String>, Option<String>) = user_profiles::table
        .filter(user_profiles::user_uuid.eq(alice.uuid))
        .select((user_profiles::display_name, user_profiles::job_title))
        .first(&mut conn)
        .expect("profile");
    assert_eq!(name.as_deref(), Some("Ali"));
    assert_eq!(title.as_deref(), Some("Lead"));
}
