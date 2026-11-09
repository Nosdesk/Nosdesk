//! The upload paths beside `/upload` read through the upload intake: each
//! keeps its own cap and type rule and stores what it stored before, and the
//! spool directory is empty once a request is answered, stored or refused.
//! Backup restore spools under `NOSDESK_MAX_IMPORT_BYTES` and leaves no
//! partial file behind.
//!
//! Each server gets its own spool area (`web::Data<SpoolArea>`), so a test
//! watches only its own directory.

#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use image::{ImageFormat, Rgba, RgbaImage};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::{PortalContext, PortalScope};
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, NewUser, Ticket, User};
use backend::services::search::SearchService;
use backend::services::upload_intake::SpoolArea;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool};

const WS: i32 = 1;
const REF: &str = "test:upload_intake_paths";
const MIB: usize = 1024 * 1024;
/// The restore cap the server is given, so a file past it stays small.
const RESTORE_CAP: u64 = 64 * 1024;

struct Fixture {
    _db: common::TestDb,
    pool: TestPool,
    storage_dir: tempfile::TempDir,
    storage: Arc<dyn Storage>,
    spool_dir: tempfile::TempDir,
    admin: User,
    ticket: Ticket,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let admin = admin(&mut pool.get().expect("conn"));
        let ticket = run_in_workspace(&pool, REF, WS, |c| {
            let state = backend::repository::workflow_states::default_state(c)?;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: "Printer jams".to_string(),
                    workflow_state_id: state.id,
                    requester_uuid: Some(admin.uuid),
                    ..Default::default()
                })
                .get_result(c)
        })
        .expect("insert ticket");
        let storage_dir = tempfile::tempdir().expect("storage dir");
        let storage = create_storage(StorageConfig::Local {
            base_path: storage_dir.path().to_string_lossy().into_owned(),
        });
        Self {
            _db: db,
            pool,
            storage_dir,
            storage,
            spool_dir: tempfile::tempdir().expect("spool dir"),
            admin,
            ticket,
        }
    }

    /// Every moved route, run as the workspace admin (a platform admin too,
    /// for backup restore), with its own spool area.
    fn spawn(&self, storage: Arc<dyn Storage>) -> actix_test::TestServer {
        self.spawn_with_budget(storage, 256 * MIB as u64)
    }

    /// [`Fixture::spawn`] with a spool budget of `budget` bytes.
    fn spawn_with_budget(&self, storage: Arc<dyn Storage>, budget: u64) -> actix_test::TestServer {
        let pool = self.pool.clone();
        let user = self.admin.clone();
        let spool = web::Data::new(SpoolArea::new(self.spool_dir.path().to_path_buf(), budget));
        let claims = Claims {
            sub: user.uuid.to_string(),
            name: user.name.clone(),
            email: String::new(),
            platform_role: "platform_admin".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        actix_test::start(move || {
            let tmp = tempfile::tempdir().expect("search dir");
            let search = Arc::new(SearchService::new(tmp.path(), &pool).expect("init search"));
            std::mem::forget(tmp);
            let claims = claims.clone();
            let corr = Uuid::now_v7();
            let actor = ActorContext::user(user.uuid, Some(corr)).with_workspace(WS);
            let ws = WorkspaceContext {
                workspace_id: WS,
                workspace_uuid: Uuid::nil(),
                slug: "default".to_string(),
                name: "Default".to_string(),
                organisation_id: None,
                custom_domain: None,
            };
            let portal = PortalContext {
                user_uuid: user.uuid,
                workspace_id: WS,
                workspace_uuid: Uuid::nil(),
                scope: PortalScope::Full,
            };
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(storage.clone()))
                .app_data(web::Data::new(search))
                .app_data(spool.clone())
                .app_data(web::Data::new(
                    backend::services::workspace_import_jobs::ImportUploadCap(RESTORE_CAP),
                ))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(ws.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .route(
                    "/documents/{doc_id}/images",
                    web::post().to(backend::handlers::collab_images::upload_collab_document_image),
                )
                .route(
                    "/assets/{id}/media",
                    web::post().to(backend::handlers::asset_media::upload_for_asset),
                )
                .route(
                    "/users/{uuid}/image",
                    web::post().to(backend::handlers::users::upload_user_image),
                )
                .route(
                    "/admin/branding/image",
                    web::post().to(backend::handlers::branding::upload_branding_image),
                )
                .route(
                    "/public/files/temp",
                    web::post().to(backend::handlers::guest::upload_guest_attachment),
                )
                .route(
                    "/portal/files",
                    web::post().to(backend::handlers::portal::upload_file),
                )
                .route(
                    "/admin/import",
                    web::post().to(backend::handlers::imports::upload),
                )
                .route(
                    "/admin/backup/restore/upload",
                    web::post().to(backend::handlers::backup::upload_restore),
                )
                .route(
                    "/admin/workspaces/import",
                    web::post().to(backend::handlers::workspace_export::import_workspace),
                )
        })
    }

    fn on_disk(&self, logical: &str) -> PathBuf {
        self.storage_dir
            .path()
            .join(format!("ws/{WS}"))
            .join(logical)
    }

    fn assert_spool_empty(&self, after: &str) {
        let left: Vec<_> = std::fs::read_dir(self.spool_dir.path())
            .map(|d| d.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "spool files left after {after}: {left:?}");
    }
}

