//! A workspace can show requesters no assets
//! (`site_settings.requester_asset_visibility`). Under `all`, the default, a
//! requester (workspace role `member`) gets the workspace's assets like staff
//! do. Under `none` they get no asset record anywhere: not in the sync
//! snapshot, the delta or the live stream, not on any asset route, and not
//! embedded in a ticket or their profile. Staff are unaffected. Switching back
//! brings the assets back.

use std::collections::HashSet;
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use futures::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::sse::{Envelope, SseEvent, SseState, SseStream};
use backend::handlers::sync::delta::ActionRow;
use backend::middleware::RequestContext;
use backend::models::{AssetUpdate, Claims, NewAsset, NewTicket};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::sync::visibility::SyncViewer;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:requester_asset_visibility";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    admin: Uuid,
    agent: Uuid,
    member: Uuid,
    /// The member's laptop, linked to their ticket.
    asset: i32,
    ticket: i32,
    search: Arc<SearchService>,
    _search_dir: tempfile::TempDir,
}

fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(6);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a;
    let (admin, member) = (ws.admin_uuid, ws.member_uuid);
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    let (asset, ticket) = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        add_membership(
            c,
            ws.workspace_id,
            agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        let asset = backend::repository::assets::create_device(
            c,
            NewAsset {
                name: "Member laptop".to_string(),
                serial_number: Some("SN-REQ-VIS-1".to_string()),
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: Some(member),
                purchase_date: None,
                asset_tag: Some("TAG-REQ-VIS".to_string()),
                kind: "computer".to_string(),
                attributes: json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
            },
        )?
        .id;
        let state = backend::repository::workflow_states::default_state(c)?.id;
        let ticket: i32 = diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Laptop won't boot".to_string(),
                workflow_state_id: state,
                requester_uuid: Some(member),
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)?;
        backend::repository::tickets::add_device_to_ticket(c, ticket, asset)?;
        Ok::<_, diesel::result::Error>((asset, ticket))
    })
    .expect("seed asset and ticket");
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    Fixture {
        pool,
        ws,
        admin,
        agent,
        member,
        asset,
        ticket,
        search,
        _search_dir: search_dir,
    }
}

/// Call `uri` as `viewer` in the fixture's workspace.
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
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
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
            .service(
                web::scope("/api")
                    .configure(backend::handlers::sync::config)
                    .configure(backend::handlers::asset_settings::config)
                    .configure(backend::handlers::assets::config)
                    .service(
                        web::resource("/tickets/{id}")
                            .route(web::get().to(backend::handlers::get_ticket))
                            .route(web::patch().to(backend::handlers::update_ticket_partial)),
                    )
                    .route(
                        "/groups/{uuid}",
                        web::get().to(backend::handlers::groups::get_group_details),
                    )
                    .route(
                        "/tickets/{id}/asset-usage",
                        web::get().to(backend::handlers::asset_usage::list_for_ticket),
                    )
                    .route(
                        "/users/{uuid}/profile",
                        web::get().to(backend::handlers::users::get_user_profile_bundle),
                    ),
            ),
    )
    .await;
    let mut req = http_test::TestRequest::default().method(method).uri(uri);
    if let Some(body) = body {
        req = req.set_json(body);
    }
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    (status, body)
}

