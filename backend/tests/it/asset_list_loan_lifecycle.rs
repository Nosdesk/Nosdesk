//! Each row of the paginated asset list carries its open loan
//! (`current_loan`) and its newest lifecycle event (`latest_lifecycle`), and
//! the list sorts by borrower and by last status change.
//!
//! A returned loan is not current. A loan whose borrower account is gone
//! stays current with no borrower. Events sharing a timestamp resolve by id.
//! A requester keeps who has the asset and when, but not the reason for the
//! last status change or who made it.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use chrono::{NaiveDate, TimeZone, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAsset};
use backend::repository::{asset_loans, assets as asset_repo};
use backend::schema::{asset_lifecycle_events as events, users};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_list_loan_lifecycle";

fn new_asset(name: &str) -> NewAsset {
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
        kind: "device".to_string(),
        attributes: json!({}),
        quantity: None,
        unit: None,
        external_sync_source: None,
        low_stock_threshold: None,
    }
}

fn loan_err(e: asset_loans::LoanError) -> diesel::result::Error {
    diesel::result::Error::QueryBuilderError(format!("{e:?}").into())
}

fn at(month: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, month, 1, 9, 0, 0)
        .single()
        .expect("time")
}

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    persona_borrower: Uuid,
    plain_borrower: Uuid,
}

/// Six assets, named so name order differs from both derived orders:
/// - `a open`: on loan to Aaron, who goes by "Zoe Persona" here; due back
///   2026-12-01. Last change May.
/// - `b returned`: loaned and returned. Both events in March, so the return
///   (the higher id) is the newest.
/// - `c open`: on loan to Mia. Last change April.
/// - `d gone`: on loan to an account since deleted. Last change February.
/// - `e repaired`: two events in June at the same instant; the second, with a
///   reason and an actor, is the newest.
/// - `f quiet`: no loan, no events.
fn seed(db: &common::TestDb) -> Fixture {
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let (persona_borrower, plain_borrower, gone_borrower) = {
        let mut c = pool.get().expect("conn");
        (
            common::insert_plain_user(&mut c, "Aaron Account"),
            common::insert_plain_user(&mut c, "Mia"),
            common::insert_plain_user(&mut c, "Gone Borrower"),
        )
    };
    let admin = ws.admin_uuid;

    let issue = |c: &mut backend::db::DbConnection,
                 asset_id: i32,
                 borrower: Uuid,
                 due: Option<NaiveDate>| {
        asset_loans::issue(
            c,
            asset_loans::IssueLoan {
                asset_id,
                borrower_user_uuid: borrower,
                loaned_at: None,
                due_back: due,
                ticket_id: None,
                notes: Some("staff note".to_string()),
                actor_uuid: Some(admin),
            },
        )
        .map_err(loan_err)
    };
    let stamp = |c: &mut backend::db::DbConnection, asset_id: i32, when: chrono::DateTime<Utc>| {
        diesel::update(events::table.filter(events::asset_id.eq(asset_id)))
            .set(events::occurred_at.eq(when))
            .execute(c)
    };

    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO user_profiles (user_uuid, display_name) VALUES ($1, 'Zoe Persona')",
        )
        .bind::<diesel::sql_types::Uuid, _>(persona_borrower)
        .execute(c)?;

        let a = asset_repo::create_device(c, new_asset("a open"))?.id;
        issue(c, a, persona_borrower, NaiveDate::from_ymd_opt(2026, 12, 1))?;
        stamp(c, a, at(5))?;

        let b = asset_repo::create_device(c, new_asset("b returned"))?.id;
        let loan = issue(c, b, plain_borrower, None)?;
        asset_loans::return_loan(c, b, loan.id, Utc::now(), Some(admin), None).map_err(loan_err)?;
        stamp(c, b, at(3))?;

        let c_id = asset_repo::create_device(c, new_asset("c open"))?.id;
        issue(c, c_id, plain_borrower, None)?;
        stamp(c, c_id, at(4))?;

        let d = asset_repo::create_device(c, new_asset("d gone"))?.id;
        issue(c, d, gone_borrower, None)?;
        stamp(c, d, at(2))?;

        let e = asset_repo::create_device(c, new_asset("e repaired"))?.id;
        for (from, to, reason, actor) in [
            ("in_service", "in_repair", None, None),
            (
                "in_repair",
                "in_service",
                Some("Screen replaced"),
                Some(admin),
            ),
        ] {
            diesel::insert_into(events::table)
                .values((
                    events::asset_id.eq(e),
                    events::from_status.eq(from),
                    events::to_status.eq(to),
                    events::reason.eq(reason),
                    events::actor_uuid.eq(actor),
                    events::metadata.eq(json!({})),
                    events::occurred_at.eq(at(6)),
                ))
                .execute(c)?;
        }

        asset_repo::create_device(c, new_asset("f quiet"))?;
        Ok(())
    })
    .expect("seed assets");

    // The borrower's account goes; the open loan stays with no borrower.
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        diesel::delete(users::table.filter(users::uuid.eq(gone_borrower))).execute(c)
    })
    .expect("delete the borrower account");

    Fixture {
        pool,
        ws,
        persona_borrower,
        plain_borrower,
    }
}

