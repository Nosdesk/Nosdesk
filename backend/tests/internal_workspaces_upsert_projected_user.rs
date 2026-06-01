//! Integration test for
//! `POST /api/internal/v1/workspaces/{slug}/upsert_projected_user`
//! (M5 product-side handoff Task 4).
//!
//! Drives the eager owner-projection endpoint through the same
//! middleware stack production uses (dual_auth -> idempotency ->
//! handler). Verifies:
//!
//!   1. Happy path: platform token + Idempotency-Key + valid body
//!      mints a user, creates a workspace_members row, returns 201
//!      with `created: true`.
//!   2. Re-projection (same iss/sub, different Idempotency-Key):
//!      returns 200 with `created: false`; only one users row +
//!      one workspace_members row exist.
//!   3. Role first-write-wins: re-projecting with a different role
//!      does NOT update the existing membership row.
//!   4. Unknown workspace -> 404.
//!   5. User-scoped token -> 403.
//!   6. Missing Idempotency-Key -> 400.
//!   7. Bad role -> 400.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::{dual_auth_middleware, idempotency_middleware};

mod common;

fn count_memberships(pool: &backend::db::Pool, workspace_id: i32, user_uuid: uuid::Uuid) -> i64 {
    use backend::schema::workspace_members;
    let mut conn = pool.get().expect("conn");
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::user_uuid.eq(user_uuid))
        .count()
        .get_result(&mut conn)
        .expect("count memberships")
}

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

#[actix_web::test]
async fn upsert_projected_user_full_contract() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let admin = common::insert_user(&mut pool.get().expect("conn"), "M5Admin");
    let platform_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &admin, "ctrl-plane", true);
    let user_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &admin, "user", false);
    let acme_id = common::mint_workspace(&mut pool.get().expect("conn"), "acme", "Acme");

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/internal/v1")
                    .wrap(actix_web::middleware::from_fn(idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route(
                        "/workspaces/{slug}/upsert_projected_user",
                        web::post().to(internal_workspaces::upsert_projected_user),
                    ),
            )
    });

    let client = awc::Client::new();
    let url = srv.url("/api/internal/v1/workspaces/acme/upsert_projected_user");

    // --- 1: happy path — first projection ---
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "owner-stable-id-001",
            "email": "owner@acme.example",
            "name": "Owner One",
            "role": "owner",
        }))
        .await
        .expect("send happy");
    assert_eq!(resp.status(), 201, "first projection must 201");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["created"], true);
    assert_eq!(body["role"], "owner");
    assert_eq!(body["workspace_id"], acme_id);
    let owner_uuid: uuid::Uuid = body["user_uuid"]
        .as_str()
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("user_uuid");
    assert_eq!(count_memberships(&pool, acme_id, owner_uuid), 1);

    // --- 2: re-projection (same iss/sub, fresh Idempotency-Key) ---
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "owner-stable-id-001",
            "email": "owner@acme.example",
            "role": "owner",
        }))
        .await
        .expect("send re-project");
    assert_eq!(resp.status(), 200, "re-projection must 200");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["created"], false);
    assert_eq!(body["user_uuid"], owner_uuid.to_string());
    assert_eq!(
        count_memberships(&pool, acme_id, owner_uuid),
        1,
        "membership must stay at one row"
    );

    // --- 3: role first-write-wins on re-projection ---
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "owner-stable-id-001",
            "email": "owner@acme.example",
            "role": "member",
        }))
        .await
        .expect("send role-downgrade attempt");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    // Endpoint echoes the request's role on the response (it's what
    // the caller asked for), but the DB row preserves the original
    // 'owner' — the gotcha is silent escalation/downgrade, not
    // the response shape.
    assert_eq!(body["role"], "member");
    assert_eq!(
        membership_role(&pool, acme_id, owner_uuid),
        "owner",
        "DB role must remain 'owner' despite the 'member' re-projection"
    );

    // --- 4: unknown workspace -> 404 ---
    let resp = client
        .post(srv.url("/api/internal/v1/workspaces/nope/upsert_projected_user"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "someone",
            "email": "x@y.z",
            "role": "member",
        }))
        .await
        .expect("send 404");
    assert_eq!(resp.status(), 404, "unknown workspace must 404");

    // --- 5: user-scoped token -> 403 ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {user_token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "x",
            "email": "x@y.z",
            "role": "member",
        }))
        .await
        .expect("send user-token");
    assert_eq!(resp.status(), 403);

    // --- 6: missing Idempotency-Key -> 400 ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "y",
            "email": "y@z.w",
            "role": "member",
        }))
        .await
        .expect("send no-key");
    assert_eq!(resp.status(), 400);

    // --- 7: bad role -> 400 ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("prov-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "iss": "https://idp.example/",
            "sub": "z",
            "email": "z@a.b",
            "role": "superadmin",
        }))
        .await
        .expect("send bad role");
    assert_eq!(resp.status(), 400);
}
