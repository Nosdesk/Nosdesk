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

/// The sync events recorded for one attachment, oldest first:
/// `(event_type, data, groups)`.
fn attachment_events(
    pool: &TestPool,
    ws: i32,
    id: i32,
) -> Vec<(String, serde_json::Value, Vec<Option<String>>)> {
    use backend::schema::sync_actions;
    run_in_workspace(pool, REF, ws, |c| {
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(id.to_string()))
            .filter(sync_actions::event_type.like("attachment.%"))
            .order(sync_actions::sync_id.asc())
            .select((
                sync_actions::event_type,
                sync_actions::data,
                sync_actions::groups,
            ))
            .load(c)
    })
    .expect("load attachment events")
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
    // A draft is private to its uploader: its one event goes only to them,
    // and the refused attempt to attach it recorded nothing.
    let events = attachment_events(&pool, ws, draft.id);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, "attachment.created");
    assert_eq!(events[0].2, vec![Some(format!("user:{uploader}"))]);

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
    // Attaching it tells every client which comment it's on, so the file
    // still shows under the reply after a reload; the upload's own event
    // predates the comment.
    let events = attachment_events(&pool, ws, draft.id);
    let (kind, data, groups) = events.last().expect("an event for the claim");
    assert_eq!(kind, "attachment.attached");
    assert_eq!(data["comment_id"], json!(draft_now.comment_id));
    assert_eq!(data["url"], json!(draft_now.url));
    assert!(
        groups.contains(&Some(format!("workspace:{ws}"))),
        "{groups:?}"
    );
    assert!(
        groups.contains(&Some(format!("ticket:{}", ticket.id))),
        "{groups:?}"
    );
}

fn comment(pool: &TestPool, ws: i32, ticket_id: i32, author: Uuid) -> backend::models::Comment {
    run_in_workspace(pool, REF, ws, |c| {
        backend::repository::comments::create_comment(
            c,
            NewComment {
                content: "<p>see attached</p>".to_string(),
                ticket_id,
                user_uuid: author,
                ..Default::default()
            },
            None,
        )
    })
    .expect("insert comment")
}

fn file(name: &str, comment_id: Option<i32>, uploaded_by: Option<Uuid>) -> NewAttachment {
    NewAttachment {
        url: format!("/uploads/temp/{}_{name}", Uuid::now_v7()),
        name: name.to_string(),
        file_size: Some(4),
        mime_type: Some("application/pdf".to_string()),
        checksum: None,
        comment_id,
        uploaded_by,
        transcription: None,
    }
}

fn ticket_audience(ws: i32, ticket_id: i32) -> Vec<Option<String>> {
    vec![
        Some(format!("workspace:{ws}")),
        Some(format!("ticket:{ticket_id}")),
    ]
}

#[test]
fn a_guest_draft_reaches_no_one_until_its_submission_claims_it() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let requester = seeded.a.admin_uuid;
    let ticket = new_ticket(&pool, ws, "Laptop won't boot");

    // A guest uploads before submitting: there is no uploader, so no client
    // receives the draft and, with no workspace audience, no webhook fires.
    let draft = attachment(&pool, ws, file("photo.pdf", None, None));
    let events = attachment_events(&pool, ws, draft.id);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, "attachment.created");
    assert!(events[0].2.is_empty(), "{events:?}");
    assert!(!backend::sync::groups::has_workspace_audience(&events[0].2));

    // The submission claims it: the whole row goes to the ticket's audience.
    let reply = comment(&pool, ws, ticket.id, requester);
    let url = format!("/uploads/tickets/{}/photo.pdf", ticket.id);
    run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::comments::reparent_attachment(c, draft.id, &url, reply.id, requester)
    })
    .expect("claim draft");
    let events = attachment_events(&pool, ws, draft.id);
    assert_eq!(events.len(), 2, "{events:?}");
    let (kind, data, groups) = &events[1];
    assert_eq!(kind, "attachment.attached");
    assert_eq!(data["comment_id"], json!(reply.id));
    assert_eq!(data["name"], json!("photo.pdf"));
    assert_eq!(groups, &ticket_audience(ws, ticket.id));
}

#[test]
fn a_deleted_file_is_announced_to_whoever_had_it() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let agent = seeded.a.admin_uuid;
    let ticket = new_ticket(&pool, ws, "VPN drops hourly");
    let delete = |id: i32| {
        run_in_workspace(&pool, REF, ws, |c| {
            backend::repository::comments::delete_attachment(c, id)
        })
        .expect("delete attachment")
    };
    let last_event = |id: i32| {
        attachment_events(&pool, ws, id)
            .pop()
            .expect("an attachment event")
    };

    // A signed-in draft: only its uploader had it.
    let own_draft = attachment(&pool, ws, file("draft.pdf", None, Some(agent)));
    assert_eq!(delete(own_draft.id), 1);
    let (kind, data, groups) = last_event(own_draft.id);
    assert_eq!(kind, "attachment.deleted");
    assert_eq!(data, json!({ "id": own_draft.id }));
    assert_eq!(groups, vec![Some(format!("user:{agent}"))]);

    // A guest's draft: no one had it, so no one hears of it going, and the
    // nightly cleanup of abandoned uploads raises no webhook.
    let guest_draft = attachment(&pool, ws, file("guest.pdf", None, None));
    assert_eq!(delete(guest_draft.id), 1);
    let (kind, _, groups) = last_event(guest_draft.id);
    assert_eq!(kind, "attachment.deleted");
    assert!(groups.is_empty(), "{groups:?}");

    // A file on a reply: the ticket's audience.
    let reply = comment(&pool, ws, ticket.id, agent);
    let on_reply = attachment(&pool, ws, file("log.pdf", Some(reply.id), Some(agent)));
    assert_eq!(delete(on_reply.id), 1);
    let (kind, _, groups) = last_event(on_reply.id);
    assert_eq!(kind, "attachment.deleted");
    assert_eq!(groups, ticket_audience(ws, ticket.id));

    // Deleting a reply deletes its files, each announced to the ticket's
    // audience before the reply itself goes.
    let reply = comment(&pool, ws, ticket.id, agent);
    let files: Vec<Attachment> = ["a.pdf", "b.pdf"]
        .iter()
        .map(|name| attachment(&pool, ws, file(name, Some(reply.id), Some(agent))))
        .collect();
    run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::comments::delete_comment(c, reply.id, None)
    })
    .expect("delete comment");
    let comment_deleted: i64 = run_in_workspace(&pool, REF, ws, |c| {
        use backend::schema::sync_actions;
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(reply.id.to_string()))
            .filter(sync_actions::event_type.eq("comment.deleted"))
            .select(sync_actions::sync_id)
            .first(c)
    })
    .expect("comment.deleted event");
    for f in &files {
        let sync_id: i64 = run_in_workspace(&pool, REF, ws, |c| {
            use backend::schema::sync_actions;
            sync_actions::table
                .filter(sync_actions::aggregate_id.eq(f.id.to_string()))
                .filter(sync_actions::event_type.eq("attachment.deleted"))
                .select(sync_actions::sync_id)
                .first(c)
        })
        .expect("attachment.deleted event");
        assert!(sync_id < comment_deleted);
        let (_, data, groups) = last_event(f.id);
        assert_eq!(data, json!({ "id": f.id }));
        assert_eq!(groups, ticket_audience(ws, ticket.id));
    }
}
