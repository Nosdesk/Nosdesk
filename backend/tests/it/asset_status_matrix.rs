//! The status rules on every path: REST moves answer with the rule's code,
//! a create starts in its own status with one event, nothing moves back to
//! on order, a loan can't race a disposal into a disposed asset on loan, a
//! loan out keeps its asset from disposed, a loan whose asset isn't on loan
//! closes and leaves the status, and `GET /assets/statuses` lists the
//! categories, with the moves for staff only. A rule step's handling is in
//! `rules_asset_actions`, a replacement's in `asset_replacement`, the
//! importer's in `imports_status`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{Bool, Integer};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{AssetStatus, Claims, NewAsset, UpdateSiteSettings};
use backend::repository::asset_lifecycle::{self, DisposalInput, TransitionError, TransitionInput};
use backend::repository::asset_loans::{self, IssueLoan, LoanError};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{assets as asset_repo, site_settings};
use backend::schema::{asset_lifecycle_events, asset_loans as loans_t, assets};
use backend::services::assets::status_rules::{Cell, RefusalCode, INTO_ON_ORDER};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:asset_status_matrix";

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
        created_by: None,
    }
}

fn asset(f: &Fixture, name: &str) -> i32 {
    in_ws(f, |c| asset_repo::create_device(c, new_asset(name))).id
}

fn status_of(f: &Fixture, id: i32) -> String {
    in_ws(f, |c| asset_repo::get_device_by_id(c, id)).status
}

/// Put `status` on the row as drift would leave it, past the rules.
fn force_status(f: &Fixture, id: i32, status: &str) {
    in_ws(f, |c| {
        diesel::update(assets::table.find(id))
            .set(assets::status.eq(status))
            .execute(c)
    });
}

fn events_of(f: &Fixture, id: i32) -> Vec<(Option<String>, String)> {
    in_ws(f, |c| {
        asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq(id))
            .order(asset_lifecycle_events::id.asc())
            .select((
                asset_lifecycle_events::from_status,
                asset_lifecycle_events::to_status,
            ))
            .load(c)
    })
}

fn loan_err(e: LoanError) -> diesel::result::Error {
    diesel::result::Error::QueryBuilderError(format!("{e:?}").into())
}

fn lend(f: &Fixture, id: i32) -> i32 {
    in_ws(f, |c| asset_loans::issue(c, issue(f, id)).map_err(loan_err)).id
}

fn issue(f: &Fixture, id: i32) -> IssueLoan {
    IssueLoan {
        asset_id: id,
        borrower_user_uuid: f.ws.member_uuid,
        loaned_at: None,
        due_back: None,
        ticket_id: None,
        notes: None,
        actor_uuid: None,
    }
}

fn to(id: i32, status: &str) -> TransitionInput {
    TransitionInput {
        asset_id: id,
        to_status: status.to_string(),
        to_status_id: None,
        reason: None,
        ticket_id: None,
        metadata: json!({}),
        actor_uuid: None,
        disposal: None,
        record: None,
        complete_record: None,
    }
}

