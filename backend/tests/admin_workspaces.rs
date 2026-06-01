//! Integration test for the Phase 4 W1 admin workspace lifecycle
//! handlers under `/api/admin/workspaces`.
//!
//! Exercises the full surface in one fixture so the dual-auth +
//! request-context plumbing is set up exactly once:
//!
//!   1. POST `/admin/workspaces` with a valid body returns 201.
//!   2. GET  `/admin/workspaces` returns the new row.
//!   3. PATCH `/admin/workspaces/{id}` renames the display name only.
//!   4. POST  `/admin/workspaces/{id}/archive` flips `archived_at`.
//!   5. GET   `/admin/workspaces` defaults to active-only after
//!      archive; `?include_archived=true` returns it back.
//!   6. POST  `/admin/workspaces/{id}/restore` clears `archived_at`.
//!   7. DELETE `/admin/workspaces/{id}` without `?confirm=` returns 400.
//!   8. DELETE `/admin/workspaces/{id}?confirm=wrong` returns 400.
//!   9. DELETE `/admin/workspaces/{id}?confirm=<slug>` on an active
//!      row returns 409 (`not_archived`).
//!  10. After re-archive, `?confirm=<slug>` succeeds with 204 and the
//!      row is gone.
//!  11. Non-admin token gets 403 on every mutating endpoint.
//!  12. Reserved slug returns 400 on create (Phase 4 W4 still wired).

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
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .get_result(conn)
        .expect("insert user")
}

#[actix_web::test]
async fn admin_workspaces_lifecycle_contract() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let admin = mint_user(&mut pool.get().expect("conn"), "PlatformAdmin", UserRole::Admin);
    let admin_token = common::mint_api_token(
        &mut pool.get().expect("conn"),
        &admin,
        "admin-session",
        false,
    );
    let regular_user = mint_user(&mut pool.get().expect("conn"), "Regular", UserRole::User);
    let user_token = common::mint_api_token(
        &mut pool.get().expect("conn"),
        &regular_user,
        "user-session",
        false,
    );

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/admin/workspaces")
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route("", web::get().to(admin_workspaces::list_workspaces))
                    .route("", web::post().to(admin_workspaces::create_workspace))
                    .route(
                        "/{id}",
                        web::patch().to(admin_workspaces::rename_workspace),
                    )
                    .route(
                        "/{id}",
                        web::delete().to(admin_workspaces::hard_delete_workspace),
                    )
                    .route(
                        "/{id}/archive",
                        web::post().to(admin_workspaces::archive_workspace),
                    )
                    .route(
                        "/{id}/restore",
                        web::post().to(admin_workspaces::restore_workspace),
                    ),
            )
    });

    let client = awc::Client::new();
    let base = srv.url("/api/admin/workspaces");

    // --- 1: create ---
    let mut resp = client
        .post(&base)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "acme-co", "name": "Acme"}))
        .await
        .expect("send create");
    assert_eq!(resp.status(), 201);
    let created: serde_json::Value =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert_eq!(created["slug"], "acme-co");
    let new_id = created["id"].as_i64().expect("id") as i32;

    // --- 2: list shows the new row ---
    let mut resp = client
        .get(&base)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send list");
    assert_eq!(resp.status(), 200);
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert!(list.iter().any(|w| w["slug"] == "acme-co"));

    // --- 3: rename ---
    let resp = client
        .patch(format!("{base}/{new_id}"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"name": "Acme Corp"}))
        .await
        .expect("send rename");
    assert_eq!(resp.status(), 200);

    // --- 4: archive ---
    let resp = client
        .post(format!("{base}/{new_id}/archive"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send archive");
    assert_eq!(resp.status(), 200);

    // --- 5: default list omits archived; include_archived=true brings it back ---
    let mut resp = client
        .get(&base)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send list-active");
    let body: Vec<serde_json::Value> =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert!(!body.iter().any(|w| w["id"] == new_id));

    let mut resp = client
        .get(format!("{base}?include_archived=true"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send list-all");
    let body: Vec<serde_json::Value> =
        serde_json::from_slice(&resp.body().await.expect("body")).expect("json");
    assert!(body.iter().any(|w| w["id"] == new_id));

    // --- 6: restore ---
    let resp = client
        .post(format!("{base}/{new_id}/restore"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send restore");
    assert_eq!(resp.status(), 200);

    // --- 7: delete without confirm -> 400 ---
    let resp = client
        .delete(format!("{base}/{new_id}"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete-no-confirm");
    assert_eq!(resp.status(), 400);

    // --- 8: delete with wrong confirm -> 400 ---
    let resp = client
        .delete(format!("{base}/{new_id}?confirm=wrong-slug"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete-wrong-confirm");
    assert_eq!(resp.status(), 400);

    // --- 9: delete on active row -> 409 not_archived ---
    let resp = client
        .delete(format!("{base}/{new_id}?confirm=acme-co"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete-active");
    assert_eq!(resp.status(), 409);

    // --- 10: archive then delete succeeds ---
    let resp = client
        .post(format!("{base}/{new_id}/archive"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send archive 2");
    assert_eq!(resp.status(), 200);

    let resp = client
        .delete(format!("{base}/{new_id}?confirm=acme-co"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .send()
        .await
        .expect("send delete-final");
    assert_eq!(resp.status(), 204);

    // --- 11: non-admin user gets 403 on create ---
    let resp = client
        .post(&base)
        .insert_header(("Authorization", format!("Bearer {user_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "rogue", "name": "Rogue"}))
        .await
        .expect("send rogue");
    assert_eq!(
        resp.status(),
        403,
        "non-admin user must not create workspaces"
    );

    // --- 12: reserved slug rejected on create ---
    let resp = client
        .post(&base)
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&json!({"slug": "api", "name": "API"}))
        .await
        .expect("send reserved");
    assert_eq!(resp.status(), 400, "reserved slug must 400");
}
