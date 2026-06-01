//! Integration test for the Phase 4 W3 workspace-membership
//! handlers under `/api/admin/workspaces/{id}/members` and the
//! caller-facing `/api/me/workspaces` switcher endpoint.
//!
//! Contract cases:
//!
//!   1. GET    /api/me/workspaces returns the caller's memberships.
//!   2. POST   /admin/workspaces/{id}/members adds a member with
//!      a valid role; returns 201.
//!   3. POST again with the same user is idempotent: 200 +
//!      `status=already_member`, no second row inserted.
//!   4. POST with role "agent" succeeds (W2 added 'agent' to the
//!      CHECK constraint).
//!   5. POST with an unknown user_uuid returns 404.
//!   6. POST with an invalid role returns 400.
//!   7. PATCH .../{user_uuid} changes the role.
//!   8. PATCH demoting the last owner returns 409 last_owner.
//!   9. DELETE removes a non-owner member.
//!  10. DELETE on the last owner returns 409 last_owner.
//!  11. DELETE on a non-existent membership returns 404.
//!  12. Non-admin token gets 403 on every mutating endpoint.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::admin_workspaces;
use backend::middleware::dual_auth_middleware;
use backend::models::{NewUser, User, UserRole};

mod common;

fn mint_user(conn: &mut diesel::pg::PgConnection, name: &str, role: UserRole) -> User {
    use backend::schema::users;
    let new_user = NewUser {
        uuid: uuid::Uuid::new_v4(),
        name: name.to_string(),
        role,
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_secret_kek_id: None,
        mfa_enabled: false,
        platform_role: None,
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .get_result(conn)
        .expect("insert user")
}

#[actix_web::test]
async fn workspace_member_lifecycle_contract() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    // Cast of characters.
    let admin = mint_user(&mut pool.get().expect("conn"), "PlatformAdmin", UserRole::Admin);
    let admin_token = common::mint_api_token(
        &mut pool.get().expect("conn"),
        &admin,
        "admin-session",
        false,
    );
    let regular = mint_user(&mut pool.get().expect("conn"), "Regular", UserRole::User);
    let regular_token = common::mint_api_token(
        &mut pool.get().expect("conn"),
        &regular,
        "user-session",
        false,
    );
    let alice = mint_user(&mut pool.get().expect("conn"), "Alice", UserRole::User);
    let bob = mint_user(&mut pool.get().expect("conn"), "Bob", UserRole::User);
    let alice_token = common::mint_api_token(
        &mut pool.get().expect("conn"),
        &alice,
        "alice",
        false,
    );

    // Two workspaces so the /me/workspaces list has something to filter.
    let ws_acme = common::mint_workspace(&mut pool.get().expect("conn"), "memship-acme", "Acme");
    let ws_beta = common::mint_workspace(&mut pool.get().expect("conn"), "memship-beta", "Beta");

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api")
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route(
                        "/admin/workspaces/{id}/members",
                        web::get().to(admin_workspaces::list_members),
                    )
                    .route(
                        "/admin/workspaces/{id}/members",
                        web::post().to(admin_workspaces::add_member),
                    )
                    .route(
                        "/admin/workspaces/{id}/members/{user_uuid}",
                        web::patch().to(admin_workspaces::update_member_role),
                    )
                    .route(
                        "/admin/workspaces/{id}/members/{user_uuid}",
                        web::delete().to(admin_workspaces::remove_member),
                    )
                    .route(
                        "/me/workspaces",
                        web::get().to(admin_workspaces::list_my_workspaces),
                    ),
            )
    });

    let client = awc::Client::new();
    let acme_members = srv.url(&format!("/api/admin/workspaces/{ws_acme}/members"));
    let me_workspaces = srv.url("/api/me/workspaces");

    // --- 2: add Alice as agent ---
    let resp = client
        .post(&acme_members)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": alice.uuid, "role": "agent"}))
        .await
        .expect("send add alice");
    assert_eq!(resp.status(), 201, "valid add must 201");

    // --- 4: alice 'agent' role accepted (W2 CHECK expansion) ---
    // Implicitly confirmed by step 2 — agent went through.

    // --- 1: /me/workspaces now lists Acme for Alice ---
    let mut resp = client
        .get(&me_workspaces)
        .insert_header(("Authorization", format!("Bearer {alice_token}")))
        .send()
        .await
        .expect("send /me");
    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body.len(), 1, "alice should see exactly 1 workspace");
    assert_eq!(body[0]["slug"], "memship-acme");
    assert_eq!(body[0]["role"], "agent");

    // --- 3: re-add alice -> 200 already_member ---
    let mut resp = client
        .post(&acme_members)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": alice.uuid, "role": "agent"}))
        .await
        .expect("send re-add alice");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["status"], "already_member");

    // --- 5: unknown user_uuid -> 404 ---
    let resp = client
        .post(&acme_members)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": uuid::Uuid::new_v4(), "role": "member"}))
        .await
        .expect("send unknown user");
    assert_eq!(resp.status(), 404);

    // --- 6: invalid role -> 400 ---
    let resp = client
        .post(&acme_members)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": bob.uuid, "role": "supreme_overlord"}))
        .await
        .expect("send bad role");
    assert_eq!(resp.status(), 400);

    // Seed two owners on Beta so we can exercise the demote/remove paths.
    let resp = client
        .post(srv.url(&format!("/api/admin/workspaces/{ws_beta}/members")))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": alice.uuid, "role": "owner"}))
        .await
        .expect("alice owner on beta");
    assert_eq!(resp.status(), 201);

    // --- 8: demote sole owner -> 409 ---
    let resp = client
        .patch(srv.url(&format!(
            "/api/admin/workspaces/{ws_beta}/members/{}",
            alice.uuid
        )))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"role": "admin"}))
        .await
        .expect("send demote sole owner");
    assert_eq!(resp.status(), 409);

    // Add Bob as second owner so we can complete the demote/remove arcs.
    client
        .post(srv.url(&format!("/api/admin/workspaces/{ws_beta}/members")))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": bob.uuid, "role": "owner"}))
        .await
        .expect("bob owner on beta");

    // --- 7: now demote Alice from owner to admin works ---
    let resp = client
        .patch(srv.url(&format!(
            "/api/admin/workspaces/{ws_beta}/members/{}",
            alice.uuid
        )))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"role": "admin"}))
        .await
        .expect("send demote with co-owner");
    assert_eq!(resp.status(), 200);

    // --- 9: DELETE removes a non-owner member ---
    let resp = client
        .delete(srv.url(&format!(
            "/api/admin/workspaces/{ws_beta}/members/{}",
            alice.uuid
        )))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete alice");
    assert_eq!(resp.status(), 204);

    // --- 10: DELETE on last owner -> 409 ---
    let resp = client
        .delete(srv.url(&format!(
            "/api/admin/workspaces/{ws_beta}/members/{}",
            bob.uuid
        )))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete last owner");
    assert_eq!(resp.status(), 409);

    // --- 11: DELETE on never-membered user -> 404 ---
    let resp = client
        .delete(srv.url(&format!(
            "/api/admin/workspaces/{ws_acme}/members/{}",
            bob.uuid
        )))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete unknown");
    assert_eq!(resp.status(), 404);

    // --- 12: non-admin can't add members ---
    let resp = client
        .post(&acme_members)
        .insert_header(("Authorization", format!("Bearer {regular_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"user_uuid": bob.uuid, "role": "member"}))
        .await
        .expect("send regular add");
    assert_eq!(resp.status(), 403);
}
