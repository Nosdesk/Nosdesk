//! A reply sent twice is saved once. The composer mints a client id per reply
//! and keeps it on a failed reply's draft until the draft is edited, so a
//! resend of the same reply carries the same id; the server answers it with
//! the comment it already saved.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, Ticket};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::outbound_email::OutboundEmailResolver;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool};

const REF: &str = "test:reply_sent_twice";

fn new_ticket(pool: &TestPool, ws: i32) -> Ticket {
    run_in_workspace(pool, REF, ws, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "VPN drops".to_string(),
                workflow_state_id: state.id,
                ..Default::default()
            })
            .get_result(c)
    })
    .expect("insert ticket")
}

fn claims(user: &backend::models::User) -> Claims {
    Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: "agent@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// `(comment id, author)` of every reply on `ticket_id` with `content`.
fn saved(pool: &TestPool, ws: i32, ticket_id: i32, content: &str) -> Vec<(i32, Uuid)> {
    use backend::schema::comments;
    run_in_workspace(pool, REF, ws, |c| {
        comments::table
            .filter(comments::ticket_id.eq(ticket_id))
            .filter(comments::content.eq(content))
            .order(comments::id.asc())
            .select((comments::id, comments::user_uuid))
            .load(c)
    })
    .expect("load comments")
}

#[actix_web::test]
async fn a_reply_sent_twice_is_saved_once() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let author = seeded.a.admin_uuid;
    let other = common::insert_plain_user(&mut pool.get().expect("conn"), "Second Agent");
    run_in_workspace(&pool, REF, ws, |c| {
        add_membership(c, ws, other, "agent", SeatWriteAuthority::ControlPlane)
    })
    .expect("add membership");
    let ticket = new_ticket(&pool, ws);

    let dir = tempfile::tempdir().expect("storage dir");
    let storage: Arc<dyn Storage> = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let resolver = Arc::new(OutboundEmailResolver::new(pool.clone(), None));
    let workspace = WorkspaceContext {
        workspace_id: ws,
        workspace_uuid: seeded.a.workspace_uuid,
        slug: seeded.a.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    // Post `content` on the ticket as `who`, with `client_id`; the new or
    // existing comment's id.
    let post = |who: Uuid, content: &'static str, client_id: Option<Uuid>| {
        let pool = pool.clone();
        let storage = storage.clone();
        let search = search.clone();
        let resolver = resolver.clone();
        let workspace = workspace.clone();
        let ticket_id = ticket.id;
        async move {
            let user =
                backend::repository::users::get_user_by_uuid(&who, &mut pool.get().expect("conn"))
                    .expect("load user");
            let claims = claims(&user);
            let corr = Uuid::now_v7();
            let actor = ActorContext::user(who, Some(corr)).with_workspace(workspace.workspace_id);
            let app = http_test::init_service(
                App::new()
                    .app_data(web::Data::new(pool.clone()))
                    .app_data(web::Data::new(storage))
                    .app_data(web::Data::new(search))
                    .app_data(web::Data::new(resolver))
                    .wrap_fn(move |req, srv| {
                        req.extensions_mut().insert(workspace.clone());
                        req.extensions_mut().insert(claims.clone());
                        req.extensions_mut()
                            .insert(RequestContext::new(corr, actor.clone()));
                        srv.call(req)
                    })
                    .service(web::scope("/api").route(
                        "/tickets/{ticket_id}/comments",
                        web::post().to(backend::handlers::add_comment_to_ticket),
                    )),
            )
            .await;
            let resp = http_test::call_service(
                &app,
                http_test::TestRequest::post()
                    .uri(&format!("/api/tickets/{ticket_id}/comments"))
                    .set_json(json!({
                        "content": content,
                        "attachments": [],
                        "client_id": client_id,
                    }))
                    .to_request(),
            )
            .await;
            assert!(resp.status().is_success(), "posts: {}", resp.status());
            let body: serde_json::Value = http_test::read_body_json(resp).await;
            body["id"].as_i64().expect("comment id") as i32
        }
    };

    // The same reply twice: one comment, and the resend gets it back.
    let reply = Some(Uuid::now_v7());
    let first = post(author, "<p>Try the new client</p>", reply).await;
    let again = post(author, "<p>Try the new client</p>", reply).await;
    assert_eq!(again, first, "the resend answers with the saved reply");
    assert_eq!(
        saved(&pool, ws, ticket.id, "<p>Try the new client</p>"),
        vec![(first, author)]
    );

    // The id is the author's own: someone else reusing it saves their reply,
    // and never gets the first author's back.
    let theirs = post(other, "<p>Try the new client</p>", reply).await;
    assert_ne!(theirs, first);
    assert_eq!(
        saved(&pool, ws, ticket.id, "<p>Try the new client</p>"),
        vec![(first, author), (theirs, other)]
    );

    // Without a client id, every post is a new reply, as before.
    post(author, "<p>Ping</p>", None).await;
    post(author, "<p>Ping</p>", None).await;
    assert_eq!(saved(&pool, ws, ticket.id, "<p>Ping</p>").len(), 2);
}
