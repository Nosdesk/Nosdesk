//! Files the browser loads by itself, under hosted workspace selection.
//!
//! The agent app names its workspace in the `X-Nosdesk-Workspace` header, but
//! an `<img>`, a PDF viewer or a download link never sends it. Each route here
//! takes the workspace from the resource instead: a member who can see the
//! resource gets the file, while anyone else (staff of another workspace
//! included) and any unknown id get a 404. No request below sends the header.
//!
//! A standalone binary rather than part of `tests/it`: selection mode is
//! process-wide environment. Every test sets the same two values and none
//! clears them, so they can share the process.

#![allow(clippy::expect_used)]

mod common;

use std::sync::{Arc, OnceLock};

use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use actix_web::http::StatusCode;
use actix_web::middleware::from_fn;
use actix_web::{test, web, App};
use chrono::{Duration, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use backend::middleware::api_token::dual_auth_middleware;
use backend::models::{
    BackupJobUpdate, NewActiveSession, NewAttachment, NewComment, NewTicket, NewWorkspaceExportJob,
    Ticket, WorkspaceExportJobUpdate,
};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::session::run_in_workspace;
use backend::utils::jwt::JwtUtils;
use backend::utils::storage::{
    create_storage, set_process_storage, Storage, StorageConfig, WorkspaceScopedStorage,
};

fn enable_selection_mode() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");
}

/// One local storage root for the binary, installed as the process storage
/// too, which the export download reads through. Tests write under their own
/// random names, so sharing it is safe.
fn storage() -> Arc<dyn Storage> {
    static STORAGE: OnceLock<(tempfile::TempDir, Arc<dyn Storage>)> = OnceLock::new();
    STORAGE
        .get_or_init(|| {
            let root = tempfile::tempdir().expect("tempdir");
            let storage = create_storage(StorageConfig::Local {
                base_path: root.path().to_string_lossy().into_owned(),
            });
            set_process_storage(storage.clone());
            (root, storage)
        })
        .1
        .clone()
}

fn session_token(pool: &backend::db::Pool, user_uuid: Uuid) -> String {
    let mut conn = pool.get().expect("conn");
    let user =
        backend::repository::users::get_user_by_uuid(&user_uuid, &mut conn).expect("load user");
    let session = backend::repository::active_sessions::create_session(
        &mut conn,
        NewActiveSession {
            user_uuid,
            device_name: Some("file-access-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
        },
    )
    .expect("create session");
    JwtUtils::create_token(&user, &session.session_id).expect("mint session JWT")
}

/// The file routes as `startup.rs` mounts them, behind the real auth funnel.
macro_rules! app {
    ($pool:expr) => {
        test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .app_data(web::Data::new(storage()))
                .service(
                    web::scope("/api/files")
                        .wrap(from_fn(dual_auth_middleware))
                        .route(
                            "/tickets/{ticket_id}/notes/{filename:.*}",
                            web::get().to(backend::handlers::serve_ticket_note_image),
                        )
                        .route(
                            "/tickets/{filename:.*}",
                            web::get().to(backend::handlers::serve_ticket_file),
                        )
                        .route(
                            "/assets/{asset_id}/media/{filename:.*}",
                            web::get().to(backend::handlers::asset_media::serve_asset_media_file),
                        )
                        .route(
                            "/temp/{filename:.*}",
                            web::get().to(backend::handlers::serve_temp_file),
                        ),
                )
                .service(
                    web::scope("/api")
                        .wrap(from_fn(dual_auth_middleware))
                        .configure(backend::handlers::workspace_data_export::config)
                        .configure(backend::handlers::backup::config)
                        .configure(backend::handlers::tickets::config)
                        .configure(backend::handlers::plugins::config),
                )
                .route(
                    "/uploads/users/avatars/{filename:.*}",
                    web::get().to(backend::handlers::serve_public_file),
                ),
        )
        .await
    };
}

/// GET as a signed-in user, with no workspace selection header.
macro_rules! get {
    ($app:expr, $uri:expr, $token:expr) => {
        test::call_service(
            $app,
            test::TestRequest::get()
                .uri($uri)
                .insert_header(("Authorization", format!("Bearer {}", $token)))
                .to_request(),
        )
        .await
    };
}

