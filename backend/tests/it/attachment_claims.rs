//! A comment attaches only its author's own uploads that are still waiting to
//! be attached. Ids are sequential, so a request naming someone else's draft,
//! or a file already on another comment, must leave both where they are.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Attachment, Claims, NewAttachment, NewComment, NewTicket, Ticket};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::outbound_email::OutboundEmailResolver;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig, WorkspaceScopedStorage};

use crate::common::{self, TestPool};

const REF: &str = "test:attachment_claims";

fn new_ticket(pool: &TestPool, ws: i32, title: &str) -> Ticket {
    run_in_workspace(pool, REF, ws, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: title.to_string(),
                workflow_state_id: state.id,
                ..Default::default()
            })
            .get_result(c)
    })
    .expect("insert ticket")
}

fn attachment(pool: &TestPool, ws: i32, row: NewAttachment) -> Attachment {
    run_in_workspace(pool, REF, ws, |c| {
        backend::repository::comments::create_attachment(c, row)
    })
    .expect("insert attachment")
}

fn reload(pool: &TestPool, ws: i32, id: i32) -> Attachment {
    run_in_workspace(pool, REF, ws, |c| {
        backend::repository::comments::get_attachment_by_id(c, id)
    })
    .expect("reload attachment")
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
async fn a_comment_attaches_only_its_authors_waiting_uploads() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let uploader = seeded.a.admin_uuid;
    // A second agent in the same workspace.
    let other = common::insert_plain_user(&mut pool.get().expect("conn"), "Second Agent");
    run_in_workspace(&pool, REF, ws, |c| {
        add_membership(c, ws, other, "agent", SeatWriteAuthority::ControlPlane)
    })
    .expect("add membership");

    let dir = tempfile::tempdir().expect("storage dir");
    let storage: Arc<dyn Storage> = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let scoped = WorkspaceScopedStorage::arc(storage.clone(), ws);

    let ticket = new_ticket(&pool, ws, "Printer jammed");
    let elsewhere = new_ticket(&pool, ws, "Payroll question");

    // The uploader's draft, waiting in temp storage.
    let draft_name = format!("{}_scan.pdf", Uuid::now_v7());
    scoped
        .put_file(b"%PDF", &format!("temp/{draft_name}"), "application/pdf")
        .await
        .expect("store draft");
    let draft = attachment(
        &pool,
        ws,
        NewAttachment {
            url: format!("/uploads/temp/{draft_name}"),
            name: "scan.pdf".to_string(),
            file_size: Some(4),
            mime_type: Some("application/pdf".to_string()),
            checksum: None,
            comment_id: None,
            uploaded_by: Some(uploader),
            transcription: None,
        },
    );
    // A file already on a comment of another ticket.
    let comment_elsewhere = run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::comments::create_comment(
            c,
            NewComment {
                content: "<p>payslip attached</p>".to_string(),
                ticket_id: elsewhere.id,
                user_uuid: uploader,
                ..Default::default()
            },
            None,
        )
    })
    .expect("insert comment");
    let attached = attachment(
        &pool,
        ws,
        NewAttachment {
            url: format!("/uploads/tickets/{}/payslip.pdf", elsewhere.id),
            name: "payslip.pdf".to_string(),
            file_size: Some(4),
            mime_type: Some("application/pdf".to_string()),
            checksum: None,
            comment_id: Some(comment_elsewhere.id),
            uploaded_by: Some(uploader),
            transcription: Some("salary details".to_string()),
        },
    );

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
    // Post a comment on `ticket` as `author`, naming attachment `ids`.
    let comment_as = |author: Uuid, ids: Vec<i32>| {
        let pool = pool.clone();
        let storage = storage.clone();
        let search = search.clone();
        let resolver = resolver.clone();
        let workspace = workspace.clone();
        let ticket_id = ticket.id;
        async move {
            let user = backend::repository::users::get_user_by_uuid(
                &author,
                &mut pool.get().expect("conn"),
            )
            .expect("load user");
            let claims = claims(&user);
            let corr = Uuid::now_v7();
            let actor =
                ActorContext::user(author, Some(corr)).with_workspace(workspace.workspace_id);
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
            let attachments: Vec<_> = ids
                .iter()
                .map(|id| json!({ "id": id, "url": "", "name": "" }))
                .collect();
            let resp = http_test::call_service(
                &app,
                http_test::TestRequest::post()
                    .uri(&format!("/api/tickets/{ticket_id}/comments"))
                    .set_json(
                        json!({ "content": "<p>see attached</p>", "attachments": attachments }),
                    )
                    .to_request(),
            )
            .await;
            assert!(
                resp.status().is_success(),
                "comment posts: {}",
                resp.status()
            );
            let body: serde_json::Value = http_test::read_body_json(resp).await;
            body["attachments"].as_array().map(Vec::len).unwrap_or(0)
        }
    };

    // Another agent names both: nothing is attached, and both stay put.
    assert_eq!(comment_as(other, vec![draft.id, attached.id]).await, 0);
    let draft_now = reload(&pool, ws, draft.id);
    assert_eq!(draft_now.comment_id, None, "the draft stays a draft");
    assert_eq!(draft_now.url, draft.url);
    assert_eq!(draft_now.uploaded_by, Some(uploader));
    let attached_now = reload(&pool, ws, attached.id);
    assert_eq!(
        attached_now.comment_id,
        Some(comment_elsewhere.id),
        "an attached file stays on its comment"
    );
    assert!(scoped
        .file_exists(&format!("temp/{draft_name}"))
        .await
        .expect("check draft"));

    // The uploader attaches their own draft: it moves under the ticket.
    assert_eq!(comment_as(uploader, vec![draft.id]).await, 1);
    let draft_now = reload(&pool, ws, draft.id);
    assert!(draft_now.comment_id.is_some());
    assert_eq!(
        draft_now.url,
        format!("/uploads/tickets/{}/{draft_name}", ticket.id)
    );
    assert!(scoped
        .file_exists(&format!("tickets/{}/{draft_name}", ticket.id))
        .await
        .expect("check moved file"));
}
