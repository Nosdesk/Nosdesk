//! The asset timeline: one list, newest first, of an asset's status changes,
//! records, replacements, loans, linked tickets and markers. A date-only
//! entry sorts after the timed entries of its day in the workspace's
//! timezone, pages concatenate to the whole list whatever their size and
//! however the entries tie, tickets are the ones the viewer can see,
//! requesters get the public projection, and a chain of replacements that
//! loops still ends.

use std::collections::HashSet;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset, NewTicket, RecordSource, UpdateSiteSettings};
use backend::repository::asset_records::{self as records_repo, NewRecord, WorkDate};
use backend::repository::asset_timeline::predecessors;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{asset_record_types, assets as asset_repo, site_settings};
use backend::schema::{asset_lifecycle_events, ticket_assets, tickets};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_timeline";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    agent: Uuid,
    asset: i32,
}

fn in_ws<T>(f: &Fixture, op: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, op).expect("run in the workspace")
}

fn at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .expect("a moment")
        .with_timezone(&Utc)
}

fn day(raw: &str) -> NaiveDate {
    raw.parse().expect("a day")
}

fn new_asset(f: &Fixture, name: &str) -> i32 {
    in_ws(f, |c| {
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
                purchase_date: Some(day("2020-01-01")),
                asset_tag: Some(format!("TAG-{name}")),
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
    .id
}

/// Workspace A with an agent, an asset, requesters shown every asset, and
/// the workspace in `timezone`.
fn seed(db: &common::TestDb, timezone: &str) -> Fixture {
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
                default_timezone: Some(timezone.to_string()),
                ..Default::default()
            },
        )
    })
    .expect("seed the agent and settings");
    let mut f = Fixture {
        pool,
        ws,
        agent,
        asset: 0,
    };
    f.asset = new_asset(&f, "CAM-014");
    f
}

/// Send `method uri` as `viewer` in workspace A; the status and the body.
async fn call(f: &Fixture, viewer: Uuid, method: Method, uri: &str) -> (StatusCode, String) {
    call_scoped(f, viewer, "full", method, uri).await
}