fn dispose(id: i32) -> TransitionInput {
    TransitionInput {
        disposal: Some(DisposalInput {
            sanitization_method: "destroy".to_string(),
            data_bearing: true,
            certificate_file_id: None,
            itad_vendor: None,
            notes: None,
        }),
        ..to(id, "disposed")
    }
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

async fn transition(f: &Fixture, id: i32, body: Value) -> (StatusCode, Value) {
    let uri = format!("/api/assets/{id}/lifecycle");
    call(f, f.agent, Method::POST, &uri, Some(body)).await
}

/// Each refusal answers with its HTTP status, its code and the words it
/// always had; a move into on order after create is the one new refusal.
#[actix_web::test]
async fn rest_moves_answer_with_the_rules_code() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let plain = asset(&f, "Plain");
    let disposed = asset(&f, "Disposed");
    in_ws(&f, |c| {
        asset_lifecycle::transition(c, dispose(disposed))
            .map(|_| ())
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    });
    let loaned = asset(&f, "Loaned");
    lend(&f, loaned);

    let cases = [
        (
            plain,
            json!({ "to_status": "in_service" }),
            StatusCode::BAD_REQUEST,
            "same_status",
            "Asset is already in the requested status",
        ),
        (
            plain,
            json!({ "to_status": "disposed" }),
            StatusCode::BAD_REQUEST,
            "needs_disposal",
            "Disposing an asset requires a disposal record",
        ),
        (
            plain,
            json!({ "to_status": "on_loan" }),
            StatusCode::CONFLICT,
            "needs_active_loan",
            "On-loan status is set by issuing a loan; there is no active loan to restore",
        ),
        (
            disposed,
            json!({ "to_status": "in_stock" }),
            StatusCode::CONFLICT,
            "terminal",
            "Disposed assets can't change status; disposal is final",
        ),
        (
            loaned,
            json!({ "to_status": "lost" }),
            StatusCode::CONFLICT,
            "loan_owned",
            "Asset is out on loan; return the loan to change its status",
        ),
    ];
    for (id, body, status, code, message) in cases {
        let (got, resp) = transition(&f, id, body.clone()).await;
        assert_eq!(got, status, "{body}: {resp}");
        assert_eq!(resp["code"], code, "{body}: {resp}");
        assert_eq!(resp["error"], message, "{body}: {resp}");
        assert_eq!(
            resp["message_key"],
            format!("asset-move-refused-{}", code.replace('_', "-")),
            "{resp}"
        );
    }

    let (got, resp) = transition(&f, plain, json!({ "to_status": "on_order" })).await;
    if INTO_ON_ORDER == Cell::Allowed {
        assert_eq!(got, StatusCode::CREATED, "{resp}");
    } else {
        assert_eq!(got, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
        assert_eq!(resp["code"], "start_only", "{resp}");
        assert_eq!(status_of(&f, plain), "in_service");
    }
    let (got, resp) = transition(&f, plain, json!({ "to_status": "sideways" })).await;
    assert_eq!(got, StatusCode::BAD_REQUEST, "{resp}");
}

/// A create naming a status starts there with one event from no status; one
/// naming none, or in service, writes none, as before. On loan, disposed and
/// an unknown key are refused.
#[actix_web::test]
async fn a_create_starts_in_its_status_and_nothing_goes_back_to_on_order() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let create = |status: Option<&str>| {
        let mut body = json!({ "name": "Ordered laptop", "kind": "device" });
        if let Some(status) = status {
            body["status"] = json!(status);
        }
        body
    };

    let (got, created) = call(
        &f,
        f.agent,
        Method::POST,
        "/api/assets",
        Some(create(Some("on_order"))),
    )
    .await;
    assert_eq!(got, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().expect("id") as i32;
    assert_eq!(status_of(&f, id), "on_order");
    assert_eq!(events_of(&f, id), vec![(None, "on_order".to_string())]);
    let actor: Option<Uuid> = in_ws(&f, |c| {
        asset_lifecycle_events::table
            .filter(asset_lifecycle_events::asset_id.eq(id))
            .select(asset_lifecycle_events::actor_uuid)
            .first(c)
    });
    assert_eq!(actor, Some(f.agent));

    let (got, resp) = transition(&f, id, json!({ "to_status": "in_stock" })).await;
    assert_eq!(got, StatusCode::CREATED, "{resp}");
    let (got, resp) = transition(&f, id, json!({ "to_status": "on_order" })).await;
    if INTO_ON_ORDER == Cell::Allowed {
        assert_eq!(got, StatusCode::CREATED, "{resp}");
    } else {
        assert_eq!(got, StatusCode::UNPROCESSABLE_ENTITY, "{resp}");
        assert_eq!(resp["code"], "start_only", "{resp}");
        assert_eq!(status_of(&f, id), "in_stock");
    }

    // The rest of the body reads as it always did beside the status.
    let mut stocked = create(Some("in_stock"));
    stocked["quantity"] = json!(3);
    stocked["unit"] = json!("box");
    stocked["attributes"] = json!({});
    stocked["kind"] = json!("generic");
    let (got, created) = call(&f, f.agent, Method::POST, "/api/assets", Some(stocked)).await;
    assert_eq!(got, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().expect("id") as i32;
    let row = in_ws(&f, |c| asset_repo::get_device_by_id(c, id));
    assert_eq!(row.status, "in_stock");
    assert_eq!(row.quantity, Some(bigdecimal::BigDecimal::from(3)));
    assert_eq!(row.unit.as_deref(), Some("box"));

    for status in [None, Some("in_service"), Some("")] {
        let (got, created) = call(
            &f,
            f.agent,
            Method::POST,
            "/api/assets",
            Some(create(status)),
        )
        .await;
        assert_eq!(got, StatusCode::CREATED, "{status:?}: {created}");
        let id = created["id"].as_i64().expect("id") as i32;
        assert_eq!(status_of(&f, id), "in_service");
        assert!(events_of(&f, id).is_empty(), "{status:?}");
    }

    for (status, code, http) in [
        ("on_loan", "start_on_loan", StatusCode::BAD_REQUEST),
        ("disposed", "needs_disposal", StatusCode::BAD_REQUEST),
    ] {
        let (got, resp) = call(
            &f,
            f.agent,
            Method::POST,
            "/api/assets",
            Some(create(Some(status))),
        )
        .await;
        assert_eq!(got, http, "{status}: {resp}");
        assert_eq!(resp["code"], code, "{status}: {resp}");
    }
    let (got, resp) = call(
        &f,
        f.agent,
        Method::POST,
        "/api/assets",
        Some(create(Some("sideways"))),
    )
    .await;
    assert_eq!(got, StatusCode::BAD_REQUEST, "{resp}");
}

/// The backend pid of `conn`.
fn backend_pid(conn: &mut DbConnection) -> i32 {
    #[derive(QueryableByName)]
    struct Pid {
        #[diesel(sql_type = Integer)]
        pid: i32,
    }
    diesel::sql_query("SELECT pg_backend_pid() AS pid")
        .get_result::<Pid>(conn)
        .expect("pid")
        .pid
}

/// Wait until backend `pid` waits on a lock.
fn wait_until_blocked(conn: &mut DbConnection, pid: i32) {
    #[derive(QueryableByName)]
    struct Blocked {
        #[diesel(sql_type = Bool)]
        blocked: bool,
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let blocked = diesel::sql_query("SELECT cardinality(pg_blocking_pids($1)) > 0 AS blocked")
            .bind::<Integer, _>(pid)
            .get_result::<Blocked>(conn)
            .expect("blocking pids")
            .blocked;
        if blocked {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "backend {pid} never waited on the asset"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run `op` in the workspace on its own thread, sending its backend pid
/// first.
fn on_thread<T: Send + 'static>(
    f: &Fixture,
    op: impl FnOnce(&mut DbConnection) -> T + Send + 'static,
) -> (std::thread::JoinHandle<T>, i32) {
    let pool = f.pool.clone();
    let ws = f.ws.workspace_id;
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut out = None;
        run_in_workspace(&pool, REF, ws, |c| {
            tx.send(backend_pid(c)).expect("send pid");
            out = Some(op(c));
            Ok(())
        })
        .expect("run on the thread");
        out.expect("ran")
    });
    let pid = rx.recv_timeout(Duration::from_secs(20)).expect("pid");
    (handle, pid)
}

/// A loan issued while a disposal holds the asset waits for it, then sees a
/// disposed asset and refuses: the asset never ends disposed and on loan.
/// The other way round, a disposal waits for the loan and is refused.
#[actix_web::test]
async fn a_loan_and_a_disposal_cant_both_land() {
    let db = common::TestDb::new();
    let f = seed(&db);

    // The disposal holds the asset; the loan waits, then sees it disposed.
    let id = asset(&f, "Racing loaner");
    let member = f.ws.member_uuid;
    let mut lend_thread = None;
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        asset_lifecycle::lock(c, id)?;
        let (handle, pid) = on_thread(&f, move |c| {
            asset_loans::issue(
                c,
                IssueLoan {
                    asset_id: id,
                    borrower_user_uuid: member,
                    loaned_at: None,
                    due_back: None,
                    ticket_id: None,
                    notes: None,
                    actor_uuid: None,
                },
            )
            .map(|loan| loan.id)
        });
        lend_thread = Some(handle);
        asset_lifecycle::transition(c, dispose(id))
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        wait_until_blocked(&mut f.pool.get().expect("conn"), pid);
        Ok(())
    })
    .expect("dispose under the lock");
    let lent = lend_thread.expect("spawned").join().expect("lend thread");
    assert!(
        matches!(&lent, Err(LoanError::NotLoanable(s)) if s == "disposed"),
        "{lent:?}"
    );
    assert_eq!(status_of(&f, id), "disposed");
    let loans: i64 = in_ws(&f, |c| {
        loans_t::table
            .filter(loans_t::asset_id.eq(id))
            .count()
            .get_result(c)
    });
    assert_eq!(loans, 0, "no loan on a disposed asset");

    // The loan holds the asset; the disposal waits, then finds it on loan.
    let id = asset(&f, "Racing disposal");
    let mut dispose_thread = None;
    run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        asset_loans::issue(c, issue(&f, id)).map_err(loan_err)?;
        let (handle, pid) = on_thread(&f, move |c| {
            asset_lifecycle::transition(c, dispose(id)).map(|_| ())
        });
        dispose_thread = Some(handle);
        wait_until_blocked(&mut f.pool.get().expect("conn"), pid);
        Ok(())
    })
    .expect("lend under the lock");
    let refused = dispose_thread
        .expect("spawned")
        .join()
        .expect("disposal thread");
    assert!(
        matches!(&refused, Err(TransitionError::Refused(r)) if r.code == RefusalCode::LoanOwned),
        "{refused:?}"
    );
    assert_eq!(status_of(&f, id), "on_loan");
}

