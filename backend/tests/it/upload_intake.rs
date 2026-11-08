//! `/upload` and ticket note images through the upload intake: each file is
//! spooled to disk and stored from there, under the same `{uuid7}_{name}`
//! key and with the same attachment row as when it was read into memory,
//! and each path keeps its own cap and type rule.

#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use async_trait::async_trait;
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, NewUser, Ticket, User};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig, StorageError, StoredFile};

use crate::common::{self, TestPool};

const WS: i32 = 1;
const REF: &str = "test:upload_intake";

/// Local storage that counts how files reach it: from bytes in memory, or
/// from a file on disk.
struct Recording {
    inner: Arc<dyn Storage>,
    from_bytes: AtomicUsize,
    from_path: AtomicUsize,
}

#[async_trait]
impl Storage for Recording {
    async fn store_file(
        &self,
        data: &[u8],
        filename: &str,
        content_type: &str,
        folder: &str,
    ) -> Result<StoredFile, StorageError> {
        self.from_bytes.fetch_add(1, Ordering::SeqCst);
        self.inner
            .store_file(data, filename, content_type, folder)
            .await
    }
    async fn put_file(
        &self,
        data: &[u8],
        path: &str,
        content_type: &str,
    ) -> Result<StoredFile, StorageError> {
        self.from_bytes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_file(data, path, content_type).await
    }
    async fn put_file_from_path(
        &self,
        local: &Path,
        path: &str,
        content_type: &str,
    ) -> Result<StoredFile, StorageError> {
        self.from_path.fetch_add(1, Ordering::SeqCst);
        self.inner
            .put_file_from_path(local, path, content_type)
            .await
    }
    async fn get_file(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        self.inner.get_file(path).await
    }
    async fn delete_file(&self, path: &str) -> Result<(), StorageError> {
        self.inner.delete_file(path).await
    }
    async fn file_exists(&self, path: &str) -> Result<bool, StorageError> {
        self.inner.file_exists(path).await
    }
    fn get_public_url(&self, path: &str) -> String {
        self.inner.get_public_url(path)
    }
    async fn move_file(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.move_file(from, to).await
    }
    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        self.inner.list_prefix(prefix).await
    }
}

