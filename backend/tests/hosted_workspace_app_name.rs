//! Hosted: a workspace's portal and emails carry the workspace's name, not
//! "Nosdesk". Covers the control-plane create, a rename (followed only while
//! the name was never changed), and the boot task that names workspaces
//! provisioned before this.
//!
//! Its own binary: hosted mode is a process-wide env var.

#![allow(clippy::expect_used)]

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::json;

use backend::handlers::{admin_workspaces, internal_workspaces};
use backend::middleware::{dual_auth_middleware, idempotency_middleware};
use backend::models::{NewUser, User};
use backend::sync::actor::ActorContext;

mod common;

fn mint_platform_admin(conn: &mut diesel::pg::PgConnection) -> User {
    use backend::schema::users;
    diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: uuid::Uuid::new_v4(),
            name: "PlatformAdmin".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some("platform_admin".to_string()),
        })
        .get_result(conn)
        .expect("insert platform admin")
}

/// The workspace's `app_name`, or `None` when it has no settings row.
fn app_name(pool: &backend::db::Pool, workspace_id: i32) -> Option<String> {
    use backend::schema::site_settings;
    site_settings::table
        .filter(site_settings::workspace_id.eq(workspace_id))
        .select(site_settings::app_name)
        .first(&mut pool.get().expect("conn"))
        .optional()
        .expect("read app_name")
}

fn set_app_name(pool: &backend::db::Pool, workspace_id: i32, name: &str) {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system("test:app_name").with_workspace(workspace_id);
    backend::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |c| {
            diesel::sql_query(
                "INSERT INTO site_settings (app_name) VALUES ($1) \
                 ON CONFLICT (workspace_id) DO UPDATE SET app_name = EXCLUDED.app_name",
            )
            .bind::<diesel::sql_types::Text, _>(name)
            .execute(c)
        },
    )
    .expect("set app_name");
}

#[actix_web::test]
async fn hosted_workspaces_are_named_after_the_workspace() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    common::ensure_test_keyring();
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let runtime = test_db.runtime_pool(4);

    let admin = mint_platform_admin(&mut pool.get().expect("conn"));
    let admin_token = common::mint_api_token(&mut pool.get().expect("conn"), &admin, "admin");
    let platform_token = common::mint_platform_jwt("platform:provision", 300);

    let pool_for_app = runtime.clone();
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
            .service(
                web::scope("/api/admin/workspaces")
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .route("/{id}", web::patch().to(admin_workspaces::rename_workspace)),
            )
    });
    let client = awc::Client::new();

    // The control plane creates a workspace: its portal carries the name.
    let resp = client
        .post(srv.url("/api/internal/v1/workspaces/create"))
        .insert_header(("Authorization", format!("Bearer {platform_token}")))
        .insert_header(("Idempotency-Key", format!("p-{}", uuid::Uuid::new_v4())))
        .send_json(&json!({
            "slug": "acme-co",
            "name": "Acme Co",
            "owner_user_uuid": uuid::Uuid::new_v4(),
            "owner_email": "owner@acme.example",
        }))
        .await
        .expect("send create");
    assert_eq!(resp.status(), 201);
    let acme: i32 = {
        use backend::schema::workspaces;
        workspaces::table
            .filter(workspaces::slug.eq("acme-co"))
            .select(workspaces::id)
            .first(&mut pool.get().expect("conn"))
            .expect("acme id")
    };
    assert_eq!(app_name(&pool, acme).as_deref(), Some("Acme Co"));

    let rename = |name: &'static str| {
        client
            .patch(srv.url(&format!("/api/admin/workspaces/{acme}")))
            .insert_header(("Authorization", format!("Bearer {admin_token}")))
            .send_json(&json!({ "name": name }))
    };

    // A rename carries a never-changed name along.
    let resp = rename("Acme Corp").await.expect("send rename");
    assert_eq!(resp.status(), 200);
    assert_eq!(app_name(&pool, acme).as_deref(), Some("Acme Corp"));

    // A name an admin chose stays put.
    set_app_name(&pool, acme, "Acme Help");
    let resp = rename("Acme Ltd").await.expect("send rename");
    assert_eq!(resp.status(), 200);
    assert_eq!(app_name(&pool, acme).as_deref(), Some("Acme Help"));

    // Boot task: workspaces provisioned before this, unnamed or never named,
    // take the workspace's name; one an admin named keeps it.
    let mut conn = pool.get().expect("conn");
    let no_row = common::mint_workspace(&mut conn, "bravo-co", "Bravo");
    let default_row = common::mint_workspace(&mut conn, "gamma-co", "Gamma");
    let named_row = common::mint_workspace(&mut conn, "delta-co", "Delta");
    drop(conn);
    set_app_name(&pool, default_row, "Nosdesk");
    set_app_name(&pool, named_row, "Delta Desk");

    assert!(backend::services::seed::name_hosted_workspaces(&runtime) >= 2);
    assert_eq!(app_name(&pool, no_row).as_deref(), Some("Bravo"));
    assert_eq!(app_name(&pool, default_row).as_deref(), Some("Gamma"));
    assert_eq!(app_name(&pool, named_row).as_deref(), Some("Delta Desk"));
    assert_eq!(app_name(&pool, acme).as_deref(), Some("Acme Help"));

    // Every boot after that finds nothing to do.
    assert_eq!(backend::services::seed::name_hosted_workspaces(&runtime), 0);
}
