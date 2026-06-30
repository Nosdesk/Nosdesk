//! Integration test for the internal workspace deprovision/restore
//! lifecycle endpoints (`DELETE /api/internal/v1/workspaces/{slug}`,
//! `POST /api/internal/v1/workspaces/{slug}/restore`). Verifies:
//!
//!   1. Deprovision soft-archives a live workspace -> 200 `archived:true`.
//!   2. A repeat deprovision is an idempotent 200 no-op and does NOT reset
//!      `archived_at` (so it can't push back the scheduler's hard delete).
//!   3. Restore clears the archive -> 200 `archived:false`.
//!   4. Both verbs 404 on a slug that never existed.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::idempotency_middleware;

mod common;

#[actix_web::test]
async fn workspaces_deprovision_restore_lifecycle() {
    common::ensure_test_keyring();
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let owner = common::insert_user(&mut pool.get().expect("conn"), "DeprovOwner");
    let platform_token = common::mint_platform_jwt("platform:provision", 300);

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
                        "/workspaces/{slug}/restore",
                        web::post().to(internal_workspaces::restore_workspace),
                    )
                    .route(
                        "/workspaces/{slug}",
                        web::delete().to(internal_workspaces::deprovision_workspace),
                    ),
            )
    });

    let client = awc::Client::new();
    let auth = ("Authorization", format!("Bearer {platform_token}"));

    // --- create the workspace to operate on ---
    let create_url = srv.url("/api/internal/v1/workspaces/create");
    let mut resp = client
        .post(&create_url)
        .insert_header(auth.clone())
        .insert_header((
            "Idempotency-Key",
            format!("provision-{}", uuid::Uuid::new_v4()),
        ))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({
            "slug": "deprov-co",
            "name": "Deprov Co",
            "owner_user_uuid": owner.uuid,
            "owner_email": "owner@deprov.example",
        }))
        .await
        .expect("send create");
    assert_eq!(resp.status(), 201, "create must 201");
    let created: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    let ws_uuid = created["workspace_uuid"].clone();

    let deprov_url = srv.url("/api/internal/v1/workspaces/deprov-co");
    let restore_url = srv.url("/api/internal/v1/workspaces/deprov-co/restore");

    // --- 1: deprovision -> 200 archived, same uuid ---
    let mut resp = client
        .delete(&deprov_url)
        .insert_header(auth.clone())
        .send()
        .await
        .expect("send deprovision");
    assert_eq!(resp.status(), 200, "deprovision must 200");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["archived"], true, "must report archived");
    assert_eq!(body["workspace_uuid"], ws_uuid, "same workspace");
    let archived_at_first = body["archived_at"].clone();
    assert!(archived_at_first.is_string(), "archived_at set");

    // --- 2: idempotent re-deprovision -> 200 no-op, archived_at NOT reset ---
    let mut resp = client
        .delete(&deprov_url)
        .insert_header(auth.clone())
        .send()
        .await
        .expect("send deprovision again");
    assert_eq!(resp.status(), 200, "re-deprovision must 200");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["archived"], true);
    assert_eq!(
        body["archived_at"], archived_at_first,
        "no-op must not reset the archive clock"
    );

    // --- 3: restore -> 200 active, archived_at cleared ---
    let mut resp = client
        .post(&restore_url)
        .insert_header(auth.clone())
        .send()
        .await
        .expect("send restore");
    assert_eq!(resp.status(), 200, "restore must 200");
    let body: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(body["archived"], false, "must report active");
    assert!(body["archived_at"].is_null(), "archived_at cleared");

    // --- 4: unknown slug -> 404 on both verbs ---
    let resp = client
        .delete(srv.url("/api/internal/v1/workspaces/nope-co"))
        .insert_header(auth.clone())
        .send()
        .await
        .expect("send deprovision unknown");
    assert_eq!(resp.status(), 404, "unknown slug deprovision must 404");

    let resp = client
        .post(srv.url("/api/internal/v1/workspaces/nope-co/restore"))
        .insert_header(auth.clone())
        .send()
        .await
        .expect("send restore unknown");
    assert_eq!(resp.status(), 404, "unknown slug restore must 404");
}