/// A loan whose asset isn't on loan any more closes and leaves the status,
/// by the ledger and over REST: drifted elsewhere, drifted to the very status
/// it recorded, recorded a status outside the nine, or disposed before the
/// rules stopped that. The return shows on the asset's timeline. The purge
/// of a borrower closes such loans the same way.
#[actix_web::test]
async fn a_loan_closes_whatever_its_asset_drifted_to() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let close = |id: i32, loan: i32| {
        run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
            asset_loans::return_loan(c, id, loan, chrono::Utc::now(), None, None).map_err(loan_err)
        })
        .expect("return the loan")
    };
    let returned = |loan: i32| -> bool {
        in_ws(&f, |c| {
            loans_t::table
                .find(loan)
                .select(loans_t::returned_at)
                .first::<Option<chrono::DateTime<chrono::Utc>>>(c)
        })
        .is_some()
    };

    // Drifted elsewhere: the ledger closes it, the status stays.
    let id = asset(&f, "Drifted");
    let loan = lend(&f, id);
    force_status(&f, id, "in_repair");
    let events_before = events_of(&f, id).len();
    close(id, loan);
    assert!(returned(loan));
    assert_eq!(status_of(&f, id), "in_repair");
    assert_eq!(events_of(&f, id).len(), events_before, "no status event");
    let (got, timeline) = call(
        &f,
        f.agent,
        Method::GET,
        &format!("/api/assets/{id}/timeline"),
        None,
    )
    .await;
    assert_eq!(got, StatusCode::OK, "{timeline}");
    assert!(
        timeline["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|e| e["source"] == "loan" && e["kind"] == "returned"),
        "{timeline}"
    );

    // Drifted to the very status it recorded: over REST, closed.
    let id = asset(&f, "Back already");
    let loan = lend(&f, id);
    force_status(&f, id, "in_service");
    let (got, resp) = call(
        &f,
        f.agent,
        Method::POST,
        &format!("/api/assets/{id}/loans/{loan}/return"),
        Some(json!({})),
    )
    .await;
    assert!(got.is_success(), "{got}: {resp}");
    assert!(returned(loan));
    assert_eq!(status_of(&f, id), "in_service");

    // A recorded status outside the nine: closed, the asset left on loan
    // with no loan, which a move by hand then puts right.
    let id = asset(&f, "Odd record");
    let loan = lend(&f, id);
    in_ws(&f, |c| {
        diesel::update(loans_t::table.find(loan))
            .set(loans_t::status_before.eq("broken"))
            .execute(c)
    });
    close(id, loan);
    assert!(returned(loan));
    assert_eq!(status_of(&f, id), "on_loan");
    let (got, resp) = transition(&f, id, json!({ "to_status": "in_stock" })).await;
    assert_eq!(got, StatusCode::CREATED, "{resp}");

    // Disposed with a loan out, as old data may have it: the loan closes,
    // so the person can be offboarded; the asset stays disposed.
    let id = asset(&f, "Disposed holder");
    let loan = lend(&f, id);
    force_status(&f, id, "disposed");
    let mut conn = f.pool.get().expect("conn");
    backend::sync::session::with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        asset_loans::return_loans_of_purged_borrower(c, &f.ws.member_uuid).map_err(loan_err)
    })
    .expect("purge the borrower's loans");
    assert!(returned(loan));
    assert_eq!(status_of(&f, id), "disposed");
}

