//! `GET /tickets/by-number/{number}`: a number names a ticket in the
//! request's workspace, behind the same visibility gate as its id.
#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, NewUser, Ticket, User};
use backend::repository::{tickets, workflow_states};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::TestPool;

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

fn claims_for(user: &User) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: "someone@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

fn spawn(pool: &TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = claims_for(user);
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
            custom_domain: None,
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
            .service(web::scope("/api").route(
                "/tickets/by-number/{number}",
                web::get().to(backend::handlers::get_ticket),
            ))
    })
}

fn open(pool: &TestPool, ws: i32, title: &str, requester: Option<Uuid>) -> Ticket {
    run_in_workspace(pool, "test:ticket_by_number", ws, |c| {
        let state = workflow_states::default_state(c)?.id;
        tickets::create_ticket(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: state,
                requester_uuid: requester,
                ..Default::default()
            },
        )
    })
    .expect("open ticket")
}

/// The status, and the id of the ticket returned.
async fn by_number(srv: &actix_test::TestServer, number: i32) -> (u16, Option<i64>) {
    let mut resp = awc::Client::new()
        .get(srv.url(&format!("/api/tickets/by-number/{number}")))
        .send()
        .await
        .expect("send");
    let status = resp.status().as_u16();
    if status != 200 {
        return (status, None);
    }
    let body: serde_json::Value = resp.json().await.expect("json");
    (status, body["id"].as_i64())
}

#[actix_web::test]
async fn a_number_finds_the_ticket_in_the_requests_workspace() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let (requester, agent) = {
        let mut conn = pool.get().expect("conn");
        (
            member(&mut conn, "Requester", "member"),
            member(&mut conn, "Agent", "agent"),
        )
    };
    let elsewhere = crate::common::seed_two_workspaces(&mut pool.get().expect("conn"))
        .a
        .workspace_id;

    // The other workspace opens tickets first: it reaches a number this one
    // doesn't, and this one's numbers no longer match their ids.
    for title in ["Laptop won't boot", "VPN drops", "Badge lost"] {
        open(&pool, elsewhere, title, None);
    }
    let theirs = open(&pool, WS, "Printer jammed", Some(requester.uuid));
    let not_theirs = open(&pool, WS, "Monitor flickers", None);
    assert_eq!((theirs.number, not_theirs.number), (1, 2));
    assert_ne!(theirs.id, theirs.number);

    let staff = spawn(&pool, &agent);
    assert_eq!(by_number(&staff, 1).await, (200, Some(theirs.id.into())));
    assert_eq!(
        by_number(&staff, 2).await,
        (200, Some(not_theirs.id.into()))
    );
    assert_eq!(
        by_number(&staff, 3).await.0,
        404,
        "3 is only the other workspace's"
    );

    let requester = spawn(&pool, &requester);
    assert_eq!(
        by_number(&requester, 1).await,
        (200, Some(theirs.id.into()))
    );
    assert_eq!(
        by_number(&requester, 2).await.0,
        404,
        "someone else's ticket"
    );
}
