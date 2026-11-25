//! The Records tab: the workspace's records across every asset. Filters
//! narrow the list, the count and the totals per currency cover the whole
//! filtered set on every page, grouped rows arrive group by group with each
//! group's totals, the export holds the import template's columns, only
//! staff see any of it, and a record's reference finds its asset in search.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, UpdateSiteSettings};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{assets as asset_repo, site_settings};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_records_list";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    /// The other workspace, whose records are its own.
    other: WorkspaceSeed,
    agent: Uuid,
    camera: i32,
    laptop: i32,
    search: Option<Arc<SearchService>>,
}

fn new_asset(pool: &TestPool, ws: i32, name: &str, tag: &str) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        asset_repo::create_device(
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
                asset_tag: Some(tag.to_string()),
                kind: "device".to_string(),
                attributes: json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
                created_by: None,
            },
        )
    })
    .expect("create an asset")
    .id
}

/// Workspace A in AUD with an agent and two assets.
fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a;
    let other = seeded.b;
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
                default_timezone: Some("UTC".to_string()),
                currency_code: Some(Some("AUD".to_string())),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and the settings");
    let camera = new_asset(&pool, ws.workspace_id, "Studio camera", "CAM-014");
    let laptop = new_asset(&pool, ws.workspace_id, "Edit laptop", "LT-002");
    Fixture {
        pool,
        ws,
        other,
        agent,
        camera,
        laptop,
        search: None,
    }
}

async fn send(f: &Fixture, viewer: Uuid, req: http_test::TestRequest) -> (StatusCode, Vec<u8>) {
    let (status, body, _) = send_raw(f, viewer, req).await;
    (status, body)
}