fn admin(conn: &mut PgConnection) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Admin".to_string(),
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
            workspace_members::role.eq("admin"),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

/// A one-part multipart body.
fn body(field: &str, filename: &str, content_type: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "--x\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n--x--\r\n");
    out
}

async fn post(srv: &actix_test::TestServer, path: &str, body: Vec<u8>) -> (u16, serde_json::Value) {
    let mut resp = awc::Client::new()
        .post(srv.url(path))
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .send_body(body)
        .await
        .expect("send");
    let status = resp.status().as_u16();
    let json = resp
        .json::<serde_json::Value>()
        .limit(1 << 20)
        .await
        .unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// Send only the headers of a request whose `Content-Length` is `length`:
/// the status line the server answers with before any of the body.
fn status_for_headers_only(srv: &actix_test::TestServer, path: &str, length: u64) -> String {
    let mut tcp = std::net::TcpStream::connect(srv.addr()).expect("connect");
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("timeout");
    write!(
        tcp,
        "POST {path} HTTP/1.1\r\nHost: localhost\r\n\
         Content-Type: multipart/form-data; boundary=x\r\n\
         Content-Length: {length}\r\n\r\n"
    )
    .expect("headers");
    let mut head = [0u8; 12];
    tcp.read_exact(&mut head)
        .expect("an answer without the body");
    String::from_utf8_lossy(&head).into_owned()
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes))
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([0x30, 0x60, 0x90, 255]));
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .expect("encode");
    out
}

/// A PNG padded past `len` bytes: it still sniffs as a PNG.
fn png_of_len(len: usize) -> Vec<u8> {
    let mut out = png(2, 2);
    out.resize(len, 0);
    out
}

/// `{folder}/{uuid7}_{name}`, the key `store_file` has always made.
fn assert_key(path: &str, folder: &str, name: &str) {
    let rest = path
        .strip_prefix(&format!("{folder}/"))
        .unwrap_or_else(|| panic!("{path} under {folder}"));
    let id = rest
        .strip_suffix(&format!("_{name}"))
        .unwrap_or_else(|| panic!("{path} ends _{name}"));
    assert_eq!(Uuid::parse_str(id).expect("a uuid").get_version_num(), 7);
}

