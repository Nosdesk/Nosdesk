//! The paginated asset list sorts by a kind attribute (`attr:<key>`).
//!
//! Numbers order numerically and dates as ISO text; assets without the
//! attribute come last in both directions. A sort key that isn't a valid
//! attribute name falls back to the name order.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, NewUser, User};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common;

const WS: i32 = 1;

fn asset(conn: &mut PgConnection, name: &str, attributes: Value) {
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
            kind: "generic".to_string(),
            attributes,
            quantity: None,
            unit: None,
            external_sync_source: None,
            low_stock_threshold: None,
        })
        .execute(conn)
        .expect("insert asset");
}

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
                "/assets/paginated",
                web::get().to(backend::handlers::get_paginated_devices),
            )
    })
}

async fn sorted_names(srv: &actix_test::TestServer, field: &str, direction: &str) -> Vec<String> {
    let url = srv.url(&format!(
        "/assets/paginated?sortField={}&sortDirection={direction}",
        urlencoding::encode(field)
    ));
    let mut resp = awc::Client::new().get(url).send().await.expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    body["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|a| a["name"].as_str().expect("name").to_string())
        .collect()
}

#[actix_web::test]
async fn assets_sort_by_a_kind_attribute() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let user = agent(&mut pool.get().expect("conn"));
    run_in_workspace(&pool, "test:asset_attribute_sort", WS, |c| {
        asset(c, "a", json!({ "bay": 10, "serviced": "2026-03-01" }));
        asset(c, "b", json!({ "bay": 9, "serviced": "2025-12-31" }));
        asset(c, "c", json!({}));
        asset(c, "d", json!({ "bay": 100, "serviced": "2026-10-13" }));
        Ok(())
    })
    .expect("seed");
    let srv = spawn(&pool, &user);

    assert_eq!(
        sorted_names(&srv, "attr:bay", "asc").await,
        ["b", "a", "d", "c"]
    );
    assert_eq!(
        sorted_names(&srv, "attr:bay", "desc").await,
        ["d", "a", "b", "c"]
    );
    assert_eq!(
        sorted_names(&srv, "attr:serviced", "asc").await,
        ["b", "a", "d", "c"]
    );
    assert_eq!(
        sorted_names(&srv, "attr:Bay); DROP TABLE assets", "asc").await,
        ["a", "b", "c", "d"],
        "an invalid key sorts by name"
    );
}