/// [`send`], with the response's headers by lower-case name.
async fn send_raw(
    f: &Fixture,
    viewer: Uuid,
    req: http_test::TestRequest,
) -> (
    StatusCode,
    Vec<u8>,
    std::collections::HashMap<String, String>,
) {
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
    let mut app = App::new()
        .app_data(web::Data::new(f.pool.clone()))
        .app_data(web::Data::new(common::test_storage()));
    if let Some(search) = &f.search {
        app = app.app_data(web::Data::new(search.clone()));
    }
    let app = http_test::init_service(
        app.wrap_fn(move |req, srv| {
            req.extensions_mut().insert(workspace.clone());
            req.extensions_mut().insert(claims.clone());
            req.extensions_mut()
                .insert(RequestContext::new(corr, actor.clone()));
            srv.call(req)
        })
        .service(
            web::scope("/api")
                .configure(backend::handlers::assets::config)
                .configure(backend::handlers::search::config),
        ),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
        .collect();
    (status, http_test::read_body(resp).await.to_vec(), headers)
}

async fn call(
    f: &Fixture,
    viewer: Uuid,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = http_test::TestRequest::default().method(method).uri(uri);
    if let Some(body) = body {
        req = req.set_json(body);
    }
    let (status, bytes) = send(f, viewer, req).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(f: &Fixture, uri: &str) -> Value {
    let (status, body) = call(f, f.agent, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    body
}

async fn add_type(f: &Fixture, slug: &str) {
    let uri = format!("/api/assets/record-types/catalogue/{slug}");
    let (status, body) = call(f, f.agent, Method::POST, &uri, None).await;
    assert!(status.is_success(), "{status}: {body}");
}

struct Work<'a> {
    asset: i32,
    kind: &'a str,
    day: &'a str,
    open: bool,
    summary: &'a str,
    vendor: Option<&'a str>,
    reference: Option<&'a str>,
    cost: Option<(&'a str, &'a str)>,
}

async fn record(f: &Fixture, w: Work<'_>) -> i64 {
    let mut body = json!({
        "record_type": w.kind,
        "summary": w.summary,
        "occurred_on": w.day,
        "vendor": w.vendor,
        "reference": w.reference,
    });
    if !w.open {
        body["completed_on"] = json!(w.day);
    }
    if let Some((amount, currency)) = w.cost {
        body["cost"] = json!({ "amount": amount, "currency": currency });
    }
    let uri = format!("/api/assets/{}/records", w.asset);
    let (status, created) = call(f, f.agent, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["record"]["id"].as_i64().expect("record id")
}

/// Six records over two assets, in AUD and USD, one still open.
async fn history(f: &Fixture) -> Vec<i64> {
    add_type(f, "repair").await;
    add_type(f, "inspection").await;
    let mut ids = Vec::new();
    for w in [
        Work {
            asset: f.camera,
            kind: "repair",
            day: "2024-03-02",
            open: false,
            summary: "Lens mount replaced",
            vendor: Some("Camera Clinic"),
            reference: Some("RMA-48213907"),
            cost: Some(("1250.50", "AUD")),
        },
        Work {
            asset: f.camera,
            kind: "repair",
            day: "2025-06-10",
            open: false,
            summary: "Sensor cleaned",
            vendor: Some("camera clinic "),
            reference: None,
            cost: Some(("200.00", "AUD")),
        },
        Work {
            asset: f.camera,
            kind: "inspection",
            day: "2025-07-01",
            open: false,
            summary: "Annual test and tag",
            vendor: None,
            reference: None,
            cost: None,
        },
        Work {
            asset: f.laptop,
            kind: "repair",
            day: "2025-08-15",
            open: false,
            summary: "Keyboard swapped",
            vendor: Some("Dell"),
            reference: Some("WO-77"),
            cost: Some(("320.00", "USD")),
        },
        Work {
            asset: f.laptop,
            kind: "repair",
            day: "2025-09-01",
            open: true,
            summary: "Battery swelling",
            vendor: Some("Dell"),
            reference: Some("WO-91"),
            cost: None,
        },
        Work {
            asset: f.laptop,
            kind: "inspection",
            day: "2023-01-20",
            open: false,
            summary: "Intake check",
            vendor: Some("Camera Clinic"),
            reference: None,
            cost: Some(("49.50", "AUD")),
        },
    ] {
        ids.push(record(f, w).await);
    }
    ids
}

fn ids(list: &Value) -> Vec<i64> {
    list["records"]
        .as_array()
        .expect("records")
        .iter()
        .map(|r| r["id"].as_i64().expect("id"))
        .collect()
}

fn totals(list: &Value) -> Vec<(String, i64)> {
    list["totals"]
        .as_array()
        .expect("totals")
        .iter()
        .map(|t| {
            (
                t["currency"].as_str().expect("code").to_string(),
                t["amount_minor"].as_i64().expect("minor"),
            )
        })
        .collect()
}

#[actix_web::test]
async fn the_count_and_totals_cover_the_whole_filtered_set_on_every_page() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let all = history(&f).await;

    let first = get(&f, "/api/assets/records?limit=4").await;
    assert_eq!(first["total"], 6, "{first}");
    assert_eq!(first["open"], 1);
    assert_eq!(first["currency"], "AUD");
    // Other currencies by code, the workspace currency last.
    let expected = vec![("USD".to_string(), 32000), ("AUD".to_string(), 150000)];
    assert_eq!(totals(&first), expected);
    assert_eq!(first["totals"][1]["amount"], "1500.00");
    assert_eq!(first["next_offset"], 4);
    // Newest work first, each row with its asset.
    assert_eq!(ids(&first), vec![all[4], all[3], all[2], all[1]]);
    assert_eq!(
        first["records"][0]["asset"],
        json!({ "id": f.laptop, "name": "Edit laptop", "asset_tag": "LT-002" })
    );
    assert_eq!(first["records"][0]["record_type"], "repair");

    let second = get(&f, "/api/assets/records?limit=4&offset=4").await;
    assert_eq!(ids(&second), vec![all[0], all[5]]);
    assert_eq!(second["total"], 6);
    assert_eq!(
        totals(&second),
        expected,
        "page two says what the whole set adds up to"
    );
    assert_eq!(second["next_offset"], Value::Null);
    // Past the end, an empty page ends the list.
    let past = get(&f, "/api/assets/records?limit=4&offset=40").await;
    assert_eq!(ids(&past), Vec::<i64>::new());
    assert_eq!(past["next_offset"], Value::Null);
    assert_eq!(past["group"], Value::Null);

    // Filtered, the totals follow the filter and not the page.
    let aud_only = get(&f, "/api/assets/records?vendor=CAMERA%20CLINIC&limit=1").await;
    assert_eq!(
        aud_only["total"], 3,
        "vendors match without case or spaces: {aud_only}"
    );
    assert_eq!(totals(&aud_only), vec![("AUD".to_string(), 150000)]);
    assert_eq!(aud_only["records"].as_array().expect("rows").len(), 1);
}

#[actix_web::test]
async fn filters_narrow_the_list() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let all = history(&f).await;
    let sorted = |mut v: Vec<i64>| {
        v.sort_unstable();
        v
    };
    let list = |query: String| {
        let f = &f;
        async move { sorted(ids(&get(f, &format!("/api/assets/records?{query}")).await)) }
    };

    assert_eq!(
        list("type=inspection".into()).await,
        sorted(vec![all[2], all[5]])
    );
    assert_eq!(list("type=repair&type=inspection".into()).await.len(), 6);
    assert_eq!(list("type=calibration".into()).await, Vec::<i64>::new());
    assert_eq!(list("status=open".into()).await, vec![all[4]]);
    assert_eq!(list("status=completed".into()).await.len(), 5);
    assert_eq!(list("status=open&status=completed".into()).await.len(), 6);
    assert_eq!(
        list("vendor=Dell&vendor=Camera%20Clinic".into())
            .await
            .len(),
        5,
        "each vendor chosen"
    );
    // Postgres folds the asked vendor as it folds the records'.
    assert_eq!(list("vendor=%20%20dell%20".into()).await.len(), 2);
    assert_eq!(
        list(format!("asset={}", f.laptop)).await,
        sorted(vec![all[3], all[4], all[5]])
    );
    assert_eq!(
        list("from=2025-01-01&to=2025-07-31".into()).await,
        sorted(vec![all[1], all[2]])
    );
    // Search reads the summary, vendor and reference, and the asset's tag
    // and name.
    assert_eq!(list("q=rma-4821".into()).await, vec![all[0]]);
    assert_eq!(list("q=keyboard".into()).await, vec![all[3]]);
    assert_eq!(
        list("q=LT-002".into()).await,
        sorted(vec![all[3], all[4], all[5]])
    );
    assert_eq!(
        list("q=100%25".into()).await,
        Vec::<i64>::new(),
        "a % is matched as itself"
    );
    assert_eq!(list(format!("record={}", all[2])).await, vec![all[2]]);

    // Sorted by cost, dearest first; work without a cost last.
    let by_cost = get(&f, "/api/assets/records?sort=cost").await;
    assert_eq!(ids(&by_cost)[..4], [all[0], all[3], all[1], all[5]]);

    let (status, _) = call(
        &f,
        f.agent,
        Method::GET,
        "/api/assets/records?status=pending",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &f,
        f.agent,
        Method::GET,
        "/api/assets/records?from=2025-02-01&to=2025-01-01",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[actix_web::test]
async fn grouped_rows_come_group_by_group_with_each_groups_totals() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let all = history(&f).await;

    let first = get(&f, "/api/assets/records?group=vendor&limit=3").await;
    assert_eq!(first["group"], "vendor", "the axis is echoed");
    let groups = first["groups"].as_array().expect("groups");
    // The biggest spend in the workspace currency first, then the most
    // records; work done in house is its own group.
    let keys: Vec<&str> = groups
        .iter()
        .map(|g| g["key"].as_str().expect("key"))
        .collect();
    assert_eq!(keys, vec!["camera clinic", "dell", ""], "{first}");
    assert_eq!(groups[0]["count"], 3);
    assert_eq!(
        groups[0]["label"], "Camera Clinic",
        "the most used spelling"
    );
    assert_eq!(
        groups[1]["totals"],
        json!([{ "currency": "USD", "amount": "320.00", "amount_minor": 32000 }])
    );
    assert_eq!(groups[1]["open"], 1);
    assert_eq!(groups[2]["label"], Value::Null);
    // Rows in group order: the first page is the whole first group.
    let rows = first["records"].as_array().expect("rows");
    assert!(
        rows.iter().all(|r| r["group"] == "camera clinic"),
        "{first}"
    );
    assert_eq!(ids(&first), vec![all[1], all[0], all[5]]);
    let rest = get(&f, "/api/assets/records?group=vendor&limit=3&offset=3").await;
    let rest_groups: Vec<&str> = rest["records"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["group"].as_str().expect("group"))
        .collect();
    assert_eq!(rest_groups, vec!["dell", "dell", ""]);
    assert_eq!(rest["groups"], first["groups"]);

    let by_month = get(&f, "/api/assets/records?group=month").await;
    let months: Vec<&str> = by_month["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .map(|g| g["key"].as_str().expect("key"))
        .collect();
    assert_eq!(
        months,
        vec!["2025-09", "2025-08", "2025-07", "2025-06", "2024-03", "2023-01"]
    );
}

#[actix_web::test]
async fn facets_offer_the_vendors_and_assets_on_records() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let empty = get(&f, "/api/assets/records/facets").await;
    assert_eq!(empty, json!({ "total": 0, "vendors": [], "assets": [] }));

    history(&f).await;
    let facets = get(&f, "/api/assets/records/facets").await;
    assert_eq!(facets["total"], 6);
    assert_eq!(
        facets["vendors"],
        json!([
            { "vendor": "Camera Clinic", "count": 3 },
            { "vendor": "Dell", "count": 2 },
        ])
    );
    let assets: Vec<(i64, i64)> = facets["assets"]
        .as_array()
        .expect("assets")
        .iter()
        .map(|a| {
            (
                a["asset"]["id"].as_i64().expect("id"),
                a["count"].as_i64().expect("count"),
            )
        })
        .collect();
    assert_eq!(
        assets,
        vec![(i64::from(f.camera), 3), (i64::from(f.laptop), 3)]
    );
}

#[actix_web::test]
async fn requesters_and_other_workspaces_see_no_records() {
    let db = common::TestDb::new();
    let f = seed(&db);
    history(&f).await;

    for uri in [
        "/api/assets/records",
        "/api/assets/records/facets",
        "/api/assets/records/export",
    ] {
        let (status, body) = call(&f, f.ws.member_uuid, Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
    }

    // RLS keeps workspace B's admin to B's records, which are none.
    let b = Fixture {
        pool: f.pool.clone(),
        ws: f.other.clone(),
        other: f.ws.clone(),
        agent: f.agent,
        camera: f.camera,
        laptop: f.laptop,
        search: None,
    };
    let (status, body) = call(
        &b,
        b.ws.admin_uuid,
        Method::GET,
        "/api/assets/records",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 0);
}

#[actix_web::test]
async fn the_export_holds_the_filtered_set_in_the_import_templates_columns() {
    let db = common::TestDb::new();
    let f = seed(&db);
    history(&f).await;

    let req = http_test::TestRequest::get().uri("/api/assets/records/export?vendor=Dell");
    let (status, bytes, headers) = send_raw(&f, f.agent, req).await;
    assert_eq!(status, StatusCode::OK);
    // Under the cap: every match exported, and the headers say so.
    assert_eq!(headers.get("x-export-rows").map(String::as_str), Some("2"));
    assert_eq!(headers.get("x-export-total").map(String::as_str), Some("2"));
    let mut reader = csv::Reader::from_reader(bytes.as_slice());
    let headers: Vec<String> = reader
        .headers()
        .expect("headers")
        .iter()
        .map(str::to_string)
        .collect();
    let mut expected: Vec<String> = backend::services::imports::asset_records::TEMPLATE_HEADERS
        .iter()
        .map(ToString::to_string)
        .collect();
    expected.push("attributes".to_string());
    assert_eq!(headers, expected);
    let rows: Vec<csv::StringRecord> = reader.records().map(|r| r.expect("row")).collect();
    assert_eq!(rows.len(), 2);
    let column = |name: &str| headers.iter().position(|h| h == name).expect(name);
    // Newest first, as the list sorts.
    assert_eq!(&rows[0][column("summary")], "Battery swelling");
    assert_eq!(&rows[0][column("asset_tag")], "LT-002");
    assert_eq!(&rows[0][column("completed_on")], "");
    assert_eq!(&rows[1][column("reference")], "WO-77");
    assert_eq!(&rows[1][column("cost")], "320.00");
    assert_eq!(&rows[1][column("currency")], "USD");

    let req =
        http_test::TestRequest::get().uri("/api/assets/records/export?format=json&type=inspection");
    let (status, bytes) = send(&f, f.agent, req).await;
    assert_eq!(status, StatusCode::OK);
    let rows: Value = serde_json::from_slice(&bytes).expect("json");
    let rows = rows.as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    // One type: a column per field of it in place of the JSON column.
    assert!(rows[0].get("attributes").is_none(), "{}", rows[0]);
    assert!(
        rows[0].get("inspector_certificate").is_some(),
        "{}",
        rows[0]
    );
}

#[actix_web::test]
async fn a_records_reference_finds_its_asset_in_search() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut f = seed(&db);
    let pool = db.runtime_pool(2);
    let index_dir = tempfile::tempdir().expect("index dir");
    f.search = Some(Arc::new(
        SearchService::new(index_dir.path(), &pool).expect("init search"),
    ));
    add_type(&f, "repair").await;
    record(
        &f,
        Work {
            asset: f.camera,
            kind: "repair",
            day: "2024-03-02",
            open: false,
            summary: "Lens mount replaced",
            vendor: Some("Camera Clinic"),
            reference: Some("RMA-48213907"),
            cost: None,
        },
    )
    .await;

    // The record's write reindexes its asset in the background; the reader
    // picks the commit up shortly after.
    let mut hits = Vec::new();
    for _ in 0..50 {
        let (status, body) =
            call(&f, f.agent, Method::GET, "/api/search?q=RMA-48213907", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        hits = body["results"]
            .as_array()
            .expect("results")
            .iter()
            .filter(|r| r["entity_type"] == "device")
            .map(|r| r["entity_id"].as_i64().expect("id"))
            .collect();
        if !hits.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(hits, vec![i64::from(f.camera)]);

    // Records are staff only: a requester's search for the reference finds
    // nothing, and its total doesn't count the asset either.
    for uri in [
        "/api/search?q=RMA-48213907",
        "/api/search?q=RMA-48213907&types=device",
    ] {
        let (status, body) = call(&f, f.ws.member_uuid, Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert_eq!(body["results"], json!([]), "{uri}: {body}");
        assert_eq!(body["total"], 0, "{uri}: {body}");
    }
}
