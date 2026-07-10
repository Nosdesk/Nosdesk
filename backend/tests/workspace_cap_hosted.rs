//! Regression: the workspace cap is gated on the license, not the deployment
//! mode. `NOSDESK_DEPLOYMENT_MODE` is an env var, so keying the self-serve
//! admin create gate on it let a self-hoster flip to `hosted` and create
//! unlimited workspaces with no Enterprise license. This proves that even in
//! hosted mode (no license -> Community edition), the self-serve
//! `/api/admin/workspaces` create is capped at one active workspace.
//!
//! Hosted deployments provision through the control-plane `/api/internal`
//! surface instead, which is a separate route this test does not touch.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use serde_json::json;

use backend::handlers::admin_workspaces;
use backend::middleware::dual_auth_middleware;
use backend::models::{NewUser, User};

mod common;

fn mint_platform_admin(conn: &mut diesel::pg::PgConnection, name: &str) -> User {
    use backend::schema::users;
    use diesel::prelude::*;
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
async fn hosted_mode_does_not_bypass_the_workspace_cap() {
    // Hosted mode, no license -> Community edition (cap = 1). Set before any
    // DeploymentMode::current() call, which caches process-wide.
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
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

    // The seeded `default` workspace already fills the Community slot, so even
    // in hosted mode the edition summary must report the cap reached.
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
    assert_eq!(
        edition["can_create_workspace"], false,
        "hosted mode must not report an uncapped self-serve create"
    );

    // The actual create must be blocked (402), not silently allowed as it was
    // when the gate keyed on the deployment mode.
    let resp = client
        .post(srv.url("/api/admin/workspaces"))
        .insert_header(auth())
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "bypass-attempt", "name": "Bypass"}))
        .await
        .expect("send create");
    assert_eq!(
        resp.status(),
        402,
        "hosted mode must not bypass the Community workspace cap"
    );
    let mut resp = resp;
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["error"], "license_required");
}
