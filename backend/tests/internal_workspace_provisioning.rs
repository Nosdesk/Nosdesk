//! Integration test for
//! `GET /api/internal/v1/workspaces/{slug}/provisioning`
//! (launch-quality P1.1, the app side of the provisioning seam).
//!
//! The control plane drives a tenant live with a multi-call sequence
//! (create -> project owner). This endpoint lets it confirm the tenant
//! is actually usable rather than assuming every call landed, and feeds
//! the stuck-provisioning sweeper. The check passes only when the
//! workspace is both seeded (workflow states / SLA / categories) and
//! owned.
//!
//! Drives the real route through awc against an actix-test server to
//! verify:
//!
//!   1. Freshly created (seeded, not yet owned) -> 200 `ready:false`
//!      with the seeded checks true and `owner` false.
//!   2. After an owner membership is projected -> 200 `ready:true`.
//!   3. Unknown slug -> 404.
//!   4. Wrong-scope token -> 401 (PlatformAuth rejects).

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::idempotency_middleware;

mod common;

fn workspace_id_for_slug(pool: &backend::db::Pool, slug: &str) -> i32 {
    use backend::schema::workspaces;
    let mut conn = pool.get().expect("conn");
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .select(workspaces::id)
        .first(&mut conn)
        .expect("workspace id")
}

#[actix_web::test]
async fn workspace_provisioning_full_contract() {
    common::ensure_test_keyring();
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let owner = common::insert_user(&mut pool.get().expect("conn"), "ProvOwner");
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
                    )
                    .route(
                        "/workspaces/{slug}/provisioning",
                        web::get().to(internal_workspaces::workspace_provisioning),
                    ),
            )
    });

    let client = awc::Client::new();
    let create_url = srv.url("/api/internal/v1/workspaces/create");
    let provisioning_url = srv.url("/api/internal/v1/workspaces/prov-co/provisioning");

    // Create the tenant: seeds workflow states / SLA / categories, but
    // the owner is projected by a separate call, so it isn't owned yet.
    let resp = client
        .post(&create_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header((
            "Idempotency-Key",
            format!("provision-{}", uuid::Uuid::new_v4()),
        ))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "slug": "prov-co",
            "name": "Prov Co",
            "owner_user_uuid": owner.uuid,
            "owner_email": "owner@prov.example",
        }))
        .await
        .expect("send create");
    assert_eq!(resp.status(), 201, "create must 201");

    // --- 1: seeded but unowned -> ready:false ---
    let mut resp = client
        .get(&provisioning_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .send()
        .await
        .expect("send provisioning pre-owner");
    assert_eq!(resp.status(), 200, "provisioning check must 200");
    let pre: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(pre["slug"], "prov-co");
    assert!(pre["workspace_uuid"].is_string());
    assert_eq!(pre["ready"], false, "unowned workspace is not ready: {pre}");
    assert_eq!(pre["checks"]["workflow_states"], true, "seeded: {pre}");
    assert_eq!(pre["checks"]["default_sla_policy"], true, "seeded: {pre}");
    assert_eq!(pre["checks"]["ticket_categories"], true, "seeded: {pre}");
    assert_eq!(pre["checks"]["owner"], false, "not yet owned: {pre}");

    // Project the owner membership (what upsert_projected_user does).
    let workspace_id = workspace_id_for_slug(&pool, "prov-co");
    backend::repository::workspaces::add_membership(
        &mut pool.get().expect("conn"),
        workspace_id,
        owner.uuid,
        "owner",
    )
    .expect("add owner membership");

    // --- 2: owned -> ready:true ---
    let mut resp = client
        .get(&provisioning_url)
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .send()
        .await
        .expect("send provisioning post-owner");
    assert_eq!(resp.status(), 200);
    let post: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(post["ready"], true, "seeded + owned is ready: {post}");
    assert_eq!(post["checks"]["owner"], true, "owned: {post}");

    // --- 3: unknown slug -> 404 ---
    let resp = client
        .get(srv.url("/api/internal/v1/workspaces/does-not-exist/provisioning"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .send()
        .await
        .expect("send unknown slug");
    assert_eq!(resp.status(), 404, "unknown slug must 404");

    // --- 4: wrong-scope token -> 401 ---
    let resp = client
        .get(&provisioning_url)
        .insert_header(("Authorization", format!("Bearer {wrong_scope_token}")))
        .send()
        .await
        .expect("send wrong-scope-token");
    assert_eq!(resp.status(), 401, "wrong-scope token must 401");
}
