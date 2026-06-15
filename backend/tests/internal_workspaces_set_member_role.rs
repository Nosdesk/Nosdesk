//! Integration test for
//! `POST /api/internal/v1/workspaces/{slug}/members/set_role`.
//!
//! The control plane promotes/demotes an existing projected member's
//! role (e.g. self-registered `member` -> billed `agent`).
//! `upsert_projected_user` is first-write-wins and never mutates an
//! existing role; this endpoint is the sanctioned path to change one.
//!
//! Drives the route through the same middleware stack production uses
//! (idempotency -> handler, gated by the `PlatformAuth` EdDSA-JWT
//! extractor). Members are seeded via the real `upsert_projected_user`
//! endpoint so the `(iss, sub)` identity the handler resolves against
//! is wired exactly as it would be in production. Verifies:
//!
//!   1. member -> agent: 200 and the DB membership role flips.
//!   2. member -> agent at seat_limit=1 (owner already holds the seat):
//!      403 `seat_limit_reached`.
//!   3. Unknown member (no `(iss, sub)` identity) -> 404.
//!   4. role `owner` -> 400 (only admin|agent|member are settable).
//!   5. Wrong-scope token -> 401; absent token -> 401.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::idempotency_middleware;

mod common;

fn membership_role(pool: &backend::db::Pool, workspace_id: i32, user_uuid: uuid::Uuid) -> String {
    use backend::schema::workspace_members;
    let mut conn = pool.get().expect("conn");
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .select(workspace_members::role)
        .first(&mut conn)
        .expect("load membership role")
}

/// Count the user's membership rows in a workspace (0 or 1).
fn membership_count(pool: &backend::db::Pool, workspace_id: i32, user_uuid: uuid::Uuid) -> i64 {
    use backend::schema::workspace_members;
    let mut conn = pool.get().expect("conn");
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .count()
        .get_result(&mut conn)
        .expect("count memberships")
}

/// Delete the user's membership row, simulating a projected user whose
/// `workspace_members` row never materialised (the production bug
/// set_member_role's create-if-absent path guards against).
fn delete_membership(pool: &backend::db::Pool, workspace_id: i32, user_uuid: uuid::Uuid) {
    use backend::schema::workspace_members;
    let mut conn = pool.get().expect("conn");
    diesel::delete(
        workspace_members::table
            .filter(workspace_members::workspace_id.eq(workspace_id))
            .filter(workspace_members::user_uuid.eq(user_uuid)),
    )
    .execute(&mut conn)
    .expect("delete membership");
}

/// Set the workspace's staff seat cap straight on the row. The seat
/// trigger only fires on `workspace_members` changes, so this is a
/// plain UPDATE; the test pool conn writes `workspaces` directly the
/// same way `mint_workspace` inserts it.
fn set_seat_limit_direct(pool: &backend::db::Pool, workspace_id: i32, limit: i32) {
    use backend::schema::workspaces;
    let mut conn = pool.get().expect("conn");
    diesel::update(workspaces::table.filter(workspaces::id.eq(workspace_id)))
        .set(workspaces::seat_limit.eq(limit))
        .execute(&mut conn)
        .expect("set seat_limit");
}

/// Project a member via the real upsert endpoint and return its
/// user_uuid. Keeps the `(iss, sub)` identity wiring identical to
/// production so `set_role`'s lookup resolves the same row.
async fn project_member(
    client: &awc::Client,
    upsert_url: &str,
    token: &str,
    iss: &str,
    sub: &str,
    role: &str,
) -> uuid::Uuid {
    let mut resp = client
        .post(upsert_url)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": iss,
            "sub": sub,
            "email": format!("{sub}@acme.example"),
            "name": sub,
            "role": role,
        }))
        .await
        .expect("project member");
    assert!(
        resp.status().is_success(),
        "projection must succeed, got {}",
        resp.status()
    );
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    body["user_uuid"]
        .as_str()
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("user_uuid")
}

