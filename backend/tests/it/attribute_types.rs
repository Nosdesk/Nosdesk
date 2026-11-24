//! Attribute types over REST: money values take the workspace currency and
//! are written canonical to it, sort by currency then amount; references to
//! people, assets and people groups are checked when a write adds or changes
//! them, with `assetKind` enforced, while one that already dangles saves
//! unchanged; and a purged person leaves the attributes that named them.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset};
use backend::sync::actor::ActorContext;
use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:attribute_types";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    other: WorkspaceSeed,
    search: std::sync::Arc<backend::services::search::SearchService>,
    _search_dir: tempfile::TempDir,
}

fn kind(pool: &TestPool, ws: &WorkspaceSeed, slug: &str, schema: Value) {
    diesel::sql_query(
        "INSERT INTO asset_kinds (slug, label, attribute_schema, sort_order, is_builtin, category, workspace_id)
         VALUES ($1, $1, $2::jsonb, 500, false, 'physical', $3)
         ON CONFLICT (workspace_id, slug) DO UPDATE SET attribute_schema = EXCLUDED.attribute_schema",
    )
    .bind::<Text, _>(slug)
    .bind::<Text, _>(schema.to_string())
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut pool.get().expect("conn"))
    .expect("seed kind");
}

fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (ws, other) = (seeded.a, seeded.b);
    kind(
        &pool,
        &ws,
        "dock",
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string", "format": "user-uuid" },
                "screen": { "type": "string", "format": "asset-ref", "assetKind": "screen" },
                "team": { "type": "string", "format": "group-ref" },
                "spares": { "type": "array", "items": { "type": "string", "format": "asset-ref" } },
                "cost": { "type": "object", "format": "money", "staffOnly": true },
                "usd_cost": { "type": "object", "format": "money", "currency": "USD", "minimum": 0 },
                "bay": { "type": "string" }
            }
        }),
    );
    kind(
        &pool,
        &ws,
        "screen",
        json!({ "type": "object", "properties": {} }),
    );
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::site_settings::update_site_settings(
            c,
            backend::models::UpdateSiteSettings {
                currency_code: Some(Some("AUD".to_string())),
                ..Default::default()
            },
        )
    })
    .expect("workspace currency");
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = std::sync::Arc::new(
        backend::services::search::SearchService::new(search_dir.path(), &pool).expect("search"),
    );
    Fixture {
        pool,
        ws,
        other,
        search,
        _search_dir: search_dir,
    }
}

fn asset(f: &Fixture, name: &str, kind: &str, attributes: Value) -> i32 {
    asset_in(f, &f.ws, name, kind, attributes)
}

fn asset_in(f: &Fixture, ws: &WorkspaceSeed, name: &str, kind: &str, attributes: Value) -> i32 {
    run_in_workspace(&f.pool, REF, ws.workspace_id, |c| {
        backend::repository::assets::create_device(
            c,
            NewAsset {
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
                attributes,
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            },
        )
        .map(|a| a.id)
    })
    .expect("seed asset")
}

fn group(f: &Fixture, name: &str) -> i32 {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        use backend::schema::groups;
        diesel::insert_into(groups::table)
            .values(groups::name.eq(name))
            .returning(groups::id)
            .get_result::<i32>(c)
    })
    .expect("seed group")
}

fn stored(f: &Fixture, id: i32) -> Value {
    stored_in(f, &f.ws, id)
}

fn stored_in(f: &Fixture, ws: &WorkspaceSeed, id: i32) -> Value {
    run_in_workspace(&f.pool, REF, ws.workspace_id, |c| {
        use backend::schema::assets;
        assets::table
            .find(id)
            .select(assets::attributes)
            .first::<Value>(c)
    })
    .expect("stored attributes")
}

async fn call(f: &Fixture, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let viewer = f.ws.admin_uuid;
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Admin".to_string(),
        email: format!("{viewer}@example.com"),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
        file_path: None,
    };
    let workspace = WorkspaceContext {
        workspace_id: f.ws.workspace_id,
        workspace_uuid: f.ws.workspace_uuid,
        slug: f.ws.slug.clone(),
        name: f.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(f.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(f.pool.clone()))
            .app_data(web::Data::new(f.search.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::assets::config)),
    )
    .await;
    let mut req = http_test::TestRequest::default().method(method).uri(uri);
    if let Some(body) = body {
        req = req.set_json(body);
    }
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn put_attributes(f: &Fixture, id: i32, attributes: Value) -> (StatusCode, Value) {
    call(
        f,
        Method::PUT,
        &format!("/api/assets/{id}"),
        Some(json!({ "attributes": attributes })),
    )
    .await
}

