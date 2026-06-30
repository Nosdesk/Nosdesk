//! Integration test for `POST /api/internal/v1/workspaces/create`
//! (M5 product-side handoff Task 3).
//!
//! Spins up an actix-test server with the same middleware stack
//! the real route uses (idempotency -> handler, with the handler
//! gated by the `PlatformAuth` EdDSA-JWT extractor) and drives it
//! through awc to verify:
//!
//!   1. Happy path: platform provisioning JWT + Idempotency-Key
//!      creates a workspace and returns 201 with the minted UUID.
//!   2. Missing Idempotency-Key -> 400 (the handler's explicit
//!      contract check, distinct from the middleware's no-op
//!      pass-through for non-keyed requests).
//!   3. Wrong-scope token -> 401 (PlatformAuth rejects).
//!   4. Slug collision -> 409 with the non-enumerable wording.
//!   5. Idempotent retry: second call with the same key returns the
//!      byte-identical 201 response, even if the payload differs;
//!      `workspaces` table holds only one row.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::idempotency_middleware;

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
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let owner = common::insert_user(&mut pool.get().expect("conn"), "M5Owner");
    let platform_token = common::mint_platform_jwt("platform:provision", 300);
    let wrong_scope_token = common::mint_platform_jwt("platform:other", 300);

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

    // --- 3: wrong-scope token -> 401 ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {wrong_scope_token}")))
        .insert_header(("Idempotency-Key", "provision-wrong-scope-attempt"))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&body)
        .await
        .expect("send wrong-scope-token");
    assert_eq!(
        resp.status(),
        401,
        "wrong-scope token must 401 (got {:?})",
        resp.status()
    );

    // --- 4: live-slug collision under a fresh key -> 200 ensure-exists ---
    // A different Idempotency-Key bypasses the middleware cache, so the handler
    // runs and finds the existing live workspace. Ensure-exists returns it as
    // 200 with the SAME workspace_uuid as #1 (not 409), so the control plane can
    // call create unconditionally and a re-provision self-heals a product-side
    // loss. (An archived or hard-deleted slug still 409s — covered at the repo
    // layer in repository/workspaces.rs.)
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
    assert_eq!(
        resp.status(),
        200,
        "live-slug collision must ensure-exists 200 (got {:?})",
        resp.status()
    );
    let ensure_body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(ensure_body["slug"], "acme-co");
    assert_eq!(
        ensure_body["workspace_uuid"], body_first["workspace_uuid"],
        "ensure-exists must return the existing workspace, not a new one"
    );

    // --- 5: idempotent retry returns cached 201 + same body ---
    let mut resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", key_a))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            // Intentionally different payload (Stripe-style): the
            // cached response wins regardless of what the retry sends.
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

    // --- 6: reserved slug -> 400 (Phase 4 W4) ---
    let resp = client
        .post(&url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header((
            "Idempotency-Key",
            format!("provision-{}", uuid::Uuid::new_v4()),
        ))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "slug": "api", // reserved
            "name": "API",
            "owner_user_uuid": owner.uuid,
            "owner_email": "x@y.z",
        }))
        .await
        .expect("send reserved");
    assert_eq!(resp.status(), 400, "reserved slug must 400");
    assert_eq!(
        count_workspaces_with_slug(&pool, "api"),
        0,
        "reserved slug must not produce a row"
    );
}
