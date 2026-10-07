//! A ticket's stored files are checked against the reply they belong to.
//!
//! A file under `tickets/{id}/` sits in the folder of the ticket it was first
//! stored for. That is not proof of the ticket it belongs to now (a merge moves
//! the reply and leaves the file where it was), nor of whether the reader may
//! see the reply it hangs off (an internal note, or one that was removed). The
//! agent file route, the raw-mail route and the portal download all go through
//! the attachment's row to its comment, and from the comment to its ticket.
//!
//! Self-hosted, so a requester-role member signs into the agent app and reaches
//! the agent routes; the hosted cases live in `tests/hosted_file_access.rs`.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::PortalContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewAttachment, NewComment, NewTicket, Ticket};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig, WorkspaceScopedStorage};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:ticket_file_access";

struct Fixture {
    _db: common::TestDb,
    _dir: tempfile::TempDir,
    pool: TestPool,
    storage: Arc<dyn Storage>,
    ws: WorkspaceSeed,
    /// An agent of the workspace.
    agent: Uuid,
    /// Requester of `ticket` (not of `source`).
    requester: Uuid,
    /// The ticket the requester asked for.
    ticket: Ticket,
    /// Another requester's ticket, merged into `ticket`.
    source: Ticket,
}

/// One stored file and the ids that reach it.
struct File {
    /// Path under `tickets/`, as the agent route takes it.
    path: String,
    attachment_id: i32,
    comment_id: i32,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let a = ws.workspace_id;
        let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "File Agent");
        let other = common::insert_plain_user(&mut pool.get().expect("conn"), "Other Requester");
        run_in_workspace(&pool, REF, a, |c| {
            add_membership(c, a, agent, "agent", SeatWriteAuthority::ControlPlane)?;
            add_membership(c, a, other, "member", SeatWriteAuthority::ControlPlane)
        })
        .expect("members");

        let new_ticket = |title: &str, requester: Uuid| -> Ticket {
            run_in_workspace(&pool, REF, a, |c| {
                let state = backend::repository::workflow_states::default_state(c)?;
                diesel::insert_into(backend::schema::tickets::table)
                    .values(&NewTicket {
                        title: title.to_string(),
                        workflow_state_id: state.id,
                        requester_uuid: Some(requester),
                        ..Default::default()
                    })
                    .get_result(c)
            })
            .expect("insert ticket")
        };
        let ticket = new_ticket("Laptop won't boot", ws.member_uuid);
        let source = new_ticket("Laptop still won't boot", other);

        let dir = tempfile::tempdir().expect("storage dir");
        let storage = create_storage(StorageConfig::Local {
            base_path: dir.path().to_string_lossy().into_owned(),
        });
        Self {
            _db: db,
            _dir: dir,
            pool,
            storage,
            requester: ws.member_uuid,
            ws,
            agent,
            ticket,
            source,
        }
    }

    async fn put(&self, path: &str, bytes: &[u8]) {
        WorkspaceScopedStorage::arc(self.storage.clone(), self.ws.workspace_id)
            .put_file(bytes, path, "application/octet-stream")
            .await
            .expect("store file");
    }

    /// A reply on `ticket_id` by `author` carrying one stored file, under that
    /// ticket's folder.
    async fn reply_with_file(
        &self,
        ticket_id: i32,
        author: Uuid,
        is_internal: bool,
        name: &str,
    ) -> File {
        let stored = format!("{}_{name}", Uuid::now_v7());
        let path = format!("{ticket_id}/{stored}");
        let url = format!("/uploads/tickets/{path}");
        let (comment_id, attachment_id) =
            run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
                let comment = backend::repository::comments::create_comment(
                    c,
                    NewComment {
                        content: format!("<p>{name}</p>"),
                        ticket_id,
                        user_uuid: author,
                        is_internal,
                        ..Default::default()
                    },
                    None,
                )?;
                let attachment = backend::repository::comments::create_attachment(
                    c,
                    NewAttachment {
                        url: url.clone(),
                        name: name.to_string(),
                        file_size: Some(4),
                        mime_type: None,
                        checksum: None,
                        comment_id: Some(comment.id),
                        uploaded_by: Some(author),
                        transcription: None,
                    },
                )?;
                Ok((comment.id, attachment.id))
            })
            .expect("reply with file");
        self.put(&format!("tickets/{path}"), name.as_bytes()).await;
        File {
            path,
            attachment_id,
            comment_id,
        }
    }

    fn in_workspace<T>(
        &self,
        f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
    ) -> T {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, f).expect("workspace write")
    }
}