async fn get_json(f: &Fixture, viewer: Uuid, uri: &str) -> Value {
    let (status, body) = call(f, viewer, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    serde_json::from_str(&body).unwrap_or_else(|_| panic!("GET {uri} json: {body}"))
}

async fn get_status(f: &Fixture, viewer: Uuid, uri: &str) -> StatusCode {
    call(f, viewer, Method::GET, uri, None).await.0
}

async fn set_visibility(f: &Fixture, viewer: Uuid, visibility: &str) -> StatusCode {
    call(
        f,
        viewer,
        Method::PUT,
        "/api/admin/asset-settings",
        Some(json!({ "requester_visibility": visibility })),
    )
    .await
    .0
}

/// The bootstrap snapshot as `viewer` asks for it, with the open ticket's
/// group: its `__meta__` header and its records.
async fn bootstrap(f: &Fixture, viewer: Uuid) -> (Value, Vec<Value>) {
    let uri = format!(
        "/api/sync/bootstrap?groups=workspace:{},user:{viewer},ticket:{}&schema=test",
        f.ws.workspace_id, f.ticket
    );
    let (status, body) = call(f, viewer, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "bootstrap: {body}");
    let lines: Vec<Value> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("bootstrap line"))
        .collect();
    let meta = lines
        .iter()
        .find_map(|l| l.get("__meta__").cloned())
        .expect("a meta header");
    let rows = lines
        .into_iter()
        .filter(|row| row.get("__model__").is_some())
        .collect();
    (meta, rows)
}

/// The newest action in the workspace's feed.
fn newest_action(f: &Fixture) -> i64 {
    use backend::schema::sync_actions;
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        sync_actions::table
            .select(diesel::dsl::max(sync_actions::sync_id))
            .first::<Option<i64>>(c)
    })
    .expect("newest action")
    .unwrap_or(0)
}

/// The delta from the start of the feed as `viewer` asks for it, once it
/// covers the newest action, and its capability flags.
async fn delta(f: &Fixture, viewer: Uuid) -> (Vec<Value>, Value) {
    let newest = newest_action(f);
    let uri = format!(
        "/api/sync/delta?from=0&limit=5000&groups=workspace:{},user:{viewer},ticket:{}",
        f.ws.workspace_id, f.ticket
    );
    for _ in 0..200 {
        let body = get_json(f, viewer, &uri).await;
        if body["last_sync_id"].as_i64().is_some_and(|id| id >= newest) {
            return (
                body["actions"].as_array().expect("actions").clone(),
                body["capabilities"].clone(),
            );
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the delta never reached the newest action ({newest})");
}

/// Every action recorded in the workspace, shaped as the live stream carries
/// them.
fn every_action(f: &Fixture) -> Vec<Value> {
    use backend::schema::sync_actions;
    let rows: Vec<ActionRow> = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        sync_actions::table
            .order(sync_actions::sync_id.asc())
            .select((
                sync_actions::sync_id,
                sync_actions::aggregate,
                sync_actions::aggregate_id,
                sync_actions::op,
                sync_actions::event_type,
                sync_actions::schema_version,
                sync_actions::data,
                sync_actions::groups,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::actor_ref,
                sync_actions::correlation_id,
                sync_actions::causation_id,
                sync_actions::occurred_at,
                sync_actions::xid8,
            ))
            .load::<ActionRow>(c)
    })
    .expect("load actions");
    rows.iter()
        .map(|r| serde_json::to_value(r).expect("row json"))
        .collect()
}

/// The rows of every recorded action that `viewer`'s live stream sends. The
/// stream is opened once; the setting is read per batch.
async fn stream_rows(f: &Fixture, viewer: Uuid) -> Vec<Value> {
    let (sync_viewer, allowed) = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        let user = backend::repository::users::get_user_by_uuid(&viewer, c)?;
        Ok::<_, diesel::result::Error>((
            SyncViewer::resolve(c, &user),
            backend::sync::groups::allowed_for_user(c, &user)?,
        ))
    })
    .expect("viewer");
    let registry =
        Arc::new(backend::services::connection_registry::ConnectionRegistry::with_limits(4, 16));
    let guard = registry
        .try_acquire((viewer, f.ws.workspace_id))
        .expect("connection slot");
    let envelope = Envelope {
        id: 1,
        event: SseEvent::SyncActions {
            actions: Value::Array(every_action(f)),
            last_xid8: 0,
            last_sync_id: 0,
            timestamp: chrono::Utc::now(),
        },
        source_client_id: None,
    };
    let mut stream = SseStream::new(
        Vec::new(),
        vec![envelope],
        format!("assets-{viewer}"),
        web::Data::new(SseState::new()),
        web::Data::new(f.pool.clone()),
        sync_viewer,
        Some(f.ws.workspace_id),
        Arc::new(allowed.into_iter().collect::<HashSet<String>>()),
        guard,
    );
    let frame = stream.next().await.expect("a frame").expect("frame bytes");
    let frame = String::from_utf8(frame.to_vec()).expect("utf8 frame");
    let data = frame
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("data line");
    let event: Value = serde_json::from_str(data).expect("frame json");
    event
        .get("actions")
        .or_else(|| event.get("data").and_then(|d| d.get("actions")))
        .unwrap_or_else(|| panic!("no actions in frame: {data}"))
        .as_array()
        .expect("actions")
        .clone()
}

