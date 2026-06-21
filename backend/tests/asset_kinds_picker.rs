//! HTTP-layer test for the technician-gated asset-kinds picker
//! endpoint (`GET /api/asset-kinds`).
//!
//! The admin CRUD list lives under `/admin/asset-kinds` and stays
//! admin-only. This read-only list is gated by `can_handle_tickets`
//! so technicians (who can create assets) can populate the
//! create/edit type picker. Workspace 1 ships the builtin kinds via
//! the initial-schema seed, so an agent should see them and a plain
//! member should be refused. Auth + workspace middleware is replaced
//! by a `wrap_fn` injecting the same request extensions production
//! would (Claims, WorkspaceContext, RequestContext), so each test
//! runs as a chosen user + role.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::sync::actor::ActorContext;

mod common;

const WS: i32 = 1;

/// Insert a user and grant them `role` in workspace 1.
fn member(conn: &mut PgConnection, name: &str, role: &str) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(workspace_members::table)
        .values((
            workspace_members::workspace_id.eq(WS),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert workspace member");
    user
}

fn claims_for(user: &User) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: "tech@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// Start a test server that runs every request as `user`, pinned to
/// workspace 1, exposing just the picker route.
fn spawn(pool: &common::TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = claims_for(user);
    let user_uuid = user.uuid;
    actix_test::start(move || {
        let pool = pool.clone();
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: Uuid::nil(),
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        App::new()
            .app_data(web::Data::new(pool))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").route(
                "/asset-kinds",
                web::get().to(backend::handlers::asset_kinds::list_for_picker),
            ))
    })
}

#[actix_web::test]
async fn agent_lists_builtin_kinds() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let agent = member(&mut pool.get().expect("conn"), "Agent", "agent");

    let srv = spawn(&pool, &agent);
    let client = awc::Client::new();
    let mut resp = client
        .get(srv.url("/api/asset-kinds"))
        .send()
        .await
        .expect("send");

    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = resp.json().await.expect("json");
    assert!(
        !body.is_empty(),
        "agent should see the seeded builtin kinds"
    );
    let slugs: Vec<&str> = body.iter().filter_map(|k| k["slug"].as_str()).collect();
    assert!(
        slugs.contains(&"laptop"),
        "builtin 'laptop' kind expected, got {slugs:?}"
    );
    assert!(
        slugs.contains(&"consumable"),
        "builtin 'consumable' kind expected, got {slugs:?}"
    );
}

#[actix_web::test]
async fn plain_member_is_forbidden() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let regular = member(&mut pool.get().expect("conn"), "Regular", "member");

    let srv = spawn(&pool, &regular);
    let client = awc::Client::new();
    let resp = client
        .get(srv.url("/api/asset-kinds"))
        .send()
        .await
        .expect("send");

    assert_eq!(
        resp.status(),
        403,
        "a non-technician must not reach the kinds registry"
    );
}
