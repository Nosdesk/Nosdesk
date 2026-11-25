//! Named asset statuses: workspace admins name statuses inside the nine
//! categories (unique names, one default per category, archive and
//! restore, delete only when nothing holds one); a move to one goes through
//! the status rules, a change of named status within a category writes an
//! event and never a record, an archived target or one of another category
//! or kind is refused, by the rules and by the database; a loan puts back
//! the named status it left; a filter on a default takes in the plain
//! assets of its category; and everyone who sees assets reads the statuses,
//! staff alone the moves. The importer's handling is in `imports_status`,
//! the archive's in `portability_round_trip`.

use std::collections::HashSet;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, UpdateSiteSettings};
use backend::repository::asset_loans::{self, IssueLoan};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{asset_record_types, assets as asset_repo, site_settings};
use backend::schema::{asset_lifecycle_events, asset_records, assets};
use backend::services::workspace_archive::plan::discover_fk_edges;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_named_statuses";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    search: std::sync::Arc<backend::services::search::SearchService>,
    _search_dir: tempfile::TempDir,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

fn new_asset(name: &str, kind: &str) -> NewAsset {
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
        attributes: json!({}),
        quantity: None,
        unit: None,
        external_sync_source: None,
        low_stock_threshold: None,
        created_by: None,
    }
}

fn asset(f: &Fixture, name: &str) -> i32 {
    in_ws(f, |c| {
        asset_repo::create_device(c, new_asset(name, "device"))
    })
    .id
}

/// The asset's category and named status.
fn status_of(f: &Fixture, id: i32) -> (String, Option<i32>) {
    let a = in_ws(f, |c| asset_repo::get_device_by_id(c, id));
    (a.status, a.status_id)
}

/// Each event of the asset: from, to, and the named statuses on each side.
type EventRow = (Option<String>, String, Option<i32>, Option<i32>);

fn events_of(f: &Fixture, id: i32) -> Vec<EventRow> {
    in_ws(f, |c| {
        asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq(id))
            .order(asset_lifecycle_events::id.asc())
            .select((
                asset_lifecycle_events::from_status,
                asset_lifecycle_events::to_status,
                asset_lifecycle_events::from_status_id,
                asset_lifecycle_events::to_status_id,
            ))
            .load(c)
    })
}

fn records_of(f: &Fixture, id: i32) -> i64 {
    in_ws(f, |c| {
        asset_records::table
            .filter(asset_records::asset_id.eq(id))
            .count()
            .get_result(c)
    })
}