macro_rules! status {
    ($app:expr, $uri:expr, $token:expr) => {
        get!($app, $uri, $token).status()
    };
}

async fn body<B: MessageBody>(resp: ServiceResponse<B>) -> Vec<u8> {
    test::read_body(resp).await.to_vec()
}

fn header<B>(resp: &ServiceResponse<B>, name: &str) -> Option<String> {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Workspace A holds the resources; B is a second tenant whose staff must
/// never see them.
struct Fixture {
    _db: common::TestDb,
    pool: backend::db::Pool,
    ws_a: i32,
    ws_b: i32,
    /// Workspace admin of A: sees every ticket there.
    staff_a: String,
    /// A requester in A who neither requested nor watches the ticket.
    requester_a: String,
    /// Workspace admin of B only.
    staff_b: String,
    a_admin_uuid: Uuid,
    a_plugin_uuid: Uuid,
    ticket: Ticket,
    comment_id: i32,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        enable_selection_mode();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
        let ws_a = seeded.a.workspace_id;

        let (ticket, comment_id) = run_in_workspace(&pool, "test:seed", ws_a, |c| {
            use backend::schema::{tickets, workflow_states};
            let state = workflow_states::table
                .filter(workflow_states::is_default.eq(true))
                .select(workflow_states::id)
                .first::<i32>(c)?;
            let ticket: Ticket = diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: "Printer on fire".to_string(),
                    workflow_state_id: state,
                    ..Default::default()
                })
                .get_result(c)?;
            let comment = backend::repository::comments::create_comment(
                c,
                NewComment {
                    content: "<p>scan attached</p>".to_string(),
                    ticket_id: ticket.id,
                    user_uuid: seeded.a.admin_uuid,
                    ..Default::default()
                },
                None,
            )?;
            Ok((ticket, comment.id))
        })
        .expect("seed ticket");

        Self {
            staff_a: session_token(&pool, seeded.a.admin_uuid),
            requester_a: session_token(&pool, seeded.a.member_uuid),
            staff_b: session_token(&pool, seeded.b.admin_uuid),
            a_admin_uuid: seeded.a.admin_uuid,
            a_plugin_uuid: seeded.a.plugin_uuid,
            _db: db,
            pool,
            ws_a,
            ws_b: seeded.b.workspace_id,
            ticket,
            comment_id,
        }
    }

    /// Insert an attachment row in A, and its bytes in A's storage.
    async fn attach(&self, url: &str, comment_id: Option<i32>, bytes: &[u8]) {
        let row = NewAttachment {
            url: url.to_string(),
            name: url.rsplit('/').next().expect("name").to_string(),
            file_size: Some(bytes.len() as i64),
            mime_type: None,
            checksum: None,
            comment_id,
            uploaded_by: Some(self.a_admin_uuid),
            transcription: None,
        };
        run_in_workspace(&self.pool, "test:seed", self.ws_a, |c| {
            backend::repository::comments::create_attachment(c, row)
        })
        .expect("insert attachment");
        let path = url.strip_prefix("/uploads/").expect("an /uploads/ url");
        self.put(path, bytes).await;
    }

    async fn put(&self, path: &str, bytes: &[u8]) {
        WorkspaceScopedStorage::arc(storage(), self.ws_a)
            .put_file(bytes, path, "application/octet-stream")
            .await
            .expect("store file");
    }
}

fn stored_name(name: &str) -> String {
    format!("{}_{name}", Uuid::now_v7())
}