struct Fixture {
    _db: common::TestDb,
    dir: tempfile::TempDir,
    pool: TestPool,
    storage: Arc<Recording>,
    agent: User,
    ticket: Ticket,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let agent = agent(&mut pool.get().expect("conn"));
        let ticket = run_in_workspace(&pool, REF, WS, |c| {
            let state = backend::repository::workflow_states::default_state(c)?;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: "Printer jams".to_string(),
                    workflow_state_id: state.id,
                    requester_uuid: Some(agent.uuid),
                    ..Default::default()
                })
                .get_result(c)
        })
        .expect("insert ticket");
        let dir = tempfile::tempdir().expect("storage dir");
        let storage = Arc::new(Recording {
            inner: create_storage(StorageConfig::Local {
                base_path: dir.path().to_string_lossy().into_owned(),
            }),
            from_bytes: AtomicUsize::new(0),
            from_path: AtomicUsize::new(0),
        });
        Self {
            _db: db,
            dir,
            pool,
            storage,
            agent,
            ticket,
        }
    }

    fn spawn(&self) -> actix_test::TestServer {
        let pool = self.pool.clone();
        let storage: Arc<dyn Storage> = self.storage.clone();
        let user = self.agent.clone();
        let claims = Claims {
            sub: user.uuid.to_string(),
            name: user.name.clone(),
            email: String::new(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        actix_test::start(move || {
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
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .app_data(web::Data::new(storage.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(ws.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .route("/upload", web::post().to(backend::handlers::upload_files))
                .route(
                    "/tickets/{ticket_id}/notes/images",
                    web::post().to(backend::handlers::upload_ticket_note_image),
                )
        })
    }

    fn on_disk(&self, logical: &str) -> std::path::PathBuf {
        self.dir.path().join(format!("ws/{WS}")).join(logical)
    }

    fn attachments(&self) -> Vec<backend::models::Attachment> {
        use backend::schema::attachments::dsl;
        let uploader = self.agent.uuid;
        run_in_workspace(&self.pool, REF, WS, |c| {
            dsl::attachments
                .filter(dsl::uploaded_by.eq(uploader))
                .order(dsl::id)
                .load(c)
        })
        .expect("attachments")
    }
}

fn agent(conn: &mut PgConnection) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Tech".to_string(),
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
            workspace_members::role.eq("agent"),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

/// A multipart body: `(field, filename, bytes)` per part.
fn body(parts: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, filename, data) in parts {
        let disposition = match filename {
            Some(f) => format!("form-data; name=\"{name}\"; filename=\"{f}\""),
            None => format!("form-data; name=\"{name}\""),
        };
        out.extend_from_slice(
            format!("--x\r\nContent-Disposition: {disposition}\r\n\r\n").as_bytes(),
        );
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"--x--\r\n");
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

fn sha256(bytes: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes))
}

/// `{folder}/{uuid7}_{name}`, as `store_file` has always made it.
fn assert_key(path: &str, folder: &str, name: &str) {
    let rest = path
        .strip_prefix(&format!("{folder}/"))
        .unwrap_or_else(|| panic!("{path} under {folder}"));
    let id = rest
        .strip_suffix(&format!("_{name}"))
        .unwrap_or_else(|| panic!("{path} ends _{name}"));
    let id = Uuid::parse_str(id).expect("a uuid prefix");
    assert_eq!(id.get_version_num(), 7, "{path}");
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0";

#[actix_web::test]
async fn an_upload_stores_from_disk_under_the_same_key_and_row() {
    let fx = Fixture::new();
    let srv = fx.spawn();
    let notes = b"Printer serial 4471\n".repeat(1000);
    let (status, json) = post(
        &srv,
        "/upload",
        body(&[
            ("transcription", None, b"the printer is jammed"),
            ("files", Some("Printer log (May).txt"), &notes),
            ("files", Some("jam.png"), PNG),
        ]),
    )
    .await;
    assert_eq!(status, 200, "{json}");

    let rows = fx.attachments();
    assert_eq!(rows.len(), 2);
    for (row, (name, key_name, bytes, mime)) in rows.iter().zip([
        (
            "Printer log (May).txt",
            "PrinterlogMay.txt",
            notes.as_slice(),
            "application/octet-stream",
        ),
        ("jam.png", "jam.png", PNG, "image/png"),
    ]) {
        let key = row.url.strip_prefix("/uploads/").expect("an uploads url");
        assert_key(key, "temp", key_name);
        assert_eq!(row.name, name);
        assert_eq!(row.file_size, Some(bytes.len() as i64));
        assert_eq!(row.mime_type.as_deref(), Some(mime));
        assert_eq!(row.checksum.as_deref(), Some(sha256(bytes).as_str()));
        assert_eq!(row.comment_id, None);
        assert_eq!(row.transcription.as_deref(), Some("the printer is jammed"));
        assert_eq!(std::fs::read(fx.on_disk(key)).expect("stored"), bytes);
    }
    assert_eq!(json[0]["id"], rows[0].id);
    assert_eq!(json[0]["url"], rows[0].url.as_str());
    assert_eq!(json[0]["name"], "Printer log (May).txt");
    assert_eq!(json[1]["thumbnail_url"], serde_json::Value::Null);

    assert_eq!(fx.storage.from_path.load(Ordering::SeqCst), 2);
    assert_eq!(
        fx.storage.from_bytes.load(Ordering::SeqCst),
        0,
        "nothing was stored from memory"
    );
}

#[actix_web::test]
async fn an_upload_one_byte_over_the_cap_is_refused() {
    let fx = Fixture::new();
    let srv = fx.spawn();
    let cap = backend::utils::file_validation::get_max_file_size();
    let big = vec![b'a'; cap + 1];
    let (status, json) = post(&srv, "/upload", body(&[("files", Some("huge.txt"), &big)])).await;
    assert_eq!(status, 413, "{json}");
    assert_eq!(json["code"], "FILE_TOO_LARGE");
    assert_eq!(json["file"], "huge.txt");
    assert!(fx.attachments().is_empty());

    let (status, json) = post(
        &srv,
        "/upload",
        body(&[("files", Some("setup.exe"), b"MZ")]),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json["code"], "FILE_TYPE_NOT_ALLOWED");
    assert_eq!(fx.storage.from_path.load(Ordering::SeqCst), 0);
}

#[actix_web::test]
async fn a_note_image_stores_beside_its_ticket_and_only_images_pass() {
    let fx = Fixture::new();
    let srv = fx.spawn();
    let route = format!("/tickets/{}/notes/images", fx.ticket.id);

    let (status, json) = post(&srv, &route, body(&[("files", Some("Jam (1).png"), PNG)])).await;
    assert_eq!(status, 200, "{json}");
    assert_eq!(json[0]["name"], "Jam (1).png");
    assert_eq!(json[0]["size"], PNG.len());
    let key = json[0]["url"]
        .as_str()
        .and_then(|u| u.strip_prefix("/uploads/"))
        .expect("an uploads url");
    assert_key(key, &format!("tickets/{}/notes", fx.ticket.id), "Jam1.png");
    assert_eq!(std::fs::read(fx.on_disk(key)).expect("stored"), PNG);
    assert_eq!(fx.storage.from_path.load(Ordering::SeqCst), 1);
    assert_eq!(fx.storage.from_bytes.load(Ordering::SeqCst), 0);

    let (status, json) = post(
        &srv,
        &route,
        body(&[("files", Some("scan.pdf"), b"%PDF-1.7\n")]),
    )
    .await;
    assert_eq!(status, 400, "{json}");
    assert_eq!(json["error"], "Only image files are allowed");

    let mut big = PNG.to_vec();
    big.resize(10 * 1024 * 1024 + 1, 0);
    let (status, json) = post(&srv, &route, body(&[("files", Some("big.png"), &big)])).await;
    assert_eq!(status, 413, "{json}");
    assert_eq!(json["code"], "FILE_TOO_LARGE");

    // A Content-Length past ten images at the cap is refused before the
    // body: the server answers while the client has sent only headers.
    let mut tcp = std::net::TcpStream::connect(srv.addr()).expect("connect");
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("timeout");
    write!(
        tcp,
        "POST {route} HTTP/1.1\r\nHost: localhost\r\n\
         Content-Type: multipart/form-data; boundary=x\r\n\
         Content-Length: {}\r\n\r\n",
        200 * 1024 * 1024
    )
    .expect("headers");
    let mut head = [0u8; 12];
    tcp.read_exact(&mut head)
        .expect("an answer without the body");
    assert_eq!(&head, b"HTTP/1.1 413");
}
