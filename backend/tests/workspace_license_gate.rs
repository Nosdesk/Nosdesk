//! Self-hosted single-workspace license gate, enforced in
//! `admin_workspaces::create_workspace`.
//!
//! Contract (Community edition, self-hosted):
//!   1. GET  /admin/edition reports community / max 1 / active 1 / can't create.
//!   2. POST /admin/workspaces at the cap returns 402 `license_required`.
//!   3. With the seeded workspace archived (active = 0), a create succeeds.
//!   4. A second create (back at the cap) returns 402 again.
//!
//! The seeded `default` workspace (id 1) means a fresh deployment is already
//! at the Community cap of one active workspace.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::admin_workspaces;
use backend::middleware::dual_auth_middleware;
use backend::models::{NewUser, User};

mod common;

fn mint_platform_admin(conn: &mut diesel::pg::PgConnection, name: &str) -> User {
    use backend::schema::users;
    let new_user = NewUser {
        uuid: uuid::Uuid::new_v4(),
        name: name.to_string(),
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_secret_kek_id: None,
        mfa_enabled: false,
        platform_role: Some("platform_admin".to_string()),
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .get_result(conn)
        .expect("insert platform admin")
}

#[actix_web::test]
async fn self_hosted_caps_workspaces_at_one() {
    // Force self-hosted (the gated mode). Set before any
    // DeploymentMode::current() call, which caches process-wide. No
    // NOSDESK_LICENSE_KEY -> Community edition (cap = 1).
    std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
    std::env::remove_var("NOSDESK_LICENSE_KEY");
    common::ensure_test_keyring();

    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let admin = mint_platform_admin(&mut pool.get().expect("conn"), "PlatformAdmin");
    let admin_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &admin, "admin-session");

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/admin")
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route("/edition", web::get().to(admin_workspaces::get_edition))
                    .route(
                        "/workspaces",
                        web::post().to(admin_workspaces::create_workspace),
                    ),
            )
    });

    let client = awc::Client::new();
    let auth = || ("Authorization", format!("Bearer {admin_token}"));

    // --- 1: edition reflects the cap ---
    let mut resp = client
        .get(srv.url("/api/admin/edition"))
        .insert_header(auth())
        .send()
        .await
        .expect("send edition");
    assert_eq!(resp.status(), 200);
    let edition: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(edition["edition"], "community");
    assert_eq!(edition["max_workspaces"], 1);
    assert_eq!(edition["active_workspaces"], 1);
    assert_eq!(edition["can_create_workspace"], false);

    // --- 2: create at the cap is rejected ---
    let resp = client
        .post(srv.url("/api/admin/workspaces"))
        .insert_header(auth())
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "second-ws", "name": "Second"}))
        .await
        .expect("send create-at-cap");
    assert_eq!(
        resp.status(),
        402,
        "creating a second workspace on self-hosted Community must be blocked"
    );
    let mut resp = resp;
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["error"], "license_required");

    // --- 3: archive the seeded workspace, then a create succeeds ---
    diesel::sql_query("UPDATE workspaces SET archived_at = now() WHERE id = 1")
        .execute(&mut pool.get().expect("conn"))
        .expect("archive seed workspace");

    let resp = client
        .post(srv.url("/api/admin/workspaces"))
        .insert_header(auth())
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "only-ws", "name": "Only"}))
        .await
        .expect("send create-under-cap");
    assert_eq!(
        resp.status(),
        201,
        "create under the cap (active=0) must succeed"
    );

    // --- 4: now back at the cap, another create is rejected ---
    let resp = client
        .post(srv.url("/api/admin/workspaces"))
        .insert_header(auth())
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "third-ws", "name": "Third"}))
        .await
        .expect("send create-over-cap");
    assert_eq!(
        resp.status(),
        402,
        "back at the cap, create is blocked again"
    );
}
