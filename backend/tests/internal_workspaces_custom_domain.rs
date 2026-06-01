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

use std::sync::Once;

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::internal_workspaces;
use backend::middleware::{dual_auth_middleware, idempotency_middleware};
use backend::models::{NewApiToken, NewWorkspace, User};
use backend::repository::api_tokens::{get_token_prefix, hash_token};

mod common;

fn ensure_test_keyring() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if std::env::var("MFA_KEK_V1").is_err() {
            std::env::set_var(
                "MFA_KEK_V1",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-32-characters-min-for-tests");
        }
        if let Err(e) = backend::utils::encryption::init_keyring() {
            panic!("ensure_test_keyring: init_keyring failed: {e}");
        }
    });
}

#[derive(Debug)]
struct WorkspaceGuc;

impl diesel::r2d2::CustomizeConnection<diesel::pg::PgConnection, diesel::r2d2::Error>
    for WorkspaceGuc
{
    fn on_acquire(&self, conn: &mut diesel::pg::PgConnection) -> Result<(), diesel::r2d2::Error> {
        use diesel::RunQueryDsl;
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(diesel::r2d2::Error::QueryError)?;
        Ok(())
    }
}

fn build_pool(url: &str) -> backend::db::Pool {
    use diesel::r2d2::{ConnectionManager, Pool};
    ensure_test_keyring();
    let mgr = ConnectionManager::<diesel::pg::PgConnection>::new(url);
    Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(WorkspaceGuc))
        .build(mgr)
        .expect("build pool")
}

fn mint_token(
    conn: &mut diesel::pg::PgConnection,
    user: &User,
    name: &str,
    is_platform_scoped: bool,
) -> String {
    use backend::schema::api_tokens;
    let raw = format!("nsk_test_{}", uuid::Uuid::new_v4().simple());
    let new_token = NewApiToken {
        token_hash: hash_token(&raw),
        token_prefix: get_token_prefix(&raw),
        user_uuid: user.uuid,
        name: name.to_string(),
        scopes: Some(vec![Some("full".to_string())]),
        created_by: user.uuid,
        expires_at: None,
        is_platform_scoped,
    };
    diesel::insert_into(api_tokens::table)
        .values(&new_token)
        .execute(conn)
        .expect("insert api_token");
    raw
}

fn mint_workspace(conn: &mut diesel::pg::PgConnection, slug: &str, name: &str) {
    use backend::schema::workspaces;
    let new_ws = NewWorkspace {
        uuid: uuid::Uuid::now_v7(),
        slug: slug.to_string(),
        name: name.to_string(),
    };
    diesel::insert_into(workspaces::table)
        .values(&new_ws)
        .execute(conn)
        .expect("insert workspace");
}

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
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());

    let admin = common::insert_user(&mut pool.get().expect("conn"), "M5DomainAdmin");
    let platform_token = mint_token(&mut pool.get().expect("conn"), &admin, "ctrl-plane", true);
    let user_token = mint_token(&mut pool.get().expect("conn"), &admin, "user", false);
    mint_workspace(&mut pool.get().expect("conn"), "acme", "Acme");
    mint_workspace(&mut pool.get().expect("conn"), "beta", "Beta");

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
        .patch(srv.url("/api/internal/v1/workspaces/beta/custom-domain"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("cd-{}", uuid::Uuid::new_v4())))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({ "hostname": "support.acme.com" }))
        .await
        .expect("send collision");
    assert_eq!(resp.status(), 409);
    assert!(read_custom_domain(&pool, "beta").is_none());

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