#[actix_web::test]
async fn set_member_role_full_contract() {
    common::ensure_test_keyring();
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let _admin = common::insert_user(&mut pool.get().expect("conn"), "M5Admin");
    let platform_token = common::mint_platform_jwt("platform:provision", 300);
    let wrong_scope_token = common::mint_platform_jwt("platform:other", 300);

    let acme_id = common::mint_workspace(&mut pool.get().expect("conn"), "acme", "Acme");
    let capco_id = common::mint_workspace(&mut pool.get().expect("conn"), "capco", "Capco");
    // Cap Capco to a single staff seat before any membership is projected.
    set_seat_limit_direct(&pool, capco_id, 1);

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/internal/v1")
                    .wrap(actix_web::middleware::from_fn(idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(
                        backend::extractors::platform_auth_middleware,
                    ))
                    .route(
                        "/workspaces/{slug}/upsert_projected_user",
                        web::post().to(internal_workspaces::upsert_projected_user),
                    )
                    .route(
                        "/workspaces/{slug}/members/set_role",
                        web::post().to(internal_workspaces::set_member_role),
                    ),
            )
    });

    let client = awc::Client::new();
    let acme_url = srv.url("/api/internal/v1/workspaces/acme/members/set_role");
    let acme_upsert = srv.url("/api/internal/v1/workspaces/acme/upsert_projected_user");
    let capco_upsert = srv.url("/api/internal/v1/workspaces/capco/upsert_projected_user");

    // --- 1: member -> agent promotes, 200 + DB role flips ---
    let member_uuid = project_member(
        &client,
        &acme_upsert,
        &platform_token,
        "https://idp.example/",
        "acme-member-1",
        "member",
    )
    .await;
    assert_eq!(membership_role(&pool, acme_id, member_uuid), "member");

    let mut resp = client
        .post(&acme_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "acme-member-1",
            "role": "agent",
        }))
        .await
        .expect("send promote");
    assert_eq!(resp.status(), 200, "member -> agent must 200");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["role"], "agent");
    assert_eq!(body["workspace_id"], acme_id);
    assert_eq!(body["user_uuid"], member_uuid.to_string());
    assert_eq!(
        membership_role(&pool, acme_id, member_uuid),
        "agent",
        "DB role must flip to agent"
    );

    // --- 1b: identity exists but membership row is missing -> create it ---
    // The user was projected (so `(iss, sub)` resolves) but has no
    // workspace_members row. set_member_role must UPSERT: create the row
    // with the requested role rather than 404 on the absent membership.
    delete_membership(&pool, acme_id, member_uuid);
    assert_eq!(membership_count(&pool, acme_id, member_uuid), 0);
    let mut resp = client
        .post(&acme_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "acme-member-1",
            "role": "admin",
        }))
        .await
        .expect("send promote-missing-membership");
    assert_eq!(
        resp.status(),
        200,
        "promoting a projected user with no membership must create it, not 404"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["role"], "admin");
    assert_eq!(
        membership_role(&pool, acme_id, member_uuid),
        "admin",
        "the recreated membership carries the requested role"
    );

    // --- 2: member -> agent at seat_limit=1 (owner holds the seat) -> 403 ---
    let _owner = project_member(
        &client,
        &capco_upsert,
        &platform_token,
        "https://idp.example/",
        "capco-owner",
        "owner",
    )
    .await;
    let capco_member = project_member(
        &client,
        &capco_upsert,
        &platform_token,
        "https://idp.example/",
        "capco-member",
        "member",
    )
    .await;

    let mut resp = client
        .post(srv.url("/api/internal/v1/workspaces/capco/members/set_role"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "capco-member",
            "role": "agent",
        }))
        .await
        .expect("send seat-capped promote");
    assert_eq!(resp.status(), 403, "promotion past the seat cap must 403");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["error"], "seat_limit_reached");
    // The blocked promotion must not have flipped the row.
    assert_eq!(membership_role(&pool, capco_id, capco_member), "member");

    // --- 3: unknown member (no (iss, sub) identity) -> 404 ---
    let resp = client
        .post(&acme_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "ghost-never-projected",
            "role": "agent",
        }))
        .await
        .expect("send unknown member");
    assert_eq!(resp.status(), 404, "unknown member must 404");

    // --- 4: role 'owner' -> 400 ---
    let resp = client
        .post(&acme_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "acme-member-1",
            "role": "owner",
        }))
        .await
        .expect("send owner role");
    assert_eq!(resp.status(), 400, "role owner must 400");

    // --- 5a: wrong-scope token -> 401 ---
    let resp = client
        .post(&acme_url)
        .insert_header(("Authorization", format!("Bearer {wrong_scope_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "acme-member-1",
            "role": "agent",
        }))
        .await
        .expect("send wrong-scope");
    assert_eq!(resp.status(), 401, "wrong-scope token must 401");

    // --- 5b: absent token -> 401 ---
    let resp = client
        .post(&acme_url)
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "acme-member-1",
            "role": "agent",
        }))
        .await
        .expect("send no-token");
    assert_eq!(resp.status(), 401, "absent token must 401");
}
