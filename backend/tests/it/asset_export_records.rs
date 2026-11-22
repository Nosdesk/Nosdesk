//! Asset exports of records and timelines: `scope=history` keeps its
//! columns, `scope=records` writes the records import template's columns
//! with cells the import reads back, `scope=timeline` and the record card
//! write one row per timeline entry, and every free-text cell is guarded
//! against spreadsheet formulas.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::{NaiveDate, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, NewTicket, UpdateSiteSettings};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{assets as asset_repo, site_settings};
use backend::schema::{tickets, user_emails};
use backend::services::imports::asset_records::{template_headers, TEMPLATE_HEADERS};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_export_records";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    asset: i32,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a;
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
                default_timezone: Some("Australia/Sydney".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and settings");
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(agent),
            user_emails::email.eq("agent@example.com"),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(&mut pool.get().expect("conn"))
        .expect("the agent's email");
    let asset = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        asset_repo::create_device(
            c,
            NewAsset {
                name: "=Camera".to_string(),
                serial_number: Some("SN-14".to_string()),
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: None,
                purchase_date: Some(NaiveDate::from_ymd_opt(2020, 1, 1).expect("a date")),
                asset_tag: Some("CAM-014".to_string()),
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
    .id;
    Fixture {
        pool,
        ws,
        agent,
        asset,
    }
}

async fn call(
    f: &Fixture,
    viewer: Uuid,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, String) {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: format!("{viewer}@example.com"),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (Utc::now().timestamp() + 3600) as usize,
        iat: Utc::now().timestamp() as usize,
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
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn get(f: &Fixture, viewer: Uuid, uri: &str) -> String {
    let (status, body) = call(f, viewer, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    body
}

/// A CSV's rows, header first.
fn rows(csv: &str) -> Vec<Vec<String>> {
    let mut all = vec![header(csv)];
    for row in csv::Reader::from_reader(csv.as_bytes()).records() {
        all.push(row.expect("a row").iter().map(str::to_string).collect());
    }
    all
}

fn header(csv: &str) -> Vec<String> {
    csv::Reader::from_reader(csv.as_bytes())
        .headers()
        .expect("a header")
        .iter()
        .map(str::to_string)
        .collect()
}

/// A repair on the asset: timed, linked to a ticket, with a cost, a vendor
/// that reads as a formula and the type's fields.
async fn seed_repair(f: &Fixture) -> i32 {
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        "/api/assets/record-types/catalogue/repair",
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let ticket: i32 = in_ws(f, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Lens fault".to_string(),
                workflow_state_id: state.id,
                ..Default::default()
            })
            .returning(tickets::number)
            .get_result(c)
    });
    let ticket_id: i32 = in_ws(f, |c| {
        tickets::table
            .filter(tickets::number.eq(ticket))
            .select(tickets::id)
            .first(c)
    });
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{}/records", f.asset),
        Some(json!({
            "record_type": "repair",
            "summary": "Lens mount replaced",
            "occurred_at": "2023-05-03T09:30:00+10:00",
            "completed_on": "2023-05-04",
            "outcome": "repaired",
            "vendor": "=HYPERLINK(\"x\")",
            "performed_by_uuid": f.agent,
            "reference": "RMA-4471",
            "cost": { "amount": "1250.50", "currency": "AUD" },
            "ticket_id": ticket_id,
            "attributes": { "fault": "Lens mount loose", "offsite": true },
            "source_ref": "old-system-1",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    ticket
}

#[actix_web::test]
async fn history_keeps_its_columns() {
    let db = common::TestDb::new();
    let f = seed(&db);
    seed_repair(&f).await;
    let csv = get(&f, f.agent, "/api/assets/export?scope=history").await;
    assert_eq!(
        header(&csv).join(","),
        "asset_tag,asset_name,occurred_at,from_status,to_status,reason,actor_uuid,ticket_id,metadata"
    );
}

#[actix_web::test]
async fn records_export_the_import_templates_columns_and_cells() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let ticket_number = seed_repair(&f).await;

    // Across types: the template's columns, then the type fields as JSON.
    let csv = get(&f, f.agent, "/api/assets/export?scope=records").await;
    let mut expected: Vec<String> = TEMPLATE_HEADERS.iter().map(ToString::to_string).collect();
    expected.push("attributes".to_string());
    let all = rows(&csv);
    assert_eq!(all[0], expected);
    assert_eq!(all.len(), 2, "{csv}");
    let attributes: Value = serde_json::from_str(all[1].last().expect("attributes")).expect("JSON");
    assert_eq!(attributes["fault"], "Lens mount loose");

    // One type: the template's columns for that type, field by field.
    let csv = get(
        &f,
        f.agent,
        "/api/assets/export?scope=records&record_type=repair",
    )
    .await;
    // The template the importer offers for the type, column for column.
    let expected = in_ws(&f, |c| template_headers(c, Some("repair"))).expect("the repair template");
    assert!(expected.contains(&"fault".to_string()), "{expected:?}");
    let all = rows(&csv);
    assert_eq!(all[0], expected);
    let cell = |name: &str| -> String {
        let at = all[0].iter().position(|h| h == name).expect(name);
        all[1][at].clone()
    };
    assert_eq!(cell("asset_tag"), "CAM-014");
    assert_eq!(cell("serial_number"), "SN-14");
    assert_eq!(cell("record_type"), "repair");
    // The moment in the workspace's timezone, which the import reads back.
    assert_eq!(cell("occurred_on"), "2023-05-03T09:30:00+10:00");
    assert_eq!(cell("completed_on"), "2023-05-04");
    assert_eq!(cell("outcome"), "repaired");
    assert_eq!(
        cell("vendor"),
        "'=HYPERLINK(\"x\")",
        "guarded against formulas"
    );
    assert_eq!(cell("performed_by"), "agent@example.com");
    assert_eq!(cell("cost"), "1250.50");
    assert_eq!(cell("currency"), "AUD");
    assert_eq!(cell("ticket"), ticket_number.to_string());
    assert_eq!(cell("source_ref"), "old-system-1");
    assert_eq!(cell("fault"), "Lens mount loose");
    assert_eq!(cell("offsite"), "true");

    // JSON carries the same cells.
    let body = get(
        &f,
        f.agent,
        "/api/assets/export?scope=records&record_type=repair&format=json",
    )
    .await;
    let json: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(json[0]["reference"], "RMA-4471", "{json}");

    // Records are staff only: anyone else gets the columns and no rows.
    let csv = get(&f, f.ws.member_uuid, "/api/assets/export?scope=records").await;
    assert_eq!(rows(&csv).len(), 1, "{csv}");
    assert!(!csv.contains("RMA-4471"));
}

#[actix_web::test]
async fn the_timeline_export_and_record_card_write_one_row_per_entry() {
    let db = common::TestDb::new();
    let f = seed(&db);
    seed_repair(&f).await;

    const COLUMNS: &str = "asset_tag,asset_name,source,day,at,type,summary,outcome,vendor,\
                           reference,cost,currency,ticket_id,actor_uuid,recorded_at,\
                           recorded_by,details";
    for uri in [
        "/api/assets/export?scope=timeline".to_string(),
        format!("/api/assets/{}/record-card", f.asset),
    ] {
        let csv = get(&f, f.agent, &uri).await;
        let all = rows(&csv);
        assert_eq!(all[0].join(","), COLUMNS, "{uri}");
        let record = all
            .iter()
            .find(|r| r[2] == "record")
            .unwrap_or_else(|| panic!("{uri}: no record row: {csv}"));
        assert_eq!(record[0], "CAM-014");
        assert_eq!(record[1], "'=Camera", "{uri}: guarded against formulas");
        assert_eq!(record[3], "2023-05-03");
        assert_eq!(record[5], "repair");
        assert_eq!(record[6], "Lens mount replaced");
        assert_eq!(record[8], "'=HYPERLINK(\"x\")");
        assert_eq!(record[10], "1250.50");
        assert_eq!(record[11], "AUD");
        assert_eq!(record[13], f.agent.to_string(), "who did the work");
        assert!(all.iter().any(|r| r[2] == "marker" && r[5] == "purchased"));
        assert!(all.iter().any(|r| r[2] == "marker" && r[5] == "added"));
    }

    // Anyone but staff gets the entries they may see.
    for uri in [
        "/api/assets/export?scope=timeline".to_string(),
        format!("/api/assets/{}/record-card", f.asset),
    ] {
        let csv = get(&f, f.ws.member_uuid, &uri).await;
        assert!(
            rows(&csv).iter().skip(1).all(|r| r[2] != "record"),
            "{uri}: {csv}"
        );
        assert!(
            !csv.contains("RMA-4471") && !csv.contains("HYPERLINK"),
            "{uri}"
        );
    }

    // Filtered to sources.
    let body = get(
        &f,
        f.agent,
        "/api/assets/export?scope=timeline&sources=marker&format=json",
    )
    .await;
    let json: Value = serde_json::from_str(&body).expect("JSON");
    let sources: Vec<&str> = json
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["source"].as_str().expect("source"))
        .collect();
    assert_eq!(sources, vec!["marker", "marker"], "{json}");
}

#[actix_web::test]
async fn a_records_export_imports_back_with_nothing_to_change() {
    use crate::imports_assets::{call_import_route, json, mapping_request, upload_request_for};
    use crate::imports_attributes::{errors, id_of};

    common::ensure_test_keyring();
    common::with_upload_dir();
    let db = common::TestDb::new();
    let f = seed(&db);
    seed_repair(&f).await;
    let csv = get(
        &f,
        f.agent,
        "/api/assets/export?scope=records&record_type=repair",
    )
    .await;

    // Upload the export as service history, each field from its column.
    let (status, body) = call_import_route(
        &f.pool,
        &f.ws,
        f.ws.admin_uuid,
        upload_request_for("asset_records", "history.csv", csv.as_bytes()),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    let mapping: serde_json::Map<String, Value> = header(&csv)
        .into_iter()
        .enumerate()
        .map(|(i, h)| (h, i.into()))
        .collect();
    let (status, body) = call_import_route(
        &f.pool,
        &f.ws,
        f.ws.admin_uuid,
        mapping_request(&id_of(&job), json!({ "mapping": mapping, "options": {} })),
    )
    .await;
    let job = json(&body);
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(errors(&job), Vec::new(), "{job}");
    let count = |key: &str| job["summary"][key].as_u64().unwrap_or(0);
    assert_eq!(
        (
            count("would_create"),
            count("would_update"),
            count("unchanged")
        ),
        (0, 0, 1),
        "{job}"
    );
}
