//! Workflow states are read per workspace, with nothing cached between reads.
//!
//! Each workspace has its own catalogue. `list_all`, `default_state` and
//! `first_in_category` return the pinned workspace's states, on an elevated
//! connection too; a lookup by id stays inside the workspace under row
//! security; an edit shows on the next read wherever it was made; and a ticket
//! can't be moved to another workspace's state.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket, WorkflowState, WorkflowStateCategory};
use backend::repository::workflow_states as states;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{pin_workspace, run_in_workspace, with_actor_bypass_context};

use crate::common::{self, TestPool};

const REF: &str = "test:workflow_states";

fn listed(pool: &TestPool, workspace_id: i32) -> Vec<WorkflowState> {
    run_in_workspace(pool, REF, workspace_id, states::list_all).expect("list states")
}

fn first_in(pool: &TestPool, workspace_id: i32, category: WorkflowStateCategory) -> WorkflowState {
    run_in_workspace(pool, REF, workspace_id, |c| {
        states::first_in_category(c, category)
    })
    .expect("first state in category")
}

#[test]
fn each_workspace_reads_its_own_states() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);

    // A, then B, then A again: no read answers with the workspace read before it.
    for ws in [a, b, a] {
        let all = listed(&pool, ws);
        assert!(!all.is_empty(), "workspace {ws} has states");
        assert!(
            all.iter().all(|s| s.workspace_id == ws),
            "workspace {ws} lists only its own states"
        );
        let default =
            run_in_workspace(&pool, REF, ws, states::default_state).expect("default state");
        assert_eq!(default.workspace_id, ws, "default state");
        assert_eq!(
            first_in(&pool, ws, WorkflowStateCategory::Done).workspace_id,
            ws,
            "first Done state"
        );
    }

    let a_state = first_in(&pool, a, WorkflowStateCategory::Done);
    let (found, category) = run_in_workspace(&pool, REF, b, |c| {
        Ok((
            states::find_by_id(c, a_state.id)?,
            states::category_of(c, a_state.id)?,
        ))
    })
    .expect("look up from B");
    assert!(found.is_none(), "B can't find A's state by id");
    assert_eq!(category, None, "nor its category");
    assert_eq!(
        run_in_workspace(&pool, REF, a, |c| states::category_of(c, a_state.id))
            .expect("look up from A"),
        Some(WorkflowStateCategory::Done)
    );
    assert_eq!(
        states::category_of_cached(a_state.id),
        Some(WorkflowStateCategory::Done),
        "a state read once is known without a connection"
    );
}

#[test]
fn an_elevated_connection_lists_only_its_pinned_workspace() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let b = seeded.b.workspace_id;

    let mut conn = pool.get().expect("conn");
    let (visible, all, default) =
        with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
            pin_workspace(c, b)?;
            let visible: i64 = backend::schema::workflow_states::table
                .count()
                .get_result(c)?;
            Ok::<_, diesel::result::Error>((
                visible,
                states::list_all(c)?,
                states::default_state(c)?,
            ))
        })
        .expect("elevated read");

    assert!(
        visible as usize > all.len(),
        "the elevated connection sees other workspaces' states"
    );
    assert!(all.iter().all(|s| s.workspace_id == b));
    assert_eq!(default.workspace_id, b);
}

#[test]
fn an_edit_shows_on_the_next_read() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = seeded.a.workspace_id;

    let state = first_in(&pool, a, WorkflowStateCategory::Active);
    // Written straight to the table, as another server process would be seen
    // from this one.
    run_in_workspace(&pool, REF, a, |c| {
        use backend::schema::workflow_states::dsl as s;
        diesel::update(s::workflow_states.find(state.id))
            .set(s::name.eq("Renamed elsewhere"))
            .execute(c)
    })
    .expect("rename");

    let renamed = listed(&pool, a)
        .into_iter()
        .find(|s| s.id == state.id)
        .expect("still listed");
    assert_eq!(renamed.name, "Renamed elsewhere");
}

#[actix_web::test]
async fn a_ticket_cannot_move_to_another_workspaces_state() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let a_done = first_in(&pool, a, WorkflowStateCategory::Done).id;
    let b_done = first_in(&pool, b, WorkflowStateCategory::Done).id;

    let ticket: Ticket = run_in_workspace(&pool, REF, a, |c| {
        let state = states::default_state(c)?;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Printer jammed".to_string(),
                workflow_state_id: state.id,
                ..Default::default()
            })
            .get_result(c)
    })
    .expect("insert ticket");
    let state_of = |id| {
        run_in_workspace(&pool, REF, a, |c| {
            use backend::schema::tickets;
            tickets::table
                .find(id)
                .select((tickets::workflow_state_id, tickets::closed_at))
                .first::<(i32, Option<chrono::NaiveDateTime>)>(c)
        })
        .expect("read ticket")
    };

    // Every request runs as workspace A's admin, with A as the workspace.
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
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let app = http_test::init_service(
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
            .service(web::scope("/api").route(
                "/tickets/{id}",
                web::patch().to(backend::handlers::update_ticket_partial),
            )),
    )
    .await;
    let patch = |state_id: i32| {
        http_test::TestRequest::patch()
            .uri(&format!("/api/tickets/{}", ticket.id))
            .set_json(json!({ "workflow_state_id": state_id }))
            .to_request()
    };

    let resp = http_test::call_service(&app, patch(b_done)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "B's state");
    assert_eq!(
        state_of(ticket.id),
        (ticket.workflow_state_id, None),
        "the ticket keeps its state"
    );

    let resp = http_test::call_service(&app, patch(a_done)).await;
    assert_eq!(resp.status(), StatusCode::OK, "A's own Done state");
    let (state, closed_at) = state_of(ticket.id);
    assert_eq!(state, a_done);
    assert!(closed_at.is_some(), "a Done state closes the ticket");
}