#[actix_web::test]
async fn ticket_files_load_without_the_selection_header() {
    let fx = Fixture::new();
    let tid = fx.ticket.id;

    // Attached upload: tickets/{ticket_id}/...
    let foldered = stored_name("scan.pdf");
    fx.attach(
        &format!("/uploads/tickets/{tid}/{foldered}"),
        Some(fx.comment_id),
        b"foldered",
    )
    .await;
    // Inbound email before it was filed by ticket: tickets/...
    let unfoldered = stored_name("legacy.pdf");
    fx.attach(
        &format!("/uploads/tickets/{unfoldered}"),
        Some(fx.comment_id),
        b"unfoldered",
    )
    .await;
    // A note image, under the ticket's notes folder.
    let note = stored_name("paste.png");
    fx.put(&format!("tickets/{tid}/notes/{note}"), b"note")
        .await;

    let app = app!(fx.pool);

    let resp = get!(
        &app,
        &format!("/api/files/tickets/{tid}/{foldered}"),
        &fx.staff_a
    );
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header(&resp, "cache-control").as_deref(),
        Some("private, max-age=3600"),
        "an authenticated file must not be kept by shared caches"
    );
    assert_eq!(
        header(&resp, "access-control-allow-origin"),
        None,
        "no CORS wildcard on a private file"
    );
    assert_eq!(
        header(&resp, "content-disposition").as_deref(),
        Some("inline; filename=\"scan.pdf\""),
        "named as uploaded, without the storage prefix"
    );
    assert_eq!(body(resp).await, b"foldered");

    let resp = get!(
        &app,
        &format!("/api/files/tickets/{unfoldered}"),
        &fx.staff_a
    );
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "an unfoldered inbound file still serves"
    );
    assert_eq!(body(resp).await, b"unfoldered");

    let resp = get!(
        &app,
        &format!("/api/files/tickets/{tid}/notes/{note}"),
        &fx.staff_a
    );
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await, b"note");

    for uri in [
        format!("/api/files/tickets/{tid}/{foldered}"),
        format!("/api/files/tickets/{unfoldered}"),
        format!("/api/files/tickets/{tid}/notes/{note}"),
    ] {
        assert_eq!(
            status!(&app, &uri, &fx.staff_b),
            StatusCode::NOT_FOUND,
            "staff of another workspace: {uri}"
        );
        assert_eq!(
            status!(&app, &uri, &fx.requester_a),
            StatusCode::NOT_FOUND,
            "a member who can't see the ticket: {uri}"
        );
    }

    for uri in [
        "/api/files/tickets/2147483000/missing.pdf".to_string(),
        format!("/api/files/tickets/{}", stored_name("missing.pdf")),
        "/api/files/tickets/2147483000/notes/missing.png".to_string(),
    ] {
        assert_eq!(
            status!(&app, &uri, &fx.staff_a),
            StatusCode::NOT_FOUND,
            "unknown: {uri}"
        );
    }

    // A personal API token reaches only the workspace it was minted in, even
    // when its owner is staff in both.
    let both = common::insert_plain_user(&mut fx.pool.get().expect("conn"), "Staff of both");
    for ws in [fx.ws_a, fx.ws_b] {
        run_in_workspace(&fx.pool, "test:seed", ws, |c| {
            add_membership(c, ws, both, "agent", SeatWriteAuthority::ControlPlane)
        })
        .expect("add membership");
    }
    let user =
        backend::repository::users::get_user_by_uuid(&both, &mut fx.pool.get().expect("conn"))
            .expect("load user");
    let token_minted_in = |ws| {
        run_in_workspace(&fx.pool, "test:seed", ws, |c| {
            Ok(common::mint_api_token(c, &user, "files"))
        })
        .expect("mint API token")
    };
    let uri = format!("/api/files/tickets/{tid}/{foldered}");
    assert_eq!(
        status!(&app, &uri, &token_minted_in(fx.ws_a)),
        StatusCode::OK,
        "a token minted in the file's workspace"
    );
    assert_eq!(
        status!(&app, &uri, &token_minted_in(fx.ws_b)),
        StatusCode::NOT_FOUND,
        "a token minted in another workspace"
    );

    // Profile photos stay public, so shared caches may keep them.
    let avatar = format!("{}_avatar.webp", Uuid::new_v4());
    storage()
        .put_file(b"photo", &format!("users/avatars/{avatar}"), "image/webp")
        .await
        .expect("store avatar");
    let resp = get!(
        &app,
        &format!("/uploads/users/avatars/{avatar}"),
        &fx.staff_a
    );
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header(&resp, "cache-control").as_deref(),
        Some("public, max-age=3600")
    );
}

