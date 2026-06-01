//! Integration test for `POST /api/internal/v1/workspaces/create`
//! (M5 product-side handoff Task 3).
//!
//! Spins up an actix-test server with the same middleware stack
//! the real route uses (dual_auth -> idempotency -> handler) and
//! drives it through awc to verify:
//!
//!   1. Happy path: platform-scoped token + Idempotency-Key creates
//!      a workspace and returns 201 with the product-minted UUID.
//!   2. Missing Idempotency-Key -> 400 (the handler's explicit
//!      contract check, distinct from the middleware's no-op
//!      pass-through for non-keyed requests).
//!   3. User-scoped token -> 403 (PlatformScope extractor rejects).
//!   4. Slug collision -> 409 with the non-enumerable wording.
//!   5. Idempotent retry: second call with the same key returns the
//!      byte-identical 201 response, even if the payload differs;
//!      `workspaces` table holds only one row.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::{dual_auth_middleware, idempotency_middleware};

mod common;

fn count_workspaces_with_slug(pool: &backend::db::Pool, slug: &str) -> i64 {
    use backend::schema::workspaces;
    let mut conn = pool.get().expect("conn");
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .count()
        .get_result(&mut conn)
        .expect("count")
}

#[actix_web::test]
async fn workspaces_create_full_contract() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let owner = common::insert_user(&mut pool.get().expect("conn"), "M5Owner");
    let platform_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &owner, "ctrl-plane", true);
    let user_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &owner, "user", false);

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/internal/v1")
                    .wrap(actix_web::middleware::from_fn(idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route(
                        "/workspaces/create",
                        web::post().to(internal_workspaces::create_workspace),
                    ),
            )
    });

    let client = awc::Client::new();
    let url = srv.url("/api/internal/v1/workspaces/create");

    // --- 1: happy path ---
    let key_a = format!("provision-{}", uuid::Uuid::new_v4());
    let body = json!({
        "slug": "acme-co",
        "name": "Acme Co",
        "owner_user_uuid": owner.uuid,
        "owner_email": "alice@acme.example",
    });
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", key_a.clone()))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&body)
        .await
        .expect("send happy");
    assert_eq!(resp.status(), 201, "happy path must 201");
    let body_first: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body_first["slug"], "acme-co");
    assert!(body_first["workspace_uuid"].is_string());
    assert!(body_first["created_at"].is_string());

    // --- 2: missing Idempotency-Key -> 400 ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "slug": "no-key-co",
            "name": "No Key",
            "owner_user_uuid": owner.uuid,
            "owner_email": "x@y.z",
        }))
        .await
        .expect("send no-key");
    assert_eq!(
        resp.status(),
        400,
        "missing Idempotency-Key must 400 (got {:?})",
        resp.status()
    );

    // --- 3: user-scoped token -> 403 ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {user_token}")))
        .insert_header(("Idempotency-Key", "provision-user-attempt"))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&body)
        .await
        .expect("send user-token");
    assert_eq!(
        resp.status(),
        403,
        "user-bound token must 403 (got {:?})",
        resp.status()
    );

    // --- 4: slug collision -> 409 ---
    let key_collision = format!("provision-{}", uuid::Uuid::new_v4());
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", key_collision))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "slug": "acme-co", // same slug as #1
            "name": "Acme Sequel",
            "owner_user_uuid": owner.uuid,
            "owner_email": "bob@acme.example",
        }))
        .await
        .expect("send collision");
    assert_eq!(resp.status(), 409, "slug collision must 409");
    let collision_body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(collision_body["error"], "slug_taken");
    assert!(
        collision_body["message"]
            .as_str()
            .is_some_and(|m| m.contains("unavailable")),
        "collision message should be non-enumerable: got {collision_body}"
    );

    // --- 5: idempotent retry returns cached 201 + same body ---
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", key_a))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            // Intentionally different payload — Stripe-style: cached
            // response wins regardless of what the retry sends.
            "slug": "different-slug-ignored",
            "name": "Different Name Ignored",
            "owner_user_uuid": owner.uuid,
            "owner_email": "ignored@retry.example",
        }))
        .await
        .expect("send retry");
    assert_eq!(resp.status(), 201, "retry returns cached 201");
    let body_retry: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(
        body_retry, body_first,
        "retry must return byte-identical cached body, got {body_retry}"
    );

    // Only one acme-co row in the DB despite the retry hitting the
    // same endpoint multiple times.
    assert_eq!(
        count_workspaces_with_slug(&pool, "acme-co"),
        1,
        "exactly one workspace row should exist for the slug"
    );
}