/// The record kinds about assets among `rows` (feed rows name an
/// `aggregate`, snapshot rows a `__model__`).
fn asset_kinds(rows: &[Value]) -> HashSet<String> {
    rows.iter()
        .filter_map(|r| {
            r.get("aggregate")
                .or_else(|| r.get("__model__"))
                .and_then(Value::as_str)
        })
        .filter(|k| matches!(*k, "asset" | "ticket_asset"))
        .map(str::to_string)
        .collect()
}

fn both_kinds() -> HashSet<String> {
    ["asset", "ticket_asset"].map(String::from).into()
}

fn ids(v: &Value) -> Vec<i64> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["id"].as_i64())
        .collect()
}

/// Everything a requester can reach about the asset, as `viewer` gets it:
/// whether they see it on every surface (`true`), or on none (`false`).
async fn assert_sees_assets(f: &Fixture, viewer: Uuid, who: &str, sees: bool) {
    let asset = f.asset;
    let want = |s: bool| if s { both_kinds() } else { HashSet::new() };

    let (meta, rows) = bootstrap(f, viewer).await;
    assert_eq!(meta["assets_visible"], json!(sees), "{who}: bootstrap flag");
    assert_eq!(asset_kinds(&rows), want(sees), "{who}: bootstrap rows");

    let (actions, capabilities) = delta(f, viewer).await;
    assert_eq!(
        capabilities["assets_visible"],
        json!(sees),
        "{who}: delta flag"
    );
    assert_eq!(asset_kinds(&actions), want(sees), "{who}: delta rows");

    let streamed = stream_rows(f, viewer).await;
    assert_eq!(
        asset_kinds(&streamed),
        want(sees),
        "{who}: live stream rows"
    );

    let all = get_json(f, viewer, "/api/assets").await;
    assert_eq!(ids(&all).contains(&(asset as i64)), sees, "{who}: /assets");
    let page = get_json(f, viewer, "/api/assets/paginated").await;
    assert_eq!(
        ids(&page["data"]).contains(&(asset as i64)),
        sees,
        "{who}: /assets/paginated"
    );
    let excluding = get_json(f, viewer, "/api/assets/paginated/excluding").await;
    assert_eq!(
        ids(&excluding["data"]).contains(&(asset as i64)),
        sees,
        "{who}: /assets/paginated/excluding"
    );
    let mine = get_json(f, viewer, &format!("/api/users/{}/assets", f.member)).await;
    assert_eq!(
        ids(&mine).contains(&(asset as i64)),
        sees,
        "{who}: /users/:uuid/assets"
    );
    let (status, export) = call(f, viewer, Method::GET, "/api/assets/export", None).await;
    assert_eq!(status, StatusCode::OK, "{who}: export");
    assert_eq!(export.contains("SN-REQ-VIS-1"), sees, "{who}: export rows");

    let expected = if sees {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    for route in [
        "",
        "/record-card",
        "/lifecycle",
        "/loans",
        "/media",
        "/usage",
        "/audits",
    ] {
        let uri = format!("/api/assets/{asset}{route}");
        assert_eq!(
            get_status(f, viewer, &uri).await,
            expected,
            "{who}: GET {uri}"
        );
    }

    let ticket = get_json(f, viewer, &format!("/api/tickets/{}", f.ticket)).await;
    assert_eq!(
        ids(&ticket["devices"]).contains(&(asset as i64)),
        sees,
        "{who}: the ticket's devices"
    );
}

#[actix_web::test]
async fn requesters_see_assets_only_while_the_workspace_shows_them() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);

    // `all` by default: the requester sees their laptop everywhere.
    assert_sees_assets(&f, f.member, "member under all", true).await;
    let profile = get_json(
        &f,
        f.member,
        &format!("/api/users/{}/profile?include=devices,loans", f.member),
    )
    .await;
    assert_eq!(ids(&profile["devices"]), vec![f.asset as i64]);

    // Only a workspace admin changes it, and only to a known choice.
    assert_eq!(
        set_visibility(&f, f.member, "none").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        set_visibility(&f, f.agent, "none").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        set_visibility(&f, f.admin, "groups").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(set_visibility(&f, f.admin, "none").await, StatusCode::OK);
    let saved = get_json(&f, f.admin, "/api/admin/asset-settings").await;
    assert_eq!(saved, json!({ "requester_visibility": "none" }));

    // A change to the asset after the switch reaches staff only.
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        backend::repository::assets::update_device(
            c,
            f.asset,
            AssetUpdate {
                location: Some(Some("Level 3".to_string())),
                ..Default::default()
            },
        )
    })
    .expect("update the asset");

    // `none`: nothing, on any path, including the asset on their own ticket
    // and the one they hold.
    assert_sees_assets(&f, f.member, "member under none", false).await;
    let profile = get_json(
        &f,
        f.member,
        &format!("/api/users/{}/profile?include=devices,loans", f.member),
    )
    .await;
    assert_eq!(profile["devices"], json!([]), "member's profile devices");
    assert_eq!(profile["loans"], json!([]), "member's profile loans");
    let usage = get_json(
        &f,
        f.member,
        &format!("/api/tickets/{}/asset-usage", f.ticket),
    )
    .await;
    assert_eq!(usage, json!([]));

    // Staff are unaffected.
    assert_sees_assets(&f, f.agent, "agent under none", true).await;
    assert_sees_assets(&f, f.admin, "admin under none", true).await;

    // Back to `all`: the assets come back.
    assert_eq!(set_visibility(&f, f.admin, "all").await, StatusCode::OK);
    assert_sees_assets(&f, f.member, "member back under all", true).await;
}