#[actix_web::test]
async fn a_documentation_image_keeps_its_folder_cap_and_image_rule() {
    let fx = Fixture::new();
    let srv = fx.spawn(fx.storage.clone());
    let doc_id = format!("ws-{}_ticket-{}", Uuid::nil(), fx.ticket.uuid);
    let route = format!("/documents/{doc_id}/images");
    let image = png(4, 4);

    let (status, json) = post(
        &srv,
        &route,
        body("files", "Paste (1).png", "image/png", &image),
    )
    .await;
    assert_eq!(status, 200, "{json}");
    assert_eq!(json[0]["name"], "Paste (1).png");
    assert_eq!(json[0]["size"], image.len());
    let folder = format!("collab/ticket/{}", fx.ticket.uuid);
    let key = json[0]["url"]
        .as_str()
        .and_then(|u| u.strip_prefix("/api/files/"))
        .expect("a collab url");
    assert_key(key, &folder, "Paste1.png");
    assert_eq!(std::fs::read(fx.on_disk(key)).expect("stored"), image);
    fx.assert_spool_empty("a documentation image");

    let (status, json) = post(
        &srv,
        &route,
        body("files", "a.pdf", "application/pdf", b"%PDF-1.7\n"),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json["error"], "Only image files are allowed");

    let (status, json) = post(
        &srv,
        &route,
        body("files", "big.png", "image/png", &png_of_len(10 * MIB + 1)),
    )
    .await;
    assert_eq!(status, 413, "{json}");
    assert_eq!(json["code"], "FILE_TOO_LARGE");
    assert_eq!(json["file"], "big.png");
    fx.assert_spool_empty("refused documentation images");

    assert_eq!(
        status_for_headers_only(&srv, &route, 200 * MIB as u64),
        "HTTP/1.1 413"
    );
}

#[actix_web::test]
async fn asset_media_keeps_its_caps_types_and_rows() {
    let fx = Fixture::new();
    let srv = fx.spawn(fx.storage.clone());
    let asset_id = run_in_workspace(&fx.pool, REF, WS, |c| {
        Ok(common::insert_stock_asset(c, "Forklift"))
    })
    .expect("asset");
    let route = format!("/assets/{asset_id}/media");

    let pdf = b"%PDF-1.7\n%service report\n".repeat(100);
    let (status, json) = post(
        &srv,
        &route,
        body("files", "Service Report.pdf", "application/pdf", &pdf),
    )
    .await;
    assert_eq!(status, 201, "{json}");
    let row = &json[0];
    assert_eq!(row["kind"], "document");
    assert_eq!(row["mime_type"], "application/pdf");
    assert_eq!(row["name"], "Service Report.pdf");
    assert_eq!(row["file_size"], pdf.len());
    assert_eq!(row["checksum"], sha256(&pdf));
    assert!(row["thumbnail_url"].is_null());
    let key = row["url"]
        .as_str()
        .and_then(|u| u.strip_prefix("/api/files/"))
        .expect("an asset url");
    assert_key(
        key,
        &format!("assets/{asset_id}/media"),
        "ServiceReport.pdf",
    );
    assert_eq!(std::fs::read(fx.on_disk(key)).expect("stored"), pdf);

    let photo = png(32, 32);
    let (status, json) = post(
        &srv,
        &route,
        body("files", "front.png", "image/png", &photo),
    )
    .await;
    assert_eq!(status, 201, "{json}");
    assert_eq!(json[0]["kind"], "photo");
    assert!(json[0]["thumbnail_url"].is_string(), "{json}");
    fx.assert_spool_empty("asset media");

    // Images stay at 10 MB; documents at 25 MB.
    let (status, json) = post(
        &srv,
        &route,
        body("files", "huge.png", "image/png", &png_of_len(10 * MIB + 1)),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json["error"], "Image too large (max 10MB)");
    let mut big = b"%PDF-1.7\n".to_vec();
    big.resize(25 * MIB + 1, b' ');
    let (status, json) = post(
        &srv,
        &route,
        body("files", "big.pdf", "application/pdf", &big),
    )
    .await;
    assert_eq!(status, 413, "{json}");
    assert_eq!(json["code"], "FILE_TOO_LARGE");
    let (status, _) = post(
        &srv,
        &route,
        body("files", "song.mp3", "audio/mpeg", b"ID3\x04\0\0\0\0\0\0"),
    )
    .await;
    assert_eq!(status, 400, "an unlisted type is refused");
    fx.assert_spool_empty("refused asset media");
}