#[actix_web::test]
async fn uploads_and_their_pdf_thumbnails_load_without_the_selection_header() {
    let fx = Fixture::new();

    let draft = stored_name("draft.pdf");
    fx.attach(&format!("/uploads/temp/{draft}"), None, b"%PDF")
        .await;
    // The upload stores a server-rendered thumbnail beside the PDF; it has no
    // row of its own.
    let thumbnail = backend::utils::pdf::thumbnail_path(&format!("temp/{draft}"))
        .expect("a PDF has a thumbnail path");
    fx.put(&thumbnail, b"webp").await;
    let thumbnail_uri = format!("/api/files/{thumbnail}");

    let app = app!(fx.pool);

    let resp = get!(&app, &format!("/api/files/temp/{draft}"), &fx.staff_a);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await, b"%PDF");

    let resp = get!(&app, &thumbnail_uri, &fx.staff_a);
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the thumbnail is authorized as its PDF"
    );
    assert_eq!(header(&resp, "content-type").as_deref(), Some("image/webp"));
    assert_eq!(body(resp).await, b"webp");

    for uri in [format!("/api/files/temp/{draft}"), thumbnail_uri.clone()] {
        assert_eq!(
            status!(&app, &uri, &fx.staff_b),
            StatusCode::NOT_FOUND,
            "staff of another workspace: {uri}"
        );
    }
    for name in [
        stored_name("missing.pdf"),
        stored_name("missing_thumb.webp"),
    ] {
        assert_eq!(
            status!(&app, &format!("/api/files/temp/{name}"), &fx.staff_a),
            StatusCode::NOT_FOUND,
            "unknown: {name}"
        );
    }
}

#[actix_web::test]
async fn asset_media_raw_mail_and_plugin_icons_load_without_the_selection_header() {
    let fx = Fixture::new();

    let asset_id = run_in_workspace(&fx.pool, "test:seed", fx.ws_a, |c| {
        Ok(common::insert_stock_asset(c, "Laptop"))
    })
    .expect("seed asset");
    let photo = stored_name("photo.png");
    fx.put(&format!("assets/{asset_id}/media/{photo}"), b"photo")
        .await;

    let eml = format!("email_raw/{}", stored_name("message.eml"));
    let raw_comment_id = run_in_workspace(&fx.pool, "test:seed", fx.ws_a, |c| {
        backend::repository::comments::create_comment(
            c,
            NewComment {
                content: "<p>from email</p>".to_string(),
                ticket_id: fx.ticket.id,
                user_uuid: fx.a_admin_uuid,
                raw_source_uri: Some(eml.clone()),
                ..Default::default()
            },
            None,
        )
        .map(|comment| comment.id)
    })
    .expect("seed inbound comment");
    fx.put(&eml, b"From: someone@example.com").await;

    run_in_workspace(&fx.pool, "test:seed", fx.ws_a, |c| {
        use backend::schema::plugins;
        diesel::update(plugins::table.filter(plugins::uuid.eq(fx.a_plugin_uuid)))
            .set(plugins::icon_svg.eq(Some(b"<svg/>".to_vec())))
            .execute(c)
    })
    .expect("seed plugin icon");

    let app = app!(fx.pool);

    let photo_uri = format!("/api/files/assets/{asset_id}/media/{photo}");
    let eml_uri = format!("/api/comments/{raw_comment_id}/raw.eml");
    let icon_uri = format!("/api/plugins/{}/icon", fx.a_plugin_uuid);

    let resp = get!(&app, &photo_uri, &fx.staff_a);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await, b"photo");

    let resp = get!(&app, &eml_uri, &fx.staff_a);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await, b"From: someone@example.com");

    let resp = get!(&app, &icon_uri, &fx.staff_a);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header(&resp, "cache-control").as_deref(),
        Some("private, max-age=300")
    );
    assert_eq!(body(resp).await, b"<svg/>");

    for (route, uri) in [
        ("asset media", &photo_uri),
        ("raw mail", &eml_uri),
        ("plugin icon", &icon_uri),
    ] {
        assert_eq!(
            status!(&app, uri, &fx.staff_b),
            StatusCode::NOT_FOUND,
            "staff of another workspace: {route}"
        );
    }
    assert_eq!(
        status!(&app, &eml_uri, &fx.requester_a),
        StatusCode::NOT_FOUND,
        "the raw mail follows its ticket's visibility"
    );

    for uri in [
        format!("/api/files/assets/2147483000/media/{photo}"),
        "/api/comments/2147483000/raw.eml".to_string(),
        format!("/api/plugins/{}/icon", Uuid::new_v4()),
    ] {
        assert_eq!(
            status!(&app, &uri, &fx.staff_a),
            StatusCode::NOT_FOUND,
            "unknown: {uri}"
        );
    }
}

