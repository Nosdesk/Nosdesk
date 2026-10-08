//! An archived workflow state can be listed and brought back.
//!
//! Archiving keeps the row so tickets in it keep their state. An admin can
//! list the archived states and restore one; it returns at the end of its
//! category, since a state added since may hold its old position.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, WorkflowStateCategory};
use backend::repository::workflow_states as states;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:workflow_state_restore";

fn claims_for(user: Uuid) -> Claims {
    Claims {
        sub: user.to_string(),
        name: "Someone".to_string(),
        email: "someone@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// The workflow state endpoints, called as `$user` in `$ws`.
macro_rules! app_as {
    ($pool:expr, $ws:expr, $user:expr $(,)?) => {{
        let pool: &TestPool = $pool;
        let ws: &WorkspaceSeed = $ws;
        let user: Uuid = $user;
        let claims = claims_for(user);
        let workspace = WorkspaceContext {
            workspace_id: ws.workspace_id,
            workspace_uuid: ws.workspace_uuid,
            slug: ws.slug.clone(),
            name: "Workspace".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), pool).expect("init search"));
        std::mem::forget(search_dir);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(search))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::workflow_states::config)),
        )
        .await
    }};
}

/// Status and JSON body of `$req` sent to `$app`.
macro_rules! send {
    ($app:expr, $req:expr $(,)?) => {{
        let resp = http_test::call_service($app, $req.to_request()).await;
        let status: StatusCode = resp.status();
        let body = http_test::read_body(resp).await;
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, json)
    }};
}

fn ids(body: &Value) -> Vec<i64> {
    body["states"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["id"].as_i64()).collect())
        .unwrap_or_default()
}

#[actix_web::test]
async fn an_archived_state_is_listed_and_restored_at_the_end_of_its_category() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = &seeded.a;
    let app = app_as!(&pool, a, a.admin_uuid);

    let (status, waiting) = send!(
        &app,
        http_test::TestRequest::post()
            .uri("/api/admin/workflow-states")
            .set_json(
                json!({ "name": "Waiting on vendor", "category": "active", "color": "blue" })
            ),
    );
    assert_eq!(status, StatusCode::CREATED);
    let waiting_id = waiting["id"].as_i64().expect("id");
    let (status, _) = send!(
        &app,
        http_test::TestRequest::delete().uri(&format!("/api/admin/workflow-states/{waiting_id}")),
    );
    assert_eq!(status, StatusCode::OK, "archived");
    // A state added since takes the archived one's position.
    let (status, later) = send!(
        &app,
        http_test::TestRequest::post()
            .uri("/api/admin/workflow-states")
            .set_json(
                json!({ "name": "Waiting on customer", "category": "active", "color": "blue" })
            ),
    );
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(later["position"], waiting["position"]);

    let (status, archived) = send!(
        &app,
        http_test::TestRequest::get().uri("/api/admin/workflow-states/archived"),
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&archived), vec![waiting_id]);

    let (status, restored) = send!(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workflow-states/{waiting_id}/restore")),
    );
    assert_eq!(status, StatusCode::OK, "restored: {restored}");
    assert_eq!(restored["archived_at"], Value::Null);
    assert!(
        restored["position"].as_i64() > later["position"].as_i64(),
        "after the state added since: {restored}"
    );

    let (_, active) = send!(
        &app,
        http_test::TestRequest::get().uri("/api/workflow-states")
    );
    assert!(ids(&active).contains(&waiting_id), "listed again");
    let (_, archived) = send!(
        &app,
        http_test::TestRequest::get().uri("/api/admin/workflow-states/archived"),
    );
    assert!(ids(&archived).is_empty());

    // The restore reached everyone through sync.
    let events: i64 = run_in_workspace(&pool, REF, a.workspace_id, |c| {
        use backend::schema::sync_actions;
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(waiting_id.to_string()))
            .filter(sync_actions::event_type.eq("workflow_state.restored"))
            .count()
            .get_result(c)
    })
    .expect("events");
    assert_eq!(events, 1);

    // Restoring a state that isn't archived changes nothing.
    let (status, again) = send!(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workflow-states/{waiting_id}/restore")),
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["position"], restored["position"]);
}

#[actix_web::test]
async fn only_an_admin_of_the_workspace_restores() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);
    let archived_in = |ws: &WorkspaceSeed| {
        run_in_workspace(&pool, REF, ws.workspace_id, |c| {
            let state = states::first_in_category(c, WorkflowStateCategory::InReview)?;
            states::archive(c, state.id)
        })
        .expect("archive")
        .id
    };
    let a_state = archived_in(a);
    let b_state = archived_in(b);

    let member = app_as!(&pool, a, a.member_uuid);
    let (status, _) = send!(
        &member,
        http_test::TestRequest::get().uri("/api/admin/workflow-states/archived"),
    );
    assert_eq!(status, StatusCode::FORBIDDEN, "a member can't list them");
    let (status, _) = send!(
        &member,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workflow-states/{a_state}/restore")),
    );
    assert_eq!(status, StatusCode::FORBIDDEN, "nor restore one");

    let admin = app_as!(&pool, a, a.admin_uuid);
    let (status, archived) = send!(
        &admin,
        http_test::TestRequest::get().uri("/api/admin/workflow-states/archived"),
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&archived), vec![i64::from(a_state)], "only A's");
    let (status, _) = send!(
        &admin,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/workflow-states/{b_state}/restore")),
    );
    assert_eq!(status, StatusCode::NOT_FOUND, "B's state is out of reach");
    let still_archived = run_in_workspace(&pool, REF, b.workspace_id, |c| {
        states::find_by_id(c, b_state)
    })
    .expect("read B")
    .expect("B's state");
    assert!(still_archived.archived_at.is_some());
}