fn spawn(f: &Fixture, viewer: Uuid) -> actix_test::TestServer {
    let pool = f.pool.clone();
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (Utc::now().timestamp() + 3600) as usize,
        iat: Utc::now().timestamp() as usize,
    };
    let ws = WorkspaceContext {
        workspace_id: f.ws.workspace_id,
        workspace_uuid: f.ws.workspace_uuid,
        slug: f.ws.slug.clone(),
        name: f.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let workspace_id = f.ws.workspace_id;
    actix_test::start(move || {
        let claims = claims.clone();
        let ws = ws.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(viewer, Some(corr)).with_workspace(workspace_id);
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

async fn page(srv: &actix_test::TestServer, query: &str) -> Vec<Value> {
    let url = srv.url(&format!("/assets/paginated?pageSize=100&{query}"));
    let mut resp = awc::Client::new().get(url).send().await.expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    body["data"].as_array().expect("data").clone()
}

fn by_name<'a>(rows: &'a [Value], name: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("row {name}"))
}

fn names(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|r| r["name"].as_str().expect("name").to_string())
        .collect()
}

#[actix_web::test]
async fn staff_rows_carry_the_open_loan_and_newest_event() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    let srv = spawn(&f, f.ws.admin_uuid);
    let rows = page(&srv, "sortField=name&sortDirection=asc").await;

    let a = by_name(&rows, "a open");
    assert_eq!(
        a["current_loan"]["borrower_user_uuid"],
        f.persona_borrower.to_string()
    );
    assert_eq!(
        a["current_loan"]["borrower_name"], "Zoe Persona",
        "the workspace display name wins over the account name"
    );
    assert_eq!(a["current_loan"]["due_at"], "2026-12-01");
    assert!(a["current_loan"]["issued_at"].is_string());
    assert_eq!(a["latest_lifecycle"]["to_status"], "on_loan");

    let b = by_name(&rows, "b returned");
    assert!(
        b["current_loan"].is_null(),
        "a returned loan is not current"
    );
    assert_eq!(
        b["latest_lifecycle"]["from_status"], "on_loan",
        "of two events at one instant, the later id is the newest"
    );
    assert_eq!(b["latest_lifecycle"]["to_status"], "in_service");

    let c = by_name(&rows, "c open");
    assert_eq!(
        c["current_loan"]["borrower_user_uuid"],
        f.plain_borrower.to_string()
    );
    assert_eq!(c["current_loan"]["borrower_name"], "Mia");
    assert!(c["current_loan"]["due_at"].is_null());

    let d = by_name(&rows, "d gone");
    assert!(
        d["current_loan"].is_object(),
        "the loan outlives the borrower"
    );
    assert!(d["current_loan"]["borrower_user_uuid"].is_null());
    assert!(d["current_loan"]["borrower_name"].is_null());

    let e = by_name(&rows, "e repaired");
    assert!(e["current_loan"].is_null());
    let event = &e["latest_lifecycle"];
    assert_eq!(event["from_status"], "in_repair");
    assert_eq!(event["to_status"], "in_service");
    assert_eq!(event["reason"], "Screen replaced");
    assert_eq!(event["actor_uuid"], f.ws.admin_uuid.to_string());
    assert_eq!(event["actor_name"], "a Admin");
    let occurred: chrono::DateTime<Utc> = event["occurred_at"]
        .as_str()
        .expect("occurred_at")
        .parse()
        .expect("RFC 3339");
    assert_eq!(occurred, at(6));

    let quiet = by_name(&rows, "f quiet");
    assert!(quiet["current_loan"].is_null());
    assert!(quiet["latest_lifecycle"].is_null());
}

#[actix_web::test]
async fn the_list_sorts_by_borrower_and_last_status_change() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    let srv = spawn(&f, f.ws.admin_uuid);

    assert_eq!(
        names(&page(&srv, "sortField=borrower&sortDirection=asc").await),
        [
            "c open",
            "a open",
            "b returned",
            "d gone",
            "e repaired",
            "f quiet"
        ],
        "by display name, rows without a named borrower last"
    );
    assert_eq!(
        names(&page(&srv, "sortField=borrower&sortDirection=desc").await),
        [
            "a open",
            "c open",
            "b returned",
            "d gone",
            "e repaired",
            "f quiet"
        ],
    );
    assert_eq!(
        names(&page(&srv, "sortField=last_status_change&sortDirection=desc").await),
        [
            "e repaired",
            "a open",
            "c open",
            "b returned",
            "d gone",
            "f quiet"
        ],
        "newest change first, assets without events last"
    );
    assert_eq!(
        names(&page(&srv, "sortField=last_status_change&sortDirection=asc").await),
        [
            "d gone",
            "b returned",
            "c open",
            "a open",
            "e repaired",
            "f quiet"
        ],
    );
}

#[actix_web::test]
async fn a_requester_sees_who_has_it_but_not_why_it_changed() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let f = seed(&db);
    let srv = spawn(&f, f.ws.member_uuid);
    let rows = page(&srv, "sortField=name&sortDirection=asc").await;
    assert_eq!(rows.len(), 6, "the workspace shows requesters every asset");

    let a = by_name(&rows, "a open");
    assert_eq!(
        a["current_loan"],
        json!({
            "borrower_user_uuid": f.persona_borrower.to_string(),
            "borrower_name": "Zoe Persona",
            "issued_at": a["current_loan"]["issued_at"],
            "due_at": "2026-12-01",
        }),
        "who has the asset and until when, as the loan ledger shows a requester"
    );

    let event = by_name(&rows, "e repaired")["latest_lifecycle"]
        .as_object()
        .expect("event")
        .clone();
    let mut keys: Vec<&str> = event.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["from_status", "occurred_at", "to_status"],
        "no reason and no actor for a requester"
    );
}