/// Staff get an asset's notes; anyone else doesn't. The asset list's
/// search finds an asset by its tag.
#[actix_web::test]
async fn asset_notes_are_staff_only_and_the_list_searches_tags() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        backend::repository::assets::update_device(
            c,
            f.asset,
            AssetUpdate {
                notes: Some(Some("Spare charger in drawer 3".to_string())),
                ..Default::default()
            },
        )
    })
    .expect("set notes");

    let uri = format!("/api/assets/{}", f.asset);
    let staff = get_json(&f, f.agent, &uri).await;
    assert_eq!(staff["notes"], json!("Spare charger in drawer 3"));
    let requester = get_json(&f, f.member, &uri).await;
    assert!(requester.get("notes").is_none(), "{requester}");
    let list = get_json(&f, f.member, "/api/assets").await;
    assert!(
        list.as_array()
            .expect("list")
            .iter()
            .all(|a| a.get("notes").is_none()),
        "{list}"
    );

    let found = get_json(&f, f.agent, "/api/assets/paginated?search=TAG-REQ").await;
    assert_eq!(ids(&found["data"]), vec![f.asset as i64], "{found}");
}

/// The note text the embed test plants; a distinctive string so a body
/// search for it can't match anything else.
const NOTE: &str = "Staff only: spare charger in drawer 3";

/// `asset`'s notes as a response carries them: `None` when the key is
/// absent or null.
fn notes_of(asset: &Value) -> Option<&str> {
    asset.get("notes").and_then(Value::as_str)
}

/// The asset with `id` among `assets`.
fn asset_in(assets: &Value, id: i32) -> &Value {
    assets
        .as_array()
        .and_then(|a| a.iter().find(|x| x["id"].as_i64() == Some(id as i64)))
        .unwrap_or_else(|| panic!("asset {id} missing from {assets}"))
}