/// [`call`] with a credential of `scope`, as an API token narrowed to it
/// carries.
async fn call_scoped(
    f: &Fixture,
    viewer: Uuid,
    scope: &str,
    method: Method,
    uri: &str,
) -> (StatusCode, String) {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: format!("{viewer}@example.com"),
        platform_role: "user".to_string(),
        scope: scope.to_string(),
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
    let req = http_test::TestRequest::default().method(method).uri(uri);
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// A timeline page as `viewer` sees it.
async fn timeline(f: &Fixture, viewer: Uuid, asset: i32, query: &str) -> Value {
    let uri = format!("/api/assets/{asset}/timeline?{query}");
    let (status, body) = call(f, viewer, Method::GET, &uri).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    serde_json::from_str(&body).expect("a JSON page")
}

fn keys(page: &Value) -> Vec<String> {
    page["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["key"].as_str().expect("key").to_string())
        .collect()
}

/// The workspace's repair type, added from the catalogue; its id.
async fn add_repair(f: &Fixture) -> i32 {
    let (status, body) = call(
        f,
        f.agent,
        Method::POST,
        "/api/assets/record-types/catalogue/repair",
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    in_ws(f, |c| asset_record_types::get_by_slug(c, "repair"))
        .expect("the repair type")
        .id
}

fn record(type_id: i32, asset: i32, occurred: WorkDate, summary: &str) -> NewRecord {
    NewRecord {
        asset_id: asset,
        record_type_id: type_id,
        summary: Some(summary.to_string()),
        notes: None,
        occurred,
        completed: None,
        outcome: None,
        vendor: Some("Camera Clinic".to_string()),
        performed_by_uuid: None,
        reference: None,
        cost: None,
        ticket_id: None,
        replacement_asset_id: None,
        snapshot: Some(Default::default()),
        attributes: json!({}),
        source: RecordSource::Manual,
        source_ref: None,
        import_job_id: None,
        rule_id: None,
        lifecycle_event_id: None,
        recorded_by: None,
    }
}

/// Write `records` in one transaction, so they share `recorded_at`; their
/// ids in order.
fn write_records(f: &Fixture, records: Vec<NewRecord>) -> Vec<i32> {
    in_ws(f, |c| {
        records
            .into_iter()
            .map(|r| {
                records_repo::create(c, r)
                    .map(|w| w.record.id)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e}").into()))
            })
            .collect()
    })
}

/// A lifecycle event at `occurred_at`, as an older entry would be.
fn status_event(f: &Fixture, asset: i32, to: &str, occurred_at: DateTime<Utc>) -> i32 {
    in_ws(f, |c| {
        diesel::insert_into(asset_lifecycle_events::table)
            .values((
                asset_lifecycle_events::asset_id.eq(asset),
                asset_lifecycle_events::from_status.eq(Some("in_stock")),
                asset_lifecycle_events::to_status.eq(to),
                asset_lifecycle_events::reason.eq(Some("Staff reason")),
                asset_lifecycle_events::metadata.eq(json!({ "vendor": "Staff vendor" })),
                asset_lifecycle_events::occurred_at.eq(occurred_at),
            ))
            .returning(asset_lifecycle_events::id)
            .get_result(c)
    })
}

/// A ticket created at `created_at`, linked to `asset`.
fn linked_ticket(
    f: &Fixture,
    asset: i32,
    title: &str,
    requester: Option<Uuid>,
    verification_state: Option<&str>,
    created_at: DateTime<Utc>,
) -> i32 {
    in_ws(f, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        let id: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: state.id,
                requester_uuid: requester,
                verification_state: verification_state.map(str::to_string),
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)?;
        diesel::update(tickets::table.find(id))
            .set(tickets::created_at.eq(created_at))
            .execute(c)?;
        diesel::insert_into(ticket_assets::table)
            .values((
                ticket_assets::ticket_id.eq(id),
                ticket_assets::asset_id.eq(asset),
            ))
            .execute(c)?;
        Ok(id)
    })
}

#[actix_web::test]
async fn a_date_only_entry_sorts_after_the_timed_entries_of_its_day() {
    let db = common::TestDb::new();
    // Ten hours ahead of UTC in May: 09:00 on the 3rd is the 2nd in UTC.
    let f = seed(&db, "Australia/Sydney");
    let repair = add_repair(&f).await;
    let ids = write_records(
        &f,
        vec![
            record(
                repair,
                f.asset,
                WorkDate::Day(day("2023-05-03")),
                "Date only",
            ),
            record(
                repair,
                f.asset,
                WorkDate::At(at("2023-05-03T09:00:00+10:00")),
                "Timed",
            ),
            record(
                repair,
                f.asset,
                WorkDate::Day(day("2023-05-02")),
                "Day before",
            ),
        ],
    );
    let (date_only, timed, day_before) = (ids[0], ids[1], ids[2]);
    let status = status_event(&f, f.asset, "in_repair", at("2023-05-03T08:00:00+10:00"));
    // Just past midnight on the 3rd, local time.
    let ticket = linked_ticket(
        &f,
        f.asset,
        "Lens fault",
        None,
        None,
        at("2023-05-03T00:00:01+10:00"),
    );

    let page = timeline(&f, f.agent, f.asset, "sources=record,status,ticket").await;
    assert_eq!(
        keys(&page),
        vec![
            format!("record:{timed}"),
            format!("status:{status}"),
            format!("ticket:{ticket}@{}", f.asset),
            format!("record:{date_only}"),
            format!("record:{day_before}"),
        ],
        "{page}"
    );
    let entries = page["entries"].as_array().expect("entries");
    assert_eq!(entries[0]["day"], "2023-05-03", "the timed record's day");
    assert_eq!(
        entries[3]["at"],
        Value::Null,
        "the date-only record has no time"
    );
    assert_eq!(
        entries[0]["data"]["summary"], "Timed",
        "a record as R3 gives it"
    );
    assert_eq!(entries[0]["data"]["record_type"], "repair");
    assert!(page["next_cursor"].is_null(), "{page}");

    // `from` and `to` are days in the workspace's timezone.
    let page = timeline(
        &f,
        f.agent,
        f.asset,
        "sources=record,status,ticket&from=2023-05-03&to=2023-05-03",
    )
    .await;
    assert_eq!(keys(&page).len(), 4, "{page}");
    let page = timeline(&f, f.agent, f.asset, "sources=record&to=2023-05-02").await;
    assert_eq!(keys(&page), vec![format!("record:{day_before}")], "{page}");

    // A type nobody has lists no records; an unknown source is refused.
    let page = timeline(&f, f.agent, f.asset, "sources=record&record_type=nope").await;
    assert_eq!(keys(&page), Vec::<String>::new(), "{page}");
    let (status, body) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{}/timeline?sources=history", f.asset),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{}/timeline?before=nonsense", f.asset),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[actix_web::test]