/// The agent file routes and the portal download, as one user. Agent routes
/// read `Claims`; the portal route reads `PortalContext`. Both are set, as
/// their middlewares would.
macro_rules! app_as {
    ($fx:expr, $user:expr) => {{
        let user: Uuid = $user;
        let a = $fx.ws.workspace_id;
        let claims = Claims {
            sub: user.to_string(),
            name: "Viewer".to_string(),
            email: "viewer@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        let workspace = WorkspaceContext {
            workspace_id: a,
            workspace_uuid: $fx.ws.workspace_uuid,
            slug: $fx.ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let portal = PortalContext {
            user_uuid: user,
            workspace_id: a,
            workspace_uuid: $fx.ws.workspace_uuid,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(a);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($fx.pool.clone()))
                .app_data(web::Data::new($fx.storage.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .route(
                    "/api/files/tickets/{ticket_id}/notes/{filename:.*}",
                    web::get().to(backend::handlers::serve_ticket_note_image),
                )
                .route(
                    "/api/files/tickets/{filename:.*}",
                    web::get().to(backend::handlers::serve_ticket_file),
                )
                .route(
                    "/api/comments/{id}/raw.eml",
                    web::get().to(backend::handlers::get_comment_raw_eml),
                )
                .route(
                    "/api/portal/tickets/{id}/attachments/{attachment_id}",
                    web::get().to(backend::handlers::portal::download_attachment),
                ),
        )
        .await
    }};
}

macro_rules! status {
    ($app:expr, $uri:expr) => {
        http_test::call_service($app, http_test::TestRequest::get().uri($uri).to_request())
            .await
            .status()
    };
}

/// The paths a requester's agent session and portal session take to the same
/// files. Every denial is a 404.
#[actix_web::test]
async fn a_ticket_file_follows_its_reply() {
    let fx = Fixture::new();
    let tid = fx.ticket.id;

    let public = fx
        .reply_with_file(tid, fx.agent, false, "invoice.pdf")
        .await;
    let internal = fx.reply_with_file(tid, fx.agent, true, "notes.txt").await;
    let removed = fx.reply_with_file(tid, fx.agent, false, "draft.txt").await;
    fx.in_workspace(|c| {
        diesel::update(backend::schema::comments::table.find(removed.comment_id))
            .set(backend::schema::comments::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
            .execute(c)
    });
    // A reply on the other requester's ticket, then that ticket merged into
    // this one: the merge moves the reply (`comments.ticket_id`) and leaves
    // the file in the source ticket's folder.
    let merged_in = fx
        .reply_with_file(fx.source.id, fx.agent, false, "photo.png")
        .await;
    fx.in_workspace(|c| {
        diesel::update(
            backend::schema::comments::table
                .filter(backend::schema::comments::ticket_id.eq(fx.source.id)),
        )
        .set(backend::schema::comments::ticket_id.eq(tid))
        .execute(c)
    });
    assert!(
        merged_in.path.starts_with(&format!("{}/", fx.source.id)),
        "the merged-in file stays in the source ticket's folder"
    );
    // A file in this ticket's folder that no attachment row accounts for.
    let orphan = format!("{tid}/{}_orphan.pdf", Uuid::now_v7());
    fx.put(&format!("tickets/{orphan}"), b"orphan").await;
    // A PDF reply's thumbnail, stored beside it with no row of its own.
    let pdf_thumb = backend::utils::pdf::thumbnail_path(&public.path).expect("a PDF");
    fx.put(&format!("tickets/{pdf_thumb}"), b"webp").await;
    // A notes image, which follows its ticket.
    let note = format!("{}_paste.png", Uuid::now_v7());
    fx.put(&format!("tickets/{tid}/notes/{note}"), b"note")
        .await;
    // Raw mail kept for the internal note.
    let eml = format!("email_raw/{}_message.eml", Uuid::now_v7());
    fx.in_workspace(|c| {
        diesel::update(backend::schema::comments::table.find(internal.comment_id))
            .set(backend::schema::comments::raw_source_uri.eq(Some(eml.clone())))
            .execute(c)
    });
    fx.put(&eml, b"From: someone@example.com").await;

    let file = |f: &File| format!("/api/files/tickets/{}", f.path);
    let portal = |f: &File| format!("/api/portal/tickets/{tid}/attachments/{}", f.attachment_id);
    let raw = |f: &File| format!("/api/comments/{}/raw.eml", f.comment_id);

    // The requester: their ticket's public replies, merged-in ones included,
    // and nothing internal, removed or unaccounted for.
    let app = app_as!(fx, fx.requester);
    for (uri, want, what) in [
        (file(&public), StatusCode::OK, "a public reply's file"),
        (
            file(&merged_in),
            StatusCode::OK,
            "a file on a reply merged in from another ticket",
        ),
        (
            file(&internal),
            StatusCode::NOT_FOUND,
            "an internal note's file",
        ),
        (
            file(&removed),
            StatusCode::NOT_FOUND,
            "a removed reply's file",
        ),
        (
            format!("/api/files/tickets/{orphan}"),
            StatusCode::NOT_FOUND,
            "a file with no attachment row",
        ),
        (
            format!("/api/files/tickets/{pdf_thumb}"),
            StatusCode::OK,
            "a public PDF's thumbnail",
        ),
        (
            raw(&internal),
            StatusCode::NOT_FOUND,
            "an internal note's raw mail",
        ),
        (
            portal(&public),
            StatusCode::OK,
            "portal: a public reply's file",
        ),
        (
            portal(&merged_in),
            StatusCode::OK,
            "portal: a file on a merged-in reply",
        ),
        (
            portal(&internal),
            StatusCode::NOT_FOUND,
            "portal: an internal note's file",
        ),
        (
            portal(&removed),
            StatusCode::NOT_FOUND,
            "portal: a removed reply's file",
        ),
    ] {
        assert_eq!(status!(&app, &uri), want, "requester, {what}: {uri}");
    }

    // An agent: every live reply's file, internal notes included, wherever
    // the file is stored; still nothing removed or unaccounted for.
    let app = app_as!(fx, fx.agent);
    for (uri, want, what) in [
        (file(&public), StatusCode::OK, "a public reply's file"),
        (file(&internal), StatusCode::OK, "an internal note's file"),
        (
            file(&merged_in),
            StatusCode::OK,
            "a file on a reply merged in from another ticket",
        ),
        (
            file(&removed),
            StatusCode::NOT_FOUND,
            "a removed reply's file",
        ),
        (
            format!("/api/files/tickets/{orphan}"),
            StatusCode::NOT_FOUND,
            "a file with no attachment row",
        ),
        (
            format!("/api/files/tickets/{pdf_thumb}"),
            StatusCode::OK,
            "a PDF's thumbnail",
        ),
        (
            format!("/api/files/tickets/{tid}/notes/{note}"),
            StatusCode::OK,
            "a notes image",
        ),
        (
            raw(&internal),
            StatusCode::OK,
            "an internal note's raw mail",
        ),
    ] {
        assert_eq!(status!(&app, &uri), want, "agent, {what}: {uri}");
    }
}