#[actix_web::test]
async fn a_profile_image_is_held_to_its_own_cap_and_types() {
    let fx = Fixture::new();
    let srv = fx.spawn(common::test_storage());
    let route = format!("/users/{}/image?type_=avatar", fx.admin.uuid);

    let (status, json) = post(
        &srv,
        &route,
        body("file", "me.png", "image/png", &png(64, 64)),
    )
    .await;
    assert_eq!(status, 200, "{json}");
    let url = json["url"].as_str().expect("an avatar url");
    assert!(
        url.ends_with(&format!("users/avatars/{}_avatar.webp", fx.admin.uuid)),
        "{url}"
    );
    assert_eq!(json["user"]["avatar_url"], url);
    fx.assert_spool_empty("an avatar");

    // A BMP is an image, but not one the profile decoder takes.
    let mut bmp = b"BM".to_vec();
    bmp.resize(64, 0);
    let (status, json) = post(&srv, &route, body("file", "me.png", "image/png", &bmp)).await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(
        json["error"],
        "Unsupported image format. Allowed: JPEG, PNG, GIF, WEBP"
    );

    // 10 MB, well under the attachment limit.
    let (status, json) = post(
        &srv,
        &format!("/users/{}/image?type_=banner", fx.admin.uuid),
        body("file", "wide.png", "image/png", &png_of_len(10 * MIB + 1)),
    )
    .await;
    assert_eq!(status, 413, "{json}");
    assert_eq!(json["code"], "FILE_TOO_LARGE");
    fx.assert_spool_empty("refused profile images");

    assert_eq!(
        status_for_headers_only(&srv, &route, 40 * MIB as u64),
        "HTTP/1.1 413"
    );
}

