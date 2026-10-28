//! A script or integration can post a comment as Markdown or plain text.
//! Markdown is stored as the comment editor's HTML, with `#n` as a link to
//! that ticket (recorded as a reference, like one picked in the composer),
//! and plain text keeps its line breaks in the text tier.

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
use backend::services::outbound_email::OutboundEmailResolver;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool};

const REF: &str = "test:comment_formats";

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

#[actix_web::test]
async fn markdown_and_plain_text_comments_are_stored_to_render() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let author = seeded.a.admin_uuid;
    let ticket = new_ticket(&pool, ws);
    let referenced = new_ticket(&pool, ws);

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
    // Post `content` on the ticket as `who` in `format`; the comment's id.
    let post = |who: Uuid, content: &'static str, format: &'static str| {
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
                        "content_format": format,
                    }))
                    .to_request(),
            )
            .await;
            assert!(resp.status().is_success(), "posts: {}", resp.status());
            let body: serde_json::Value = http_test::read_body_json(resp).await;
            body["id"].as_i64().expect("comment id") as i32
        }
    };

    let stored = |id: i32| {
        use backend::schema::comments;
        run_in_workspace(&pool, REF, ws, |c| {
            comments::table
                .find(id)
                .select((
                    comments::content,
                    comments::content_format,
                    comments::render_kind,
                ))
                .first::<(String, String, Option<String>)>(c)
        })
        .expect("load comment")
    };

    // Markdown: the editor's HTML, with the ticket number as a ticket link.
    let markdown: &'static str = Box::leak(
        format!(
            "**Fixed.** Same cause as #{} and #99999.\n\n- restarted\n- verified",
            referenced.number
        )
        .into_boxed_str(),
    );
    let id = post(author, markdown, "markdown").await;
    let (content, format, kind) = stored(id);
    assert_eq!(format, "html");
    assert_eq!(kind.as_deref(), Some("simple"));
    assert_eq!(
        content,
        format!(
            "<p><strong>Fixed.</strong> Same cause as <span data-ticket-link=\"true\" data-ticket-id=\"{0}\" \
             data-href=\"/tickets/id/{0}\">#{1}</span> and #99999.</p><ul><li>restarted</li><li>verified</li></ul>",
            referenced.id, referenced.number
        )
    );
    let references: Vec<i32> = run_in_workspace(&pool, REF, ws, |c| {
        use backend::schema::comment_ticket_references as r;
        r::table
            .filter(r::comment_id.eq(id))
            .select(r::referenced_ticket_id)
            .load(c)
    })
    .expect("load references");
    assert_eq!(
        references,
        vec![referenced.id],
        "the link is recorded as a reference"
    );

    // Plain text: stored as sent, rendered as text so its line breaks hold.
    let id = post(author, "Line one\nLine two", "plaintext").await;
    assert_eq!(
        stored(id),
        (
            "Line one\nLine two".to_string(),
            "plaintext".to_string(),
            Some("text".to_string())
        )
    );

    // HTML from the composer is unchanged.
    let id = post(author, "<p>From the app</p>", "html").await;
    assert_eq!(
        stored(id),
        (
            "<p>From the app</p>".to_string(),
            "html".to_string(),
            Some("simple".to_string())
        )
    );
}
