//! `GET /tickets/{id}/activity` for a restricted viewer.
//!
//! Ticket access says the viewer may see the ticket; it does not make every
//! row on its timeline theirs. An internal note's `comment.created` carries
//! the note's content, and `ticket_reference` rows are staff-only, so the
//! endpoint applies the same keep-mask as the sync delta.
#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewComment, NewTicket, NewUser, Ticket, User};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

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

fn spawn(pool: &crate::common::TestPool, user: &User) -> actix_test::TestServer {
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
                "/tickets/{id}/activity",
                web::get().to(backend::handlers::get_ticket_activity),
            ))
    })
}

async fn event_types(srv: &actix_test::TestServer, ticket_id: i32) -> Vec<String> {
    let mut resp = awc::Client::new()
        .get(srv.url(&format!("/api/tickets/{ticket_id}/activity")))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    body["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[actix_web::test]
async fn a_restricted_viewer_does_not_see_internal_notes_or_references() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let (requester, agent, ticket, other) = {
        let mut conn = pool.get().expect("conn");
        let requester = member(&mut conn, "Requester", "member");
        let agent = member(&mut conn, "Agent", "agent");
        let state = backend::repository::workflow_states::default_state(&mut conn)
            .expect("default state")
            .id;
        let mk = |conn: &mut PgConnection, title: &str| -> Ticket {
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state,
                    requester_uuid: Some(requester.uuid),
                    ..Default::default()
                })
                .get_result(conn)
                .expect("insert ticket")
        };
        let ticket = mk(&mut conn, "Mine");
        let other = mk(&mut conn, "Other");
        (requester, agent, ticket, other)
    };

    // A public reply, an internal note, and a comment on the other ticket
    // that references this one (a staff-only row on this ticket's timeline).
    run_in_workspace(&pool, "test:seed", WS, |conn| {
        for (content, is_internal, on) in [
            ("<p>public reply</p>".to_string(), false, ticket.id),
            ("<p>internal note</p>".to_string(), true, ticket.id),
            (
                format!(
                    r#"<p>see <span data-ticket-link="true" data-ticket-id="{}"></span></p>"#,
                    ticket.id
                ),
                false,
                other.id,
            ),
        ] {
            backend::repository::comments::create_comment(
                conn,
                NewComment {
                    content,
                    ticket_id: on,
                    user_uuid: agent.uuid,
                    is_internal,
                    ..Default::default()
                },
                None,
            )?;
        }
        Ok(())
    })
    .expect("seed comments");

    let staff = event_types(&spawn(&pool, &agent), ticket.id).await;
    assert!(
        staff.contains(&"ticket_reference.added".to_string()),
        "{staff:?}"
    );
    assert_eq!(
        staff.iter().filter(|t| *t == "comment.created").count(),
        2,
        "staff see both comments: {staff:?}"
    );

    let restricted = event_types(&spawn(&pool, &requester), ticket.id).await;
    assert_eq!(
        restricted
            .iter()
            .filter(|t| *t == "comment.created")
            .count(),
        1,
        "the requester sees only the public reply: {restricted:?}"
    );
    assert!(
        !restricted.contains(&"ticket_reference.added".to_string()),
        "references are staff-only: {restricted:?}"
    );
}
