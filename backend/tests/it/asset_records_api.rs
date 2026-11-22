//! Asset records over REST: staff read and write them, requesters never see
//! them or their files even when they see every asset, an edit holds to the
//! version it read, a source reference makes a write idempotent, a record is
//! deleted by an admin or by whoever entered it within a day, an upload can
//! belong to a record, and an `assets:read` token reads records and writes
//! none.

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::NaiveDate;
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, NewAssetMedia, UpdateSiteSettings};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{asset_media, assets as asset_repo, site_settings};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_records_api";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    asset: i32,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

fn new_asset(pool: &TestPool, ws: i32, name: &str) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        asset_repo::create_device(
            c,
            NewAsset {
                name: name.to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: Some("Studio A".to_string()),
                notes: None,
                primary_user_uuid: None,
                purchase_date: Some(NaiveDate::from_ymd_opt(2024, 1, 1).expect("a date")),
                asset_tag: None,
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

/// Workspace A with an agent, an asset, and requesters shown every asset.
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
                // Timed work reads its day in UTC.
                default_timezone: Some("UTC".to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and the visibility");
    let asset = new_asset(&pool, ws.workspace_id, "Camera 14");
    Fixture {
        pool,
        ws,
        agent,
        asset,
    }
}

/// Send `req` as `viewer` in workspace A, signed in.
async fn send(f: &Fixture, viewer: Uuid, req: http_test::TestRequest) -> (StatusCode, Value) {
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
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
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
    send(f, viewer, req).await
}

async fn add_repair(f: &Fixture) {
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        "/api/assets/record-types/catalogue/repair",
        None,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
}

fn repair(occurred_on: &str) -> Value {
    json!({
        "record_type": "repair",
        "summary": "Lens mount replaced",
        "occurred_on": occurred_on,
        "completed_on": occurred_on,
        "outcome": "repaired",
        "vendor": "Camera Clinic",
        "reference": "RMA-4471",
        "cost": { "amount": "1250.50", "currency": "AUD" },
        "attributes": { "fault": "Lens mount loose", "offsite": true },
    })
}

/// Create a record as `viewer`; its id.
async fn create(f: &Fixture, viewer: Uuid, body: Value) -> i64 {
    let uri = format!("/api/assets/{}/records", f.asset);
    let (status, created) = call(f, viewer, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["record"]["id"].as_i64().expect("record id")
}

fn record_file(f: &Fixture, name: &str, record_id: Option<i32>) -> i32 {
    in_ws(f, |c| {
        asset_media::create(
            c,
            NewAssetMedia {
                asset_id: f.asset,
                url: format!("/api/files/assets/{}/media/{}", f.asset, Uuid::new_v4()),
                name: name.to_string(),
                file_size: Some(10),
                mime_type: Some("application/pdf".to_string()),
                checksum: None,
                kind: "document".to_string(),
                sort_order: 0,
                caption: None,
                uploaded_by: None,
                thumbnail_url: None,
                record_id,
            },
        )
    })
    .id
}

#[actix_web::test]
async fn requesters_get_404_and_never_see_records_or_their_files() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let id = create(&f, f.agent, repair("2023-05-03")).await;
    let report = record_file(&f, "report.pdf", Some(id as i32));
    let photo = record_file(&f, "photo.pdf", None);

    let requester = f.ws.member_uuid;
    let a = f.asset;
    for (method, uri, body) in [
        (Method::GET, format!("/api/assets/{a}/records"), None),
        (
            Method::POST,
            format!("/api/assets/{a}/records"),
            Some(repair("2023-05-04")),
        ),
        (Method::GET, format!("/api/assets/{a}/records/{id}"), None),
        (
            Method::PATCH,
            format!("/api/assets/{a}/records/{id}"),
            Some(json!({ "expected_version": 1, "summary": "x" })),
        ),
        (
            Method::DELETE,
            format!("/api/assets/{a}/records/{id}"),
            None,
        ),
        (
            Method::PUT,
            format!("/api/assets/{a}/records/{id}/media"),
            Some(json!({ "media_ids": [] })),
        ),
        (Method::GET, "/api/assets/record-types".to_string(), None),
        (
            Method::GET,
            "/api/assets/record-types/catalogue".to_string(),
            None,
        ),
        (
            Method::POST,
            "/api/assets/record-types/catalogue/inspection".to_string(),
            None,
        ),
        (Method::GET, "/api/assets/record-vendors".to_string(), None),
    ] {
        let (status, body) = call(&f, requester, method.clone(), &uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {body}");
    }

    // The asset and its own files are still theirs to see; the record's
    // file isn't.
    let (status, media) = call(
        &f,
        requester,
        Method::GET,
        &format!("/api/assets/{a}/media"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{media}");
    let ids: Vec<i64> = media
        .as_array()
        .expect("media")
        .iter()
        .filter_map(|m| m["id"].as_i64())
        .collect();
    assert_eq!(ids, vec![i64::from(photo)], "{media}");
    let (_, staff_media) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{a}/media"),
        None,
    )
    .await;
    assert_eq!(staff_media.as_array().expect("media").len(), 2);
    assert!(staff_media
        .as_array()
        .expect("media")
        .iter()
        .any(|m| m["id"] == json!(report) && m["record_id"] == json!(id)));
}

#[actix_web::test]
async fn agents_write_records_with_warnings_and_edits_hold_to_their_version() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let uri = format!("/api/assets/{}/records", f.asset);

    // Work before the asset's purchase is recorded, with a warning.
    let (status, created) = call(&f, f.agent, Method::POST, &uri, Some(repair("2023-05-03"))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let record = &created["record"];
    assert_eq!(record["record_type"], "repair");
    assert_eq!(record["source"], "manual");
    assert_eq!(record["recorded_by"], json!(f.agent));
    assert_eq!(
        record["cost"],
        json!({ "amount": "1250.50", "currency": "AUD", "amount_minor": 125050 })
    );
    assert!(record.get("cost_minor").is_none(), "{record}");
    assert!(created["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|w| w["code"] == "before_purchase"));
    let id = record["id"].as_i64().expect("id");

    // A cost without a code is in the workspace currency (none set: the
    // default locale's suggestion).
    let mut body = repair("2024-02-01");
    body["cost"] = json!({ "amount": "10" });
    let (status, created) = call(&f, f.agent, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["record"]["cost"]["currency"], "USD");

    // Refusals by name.
    let mut body = repair("2024-02-01");
    body["outcome"] = json!("exploded");
    let (status, refused) = call(&f, f.agent, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "UNKNOWN_OUTCOME");
    let mut body = repair("2099-01-01");
    body["completed_on"] = Value::Null;
    body["outcome"] = Value::Null;
    let (status, refused) = call(&f, f.agent, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "DATE_IN_FUTURE");
    let mut body = repair("2024-02-01");
    body["record_type"] = json!("inspection");
    let (status, refused) = call(&f, f.agent, Method::POST, &uri, Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "RECORD_TYPE_NOT_FOUND");

    // An edit at the version it read; a second edit from the same read is
    // refused with the version now current.
    let one = format!("{uri}/{id}");
    let (status, edited) = call(
        &f,
        f.agent,
        Method::PATCH,
        &one,
        Some(json!({ "expected_version": 1, "vendor": null, "summary": "Mount reseated" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["record"]["version"], 2);
    assert_eq!(edited["record"]["vendor"], Value::Null);
    assert_eq!(edited["record"]["summary"], "Mount reseated");
    assert_eq!(edited["record"]["reference"], "RMA-4471", "left out, kept");
    let (status, stale) = call(
        &f,
        f.agent,
        Method::PATCH,
        &one,
        Some(json!({ "expected_version": 1, "summary": "Lost update" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["code"], "VERSION_CONFLICT");
    assert_eq!(stale["current_version"], 2);

    // A record of another asset isn't this asset's.
    let other = new_asset(&f.pool, f.ws.workspace_id, "Camera 15");
    let (status, _) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{other}/records/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn a_source_reference_answers_a_second_post_with_the_existing_record() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let mut body = repair("2023-05-03");
    body["source_ref"] = json!("sheet-row-17");
    let id = create(&f, f.agent, body.clone()).await;
    let (status, again) = call(
        &f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{}/records", f.asset),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["code"], "RECORD_EXISTS");
    assert_eq!(again["id"], json!(id));
}

#[actix_web::test]
async fn the_recorder_deletes_within_a_day_and_an_admin_any_time() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let a = f.asset;

    // The agent deletes their own fresh record; its file stays on the asset.
    let own = create(&f, f.agent, repair("2023-05-03")).await;
    let file = record_file(&f, "report.pdf", Some(own as i32));
    let (status, body) = call(
        &f,
        f.agent,
        Method::DELETE,
        &format!("/api/assets/{a}/records/{own}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["files_deleted"], 0);
    let left = in_ws(&f, |c| asset_media::get_by_id(c, file));
    assert_eq!(left.record_id, None, "the file stays, pointing at nothing");

    // Not someone else's, and not their own a day on.
    let admins = create(&f, f.ws.admin_uuid, repair("2023-05-03")).await;
    let (status, _) = call(
        &f,
        f.agent,
        Method::DELETE,
        &format!("/api/assets/{a}/records/{admins}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let old = create(&f, f.agent, repair("2023-05-03")).await;
    let mut conn = f.pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        diesel::sql_query(
            "UPDATE asset_records SET recorded_at = now() - interval '25 hours' WHERE id = $1",
        )
        .bind::<diesel::sql_types::Integer, _>(old as i32)
        .execute(c)
    })
    .expect("backdate the entry");
    let (status, _) = call(
        &f,
        f.agent,
        Method::DELETE,
        &format!("/api/assets/{a}/records/{old}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // An admin deletes either, and with its files when asked.
    let file = record_file(&f, "invoice.pdf", Some(old as i32));
    let (status, body) = call(
        &f,
        f.ws.admin_uuid,
        Method::DELETE,
        &format!("/api/assets/{a}/records/{old}?delete_files=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["files_deleted"], 1);
    let gone = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        asset_media::get_by_id(c, file).optional()
    })
    .expect("read");
    assert!(gone.is_none());
    let (status, _) = call(
        &f,
        f.ws.admin_uuid,
        Method::DELETE,
        &format!("/api/assets/{a}/records/{admins}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[actix_web::test]
async fn pages_follow_the_cursor_through_timed_and_date_only_work() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    // One day with two timed records and two date-only ones, and an open
    // record the day before.
    let mut ids = Vec::new();
    for at in ["2024-03-05T09:00:00Z", "2024-03-05T15:00:00Z"] {
        let mut body = repair("2024-03-05");
        body.as_object_mut().expect("body").remove("occurred_on");
        body.as_object_mut().expect("body").remove("completed_on");
        body["occurred_at"] = json!(at);
        body["completed_at"] = json!(at);
        ids.push(create(&f, f.agent, body).await);
    }
    for _ in 0..2 {
        ids.push(create(&f, f.agent, repair("2024-03-05")).await);
    }
    let mut open = repair("2024-03-04");
    open["completed_on"] = Value::Null;
    open["outcome"] = Value::Null;
    let open_id = create(&f, f.agent, open).await;

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut uri = format!("/api/assets/{}/records?limit=2", f.asset);
        if let Some(c) = &cursor {
            uri.push_str(&format!("&before={c}"));
        }
        let (status, page) = call(&f, f.agent, Method::GET, &uri, None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        seen.extend(
            page["records"]
                .as_array()
                .expect("records")
                .iter()
                .map(|r| r["id"].as_i64().expect("id")),
        );
        match page["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }
    // Timed work later in the day first, then the day's date-only work
    // newest entry first, then the day before.
    assert_eq!(seen, vec![ids[1], ids[0], ids[3], ids[2], open_id]);

    let (_, open_only) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{}/records?open=true&type=repair", f.asset),
        None,
    )
    .await;
    let open_ids: Vec<i64> = open_only["records"]
        .as_array()
        .expect("records")
        .iter()
        .map(|r| r["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(open_ids, vec![open_id]);
    let (_, none) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{}/records?type=inspection", f.asset),
        None,
    )
    .await;
    assert_eq!(none["records"], json!([]));
}

#[actix_web::test]
async fn vendors_are_suggested_most_used_first_whatever_their_case() {
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    for vendor in [
        "Camera Clinic",
        "Camera Clinic",
        "camera clinic",
        "Lens 100%",
    ] {
        let mut body = repair("2023-05-03");
        body["vendor"] = json!(vendor);
        create(&f, f.agent, body).await;
    }
    let (status, all) = call(&f, f.agent, Method::GET, "/api/assets/record-vendors", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(all, json!(["Camera Clinic", "Lens 100%"]));
    let (_, matched) = call(
        &f,
        f.agent,
        Method::GET,
        "/api/assets/record-vendors?q=100%25",
        None,
    )
    .await;
    assert_eq!(
        matched,
        json!(["Lens 100%"]),
        "% is a character, not a wildcard"
    );
}

fn multipart(filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--x--\r\n");
    body
}

#[actix_web::test]
async fn an_upload_for_a_record_lands_in_both_lists_and_links_hold_to_the_asset() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let a = f.asset;
    let id = create(&f, f.agent, repair("2023-05-03")).await;
    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << >>\n%%EOF\n";

    let upload = |uri: String| {
        http_test::TestRequest::post()
            .uri(&uri)
            .insert_header(("content-type", "multipart/form-data; boundary=x"))
            .set_payload(multipart("Service Report.pdf", pdf))
    };
    let (status, uploaded) = send(
        &f,
        f.agent,
        upload(format!("/api/assets/{a}/media?record_id={id}")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let media_id = uploaded[0]["id"].as_i64().expect("media id");
    assert_eq!(uploaded[0]["record_id"], json!(id));

    let (_, media) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{a}/media"),
        None,
    )
    .await;
    assert!(media
        .as_array()
        .expect("media")
        .iter()
        .any(|m| m["id"] == json!(media_id)));
    let (_, record) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{a}/records/{id}"),
        None,
    )
    .await;
    assert_eq!(record["files"][0]["id"], json!(media_id), "{record}");

    // Not a record of this asset: refused before anything is stored.
    let other = new_asset(&f.pool, f.ws.workspace_id, "Camera 15");
    let (status, _) = send(
        &f,
        f.agent,
        upload(format!("/api/assets/{other}/media?record_id={id}")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Linking: a file of another asset refuses the whole change.
    let loose = record_file(&f, "older-report.pdf", None);
    let foreign = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        asset_media::create(
            c,
            NewAssetMedia {
                asset_id: other,
                url: format!("/api/files/assets/{other}/media/x.pdf"),
                name: "x.pdf".to_string(),
                file_size: None,
                mime_type: None,
                checksum: None,
                kind: "document".to_string(),
                sort_order: 0,
                caption: None,
                uploaded_by: None,
                thumbnail_url: None,
                record_id: None,
            },
        )
    })
    .expect("a file of another asset")
    .id;
    let link = format!("/api/assets/{a}/records/{id}/media");
    let (status, refused) = call(
        &f,
        f.agent,
        Method::PUT,
        &link,
        Some(json!({ "media_ids": [loose, foreign] })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["media_ids"], json!([foreign]));
    let (_, record) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{a}/records/{id}"),
        None,
    )
    .await;
    assert_eq!(
        record["files"].as_array().expect("files").len(),
        1,
        "nothing changed: {record}"
    );

    let (status, files) = call(
        &f,
        f.agent,
        Method::PUT,
        &link,
        Some(json!({ "media_ids": [loose, loose] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{files}");
    let ids: Vec<i64> = files
        .as_array()
        .expect("files")
        .iter()
        .map(|m| m["id"].as_i64().expect("id"))
        .collect();
    assert_eq!(ids, vec![i64::from(loose)]);
}

/// Mint a token for `user` through the admin route.
async fn mint(pool: &TestPool, ws: &WorkspaceSeed, user: Uuid, scopes: Option<&[&str]>) -> String {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
        file_path: None,
    };
    let workspace = workspace_context(ws);
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::api_tokens::config)),
    )
    .await;
    let mut body = json!({ "name": "records", "user_uuid": user });
    if let Some(scopes) = scopes {
        body["scopes"] = json!(scopes);
    }
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/api/admin/api-tokens")
            .set_json(body)
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = http_test::read_body_json(resp).await;
    body["token"].as_str().expect("token").to_string()
}

fn workspace_context(ws: &WorkspaceSeed) -> WorkspaceContext {
    WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    }
}

/// Call with `token` through the real auth middleware, which holds a
/// narrowed token to its scopes.
async fn with_token(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    token: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let workspace = workspace_context(ws);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(actix_web::middleware::from_fn(
                backend::middleware::dual_auth_middleware,
            ))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::assets::config)),
    )
    .await;
    let mut req = http_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")));
    if let Some(body) = body {
        req = req.set_json(body);
    }
    match http_test::try_call_service(&app, req.to_request()).await {
        Ok(resp) => {
            let status = resp.status();
            let body = http_test::read_body(resp).await;
            (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
        }
        Err(e) => (e.as_response_error().status_code(), Value::Null),
    }
}

#[actix_web::test]
async fn an_assets_read_token_reads_records_and_writes_none() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    add_repair(&f).await;
    let id = create(&f, f.agent, repair("2023-05-03")).await;
    let pool = db.runtime_pool(4);
    let a = f.asset;

    let reader = mint(&pool, &f.ws, f.agent, Some(&["assets:read"])).await;
    for uri in [
        format!("/api/assets/{a}/records"),
        format!("/api/assets/{a}/records/{id}"),
        "/api/assets/record-types".to_string(),
        "/api/assets/record-vendors".to_string(),
    ] {
        let (status, body) = with_token(&pool, &f.ws, &reader, Method::GET, &uri, None).await;
        assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    }
    for (method, uri, body) in [
        (
            Method::POST,
            format!("/api/assets/{a}/records"),
            Some(repair("2023-05-04")),
        ),
        (
            Method::PATCH,
            format!("/api/assets/{a}/records/{id}"),
            Some(json!({ "expected_version": 1, "summary": "x" })),
        ),
        (
            Method::DELETE,
            format!("/api/assets/{a}/records/{id}"),
            None,
        ),
        (
            Method::POST,
            "/api/assets/record-types/catalogue/inspection".to_string(),
            None,
        ),
    ] {
        let (status, _) = with_token(&pool, &f.ws, &reader, method.clone(), &uri, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    // A token that may write records them as written through the API.
    let writer = mint(&pool, &f.ws, f.agent, None).await;
    let (status, created) = with_token(
        &pool,
        &f.ws,
        &writer,
        Method::POST,
        &format!("/api/assets/{a}/records"),
        Some(repair("2023-05-04")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["record"]["source"], "api");
}
