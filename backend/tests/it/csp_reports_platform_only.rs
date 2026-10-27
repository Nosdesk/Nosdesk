//! CSP violation reports from every workspace are stored under workspace 1,
//! so listing them is an operator task: an admin of workspace 1 without the
//! platform-admin role gets 403 from `/api/admin/csp-reports`, and a platform
//! admin gets the list.

#![allow(clippy::expect_used)]

use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};

use backend::extractors::WorkspaceContext;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common;

const DEFAULT_WORKSPACE: i32 = 1;

#[actix_web::test]
async fn csp_reports_list_is_platform_admin_only() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    // Both hold an admin seat in workspace 1; only the operator is a
    // platform admin.
    let (admin, operator) = {
        let mut conn = pool.get().expect("conn");
        let admin_uuid = common::insert_plain_user(&mut conn, "Workspace Admin");
        let operator = common::insert_user(&mut conn, "Operator");
        for user in [admin_uuid, operator.uuid] {
            let actor = ActorContext::user(user, None).with_workspace(DEFAULT_WORKSPACE);
            with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
                add_membership(
                    c,
                    DEFAULT_WORKSPACE,
                    user,
                    "admin",
                    SeatWriteAuthority::ControlPlane,
                )
                .map(|_| ())
            })
            .expect("seat in workspace 1");
        }
        let admin = backend::repository::users::get_user_by_uuid(&admin_uuid, &mut conn)
            .expect("load admin");
        (admin, operator)
    };
    let admin_token = common::mint_api_token(&mut pool.get().expect("conn"), &admin, "ws-admin");
    let operator_token =
        common::mint_api_token(&mut pool.get().expect("conn"), &operator, "operator");

    let workspace = {
        let mut conn = pool.get().expect("conn");
        let row = backend::repository::workspaces::find_by_id(&mut conn, DEFAULT_WORKSPACE)
            .expect("query default workspace")
            .expect("default workspace");
        WorkspaceContext {
            workspace_id: row.id,
            workspace_uuid: row.uuid,
            slug: row.slug,
            name: row.name,
            organisation_id: None,
            custom_domain: None,
        }
    };

    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .route(
                "/api/csp-report",
                web::post().to(backend::handlers::csp_reports::report_violation),
            )
            .service(
                web::scope("/api")
                    .wrap(actix_web::middleware::from_fn(
                        backend::middleware::dual_auth_middleware,
                    ))
                    .wrap_fn(move |req, srv| {
                        req.extensions_mut().insert(workspace.clone());
                        actix_web::dev::Service::call(srv, req)
                    })
                    .configure(backend::handlers::csp_reports::config),
            ),
    )
    .await;

    // A report from a page in some workspace, stored under workspace 1.
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri("/api/csp-report")
            .insert_header(("Content-Type", "application/csp-report"))
            .set_payload(
                json!({"csp-report": {
                    "document-uri": "https://acme.example.com/tickets/42",
                    "blocked-uri": "https://cdn.example.net/x.js",
                    "effective-directive": "script-src",
                    "disposition": "enforce"
                }})
                .to_string(),
            )
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 204, "report intake");

    let list = |token: String| {
        http_test::TestRequest::get()
            .uri("/api/admin/csp-reports")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .to_request()
    };

    let resp = http_test::call_service(&app, list(admin_token)).await;
    assert_eq!(
        resp.status().as_u16(),
        403,
        "an admin of workspace 1 without the platform role must not list CSP reports"
    );

    let resp = http_test::call_service(&app, list(operator_token)).await;
    assert_eq!(resp.status().as_u16(), 200, "a platform admin lists them");
    let rows: Value = http_test::read_body_json(resp).await;
    let uris: Vec<&str> = rows
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|r| r["document_uri"].as_str())
        .collect();
    assert!(
        uris.contains(&"https://acme.example.com/tickets/42"),
        "the stored report is listed, got {uris:?}"
    );
}