async fn pages_of_any_size_concatenate_to_the_whole_timeline() {
    let db = common::TestDb::new();
    let f = seed(&db, "Australia/Sydney");
    let repair = add_repair(&f).await;
    let borrower = f.ws.member_uuid;

    // Date-only records written together tie on everything but their ids.
    let mut batch: Vec<NewRecord> = (0..6)
        .map(|i| {
            record(
                repair,
                f.asset,
                WorkDate::Day(day("2023-05-03")),
                &format!("Tied {i}"),
            )
        })
        .collect();
    batch.push(record(
        repair,
        f.asset,
        WorkDate::At(at("2023-05-03T10:00:00+10:00")),
        "Timed",
    ));
    batch.push(record(
        repair,
        f.asset,
        WorkDate::At(at("2023-05-03T00:00:00+10:00")),
        "At midnight",
    ));
    batch.push(record(
        repair,
        f.asset,
        WorkDate::Day(day("2023-05-04")),
        "Next day",
    ));
    write_records(&f, batch);

    // A status change, a ticket and a loan's return at one moment tie on
    // the day, time and entry time: the source rank, then the id, decides.
    let moment = at("2023-05-03T10:00:00+10:00");
    status_event(&f, f.asset, "in_repair", moment);
    status_event(&f, f.asset, "in_service", moment);
    linked_ticket(&f, f.asset, "Same moment", None, None, moment);
    linked_ticket(&f, f.asset, "Same moment too", None, None, moment);
    in_ws(&f, |c| {
        let loan = backend::repository::asset_loans::issue(
            c,
            backend::repository::asset_loans::IssueLoan {
                asset_id: f.asset,
                borrower_user_uuid: borrower,
                loaned_at: Some(day("2023-05-01")),
                due_back: None,
                ticket_id: None,
                notes: None,
                actor_uuid: Some(f.agent),
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        backend::repository::asset_loans::return_loan(
            c,
            f.asset,
            loan.id,
            moment,
            Some(f.agent),
            None,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });

    assert_pages_concatenate(&f, f.asset, "", 17).await;
}

/// Pages of every size from one entry up, followed by their cursors, list
/// exactly the entries one page of all of them does, in the same order.
async fn assert_pages_concatenate(f: &Fixture, asset: i32, extra: &str, at_least: usize) {
    let whole = timeline(f, f.agent, asset, &format!("limit=200{extra}")).await;
    assert!(whole["next_cursor"].is_null(), "{whole}");
    let all = keys(&whole);
    assert!(all.len() >= at_least, "{whole}");
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(unique.len(), all.len(), "keys are unique: {all:?}");

    for limit in 1..=all.len() + 1 {
        let mut paged: Vec<String> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..=all.len() + 1 {
            let query = match &cursor {
                Some(c) => format!("limit={limit}{extra}&before={c}"),
                None => format!("limit={limit}{extra}"),
            };
            let page = timeline(f, f.agent, asset, &query).await;
            let got = keys(&page);
            assert!(got.len() <= limit, "limit {limit}: {page}");
            paged.extend(got);
            match page["next_cursor"].as_str() {
                Some(next) => cursor = Some(next.to_string()),
                None => break,
            }
        }
        assert_eq!(paged, all, "pages of {limit}");
    }
}

#[actix_web::test]
async fn pages_across_a_chain_of_replacements_concatenate_too() {
    let db = common::TestDb::new();
    let f = seed(&db, "Australia/Sydney");
    let repair = add_repair(&f).await;
    let older = new_asset(&f, "CAM-007");
    let newer = new_asset(&f, "CAM-022");
    let swap = |from: i32, to: i32, on: &str| {
        let mut r = record(repair, from, WorkDate::Day(day(on)), "Swapped");
        r.completed = Some(WorkDate::Day(day(on)));
        r.outcome = Some("replaced".to_string());
        r.replacement_asset_id = Some(to);
        r
    };
    // older was replaced by f.asset, and f.asset by newer, on the same day,
    // beside date-only work on each written together.
    write_records(
        &f,
        vec![
            swap(older, f.asset, "2023-05-03"),
            swap(f.asset, newer, "2023-05-03"),
            record(repair, older, WorkDate::Day(day("2023-05-03")), "Older"),
            record(repair, f.asset, WorkDate::Day(day("2023-05-03")), "Middle"),
            record(repair, newer, WorkDate::Day(day("2023-05-03")), "Newer"),
        ],
    );
    // One ticket on every asset of the chain, and another at its moment.
    let moment = at("2023-05-03T10:00:00+10:00");
    let shared = linked_ticket(&f, newer, "Shared", None, None, moment);
    in_ws(&f, |c| {
        diesel::insert_into(ticket_assets::table)
            .values(vec![
                (
                    ticket_assets::ticket_id.eq(shared),
                    ticket_assets::asset_id.eq(f.asset),
                ),
                (
                    ticket_assets::ticket_id.eq(shared),
                    ticket_assets::asset_id.eq(older),
                ),
            ])
            .execute(c)
    });
    linked_ticket(&f, older, "Same moment", None, None, moment);
    status_event(&f, older, "in_repair", moment);

    let chain = in_ws(&f, |c| predecessors(c, newer));
    assert_eq!(chain, vec![f.asset, older]);
    assert_pages_concatenate(&f, newer, "&include_predecessors=true", 18).await;
}

#[actix_web::test]
async fn tickets_are_the_ones_the_viewer_can_see() {
    let db = common::TestDb::new();
    let f = seed(&db, "UTC");
    let requester = f.ws.member_uuid;
    let when = at("2024-03-01T09:00:00Z");
    let theirs = linked_ticket(&f, f.asset, "Their ticket", Some(requester), None, when);
    let other = linked_ticket(&f, f.asset, "Someone else's", None, None, when);
    // A guest ticket waiting on email confirmation isn't in the workspace
    // yet: nobody sees it, staff included.
    let pending = linked_ticket(&f, f.asset, "Unconfirmed", None, Some("pending"), when);

    let ticket_keys = |page: &Value| -> HashSet<String> {
        keys(page)
            .into_iter()
            .filter(|k| k.starts_with("ticket:"))
            .collect()
    };
    let a = f.asset;
    let agent_page = timeline(&f, f.agent, a, "sources=ticket").await;
    assert_eq!(
        ticket_keys(&agent_page),
        HashSet::from([
            format!("ticket:{theirs}@{a}"),
            format!("ticket:{other}@{a}")
        ]),
        "{agent_page}"
    );
    let staff_ticket = &agent_page["entries"][0]["data"];
    assert!(staff_ticket["linked_at"].is_string(), "{staff_ticket}");
    assert!(staff_ticket["state"]["name"].is_string(), "{staff_ticket}");

    let requester_page = timeline(&f, requester, a, "sources=ticket").await;
    assert_eq!(
        ticket_keys(&requester_page),
        HashSet::from([format!("ticket:{theirs}@{a}")]),
        "{requester_page}"
    );
    let ticket = &requester_page["entries"][0]["data"];
    let mut fields: Vec<&str> = ticket
        .as_object()
        .expect("a ticket")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, vec!["id", "number", "state", "title"], "{ticket}");
    let _ = pending;
}

#[actix_web::test]
async fn a_token_that_cant_read_tickets_names_them_by_id() {
    let db = common::TestDb::new();
    let f = seed(&db, "UTC");
    let ticket = linked_ticket(
        &f,
        f.asset,
        "Lens fault",
        None,
        None,
        at("2024-03-01T09:00:00Z"),
    );
    let uri = format!("/api/assets/{}/timeline?sources=ticket", f.asset);
    let ticket_data = |body: &str| -> Value {
        let page: Value = serde_json::from_str(body).expect("a JSON page");
        page["entries"][0]["data"].clone()
    };

    // A session, and a token that can read tickets too, get the ticket.
    for scope in ["full", "assets:read tickets:read"] {
        let (status, body) = call_scoped(&f, f.agent, scope, Method::GET, &uri).await;
        assert_eq!(status, StatusCode::OK, "{scope}: {body}");
        let data = ticket_data(&body);
        assert_eq!(data["id"], json!(ticket), "{scope}: {data}");
        assert_eq!(data["title"], "Lens fault", "{scope}: {data}");
        assert!(data["number"].is_number(), "{scope}: {data}");
    }

    // A token for assets alone gets the id the lifecycle already shows.
    let (status, body) = call_scoped(&f, f.agent, "assets:read", Method::GET, &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ticket_data(&body), json!({ "id": ticket }), "{body}");
    for uri in [
        format!("/api/assets/{}/record-card", f.asset),
        "/api/assets/export?scope=timeline&format=json".to_string(),
    ] {
        let (status, body) = call_scoped(&f, f.agent, "assets:read", Method::GET, &uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert!(body.contains("ticket"), "{uri} lists the ticket: {body}");
        assert!(!body.contains("Lens fault"), "{uri}: {body}");
        let (_, body) = call_scoped(&f, f.agent, "full", Method::GET, &uri).await;
        assert!(body.contains("Lens fault"), "{uri}: {body}");
    }
}

#[actix_web::test]
async fn requesters_get_the_public_projection_and_no_records() {
    let db = common::TestDb::new();
    let f = seed(&db, "UTC");
    let repair = add_repair(&f).await;
    let requester = f.ws.member_uuid;
    let other = new_asset(&f, "CAM-022");
    let mut replaced = record(repair, f.asset, WorkDate::Day(day("2024-02-01")), "Swapped");
    replaced.completed = Some(WorkDate::Day(day("2024-02-01")));
    replaced.outcome = Some("replaced".to_string());
    replaced.replacement_asset_id = Some(other);
    write_records(&f, vec![replaced]);
    status_event(&f, f.asset, "in_repair", at("2024-01-05T09:00:00Z"));
    in_ws(&f, |c| {
        backend::repository::asset_lifecycle::transition(
            c,
            backend::repository::asset_lifecycle::TransitionInput {
                asset_id: f.asset,
                to_status: "disposed".to_string(),
                reason: Some("End of life".to_string()),
                ticket_id: None,
                metadata: json!({}),
                actor_uuid: Some(f.agent),
                disposal: Some(backend::repository::asset_lifecycle::DisposalInput {
                    sanitization_method: "destroy".to_string(),
                    data_bearing: true,
                    certificate_file_id: None,
                    itad_vendor: Some("Shred Vendor".to_string()),
                    notes: Some("Drive shredded".to_string()),
                }),
            },
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });

    let staff = timeline(&f, f.agent, f.asset, "").await;
    let sources: HashSet<String> = staff["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["source"].as_str().expect("source").to_string())
        .collect();
    for source in ["record", "replacement", "status", "marker"] {
        assert!(sources.contains(source), "staff see {source}: {staff}");
    }
    let disposed = staff["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|e| e["data"]["to_status"] == "disposed")
        .expect("the disposal");
    assert_eq!(disposed["data"]["disposal"]["itad_vendor"], "Shred Vendor");
    assert_eq!(disposed["actor_uuid"], json!(f.agent));

    let page = timeline(&f, requester, f.asset, "include_predecessors=true").await;
    let entries = page["entries"].as_array().expect("entries");
    assert!(!entries.is_empty(), "{page}");
    for entry in entries {
        let source = entry["source"].as_str().expect("source");
        assert!(
            ["status", "loan", "ticket", "marker"].contains(&source),
            "requesters see no {source}: {entry}"
        );
        assert!(entry["actor_uuid"].is_null(), "{entry}");
        assert!(entry["recorded_by"].is_null(), "{entry}");
        assert_eq!(entry["asset_id"], json!(f.asset), "{entry}");
    }
    let body = page.to_string();
    for secret in [
        "Staff reason",
        "Staff vendor",
        "End of life",
        "Shred Vendor",
        "Drive shredded",
        "Swapped",
        "Camera Clinic",
    ] {
        assert!(!body.contains(secret), "requesters don't see {secret:?}");
    }
    let disposed = entries
        .iter()
        .find(|e| e["data"]["to_status"] == "disposed")
        .expect("the disposal");
    assert_eq!(
        disposed["data"]["disposal"]["sanitization_method"],
        "destroy"
    );

    // Asking for records gets none rather than a refusal.
    let page = timeline(&f, requester, f.asset, "sources=record,replacement").await;
    assert_eq!(keys(&page), Vec::<String>::new(), "{page}");

    // A workspace that shows requesters no assets shows no timeline.
    in_ws(&f, |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                requester_asset_visibility: Some("none".to_string()),
                ..Default::default()
            },
        )
    });
    let (status, body) = call(
        &f,
        requester,
        Method::GET,
        &format!("/api/assets/{}/timeline", f.asset),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _) = call(&f, f.agent, Method::GET, "/api/assets/999999/timeline").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn a_chain_of_replacements_that_loops_still_ends() {
    let db = common::TestDb::new();
    let f = seed(&db, "UTC");
    let repair = add_repair(&f).await;
    let b = new_asset(&f, "CAM-022");
    let c = new_asset(&f, "CAM-031");
    let swap = |from: i32, to: i32, on: &str| {
        let mut r = record(
            repair,
            from,
            WorkDate::Day(day(on)),
            &format!("{from} to {to}"),
        );
        r.completed = Some(WorkDate::Day(day(on)));
        r.outcome = Some("replaced".to_string());
        r.replacement_asset_id = Some(to);
        r
    };
    // f.asset was replaced by b, b by c, and c (somehow) by f.asset again.
    write_records(
        &f,
        vec![
            swap(f.asset, b, "2022-01-01"),
            swap(b, c, "2023-01-01"),
            swap(c, f.asset, "2024-01-01"),
        ],
    );

    let chain = in_ws(&f, |conn| predecessors(conn, c));
    assert_eq!(chain, vec![b, f.asset], "nearest first, and it ends");

    let own = timeline(&f, f.agent, c, "sources=record,replacement").await;
    let assets: HashSet<i64> = own["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|e| e["asset_id"].as_i64())
        .collect();
    assert_eq!(assets, HashSet::from([i64::from(c)]), "{own}");
    // c holds its own record (which names f.asset) and the record that
    // names c as b's replacement.
    let kinds: HashSet<String> = own["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter(|e| e["source"] == "replacement")
        .map(|e| e["kind"].as_str().expect("kind").to_string())
        .collect();
    assert_eq!(
        kinds,
        HashSet::from(["replaces".to_string(), "replaced_by".to_string()])
    );

    let merged = timeline(
        &f,
        f.agent,
        c,
        "sources=record,replacement&include_predecessors=true",
    )
    .await;
    let assets: HashSet<i64> = merged["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|e| e["asset_id"].as_i64())
        .collect();
    assert_eq!(
        assets,
        HashSet::from([i64::from(c), i64::from(b), i64::from(f.asset)]),
        "{merged}"
    );
    let records = keys(&merged)
        .into_iter()
        .filter(|k| k.starts_with("record:"))
        .count();
    assert_eq!(records, 3, "each record once: {merged}");
}