/// A loan out keeps its asset from being disposed, whatever status the asset
/// drifted to, so the rules can't make a disposed asset that holds a loan.
#[actix_web::test]
async fn an_asset_with_a_loan_out_cant_be_disposed() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let id = asset(&f, "Lent and drifted");
    lend(&f, id);
    force_status(&f, id, "in_stock");
    let (got, resp) = transition(
        &f,
        id,
        json!({
            "to_status": "disposed",
            "disposal": { "sanitization_method": "destroy", "data_bearing": true },
        }),
    )
    .await;
    assert_eq!(got, StatusCode::CONFLICT, "{resp}");
    assert_eq!(resp["code"], "loan_owned", "{resp}");
    assert_eq!(status_of(&f, id), "in_stock");
}

/// The categories in picker order with their flags; staff also get each
/// category's moves and what they need, a requester none, and a workspace
/// that hides assets from requesters answers them 404.
#[actix_web::test]
async fn the_statuses_endpoint_lists_categories_and_moves_for_staff() {
    let db = common::TestDb::new();
    let f = seed(&db);

    let (got, body) = call(&f, f.agent, Method::GET, "/api/assets/statuses", None).await;
    assert_eq!(got, StatusCode::OK, "{body}");
    let categories = body["categories"].as_array().expect("categories");
    let keys: Vec<&str> = categories
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        AssetStatus::ALL.map(AssetStatus::as_str).to_vec(),
        "{body}"
    );
    assert_eq!(body["statuses"], json!([]));
    let category = |key: &str| {
        categories
            .iter()
            .find(|c| c["key"] == key)
            .expect("category")
    };
    let in_service = category("in_service");
    assert_eq!(in_service["default"], true);
    assert_eq!(in_service["start"], true);
    assert_eq!(in_service["loanable"], true);
    let targets = in_service["targets"].as_array().expect("targets");
    let target = |key: &str| targets.iter().find(|t| t["key"] == key);
    assert_eq!(target("in_stock").expect("in_stock")["needs"], json!([]));
    assert_eq!(
        target("disposed").expect("disposed")["needs"],
        json!(["disposal", "no_active_loan"])
    );
    assert_eq!(
        target("on_loan").expect("on_loan")["needs"],
        json!(["active_loan"])
    );
    assert_eq!(target("on_order").is_some(), INTO_ON_ORDER == Cell::Allowed);
    assert_eq!(category("disposed")["targets"], json!([]));
    assert_eq!(category("disposed")["terminal"], true);
    assert_eq!(category("on_loan")["start"], false);

    let (got, body) = call(
        &f,
        f.ws.member_uuid,
        Method::GET,
        "/api/assets/statuses",
        None,
    )
    .await;
    assert_eq!(got, StatusCode::OK, "{body}");
    for category in body["categories"].as_array().expect("categories") {
        assert!(category.get("targets").is_none(), "{category}");
    }

    in_ws(&f, |c| {
        site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                requester_asset_visibility: Some("none".to_string()),
                ..Default::default()
            },
        )
    });
    let (got, _) = call(
        &f,
        f.ws.member_uuid,
        Method::GET,
        "/api/assets/statuses",
        None,
    )
    .await;
    assert_eq!(got, StatusCode::NOT_FOUND);
}
