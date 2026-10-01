//! A request naming another workspace's row gets a client error, not a 500:
//! the database refuses the reference (keys between workspace tables include
//! the workspace), and the handler answers 400 for an id in the body and 404
//! for one in the path.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Uuid as SqlUuid};
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket};
use backend::repository::workflow_states;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool};

const REF: &str = "test:foreign_references_refused";

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct RowUuid {
    #[diesel(sql_type = SqlUuid)]
    uuid: Uuid,
}

fn insert_id(pool: &TestPool, ws: i32, sql: String) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        diesel::sql_query(sql).get_result::<Id>(c)
    })
    .expect("seed row")
    .id
}

fn new_ticket(pool: &TestPool, ws: i32) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        let state = workflow_states::default_state(c)?.id;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Printer jammed".to_string(),
                workflow_state_id: state,
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)
    })
    .expect("seed ticket")
}

#[actix_web::test]
async fn another_workspaces_ids_get_a_client_error() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);

    let a_ticket = new_ticket(&pool, a);
    let a_tag = insert_id(
        &pool,
        a,
        "INSERT INTO tags (name) VALUES ('urgent') RETURNING id".into(),
    );
    let a_project = insert_id(
        &pool,
        a,
        "INSERT INTO projects (name) VALUES ('Office move') RETURNING id".into(),
    );
    let a_cycle = run_in_workspace(&pool, REF, a, |c| {
        diesel::sql_query(format!(
            "INSERT INTO cycles (name, project_id) VALUES ('Sprint 1', {a_project}) RETURNING uuid"
        ))
        .get_result::<RowUuid>(c)
    })
    .expect("seed cycle")
    .uuid;
    let b_ticket = new_ticket(&pool, b);
    let b_state = run_in_workspace(&pool, REF, b, workflow_states::default_state)
        .expect("B's state")
        .id;
    let b_tag = insert_id(
        &pool,
        b,
        "INSERT INTO tags (name) VALUES ('vip') RETURNING id".into(),
    );
    let b_project = insert_id(
        &pool,
        b,
        "INSERT INTO projects (name) VALUES ('Payroll') RETURNING id".into(),
    );

    // Every request runs as workspace A's admin, in A.
    let admin = backend::repository::users::get_user_by_uuid(
        &seeded.a.admin_uuid,
        &mut pool.get().expect("conn"),
    )
    .expect("load admin");
    let claims = Claims {
        sub: admin.uuid.to_string(),
        name: admin.name.clone(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: a,
        workspace_uuid: seeded.a.workspace_uuid,
        slug: seeded.a.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(admin.uuid, Some(corr)).with_workspace(a);
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
            .service(
                web::scope("/api")
                    .route(
                        "/tickets/{id}",
                        web::put().to(backend::handlers::update_ticket),
                    )
                    .route(
                        "/tickets/{id}/tags",
                        web::put().to(backend::handlers::tags::set_ticket_tags),
                    )
                    .route(
                        "/projects/{project_id}/cycles",
                        web::post().to(backend::handlers::cycles::create),
                    )
                    .route(
                        "/cycles/{uuid}/tickets/{ticket_id}",
                        web::post().to(backend::handlers::cycles::add_ticket),
                    ),
            ),
    )
    .await;

    let put_ticket = http_test::TestRequest::put()
        .uri(&format!("/api/tickets/{a_ticket}"))
        .set_json(NewTicket {
            title: "Printer jammed".to_string(),
            workflow_state_id: b_state,
            ..Default::default()
        })
        .to_request();
    assert_eq!(
        http_test::call_service(&app, put_ticket).await.status(),
        StatusCode::BAD_REQUEST,
        "a ticket in A can't take B's state"
    );

    let tags = |tag: i32| {
        http_test::TestRequest::put()
            .uri(&format!("/api/tickets/{a_ticket}/tags"))
            .set_json(json!({ "tag_ids": [tag] }))
            .to_request()
    };
    assert_eq!(
        http_test::call_service(&app, tags(b_tag)).await.status(),
        StatusCode::BAD_REQUEST,
        "B's tag"
    );
    assert_eq!(
        http_test::call_service(&app, tags(a_tag)).await.status(),
        StatusCode::OK,
        "A's own tag"
    );

    let cycle_in = |project: i32| {
        http_test::TestRequest::post()
            .uri(&format!("/api/projects/{project}/cycles"))
            .set_json(json!({ "name": "Sprint 2" }))
            .to_request()
    };
    assert_eq!(
        http_test::call_service(&app, cycle_in(b_project))
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "a cycle in B's project"
    );

    let add_to_cycle = http_test::TestRequest::post()
        .uri(&format!("/api/cycles/{a_cycle}/tickets/{b_ticket}"))
        .to_request();
    assert_eq!(
        http_test::call_service(&app, add_to_cycle).await.status(),
        StatusCode::NOT_FOUND,
        "B's ticket in A's cycle"
    );
}