#[actix_web::test]
async fn exports_and_backups_download_without_the_selection_header() {
    let fx = Fixture::new();

    // The export is Owner-only.
    let owner_uuid = common::insert_plain_user(&mut fx.pool.get().expect("conn"), "A Owner");
    let artifact = format!("exports/{}.nosdesk", Uuid::new_v4());
    let export_id = run_in_workspace(&fx.pool, "test:seed", fx.ws_a, |c| {
        add_membership(
            c,
            fx.ws_a,
            owner_uuid,
            "owner",
            SeatWriteAuthority::ControlPlane,
        )?;
        let job = backend::repository::workspace_export_jobs::create(
            c,
            NewWorkspaceExportJob {
                workspace_id: fx.ws_a,
                requested_by: Some(owner_uuid),
                status: "completed".to_string(),
            },
        )?;
        let now = Utc::now().naive_utc();
        backend::repository::workspace_export_jobs::update(
            c,
            job.id,
            WorkspaceExportJobUpdate {
                file_path: Some(artifact.clone()),
                completed_at: Some(now),
                expires_at: Some(now + Duration::days(1)),
                ..Default::default()
            },
        )
        .map(|job| job.id)
    })
    .expect("seed export");
    fx.put(&artifact, b"archive").await;
    let owner_a = session_token(&fx.pool, owner_uuid);

    // Backups cover the whole instance and belong to platform admins.
    let platform_admin = common::insert_user(&mut fx.pool.get().expect("conn"), "Operator");
    let backup_file = tempfile::NamedTempFile::new().expect("backup file");
    std::fs::write(backup_file.path(), b"backup").expect("write backup");
    let backup_id = {
        let mut conn = fx.pool.get().expect("conn");
        let id = common::seed_backup_job(&mut conn);
        backend::repository::backup::update_backup_job(
            &mut conn,
            id,
            BackupJobUpdate {
                status: Some("completed".to_string()),
                file_path: Some(backup_file.path().to_string_lossy().into_owned()),
                file_size: Some(6),
                error_message: None,
                completed_at: Some(Utc::now().naive_utc()),
            },
        )
        .expect("complete backup job");
        id
    };
    let operator = session_token(&fx.pool, platform_admin.uuid);

    let app = app!(fx.pool);

    // The harness really is in selection mode: a route that still needs the
    // header refuses the same request without it.
    assert_eq!(
        status!(
            &app,
            &format!("/api/workspace/export/{export_id}"),
            &owner_a
        ),
        StatusCode::BAD_REQUEST,
        "the export status poll needs the header"
    );

    let export_uri = format!("/api/workspace/export/{export_id}/download");
    let resp = get!(&app, &export_uri, &owner_a);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header(&resp, "cache-control").as_deref(), Some("no-store"));
    assert_eq!(body(resp).await, b"archive");
    assert_eq!(
        status!(&app, &export_uri, &fx.staff_a),
        StatusCode::NOT_FOUND,
        "an admin of the workspace who isn't its owner"
    );
    assert_eq!(
        status!(&app, &export_uri, &fx.staff_b),
        StatusCode::NOT_FOUND,
        "staff of another workspace"
    );
    assert_eq!(
        status!(
            &app,
            &format!("/api/workspace/export/{}/download", Uuid::new_v4()),
            &owner_a
        ),
        StatusCode::NOT_FOUND,
        "unknown export"
    );

    let backup_uri = format!("/api/admin/backup/download/{backup_id}");
    let resp = get!(&app, &backup_uri, &operator);
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header(&resp, "cache-control").as_deref(), Some("no-store"));
    assert_eq!(body(resp).await, b"backup");
    assert_eq!(
        status!(&app, &backup_uri, &owner_a),
        StatusCode::FORBIDDEN,
        "backups are for platform admins only"
    );
    assert_eq!(
        status!(
            &app,
            &format!("/api/admin/backup/download/{}", Uuid::new_v4()),
            &operator
        ),
        StatusCode::NOT_FOUND,
        "unknown backup"
    );
}