/// Workspace A with an agent, and requesters shown every asset.
fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(6);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        add_membership(
            c,
            ws.workspace_id,
            agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                requester_asset_visibility: Some("all".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and the visibility");
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = std::sync::Arc::new(
        backend::services::search::SearchService::new(search_dir.path(), &pool)
            .expect("init search"),
    );
    Fixture {
        pool,
        ws,
        agent,
        search,
        _search_dir: search_dir,
    }
}

/// Send `method uri` with `body` as `viewer` in workspace A, signed in.
async fn call(
    f: &Fixture,
    viewer: Uuid,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
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
            .app_data(web::Data::new(common::test_storage()))
            .app_data(web::Data::new(f.search.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::assets::config)
                    .configure(backend::handlers::asset_statuses::config),
            ),
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

async fn admin(f: &Fixture, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    call(f, f.ws.admin_uuid, method, uri, body).await
}

/// A named status made by an admin: its id.
async fn named(f: &Fixture, category: &str, slug: &str, label: &str, extra: Value) -> i32 {
    let mut body = json!({ "category": category, "slug": slug, "label": label });
    if let (Some(body), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
        body.extend(extra.clone());
    }
    let (status, made) = admin(f, Method::POST, "/api/admin/asset-statuses", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    made["id"].as_i64().expect("id") as i32
}

async fn transition(f: &Fixture, id: i32, body: Value) -> (StatusCode, Value) {
    let uri = format!("/api/assets/{id}/lifecycle");
    call(f, f.agent, Method::POST, &uri, Some(body)).await
}

async fn statuses(f: &Fixture) -> Vec<Value> {
    let (status, list) = admin(f, Method::GET, "/api/admin/asset-statuses", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    list.as_array().expect("a list").clone()
}

fn by_id(list: &[Value], id: i32) -> Value {
    list.iter()
        .find(|s| s["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("status {id} in {list:?}"))
}

/// Admins create, name, order, archive, restore and delete; names are
/// unique among live statuses ignoring case; a category has one default,
/// which names assets of every kind; a status something holds is archived,
/// not deleted. Agents read, never write.
#[actix_web::test]
async fn admins_manage_named_statuses() {
    let db = common::TestDb::new();
    let f = seed(&db);

    let body = json!({ "category": "in_repair", "slug": "at_vendor", "label": "At vendor" });
    let (status, _) = call(
        &f,
        f.agent,
        Method::POST,
        "/api/admin/asset-statuses",
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let vendor = named(&f, "in_repair", "at_vendor", "At vendor", json!({})).await;
    let parts = named(
        &f,
        "in_repair",
        "awaiting_parts",
        "Awaiting parts",
        json!({}),
    )
    .await;

    for (body, want, code) in [
        (
            json!({ "category": "in_stock", "slug": "vendor_2", "label": "AT VENDOR" }),
            StatusCode::CONFLICT,
            "LABEL_TAKEN",
        ),
        (
            json!({ "category": "in_stock", "slug": "at_vendor", "label": "Shelf" }),
            StatusCode::CONFLICT,
            "SLUG_TAKEN",
        ),
        (
            json!({ "category": "in_stock", "slug": "in_stock", "label": "Shelf" }),
            StatusCode::CONFLICT,
            "SLUG_RESERVED",
        ),
        (
            json!({ "category": "broken", "slug": "broken", "label": "Broken" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNKNOWN_CATEGORY",
        ),
        (
            json!({ "category": "in_stock", "slug": "Shelf", "label": "Shelf" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_KEY",
        ),
        (
            json!({ "category": "in_stock", "slug": "shelf", "label": "Shelf",
                    "is_default": true, "applies_to_kinds": ["device"] }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "DEFAULT_LIMITED_TO_KINDS",
        ),
        (
            json!({ "category": "in_stock", "slug": "shelf", "label": "Shelf",
                    "applies_to_kinds": ["spaceship"] }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNKNOWN_KIND",
        ),
    ] {
        let (status, refused) =
            admin(&f, Method::POST, "/api/admin/asset-statuses", Some(body)).await;
        assert_eq!(status, want, "{refused}");
        assert_eq!(refused["code"], code, "{refused}");
    }

    // Renamed, and ordered within its category.
    let (status, renamed) = admin(
        &f,
        Method::PUT,
        &format!("/api/admin/asset-statuses/{vendor}"),
        Some(json!({ "label": "With the vendor", "color": "#aa3300" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{renamed}");
    assert_eq!(renamed["label"], "With the vendor");
    assert_eq!(renamed["slug"], "at_vendor", "the slug never changes");
    let (status, ordered) = admin(
        &f,
        Method::PUT,
        "/api/admin/asset-statuses/order",
        Some(json!({ "category": "in_repair", "ids": [parts, vendor] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ordered}");
    let order: Vec<i64> = ordered
        .as_array()
        .expect("list")
        .iter()
        .map(|s| s["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(order, vec![i64::from(parts), i64::from(vendor)]);

    // One default per category: the second takes the flag.
    let deployed = named(
        &f,
        "in_service",
        "deployed",
        "Deployed",
        json!({ "is_default": true }),
    )
    .await;
    let issued = named(
        &f,
        "in_service",
        "issued",
        "Issued",
        json!({ "is_default": true }),
    )
    .await;
    let list = statuses(&f).await;
    assert_eq!(by_id(&list, deployed)["is_default"], false);
    assert_eq!(by_id(&list, issued)["is_default"], true);

    // Archived, a status frees its name; brought back, it needs it again.
    let (status, archived) = admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{parts}/archive"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{archived}");
    assert!(!archived["archived_at"].is_null(), "{archived}");
    let parts_again = named(
        &f,
        "in_repair",
        "awaiting_parts_2",
        "Awaiting parts",
        json!({}),
    )
    .await;
    let (status, refused) = admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{parts}/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "LABEL_TAKEN");
    let (status, _) = admin(
        &f,
        Method::DELETE,
        &format!("/api/admin/asset-statuses/{parts_again}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "an unused status deletes");
    let (status, restored) = admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{parts}/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert!(restored["archived_at"].is_null(), "{restored}");

    // An archived default comes back as one only when the category has none.
    for id in [issued, deployed] {
        admin(
            &f,
            Method::POST,
            &format!("/api/admin/asset-statuses/{id}/archive"),
            None,
        )
        .await;
    }
    let (_, back) = admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{issued}/restore"),
        None,
    )
    .await;
    assert_eq!(back["is_default"], true, "{back}");

    // Held, a status is archived, not deleted.
    let a = asset(&f, "Held");
    let (status, moved) = transition(&f, a, json!({ "to_status_id": vendor })).await;
    assert_eq!(status, StatusCode::CREATED, "{moved}");
    let (status, refused) = admin(
        &f,
        Method::DELETE,
        &format!("/api/admin/asset-statuses/{vendor}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "IN_USE");
    assert_eq!(by_id(&statuses(&f).await, vendor)["asset_count"], 1);
}

/// A move between named statuses of one category is a relabel: an event
/// with the same category on both sides and both names, no record opened or
/// completed, in every category, `disposed` included. Same category and
/// same name is no move.
#[actix_web::test]
async fn a_change_of_named_status_within_a_category_writes_an_event_and_no_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    in_ws(&f, |c| {
        asset_record_types::add_from_catalogue(c, "repair", None)
            .map(|_| ())
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });
    let vendor = named(&f, "in_repair", "at_vendor", "At vendor", json!({})).await;
    let parts = named(
        &f,
        "in_repair",
        "awaiting_parts",
        "Awaiting parts",
        json!({}),
    )
    .await;

    let a = asset(&f, "Relabel");
    let (status, opened) = transition(
        &f,
        a,
        json!({ "to_status": "in_repair", "to_status_id": vendor, "record": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    assert_eq!(opened["to_status_id"], vendor, "{opened}");
    assert_eq!(records_of(&f, a), 1, "the move into repair opens a record");

    // A 1.2 body naming only the status.
    let (status, relabel) = transition(&f, a, json!({ "to_status_id": parts })).await;
    assert_eq!(status, StatusCode::CREATED, "{relabel}");
    assert_eq!(status_of(&f, a), ("in_repair".to_string(), Some(parts)));
    assert_eq!(records_of(&f, a), 1, "a relabel opens no record");
    assert_eq!(
        events_of(&f, a).last().cloned(),
        Some((
            Some("in_repair".to_string()),
            "in_repair".to_string(),
            Some(vendor),
            Some(parts)
        ))
    );

    // A relabel completes nothing either, and says so.
    let (status, refused) = transition(
        &f,
        a,
        json!({ "to_status_id": vendor, "complete_record": {} }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "RECORD_ON_RELABEL");

    // The same named status is no move.
    let (status, same) = transition(&f, a, json!({ "to_status_id": parts })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{same}");
    assert_eq!(same["code"], "same_status");

    // A 1.1 body (the category alone) goes back to the plain category.
    let (status, plain) = transition(&f, a, json!({ "to_status": "in_repair" })).await;
    assert_eq!(status, StatusCode::CREATED, "{plain}");
    assert_eq!(status_of(&f, a), ("in_repair".to_string(), None));
    assert_eq!(records_of(&f, a), 1);

    // Disposed stays disposed, under another name.
    let recycled = named(&f, "disposed", "recycled", "Recycled", json!({})).await;
    let gone = asset(&f, "Gone");
    let (status, disposed) = transition(
        &f,
        gone,
        json!({ "to_status": "disposed", "disposal": {
            "sanitization_method": "destroy", "data_bearing": false } }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{disposed}");
    let (status, relabel) = transition(&f, gone, json!({ "to_status_id": recycled })).await;
    assert_eq!(status, StatusCode::CREATED, "{relabel}");
    assert_eq!(
        status_of(&f, gone),
        ("disposed".to_string(), Some(recycled))
    );
    let (status, refused) = transition(&f, gone, json!({ "to_status": "in_stock" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "terminal");
}

/// A named target must be live, of the category the move names and offered
/// for the asset's kind; the database refuses a named status of another
/// category whoever writes it.
#[actix_web::test]
async fn named_targets_are_checked_by_the_rules_and_the_database() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let vendor = named(&f, "in_repair", "at_vendor", "At vendor", json!({})).await;
    let spare = named(&f, "in_stock", "spare", "Spare", json!({})).await;
    let vans = named(
        &f,
        "in_repair",
        "at_the_garage",
        "At the garage",
        json!({ "applies_to_kinds": ["vehicle"] }),
    )
    .await;
    let a = asset(&f, "Checked");

    let cases = [
        (
            json!({ "to_status": "in_stock", "to_status_id": vendor }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "wrong_category",
        ),
        (
            json!({ "to_status_id": vans }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "status_not_for_kind",
        ),
    ];
    for (body, want, code) in cases {
        let (status, refused) = transition(&f, a, body).await;
        assert_eq!(status, want, "{refused}");
        assert_eq!(refused["code"], code, "{refused}");
        assert!(
            refused["error"].as_str().is_some_and(|m| !m.contains('{')),
            "{refused}"
        );
    }
    admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{spare}/archive"),
        None,
    )
    .await;
    let (status, refused) = transition(&f, a, json!({ "to_status_id": spare })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["code"], "status_archived");
    let (status, _) = transition(&f, a, json!({ "to_status_id": i32::MAX })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = transition(&f, a, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(status_of(&f, a), ("in_service".to_string(), None));
    assert!(events_of(&f, a).is_empty(), "no refused move left an event");

    // Past the rules, the three-column key still refuses it.
    // In a savepoint, so the refused write leaves the transaction usable.
    let written = in_ws(&f, |c| {
        Ok(c.transaction(|c| {
            diesel::update(assets::table.find(a))
                .set((
                    assets::status.eq("in_stock"),
                    assets::status_id.eq(Some(vendor)),
                ))
                .execute(c)
        }))
    });
    match written {
        Err(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::ForeignKeyViolation,
            info,
        )) => assert_eq!(info.constraint_name(), Some("assets_status_category_fkey")),
        other => panic!("expected the category key to refuse it, got {other:?}"),
    }
}

/// An asset created in a named status starts there, with one event naming
/// it.
#[actix_web::test]
async fn a_create_starts_in_a_named_status() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let spare = named(&f, "in_stock", "spare", "Spare", json!({})).await;
    let (status, made) = call(
        &f,
        f.agent,
        Method::POST,
        "/api/assets",
        Some(json!({ "name": "New spare", "kind": "device", "status_id": spare })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    let id = made["id"].as_i64().expect("id") as i32;
    assert_eq!(status_of(&f, id), ("in_stock".to_string(), Some(spare)));
    assert_eq!(
        events_of(&f, id),
        vec![(None, "in_stock".to_string(), None, Some(spare))]
    );

    let (status, refused) = call(
        &f,
        f.agent,
        Method::POST,
        "/api/assets",
        Some(json!({ "name": "Mismatch", "kind": "device",
                     "status": "in_repair", "status_id": spare })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "wrong_category");
}

/// A loan leaves the named status for plain on loan, records it, and its
/// return puts it back, archived since or not.
#[actix_web::test]
async fn a_loan_return_restores_the_named_status() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let pool_shelf = named(&f, "in_stock", "loaner_pool", "Loaner pool", json!({})).await;
    let a = asset(&f, "Loaner");
    let (status, moved) = transition(&f, a, json!({ "to_status_id": pool_shelf })).await;
    assert_eq!(status, StatusCode::CREATED, "{moved}");

    let issue = IssueLoan {
        asset_id: a,
        borrower_user_uuid: f.ws.member_uuid,
        loaned_at: None,
        due_back: None,
        ticket_id: None,
        notes: None,
        actor_uuid: None,
    };
    let loan = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        asset_loans::issue(c, issue)
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("lend");
    assert_eq!(loan.status_before, "in_stock");
    assert_eq!(loan.status_id_before, Some(pool_shelf));
    assert_eq!(status_of(&f, a), ("on_loan".to_string(), None));

    admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{pool_shelf}/archive"),
        None,
    )
    .await;
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        asset_loans::return_loan(c, a, loan.id, chrono::Utc::now(), None, None)
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("return");
    assert_eq!(status_of(&f, a), ("in_stock".to_string(), Some(pool_shelf)));
    assert_eq!(
        events_of(&f, a).last().cloned(),
        Some((
            Some("on_loan".to_string()),
            "in_stock".to_string(),
            None,
            Some(pool_shelf)
        ))
    );
}

/// `status_id=` matches assets holding any of the named statuses, and a
/// live default also the plain assets of its category.
#[actix_web::test]
async fn a_filter_on_the_default_takes_in_the_plain_assets() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let deployed = named(
        &f,
        "in_service",
        "deployed",
        "Deployed",
        json!({ "is_default": true }),
    )
    .await;
    let kiosk = named(&f, "in_service", "kiosk", "Kiosk", json!({})).await;
    asset(&f, "Plain");
    let named_default = asset(&f, "Named default");
    let other = asset(&f, "Kiosk one");
    let stock = asset(&f, "In stock");
    for (id, body) in [
        (named_default, json!({ "to_status_id": deployed })),
        (other, json!({ "to_status_id": kiosk })),
        (stock, json!({ "to_status": "in_stock" })),
    ] {
        let (status, moved) = transition(&f, id, body).await;
        assert_eq!(status, StatusCode::CREATED, "{moved}");
    }

    let names = |page: &Value| -> HashSet<String> {
        page["data"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| r["name"].as_str().expect("name").to_string())
            .collect()
    };
    let list = |query: String| {
        let f = &f;
        async move {
            let (status, page) = call(
                f,
                f.agent,
                Method::GET,
                &format!("/api/assets/paginated?{query}"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{page}");
            page
        }
    };
    let page = list(format!("status_id={deployed}")).await;
    assert_eq!(
        names(&page),
        HashSet::from(["Plain".to_string(), "Named default".to_string()])
    );
    let page = list(format!("status_id={kiosk}")).await;
    assert_eq!(names(&page), HashSet::from(["Kiosk one".to_string()]));
    let page = list(format!("status_id={kiosk},{deployed}&status=in_service")).await;
    assert_eq!(page["total"], 3, "{page}");
    // The row carries its named status.
    let page = list(format!("status_id={kiosk}")).await;
    assert_eq!(page["data"][0]["status_id"], kiosk, "{page}");
}

/// Everyone who sees assets reads the named statuses, archived ones
/// flagged; staff alone get the moves; a requester who sees no assets gets
/// nothing.
#[actix_web::test]
async fn requesters_read_the_statuses_without_the_moves() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let vendor = named(&f, "in_repair", "at_vendor", "At vendor", json!({})).await;
    let old = named(&f, "in_repair", "old_name", "Old name", json!({})).await;
    admin(
        &f,
        Method::POST,
        &format!("/api/admin/asset-statuses/{old}/archive"),
        None,
    )
    .await;

    let (status, body) = call(
        &f,
        f.ws.member_uuid,
        Method::GET,
        "/api/assets/statuses",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let listed = body["statuses"].as_array().expect("statuses");
    assert_eq!(by_id(listed, vendor)["label"], "At vendor");
    assert_eq!(by_id(listed, vendor)["archived"], false);
    assert_eq!(by_id(listed, old)["archived"], true);
    assert!(
        body["categories"]
            .as_array()
            .expect("categories")
            .iter()
            .all(|c| c.get("targets").is_none()),
        "{body}"
    );

    let (_, staff) = call(&f, f.agent, Method::GET, "/api/assets/statuses", None).await;
    assert!(staff["categories"][0]["targets"].is_array(), "{staff}");

    in_ws(&f, |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                requester_asset_visibility: Some("none".to_string()),
                ..Default::default()
            },
        )
    });
    let (status, _) = call(
        &f,
        f.ws.member_uuid,
        Method::GET,
        "/api/assets/statuses",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Each named status column has both keys: the two-column one the workspace
/// archive finds and remaps ids through, and the three-column one that
/// keeps the status in its category, which the archive can't see. Dropping
/// the first as redundant would carry ids unmapped.
#[test]
fn both_status_keys_are_pinned() {
    #[derive(QueryableByName)]
    struct Key {
        #[diesel(sql_type = Text)]
        child_table: String,
        #[diesel(sql_type = Text)]
        columns: String,
        #[diesel(sql_type = Text)]
        parent_columns: String,
        #[diesel(sql_type = BigInt)]
        width: i64,
    }
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let keys: Vec<Key> = diesel::sql_query(
        "SELECT con.conrelid::regclass::text AS child_table, \
                (SELECT string_agg(a.attname, ',' ORDER BY k.n) \
                   FROM unnest(con.conkey) WITH ORDINALITY AS k(attnum, n) \
                   JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum) \
                  AS columns, \
                (SELECT string_agg(a.attname, ',' ORDER BY k.n) \
                   FROM unnest(con.confkey) WITH ORDINALITY AS k(attnum, n) \
                   JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.attnum) \
                  AS parent_columns, \
                array_length(con.conkey, 1)::bigint AS width \
         FROM pg_constraint con \
         WHERE con.contype = 'f' AND con.confrelid = 'public.asset_statuses'::regclass",
    )
    .load(&mut conn)
    .expect("read the keys");
    let has = |table: &str, columns: &str, parent: &str| {
        keys.iter()
            .any(|k| k.child_table == table && k.columns == columns && k.parent_columns == parent)
    };
    let pinned = [
        ("assets", "status", "status_id"),
        ("asset_lifecycle_events", "from_status", "from_status_id"),
        ("asset_lifecycle_events", "to_status", "to_status_id"),
        ("asset_loans", "status_before", "status_id_before"),
    ];
    for (table, category, id) in pinned {
        assert!(
            has(table, &format!("workspace_id,{id}"), "workspace_id,id"),
            "{table}.{id}: the two-column key the archive remaps through"
        );
        assert!(
            has(
                table,
                &format!("workspace_id,{category},{id}"),
                "workspace_id,category,id"
            ),
            "{table}.{id}: the three-column key that keeps it in its category"
        );
    }
    assert!(keys.iter().all(|k| k.width <= 3));

    let tables: HashSet<String> = [
        "asset_statuses",
        "assets",
        "asset_lifecycle_events",
        "asset_loans",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let edges = discover_fk_edges(&mut conn, &tables).expect("discover the keys");
    for (table, _, id) in pinned {
        assert!(
            edges.iter().any(|e| e.child_table == table
                && e.child_column == id
                && e.parent_table == "asset_statuses"),
            "the archive remaps {table}.{id}"
        );
    }
}
