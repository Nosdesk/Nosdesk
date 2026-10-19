//! The client cache fence. Replacing a workspace's data (an archive import)
//! calls `repository::workspaces::bump_data_epoch`, which gives the workspace a
//! new `data_epoch` and a `resync_floor` above every cursor issued before.
//! `/api/sync/schema` and every delta carry the epoch, so a client can tell
//! its cache is from before; a delta from an older cursor also returns
//! `resync_required`. A bootstrap after the bump hands out a cursor that is
//! not stale, and the other workspace is untouched.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::repository::workspaces::{bump_data_epoch, DataFence};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:workspace_data_epoch";

/// GET `uri` as the admin of `ws` and return the body as JSON or NDJSON lines.
async fn get(pool: &TestPool, ws: &WorkspaceSeed, uri: &str) -> String {
    let viewer = ws.admin_uuid;
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
    };
    let workspace = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(ws.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::sync::config)),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    body
}

async fn get_json(pool: &TestPool, ws: &WorkspaceSeed, uri: &str) -> Value {
    let body = get(pool, ws, uri).await;
    serde_json::from_str(&body).unwrap_or_else(|_| panic!("GET {uri} json: {body}"))
}

/// The epoch `/api/sync/schema` reports for `ws`.
async fn schema_epoch(pool: &TestPool, ws: &WorkspaceSeed) -> Value {
    get_json(pool, ws, "/api/sync/schema").await["workspace_epoch"].clone()
}

/// A delta for `ws`'s workspace group from cursor `from` (and `from_xid8`
/// when given).
async fn delta(pool: &TestPool, ws: &WorkspaceSeed, from: i64, from_xid8: Option<i64>) -> Value {
    let mut uri = format!(
        "/api/sync/delta?from={from}&groups=workspace:{}",
        ws.workspace_id
    );
    if let Some(x) = from_xid8 {
        uri.push_str(&format!("&from_xid8={x}"));
    }
    get_json(pool, ws, &uri).await
}

/// The cursor a bootstrap of `ws`'s workspace group ends on.
async fn bootstrap_cursor(pool: &TestPool, ws: &WorkspaceSeed) -> (i64, i64) {
    let uri = format!(
        "/api/sync/bootstrap?groups=workspace:{}&schema=test",
        ws.workspace_id
    );
    let body = get(pool, ws, &uri).await;
    let end = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("bootstrap line"))
        .find_map(|l| l.get("__end__").cloned())
        .expect("an end line");
    (
        end["last_sync_id"].as_i64().expect("last_sync_id"),
        end["last_xid8"].as_i64().expect("last_xid8"),
    )
}

/// The newest action in `ws`'s feed: the cursor of a client that has seen
/// everything.
fn caught_up_cursor(pool: &TestPool, ws: &WorkspaceSeed) -> i64 {
    use backend::schema::sync_actions;
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        sync_actions::table
            .select(diesel::dsl::max(sync_actions::sync_id))
            .first::<Option<i64>>(c)
    })
    .expect("newest action")
    .unwrap_or(0)
}

/// Replace `ws`'s data, as far as the fence is concerned.
fn bump(pool: &TestPool, ws: &WorkspaceSeed) -> DataFence {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system(REF).with_workspace(ws.workspace_id);
    // elevated-in-workspace: nosdesk_app can only read `workspaces`; the import that bumps the epoch runs as the owner role.
    with_actor_bypass_context(&mut conn, &actor, |c| bump_data_epoch(c, ws.workspace_id))
        .expect("bump the data epoch")
}

#[actix_web::test]
async fn a_cursor_from_before_the_data_was_replaced_must_resync() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a, seeded.b);

    // Never replaced: no epoch, and a caught-up cursor is current.
    assert_eq!(schema_epoch(&pool, &a).await, Value::Null);
    let cursor = caught_up_cursor(&pool, &a);
    let before = delta(&pool, &a, cursor, None).await;
    assert_eq!(before["resync_required"], false, "{before}");
    assert_eq!(before["workspace_epoch"], Value::Null, "{before}");

    let fence = bump(&pool, &a);
    let epoch = fence.epoch.expect("an epoch").to_string();
    let floor = fence.resync_floor.expect("a floor");
    assert!(
        floor > cursor,
        "the floor {floor} sits above the old cursor {cursor}"
    );

    // The same caught-up cursor is now stale, and the epoch says why.
    assert_eq!(schema_epoch(&pool, &a).await, Value::String(epoch.clone()));
    let after = delta(&pool, &a, cursor, None).await;
    assert_eq!(after["resync_required"], true, "{after}");
    assert_eq!(
        after["workspace_epoch"],
        Value::String(epoch.clone()),
        "{after}"
    );

    // A client with no cursor bootstraps anyway; it isn't told to resync.
    let cursorless = delta(&pool, &a, 0, None).await;
    assert_eq!(cursorless["resync_required"], false, "{cursorless}");

    // A second replacement moves the epoch again.
    let again = bump(&pool, &a);
    assert_ne!(again.epoch, fence.epoch);
    assert!(again.resync_floor.expect("a floor") > floor);

    // Workspace b was never replaced: no epoch, and its caught-up cursor,
    // though below a's floor, is current.
    assert_eq!(schema_epoch(&pool, &b).await, Value::Null);
    let b_cursor = caught_up_cursor(&pool, &b);
    assert!(b_cursor < floor);
    let other = delta(&pool, &b, b_cursor, None).await;
    assert_eq!(other["resync_required"], false, "{other}");
    assert_eq!(other["workspace_epoch"], Value::Null, "{other}");
}

#[actix_web::test]
async fn a_bootstrap_after_the_data_was_replaced_hands_out_a_current_cursor() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = seeded.a;

    let floor = bump(&pool, &a).resync_floor.expect("a floor");
    // Nothing has happened in the workspace since, so its newest action is
    // below the floor; the bootstrap cursor must not be.
    assert!(caught_up_cursor(&pool, &a) < floor);
    let (last_sync_id, last_xid8) = bootstrap_cursor(&pool, &a).await;
    assert!(
        last_sync_id >= floor,
        "bootstrap cursor {last_sync_id} is below the floor {floor}"
    );

    // Otherwise the client would wipe and bootstrap again on every poll.
    let next = delta(&pool, &a, last_sync_id, Some(last_xid8)).await;
    assert_eq!(next["resync_required"], false, "{next}");
    let legacy = delta(&pool, &a, last_sync_id, None).await;
    assert_eq!(legacy["resync_required"], false, "{legacy}");
}