#[actix_web::test]
async fn a_branding_image_keeps_its_2_mb_cap_and_message() {
    let fx = Fixture::new();
    let srv = fx.spawn(fx.storage.clone());

    let logo = png(192, 48);
    let (status, json) = post(
        &srv,
        "/admin/branding/image?type=favicon",
        body("file", "favicon.png", "image/png", &logo),
    )
    .await;
    assert_eq!(status, 200, "{json}");
    let url = json["url"].as_str().expect("a branding url");
    assert!(
        url.starts_with(&format!("/uploads/branding/{}/favicon.png?v=", Uuid::nil())),
        "{url}"
    );
    assert_eq!(
        std::fs::read(fx.on_disk("branding/favicon.png")).expect("stored"),
        logo
    );
    fx.assert_spool_empty("a favicon");

    let (status, json) = post(
        &srv,
        "/admin/branding/image?type=logo",
        body("file", "logo.png", "image/png", &png_of_len(2 * MIB + 1)),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json["error"], "File too large. Maximum size is 2MB");
    let (status, json) = post(
        &srv,
        "/admin/branding/image?type=logo",
        body("file", "logo.png", "image/png", b"not a png"),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(
        json["error"],
        "Invalid file type for logo. Allowed: PNG, JPEG, or WebP"
    );
    fx.assert_spool_empty("refused branding images");
}

#[actix_web::test]
async fn a_guest_and_a_portal_upload_keep_the_guest_rules() {
    let fx = Fixture::new();
    run_in_workspace(&fx.pool, REF, WS, |c| {
        diesel::sql_query(
            "UPDATE site_settings SET guest_tickets_enabled = true, \
             guest_ticket_attachments_enabled = true WHERE workspace_id = 1",
        )
        .execute(c)
    })
    .expect("enable guest attachments");
    let srv = fx.spawn(fx.storage.clone());

    let log = b"printer error 4471\n".repeat(50);
    for route in ["/public/files/temp", "/portal/files"] {
        let (status, json) = post(
            &srv,
            route,
            body("file", "Printer log.txt", "text/plain", &log),
        )
        .await;
        assert_eq!(status, 201, "{route}: {json}");
        let id = json["id"].as_i64().expect("an attachment id") as i32;
        let row: backend::models::Attachment = run_in_workspace(&fx.pool, REF, WS, |c| {
            backend::schema::attachments::table.find(id).first(c)
        })
        .expect("attachment row");
        assert_eq!(row.name, "Printer log.txt");
        assert_eq!(row.mime_type.as_deref(), Some("text/plain"));
        assert_eq!(row.file_size, Some(log.len() as i64));
        assert_eq!(row.checksum.as_deref(), Some(sha256(&log).as_str()));
        let key = row.url.strip_prefix("/uploads/").expect("an uploads url");
        assert_key(key, "temp", "Printerlog.txt");
        assert_eq!(std::fs::read(fx.on_disk(key)).expect("stored"), log);
        fx.assert_spool_empty(route);

        let mut big = b"x".repeat(10 * MIB + 1);
        big[0] = b'a';
        let (status, json) = post(&srv, route, body("file", "big.txt", "text/plain", &big)).await;
        assert_eq!(status, 413, "{route}: {json}");
        assert_eq!(json["error"], "File exceeds 10MB limit");
        let (status, json) = post(
            &srv,
            route,
            body("file", "page.html", "text/html", b"<html>"),
        )
        .await;
        assert_eq!(status, 400, "{route}: {json}");
        fx.assert_spool_empty(route);
        assert_eq!(
            status_for_headers_only(&srv, route, 20 * MIB as u64),
            "HTTP/1.1 413"
        );
    }
}

#[actix_web::test]
async fn a_spreadsheet_import_keeps_its_cap_and_refusal() {
    let fx = Fixture::new();
    let srv = fx.spawn(fx.storage.clone());
    let route = "/admin/import?type=assets";

    let csv = b"name,kind\nForklift,generic\n".to_vec();
    let (status, json) = post(&srv, route, body("file", "assets.csv", "text/csv", &csv)).await;
    assert_eq!(status, 200, "{json}");
    assert_eq!(json["status"], "parsed", "{json}");
    assert_eq!(json["filename"], "assets.csv", "{json}");
    let job = json["id"].as_str().expect("a job id");
    assert_eq!(
        std::fs::read(fx.on_disk(&format!("imports/{job}/upload.csv"))).expect("stored"),
        csv
    );
    fx.assert_spool_empty("an import");

    let (status, json) = post(
        &srv,
        route,
        body(
            "file",
            "big.csv",
            "text/csv",
            &b"a,b\n".repeat(10 * MIB / 4 + 1),
        ),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json["code"], "import_file_too_large");
    assert_eq!(json["args"]["max"], 10);
    fx.assert_spool_empty("a refused import");
}

/// Every path spools into the request's spool area: with its budget spent,
/// each answers 503 with Retry-After and leaves nothing behind.
#[actix_web::test]
async fn each_path_spools_under_the_budget() {
    let fx = Fixture::new();
    run_in_workspace(&fx.pool, REF, WS, |c| {
        diesel::sql_query(
            "UPDATE site_settings SET guest_tickets_enabled = true, \
             guest_ticket_attachments_enabled = true WHERE workspace_id = 1",
        )
        .execute(c)
    })
    .expect("enable guest attachments");
    let asset_id = run_in_workspace(&fx.pool, REF, WS, |c| {
        Ok(common::insert_stock_asset(c, "Forklift"))
    })
    .expect("asset");
    let srv = fx.spawn_with_budget(common::test_storage(), 1024);
    let image = png_of_len(4096);
    let cases = [
        (
            format!(
                "/documents/ws-{}_ticket-{}/images",
                Uuid::nil(),
                fx.ticket.uuid
            ),
            "files",
        ),
        (format!("/assets/{asset_id}/media"), "files"),
        (
            format!("/users/{}/image?type_=avatar", fx.admin.uuid),
            "file",
        ),
        ("/admin/branding/image?type=logo".to_string(), "file"),
        ("/public/files/temp".to_string(), "file"),
        ("/portal/files".to_string(), "file"),
        ("/admin/import?type=assets".to_string(), "file"),
    ];
    for (route, field) in cases {
        let mut resp = awc::Client::new()
            .post(srv.url(&route))
            .insert_header(("content-type", "multipart/form-data; boundary=x"))
            .send_body(body(field, "pic.png", "image/png", &image))
            .await
            .expect("send");
        let json: serde_json::Value = resp.json().await.unwrap_or_default();
        assert_eq!(resp.status().as_u16(), 503, "{route}: {json}");
        assert!(resp.headers().contains_key("retry-after"), "{route}");
        fx.assert_spool_empty(&route);
    }
}

/// The files a restore upload left in its folder.
fn restore_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[actix_web::test]
async fn a_restore_upload_is_capped_and_leaves_nothing_when_refused_or_cut_off() {
    let fx = Fixture::new();
    common::with_upload_dir();
    let dir = backend::services::backup::restore_uploads_dir();
    let srv = fx.spawn(fx.storage.clone());
    let route = "/admin/backup/restore/upload";
    let before = restore_files(&dir);

    // A backup under the cap is kept under its name, whole.
    let name = format!("restore-test-{}.zip", Uuid::new_v4());
    let backup = b"PK\x03\x04sealed".repeat(1000);
    let (status, json) = post(&srv, route, body("file", &name, "application/zip", &backup)).await;
    assert_eq!(status, 201, "{json}");
    assert_eq!(json["job_type"], "restore");
    assert_eq!(json["file_size"], backup.len());
    let kept = dir.join(&name);
    assert_eq!(json["file_path"], kept.to_string_lossy().as_ref());
    assert_eq!(std::fs::read(&kept).expect("kept"), backup);
    // The partial sweep never takes a kept upload, which the pending job
    // names by path, however old.
    backend::services::backup::sweep_restore_partials(&dir, std::time::Duration::ZERO)
        .expect("sweep");
    assert!(kept.exists(), "the pending restore's file stays");
    std::fs::remove_file(&kept).expect("remove");

    // One past the cap is refused, and its partial file goes.
    let over = vec![b'z'; RESTORE_CAP as usize + 1];
    let (status, json) = post(
        &srv,
        route,
        body("file", "over.zip", "application/zip", &over),
    )
    .await;
    assert_eq!(status, 413, "{json}");
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|e| e.contains("NOSDESK_MAX_IMPORT_BYTES")),
        "{json}"
    );
    assert_eq!(restore_files(&dir), before, "nothing kept past the cap");

    // A Content-Length plainly over the cap is refused unread.
    assert_eq!(
        status_for_headers_only(&srv, route, RESTORE_CAP + 2 * MIB as u64),
        "HTTP/1.1 413"
    );

    // An upload cut off part way leaves no partial file.
    let mut tcp = std::net::TcpStream::connect(srv.addr()).expect("connect");
    let part = body("file", "cut.zip", "application/zip", &[b'c'; 32 * 1024]);
    write!(
        tcp,
        "POST {route} HTTP/1.1\r\nHost: localhost\r\n\
         Content-Type: multipart/form-data; boundary=x\r\n\
         Content-Length: {}\r\n\r\n",
        part.len() + 16 * 1024
    )
    .expect("headers");
    tcp.write_all(&part[..part.len() - 64])
        .expect("part of the body");
    drop(tcp);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while restore_files(&dir) != before && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        restore_files(&dir),
        before,
        "the cut-off upload left a file"
    );
}

/// The platform-admin import reads `NOSDESK_MAX_IMPORT_BYTES` as the import
/// jobs and backup restore do, so the server's cap applies to it too.
#[actix_web::test]
async fn the_platform_admin_import_takes_the_shared_import_cap() {
    let fx = Fixture::new();
    let srv = fx.spawn(fx.storage.clone());
    let archive = vec![b'a'; RESTORE_CAP as usize + 1];
    let (status, json) = post(
        &srv,
        "/admin/workspaces/import",
        body(
            "archive",
            "ws.nosdesk",
            "application/octet-stream",
            &archive,
        ),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(
        json["error"],
        format!("archive exceeds the {RESTORE_CAP}-byte limit (raise NOSDESK_MAX_IMPORT_BYTES)")
    );
}