#[actix_web::test]
async fn references_are_checked_when_a_write_adds_or_changes_them() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let screen = asset(&f, "Screen", "screen", json!({}));
    let not_a_screen = asset(&f, "Other dock", "dock", json!({}));
    let team = group(&f, "Desk team");
    let dock = asset(&f, "Dock", "dock", json!({}));
    let member = f.ws.member_uuid.to_string();
    let stranger = Uuid::new_v4().to_string();

    let (status, body) = put_attributes(
        &f,
        dock,
        json!({
            "owner": member,
            "screen": screen.to_string(),
            "team": team.to_string(),
            "spares": [screen.to_string(), not_a_screen.to_string()]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let refused = [
        ("owner", json!(stranger), "not a member"),
        ("screen", json!(not_a_screen.to_string()), "of another kind"),
        ("screen", json!("999999"), "no such asset"),
        ("team", json!("999999"), "no such group"),
        (
            "spares",
            json!([screen.to_string(), "999999"]),
            "a new item",
        ),
    ];
    for (key, value, why) in refused {
        let mut attributes = stored(&f, dock);
        attributes[key] = value;
        let (status, body) = put_attributes(&f, dock, attributes).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{key} {why}: {body}"
        );
        let message = body.to_string();
        assert!(message.contains(key), "the refusal names {key}: {message}");
    }

    // A reference that already dangles (its group deleted under it) saves
    // unchanged beside an edit to another field.
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        use backend::schema::groups;
        diesel::delete(groups::table.find(team)).execute(c)
    })
    .expect("delete the group");
    let mut attributes = stored(&f, dock);
    attributes["bay"] = json!("B4");
    let (status, body) = put_attributes(&f, dock, attributes).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(stored(&f, dock)["team"], json!(team.to_string()));
}

#[actix_web::test]
async fn money_takes_the_workspace_currency_and_sorts_by_currency_then_amount() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let dock = asset(&f, "Dock", "dock", json!({}));

    let (status, body) = put_attributes(
        &f,
        dock,
        json!({ "cost": { "amount": "1250.5" }, "usd_cost": { "amount": 3 } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        stored(&f, dock),
        json!({
            "cost": { "amount": "1250.50", "currency": "AUD" },
            "usd_cost": { "amount": "3.00", "currency": "USD" }
        })
    );
    assert_eq!(
        body["attributes"]["cost"],
        json!({ "amount": "1250.50", "currency": "AUD" })
    );

    for (value, why) in [
        (
            json!({ "usd_cost": { "amount": "3", "currency": "AUD" } }),
            "pinned",
        ),
        (
            json!({ "usd_cost": { "amount": "-1" } }),
            "below the minimum",
        ),
        (
            json!({ "cost": { "amount": "1.234" } }),
            "too many decimals",
        ),
        (
            json!({ "cost": { "amount": "1", "currency": "ZZZ" } }),
            "unknown code",
        ),
        (json!({ "cost": "1250.50" }), "not an object"),
    ] {
        let (status, body) = put_attributes(&f, dock, value).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{why}: {body}");
    }

    // jsonb would put "100.00" before "99.00"; money sorts by currency, then
    // by amount as a number.
    let cost = |amount: &str, currency: &str| json!({ "cost": { "amount": amount, "currency": currency } });
    let a = asset(&f, "A", "dock", cost("99.00", "AUD"));
    let b = asset(&f, "B", "dock", cost("100.00", "AUD"));
    let c = asset(&f, "C", "dock", cost("5.00", "USD"));
    let (status, page) = call(
        &f,
        Method::GET,
        "/api/assets/paginated?kind=dock&sortField=attr:cost&sortDirection=asc&pageSize=50",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let order: Vec<i64> = page["data"]
        .as_array()
        .expect("data")
        .iter()
        .filter_map(|a| a["id"].as_i64())
        .filter(|id| [a, b, c].contains(&(*id as i32)))
        .collect();
    assert_eq!(order, [a, b, c].map(i64::from));
}

#[actix_web::test]
async fn a_purged_person_leaves_the_attributes_that_named_them() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let member = f.ws.member_uuid;
    let other = f.ws.admin_uuid;
    let dock = asset(
        &f,
        "Dock",
        "dock",
        json!({ "owner": member.to_string(), "bay": member.to_string() }),
    );
    let kept = asset(&f, "Kept", "dock", json!({ "owner": other.to_string() }));
    // Another workspace's kind of the same slug swaps the roles: there `bay`
    // names a person and `owner` is text.
    kind(
        &f.pool,
        &f.other,
        "dock",
        json!({
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "bay": { "type": "array", "items": { "type": "string", "format": "user-uuid" } }
            }
        }),
    );
    let swapped = asset_in(
        &f,
        &f.other,
        "Swapped",
        "dock",
        json!({ "owner": member.to_string(), "bay": [member.to_string(), other.to_string()] }),
    );

    let mut conn = f.pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, f.ws.workspace_id)?;
        backend::repository::users::purge_user(&member, c, None)
    })
    .expect("purge the member");

    // Only the reference goes: a text field holding the same characters
    // isn't one.
    assert_eq!(stored(&f, dock), json!({ "bay": member.to_string() }));
    assert_eq!(stored(&f, kept), json!({ "owner": other.to_string() }));
    assert_eq!(
        stored_in(&f, &f.other, swapped),
        json!({ "owner": member.to_string(), "bay": [other.to_string()] })
    );
}
