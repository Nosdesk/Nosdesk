//! Integration test for
//! `PATCH /api/internal/v1/workspaces/{slug}/custom-domain`
//! plus the middleware lookup side (M5 product-side handoff Task 5).
//!
//! Covers:
//!
//!   1. PATCH sets a hostname and the row is updated.
//!   2. PATCH with a different workspace and a conflicting hostname
//!      returns 409.
//!   3. PATCH with `null` clears the hostname.
//!   4. Unknown workspace slug -> 404.
//!   5. Invalid hostname shape -> 400.
//!   6. User-scoped token -> 403; missing Idempotency-Key -> 400.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::{dual_auth_middleware, idempotency_middleware};

mod common;

fn read_custom_domain(pool: &backend::db::Pool, slug: &str) -> Option<String> {
    use backend::schema::workspaces;
    let mut conn = pool.get().expect("conn");
    workspaces::table
        .filter(workspaces::slug.eq(slug))
        .select(workspaces::custom_domain)
        .first::<Option<String>>(&mut conn)
        .expect("query custom_domain")
}

#[actix_web::test]
async fn custom_domain_set_clear_collide() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let admin = common::insert_user(&mut pool.get().expect("conn"), "M5DomainAdmin");
    let platform_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &admin, "ctrl-plane", true);
    let user_token = common::mint_api_token(&mut pool.get().expect("conn"), &admin, "user", false);
    common::mint_workspace(&mut pool.get().expect("conn"), "acme", "Acme");
    // Non-reserved slug: `beta` is on the workspaces_slug_not_reserved
    // denylist (Phase 4 W4), so minting it trips the CHECK constraint.
    common::mint_workspace(&mut pool.get().expect("conn"), "globex", "Globex");

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/internal/v1")
                    .wrap(actix_web::middleware::from_fn(idempotency_middleware))
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route(
                        "/workspaces/{slug}/custom-domain",
                        web::patch().to(internal_workspaces::set_custom_domain),
                    ),
            )
    });

    let client = awc::Client::new();

    // --- 1: PATCH sets a hostname ---
    let resp = client
        .patch(srv.url("/api/internal/v1/workspaces/acme/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": "support.acme.com" }))
        .await
        .expect("send set");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        read_custom_domain(&pool, "acme").as_deref(),
        Some("support.acme.com")
    );

    // --- 2: collision -> 409 ---
    let resp = client
        .patch(srv.url("/api/internal/v1/workspaces/globex/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": "support.acme.com" }))
        .await
        .expect("send collision");
    assert_eq!(resp.status(), 409);
    assert!(read_custom_domain(&pool, "globex").is_none());

    // --- 3: clear via null ---
    let resp = client
        .patch(srv.url("/api/internal/v1/workspaces/acme/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": null }))
        .await
        .expect("send clear");
    assert_eq!(resp.status(), 200);
    assert!(read_custom_domain(&pool, "acme").is_none());

    // --- 4: unknown slug -> 404 ---
    let resp = client
        .patch(srv.url("/api/internal/v1/workspaces/nope/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": "any.example.com" }))
        .await
        .expect("send 404");
    assert_eq!(resp.status(), 404);

    // --- 5: invalid hostname shape -> 400 ---
    // Uppercase input is lowercased per RFC 4343 before validation,
    // so it lands as the lowercase form; that's a successful set,
    // not a 400, and excluded from this case list.
    for bad in [
        "no-dot",
        ".leading-dot.com",
        "trailing-dot.com.",
        "-leading-hyphen.com",
        "trailing-hyphen.com-",
        "with spaces.com",
    ] {
        let resp = client
            .patch(srv.url("/api/internal/v1/workspaces/acme/custom-domain"))
            .insert_header(("Authorization", format!("Bearer {platform_token}")))
            .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
            .insert_header(("Content-Type", "application/json"))
            .send_json(&json!({ "hostname": bad }))
            .await
            .expect("send bad");
        assert_eq!(
            resp.status(),
            400,
            "bad hostname '{bad}' must be rejected, got {:?}",
            resp.status()
        );
    }

    // --- 6: user-scoped token -> 403 ---
    let resp = client
        .patch(srv.url("/api/internal/v1/workspaces/acme/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {user_token}")))
        .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": "x.example.com" }))
        .await
        .expect("send user-token");
    assert_eq!(resp.status(), 403);

    // --- 7: missing Idempotency-Key -> 400 ---
    let resp = client
        .patch(srv.url("/api/internal/v1/workspaces/acme/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": "x.example.com" }))
        .await
        .expect("send no-key");
    assert_eq!(resp.status(), 400);
}
