//! A project's ticket list and dependency edges follow ticket visibility:
//! staff see every ticket in the project, a requester only their own.
#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewProject, NewTicket, NewUser, ProjectStatus, Ticket, User};
use backend::repository::{self, tickets, workflow_states};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

mod common;

use common::TestPool;

const WS: i32 = 1;

fn member(conn: &mut PgConnection, name: &str, role: &str) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(workspace_members::table)
        .values((
            workspace_members::workspace_id.eq(WS),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert workspace member");
    user
}

fn spawn(pool: &TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: "someone@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let user_uuid = user.uuid;
    actix_test::start(move || {
        let pool = pool.clone();
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: Uuid::nil(),
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
        };
        App::new()
            .app_data(web::Data::new(pool))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .route(
                        "/projects/{id}/tickets",
                        web::get().to(backend::handlers::get_project_tickets),
                    )
                    .route(
                        "/projects/{id}/dependencies",
                        web::get().to(backend::handlers::projects::get_project_dependencies),
                    ),
            )
    })
}

async fn get_json(srv: &actix_test::TestServer, path: &str) -> serde_json::Value {
    let mut resp = awc::Client::new()
        .get(srv.url(path))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status().as_u16(), 200, "{path}");
    resp.json().await.expect("json")
}

/// The ids of the project's tickets, and its dependency edges, as `user` sees them.
async fn seen_by(pool: &TestPool, user: &User, project_id: i32) -> (Vec<i64>, Vec<(i64, i64)>) {
    let srv = spawn(pool, user);
    let tickets = get_json(&srv, &format!("/api/projects/{project_id}/tickets")).await;
    let edges = get_json(&srv, &format!("/api/projects/{project_id}/dependencies")).await;
    let mut ids: Vec<i64> = tickets
        .as_array()
        .expect("ticket array")
        .iter()
        .map(|t| t["id"].as_i64().expect("id"))
        .collect();
    ids.sort_unstable();
    let edges = edges
        .as_array()
        .expect("edge array")
        .iter()
        .map(|e| {
            (
                e["from"].as_i64().expect("from"),
                e["to"].as_i64().expect("to"),
            )
        })
        .collect();
    (ids, edges)
}

#[actix_web::test]
async fn a_requester_sees_only_their_own_tickets_in_a_project() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let (requester, agent) = {
        let mut conn = pool.get().expect("conn");
        (
            member(&mut conn, "Requester", "member"),
            member(&mut conn, "Agent", "agent"),
        )
    };
    let (theirs, not_theirs, project_id) =
        run_in_workspace(&pool, "test:project_ticket_visibility", WS, |c| {
            let state = workflow_states::default_state(c)?.id;
            let mut open = |title: &str, requester_uuid: Option<Uuid>| -> QueryResult<Ticket> {
                tickets::create_ticket(
                    c,
                    NewTicket {
                        title: title.to_string(),
                        workflow_state_id: state,
                        requester_uuid,
                        ..Default::default()
                    },
                )
            };
            let theirs = open("Printer jammed", Some(requester.uuid))?;
            let not_theirs = open("Payroll export", None)?;
            let project = repository::create_project(
                c,
                NewProject {
                    name: "Office move".to_string(),
                    description: None,
                    status: ProjectStatus::Active,
                    start_date: None,
                    end_date: None,
                },
                None,
            )?;
            for ticket in [&theirs, &not_theirs] {
                repository::add_ticket_to_project(c, project.id, ticket.id)?;
            }
            repository::link_tickets(c, theirs.id, not_theirs.id)?;
            Ok((theirs.id, not_theirs.id, project.id))
        })
        .expect("seed project");
    let (theirs, not_theirs) = (i64::from(theirs), i64::from(not_theirs));

    let (ids, edges) = seen_by(&pool, &agent, project_id).await;
    assert_eq!(ids, vec![theirs, not_theirs]);
    assert!(!edges.is_empty(), "staff see the link");

    let (ids, edges) = seen_by(&pool, &requester, project_id).await;
    assert_eq!(ids, vec![theirs], "only the requester's own ticket");
    assert!(edges.is_empty(), "the link reaches a ticket they can't see");
}