/// Every response that embeds a whole asset row (a ticket, a group, a
/// profile's devices and loans, the export) carries its notes to staff
/// only.
#[actix_web::test]
async fn asset_notes_stay_with_staff_where_an_asset_is_embedded() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    let group_uuid = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        backend::repository::assets::update_device(
            c,
            f.asset,
            AssetUpdate {
                notes: Some(Some(NOTE.to_string())),
                ..Default::default()
            },
        )?;
        let group = backend::repository::groups::create_group(
            c,
            backend::models::NewGroup {
                name: "Loaner pool".to_string(),
                description: None,
                color: None,
                created_by: Some(f.admin),
            },
        )?;
        backend::repository::groups::set_group_devices(c, group.id, vec![f.asset], None)?;
        Ok::<_, diesel::result::Error>(group.uuid)
    })
    .expect("set notes and group the asset");
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        backend::repository::asset_loans::issue(
            c,
            backend::repository::asset_loans::IssueLoan {
                asset_id: f.asset,
                borrower_user_uuid: f.member,
                loaned_at: None,
                due_back: None,
                ticket_id: Some(f.ticket),
                notes: Some("Loan note for staff".to_string()),
                actor_uuid: Some(f.agent),
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("loan the asset to the member");

    // (viewer, who, sees notes). The profile is self or workspace admin, so
    // the admin stands in for staff there.
    for (viewer, who, sees) in [(f.member, "member", false), (f.agent, "agent", true)] {
        let want = sees.then_some(NOTE);

        let ticket = get_json(&f, viewer, &format!("/api/tickets/{}", f.ticket)).await;
        assert_eq!(
            notes_of(asset_in(&ticket["devices"], f.asset)),
            want,
            "{who}: GET ticket"
        );

        let (status, body) = call(
            &f,
            viewer,
            Method::PATCH,
            &format!("/api/tickets/{}", f.ticket),
            Some(json!({ "title": format!("Laptop won't boot ({who})") })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{who}: PATCH ticket: {body}");
        let patched: Value = serde_json::from_str(&body).expect("patched ticket json");
        assert_eq!(
            notes_of(asset_in(&patched["devices"], f.asset)),
            want,
            "{who}: PATCH ticket"
        );

        let group = get_json(&f, viewer, &format!("/api/groups/{group_uuid}")).await;
        assert_eq!(
            notes_of(asset_in(&group["devices"], f.asset)),
            want,
            "{who}: group details"
        );

        let (status, csv) = call(&f, viewer, Method::GET, "/api/assets/export", None).await;
        assert_eq!(status, StatusCode::OK, "{who}: CSV export");
        let header = csv.lines().next().expect("CSV header");
        assert!(header.split(',').any(|c| c == "notes"), "{who}: {header}");
        assert!(csv.contains("SN-REQ-VIS-1"), "{who}: CSV rows: {csv}");
        assert_eq!(csv.contains(NOTE), sees, "{who}: CSV notes: {csv}");

        let json_export = get_json(&f, viewer, "/api/assets/export?format=json").await;
        assert_eq!(
            notes_of(asset_in(&json_export, f.asset)),
            want,
            "{who}: JSON export"
        );
    }

    for (viewer, who, sees) in [(f.member, "member", false), (f.admin, "admin", true)] {
        let profile = get_json(
            &f,
            viewer,
            &format!("/api/users/{}/profile?include=devices,loans", f.member),
        )
        .await;
        assert_eq!(
            notes_of(asset_in(&profile["devices"], f.asset)),
            sees.then_some(NOTE),
            "{who}: profile devices"
        );
        let loan = &profile["loans"][0];
        assert_eq!(
            notes_of(&loan["asset"]),
            sees.then_some(NOTE),
            "{who}: profile loan's asset"
        );
        assert_eq!(
            notes_of(loan),
            sees.then_some("Loan note for staff"),
            "{who}: profile loan's own notes"
        );
    }
}
