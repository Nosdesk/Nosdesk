//! Plugin API additions behind existing permissions.
//!
//! Storage compare-and-set: reads carry a version, a write with
//! `expected_version` applies only while the entry is at that version (0 = not
//! yet set), and a stale one gets 409 with the current version. Unconditional
//! writes still work and bump the version.
//!
//! `assets.query` filters `/assets/paginated` by kind slug.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, NewPlugin, NewUser, Plugin, PluginState, User};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::common;

const WS: i32 = 1;

fn agent(conn: &mut PgConnection) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Agent".to_string(),
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
            workspace_members::role.eq("agent"),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

fn seed_plugin(conn: &mut backend::db::DbConnection, admin: Uuid) -> Uuid {
    let actor = ActorContext::user(admin, None).with_workspace(WS);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        let p: Plugin = diesel::insert_into(backend::schema::plugins::table)
            .values(&NewPlugin {
                name: "taxonomy".to_string(),
                display_name: "Taxonomy".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                manifest: json!({ "name": "taxonomy", "version": "1.0.0" }),
                state: PluginState::Installed,
                trust_level: "verified".to_string(),
                installed_by: Some(admin),
                source: "provisioned".to_string(),
                signer_pubkey: None,
                signer_source: None,
                signature_metadata: None,
                icon_svg: None,
                consented_permissions: Some(json!(["storage:plugin", "asset:read"])),
                consented_at: None,
                consented_by: None,
            })
            .get_result(c)?;
        Ok(p.uuid)
    })
    .expect("seed plugin")
}

fn spawn(pool: &common::TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let user_uuid = user.uuid;
    actix_test::start(move || {
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
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/plugins/{uuid}/storage/{key}",
                web::get().to(backend::handlers::plugins::get_plugin_storage),
            )
            .route(
                "/plugins/{uuid}/storage",
                web::post().to(backend::handlers::plugins::set_plugin_storage),
            )
            .route(
                "/assets/paginated",
                web::get().to(backend::handlers::get_paginated_devices),
            )
    })
}

async fn get(srv: &actix_test::TestServer, path: &str) -> Value {
    let mut resp = awc::Client::new()
        .get(srv.url(path))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "GET {path}");
    resp.json().await.expect("json")
}

async fn set(srv: &actix_test::TestServer, plugin: Uuid, body: Value) -> (u16, Value) {
    let mut resp = awc::Client::new()
        .post(srv.url(&format!("/plugins/{plugin}/storage")))
        .send_json(&body)
        .await
        .expect("send");
    let status = resp.status().as_u16();
    (status, resp.json().await.expect("json"))
}

#[actix_web::test]
async fn storage_compare_and_set_refuses_a_stale_version() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let user = agent(&mut conn);
    let plugin = seed_plugin(&mut conn, user.uuid);
    let srv = spawn(&pool, &user);
    let entry = format!("/plugins/{plugin}/storage/taxonomy");

    let unset = get(&srv, &entry).await;
    assert_eq!(unset["version"], 0, "{unset}");
    assert!(unset["value"].is_null());

    // Create only if absent.
    let (status, body) = set(
        &srv,
        plugin,
        json!({ "key": "taxonomy", "value": { "lists": ["Laptops"] }, "expected_version": 0 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["version"], 1);
    let (status, body) = set(
        &srv,
        plugin,
        json!({ "key": "taxonomy", "value": { "lists": [] }, "expected_version": 0 }),
    )
    .await;
    assert_eq!(status, 409, "the key exists now: {body}");
    assert_eq!(body["version"], 1);

    // Two editors read version 1; the first write wins, the second conflicts.
    let (status, body) = set(
        &srv,
        plugin,
        json!({ "key": "taxonomy", "value": { "lists": ["Laptops", "Monitors"] }, "expected_version": 1 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["version"], 2);
    let (status, body) = set(
        &srv,
        plugin,
        json!({ "key": "taxonomy", "value": { "lists": ["Laptops", "Phones"] }, "expected_version": 1 }),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["version"], 2);
    let now = get(&srv, &entry).await;
    assert_eq!(now["value"], json!({ "lists": ["Laptops", "Monitors"] }));
    assert_eq!(now["version"], 2);

    // An unconditional write still lands, and bumps the version.
    let (status, body) = set(
        &srv,
        plugin,
        json!({ "key": "taxonomy", "value": { "lists": [] } }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["version"], 3);
}

fn asset(conn: &mut PgConnection, name: &str, kind: &str) {
    diesel::insert_into(backend::schema::assets::table)
        .values(&NewAsset {
            name: name.to_string(),
            serial_number: None,
            manufacturer: None,
            model: None,
            location: None,
            notes: None,
            primary_user_uuid: None,
            purchase_date: None,
            asset_tag: None,
            kind: kind.to_string(),
            attributes: json!({}),
            quantity: None,
            unit: None,
            external_sync_source: None,
            low_stock_threshold: None,
            created_by: None,
        })
        .execute(conn)
        .expect("insert asset");
}

#[actix_web::test]
async fn paginated_assets_filter_by_kind() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let user = agent(&mut pool.get().expect("conn"));
    run_in_workspace(&pool, "test:asset_kind_filter", WS, |c| {
        asset(c, "Van", "vehicle");
        asset(c, "Laptop", "laptop");
        asset(c, "Drill", "equipment");
        Ok(())
    })
    .expect("seed");
    let srv = spawn(&pool, &user);

    let names = |page: &Value| -> Vec<String> {
        page["data"]
            .as_array()
            .expect("data")
            .iter()
            .map(|a| a["name"].as_str().expect("name").to_string())
            .collect()
    };
    let page = get(&srv, "/assets/paginated?kind=vehicle,equipment").await;
    assert_eq!(names(&page), ["Drill", "Van"]);
    assert_eq!(page["total"], 2);
    let page = get(&srv, "/assets/paginated").await;
    assert_eq!(page["total"], 3);
}
