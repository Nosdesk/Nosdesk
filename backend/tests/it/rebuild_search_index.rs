//! Rebuilding the search index re-indexes every workspace where the app runs
//! as `nosdesk_app`. The index holds every workspace on the machine and the
//! content tables are row-secured, so the rebuild reads elevated. Driven
//! through a pool shaped like production's, where a pooled connection starts
//! with no workspace.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::Value;

use backend::models::{Claims, NewTicket};
use backend::repository::workflow_states;
use backend::services::search::SearchService;
use backend::sync::session::run_in_workspace;

use crate::common;

const REF: &str = "test:rebuild_search_index";

#[actix_web::test]
async fn a_rebuild_indexes_every_workspaces_tickets() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let seed_pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut seed_pool.get().expect("conn"));
    for (ws, title) in [
        (seeded.a.workspace_id, "Printer jammed"),
        (seeded.b.workspace_id, "Laptop won't boot"),
    ] {
        run_in_workspace(&seed_pool, REF, ws, |c| {
            let state = workflow_states::default_state(c)?.id;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state,
                    ..Default::default()
                })
                .execute(c)
        })
        .expect("seed ticket");
    }

    let pool = db.runtime_pool(2);
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: seeded.a.admin_uuid.to_string(),
        name: "Operator".to_string(),
        email: "operator@example.com".to_string(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(claims.clone());
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::search::config)),
    )
    .await;

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/api/search/rebuild")
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["stats"]["tickets"], 2, "{body}");
    assert!(!search.is_rebuilding());
}
