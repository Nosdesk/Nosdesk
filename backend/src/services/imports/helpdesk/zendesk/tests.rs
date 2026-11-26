//! The connector against a fake Zendesk on 127.0.0.1. Requests go through
//! the production client path (safe_http with the egress allowlist): the
//! mock's host is allowlisted, exactly as an operator would allowlist a
//! private stand-in.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::client::RetryPolicy;
use super::*;
use crate::services::imports::helpdesk::bundle::{BodyFormat, PersonRole, StatusKind};

/// The allowlist is a process-global env var, and other tests in the crate
/// (utils::egress, utils::safe_http) assert 127.0.0.1 is refused while it
/// is unset. So a test sets it only for as long as it takes to build a
/// source (which trusts its base host from then on), under this lock, and
/// puts it back.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs `f` with 127.0.0.1 allowlisted.
fn with_local_allowlisted<T>(f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("NOSDESK_OUTBOUND_ALLOWED_HOSTS").ok();
    std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", "127.0.0.1");
    let out = f();
    match previous {
        Some(v) => std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", v),
        None => std::env::remove_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS"),
    }
    out
}

#[derive(Debug, Clone)]
struct Req {
    path: String,
    query: HashMap<String, String>,
    auth: Option<String>,
}

struct Resp {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    /// A Content-Length longer than the body: the connection drops short.
    declared_len: Option<usize>,
}

fn json_resp(v: Value) -> Resp {
    Resp {
        status: 200,
        headers: vec![("Content-Type", "application/json".into())],
        body: v.to_string().into_bytes(),
        declared_len: None,
    }
}

fn status(code: u16) -> Resp {
    Resp {
        status: code,
        headers: vec![],
        body: b"{}".to_vec(),
        declared_len: None,
    }
}

type Handler = Arc<dyn Fn(&Req) -> Resp + Send + Sync>;

/// A minimal HTTP/1.1 server: one GET per connection.
struct MockServer {
    port: u16,
    log: Arc<Mutex<Vec<Req>>>,
}

impl MockServer {
    async fn start(handler: Handler) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_in = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let handler = handler.clone();
                let log = log_in.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let text = String::from_utf8_lossy(&buf).to_string();
                    let mut lines = text.split("\r\n");
                    let target = lines
                        .next()
                        .and_then(|l| l.split(' ').nth(1))
                        .unwrap_or("/")
                        .to_string();
                    let auth = lines.take_while(|l| !l.is_empty()).find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("authorization")
                            .then(|| v.trim().to_string())
                    });
                    let url = url::Url::parse(&format!("http://mock{target}")).expect("target");
                    let req = Req {
                        path: url.path().to_string(),
                        query: url.query_pairs().into_owned().collect(),
                        auth,
                    };
                    log.lock().expect("log").push(req.clone());
                    let resp = handler(&req);
                    let mut head = format!(
                        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                        resp.status,
                        resp.declared_len.unwrap_or(resp.body.len())
                    );
                    for (k, v) in &resp.headers {
                        head.push_str(&format!("{k}: {v}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&resp.body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        MockServer { port, log }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self, path: &str) -> Vec<Req> {
        self.log
            .lock()
            .expect("log")
            .iter()
            .filter(|r| r.path == path)
            .cloned()
            .collect()
    }
}

/// Switches for the misbehaviour a test wants from the fake account.
#[derive(Default)]
struct Twists {
    throttle_users_once: AtomicBool,
    fields_503_once: AtomicBool,
    /// 401 on ticket 103's comments, i.e. midway through ticket page two.
    deny_ticket_103: AtomicBool,
    deny_everything: AtomicBool,
    users_calls: AtomicU32,
    /// The first download of 5001 breaks off after two bytes.
    break_5001_once: AtomicBool,
    /// User 4 is created while the tickets are fetched; ticket 103 names
    /// them.
    late_user: AtomicBool,
}

/// A small Zendesk account: two orgs and three users over two pages each,
/// tickets over two incremental pages (one repeated, one deleted),
/// comments over two pages, and attachments on and off the Zendesk host.
fn account(api_base: &str, files_base: &str, twists: &Twists, req: &Req) -> Resp {
    let q = |k: &str| req.query.get(k).map(String::as_str);
    if twists.deny_everything.load(Ordering::SeqCst) {
        return status(401);
    }
    match req.path.as_str() {
        "/api/v2/organizations.json" => match q("page[after]") {
            None => json_resp(json!({
                "organizations": [{"id": 900, "name": "Acme"}],
                "meta": {"has_more": true, "after_cursor": "o2"}
            })),
            Some("o2") => json_resp(json!({
                "organizations": [{"id": 901, "name": "Beta"}],
                "meta": {"has_more": false, "after_cursor": null}
            })),
            _ => status(400),
        },
        "/api/v2/incremental/users/cursor.json" => {
            twists.users_calls.fetch_add(1, Ordering::SeqCst);
            if twists.throttle_users_once.swap(false, Ordering::SeqCst) {
                return Resp {
                    status: 429,
                    headers: vec![("Retry-After", "1".into())],
                    body: b"{}".to_vec(),
                    declared_len: None,
                };
            }
            match (q("start_time"), q("cursor")) {
                (Some("0"), None) => json_resp(json!({
                    "users": [
                        {"id": 1, "name": "Ada Admin", "email": "ada@acme.test", "role": "admin", "active": true},
                        {"id": 2, "name": "Alan Agent", "email": "alan@acme.test", "role": "agent", "active": true}
                    ],
                    "after_cursor": "u2", "end_of_stream": false
                })),
                (None, Some("u2")) => json_resp(json!({
                    "users": [
                        {"id": 3, "name": "Cara", "email": "Cara@Example.COM", "role": "end-user",
                         "active": true, "suspended": true, "organization_id": 900},
                        {"id": 2, "name": "Alan Agent", "email": "alan@acme.test", "role": "agent", "active": true}
                    ],
                    "after_cursor": "u3", "end_of_stream": true
                })),
                // Asked again after the tickets: whoever came since.
                (None, Some("u3")) => {
                    let users = if twists.late_user.load(Ordering::SeqCst) {
                        json!([{"id": 4, "name": "Late Larry", "email": "larry@example.com",
                                "role": "end-user", "active": true}])
                    } else {
                        json!([])
                    };
                    json_resp(json!({"users": users, "after_cursor": "u4", "end_of_stream": true}))
                }
                (None, Some("u4")) => json_resp(json!({
                    "users": [], "after_cursor": "u4", "end_of_stream": true
                })),
                _ => status(400),
            }
        }
        "/api/v2/ticket_fields.json" => {
            if twists.fields_503_once.swap(false, Ordering::SeqCst) {
                return status(503);
            }
            json_resp(json!({
                "ticket_fields": [{"id": 77, "title": "Region"}],
                "meta": {"has_more": false}
            }))
        }
        "/api/v2/incremental/tickets/cursor.json" => match (q("start_time"), q("cursor")) {
            (Some("0"), None) if q("exclude_deleted") == Some("true") => json_resp(json!({
                "tickets": [ticket(101, "open", &[3, 2]), ticket(102, "hold", &[])],
                "after_cursor": "t2", "end_of_stream": false
            })),
            (None, Some("t2")) => {
                let mut solved = ticket(103, "solved", &[]);
                if twists.late_user.load(Ordering::SeqCst) {
                    solved["requester_id"] = json!(4);
                }
                json_resp(json!({
                    "tickets": [solved, ticket(102, "hold", &[]), ticket(104, "deleted", &[])],
                    "metric_sets": [{"ticket_id": 103, "solved_at": "2024-01-04T12:00:00Z"}],
                    "after_cursor": "t3", "end_of_stream": true
                }))
            }
            _ => status(400),
        },
        "/api/v2/tickets/101/comments.json" => match q("page[after]") {
            None => json_resp(json!({
                "comments": [comment(1001, "2024-01-01T10:00:00Z", true, json!([
                    {"id": 5001, "file_name": "on-host.txt", "content_type": "text/plain", "size": 5,
                     "content_url": format!("{api_base}/attachments/5001")}
                ]))],
                "meta": {"has_more": true, "after_cursor": "c2"}
            })),
            Some("c2") => json_resp(json!({
                "comments": [comment(1002, "2024-01-01T11:00:00Z", false, json!([
                    {"id": 5002, "file_name": "off-host.txt", "content_type": "text/plain", "size": 5,
                     "content_url": format!("{files_base}/files/5002")}
                ]))],
                "meta": {"has_more": false}
            })),
            _ => status(400),
        },
        "/api/v2/tickets/102/comments.json" => json_resp(json!({
            "comments": [comment(1003, "2024-01-02T10:00:00Z", true, json!([]))],
            "meta": {"has_more": false}
        })),
        "/api/v2/tickets/103/comments.json" => {
            if twists.deny_ticket_103.load(Ordering::SeqCst) {
                return status(401);
            }
            json_resp(json!({
                "comments": [comment(1004, "2024-01-03T10:00:00Z", true, json!([
                    {"id": 5003, "file_name": "gone.txt", "size": 3,
                     "content_url": format!("{api_base}/attachments/5003")}
                ]))],
                "meta": {"has_more": false}
            }))
        }
        "/attachments/5001" if twists.break_5001_once.swap(false, Ordering::SeqCst) => Resp {
            status: 200,
            headers: vec![],
            body: b"he".to_vec(),
            declared_len: Some(5),
        },
        "/attachments/5001" => Resp {
            status: 200,
            headers: vec![],
            body: b"hello".to_vec(),
            declared_len: None,
        },
        // A private attachment: only a signed request gets it.
        "/attachments/5009" if req.auth.is_none() => status(401),
        "/attachments/5009" => Resp {
            status: 200,
            headers: vec![],
            body: b"secret".to_vec(),
            declared_len: None,
        },
        _ => status(404),
    }
}

fn ticket(id: i64, status: &str, collaborators: &[i64]) -> Value {
    json!({
        "id": id, "subject": format!("Ticket {id}"), "status": status, "priority": "high",
        "type": "incident", "tags": ["t"], "requester_id": 3, "assignee_id": 2,
        "collaborator_ids": collaborators, "follower_ids": [], "email_cc_ids": [],
        "created_at": "2024-01-01T09:00:00Z", "updated_at": "2024-01-05T09:00:00Z",
        "custom_fields": [{"id": 77, "value": "emea"}, {"id": 78, "value": null}]
    })
}

fn comment(id: i64, at: &str, public: bool, attachments: Value) -> Value {
    json!({
        "id": id, "author_id": 3, "public": public, "created_at": at,
        "body": format!("plain {id}"), "plain_body": format!("plain {id}"),
        "html_body": format!("<p>comment {id}</p>"), "attachments": attachments
    })
}

struct Setup {
    api: MockServer,
    files: MockServer,
    twists: Arc<Twists>,
}

async fn setup() -> Setup {
    let twists = Arc::new(Twists::default());
    let files = MockServer::start(Arc::new(|req: &Req| match req.path.as_str() {
        "/files/5002" => Resp {
            status: 200,
            headers: vec![],
            body: b"world".to_vec(),
            declared_len: None,
        },
        _ => status(404),
    }))
    .await;
    // The account's content URLs need its own address, known only once it
    // is bound; the handler reads it from a cell filled just after.
    let api_base: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let api_base_in = api_base.clone();
    let files_base = files.base();
    let twists_in = twists.clone();
    let api = MockServer::start(Arc::new(move |req: &Req| {
        let base = api_base_in.lock().expect("base").clone();
        account(&base, &files_base, &twists_in, req)
    }))
    .await;
    *api_base.lock().expect("base") = api.base();
    Setup { api, files, twists }
}

fn source(base: &str, auth: ZendeskAuth) -> ZendeskSource {
    let mut s = with_local_allowlisted(|| ZendeskSource::with_base_url(base, auth))
        .expect("allowlisted base");
    s.http.policy = RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(50),
        max_throttle_waits: 5,
        max_retry_after: Duration::from_secs(2),
    };
    s
}

fn no_progress(_: &FetchProgress) {}

fn ticket_ids(b: &Bundle) -> Vec<String> {
    b.tickets()
        .expect("tickets")
        .map(|t| t.expect("ticket").source_id)
        .collect()
}

#[tokio::test]
async fn fetches_people_tickets_comments_and_attachments_across_pages() {
    let s = setup().await;
    s.twists.fields_503_once.store(true, Ordering::SeqCst);
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = source(
        &s.api.base(),
        ZendeskAuth::Basic {
            email: "admin@acme.test".into(),
            token: "tok".into(),
        },
    );

    let seen = Arc::new(Mutex::new(Vec::<FetchProgress>::new()));
    let seen_in = seen.clone();
    let progress = move |p: &FetchProgress| seen_in.lock().expect("progress").push(p.clone());
    let manifest = src
        .fetch(&bundle, &progress, CancellationToken::new())
        .await
        .expect("fetch");

    // People: deduplicated across pages, mapped.
    let people: Vec<_> = bundle
        .people()
        .expect("people")
        .map(|p| p.expect("person"))
        .collect();
    assert_eq!(people.len(), 3);
    let cara = people.iter().find(|p| p.source_id == "3").expect("cara");
    assert_eq!(cara.email.as_deref(), Some("cara@example.com"));
    assert_eq!(cara.role, PersonRole::Requester);
    assert_eq!(cara.organization.as_deref(), Some("Acme"));
    assert!(!cara.active);
    assert_eq!(
        people.iter().find(|p| p.source_id == "1").map(|p| p.role),
        Some(PersonRole::Admin)
    );

    // Tickets: both pages, the repeat and the deleted one left out.
    assert_eq!(ticket_ids(&bundle), vec!["101", "102", "103"]);
    let tickets: Vec<_> = bundle
        .tickets()
        .expect("tickets")
        .map(|t| t.expect("ticket"))
        .collect();
    let t101 = &tickets[0];
    assert_eq!(t101.number, Some(101));
    assert_eq!(t101.status, StatusKind::Open);
    assert_eq!(t101.watchers, vec!["2"], "requester is not a watcher");
    assert_eq!(t101.fields.get("Region").map(String::as_str), Some("emea"));
    assert_eq!(t101.comments.len(), 2, "both comment pages");
    assert_eq!(t101.comments[0].source_id, "1001");
    assert_eq!(t101.comments[0].body.format, BodyFormat::Html);
    assert!(!t101.comments[1].public);
    assert_eq!(tickets[1].status, StatusKind::OnHold);
    assert_eq!(tickets[2].status, StatusKind::Solved);

    // Attachments: on the account's host stored with credentials; a link
    // off it is refused without a request; a 404 recorded as missing.
    assert!(t101.comments[0].attachments[0].stored);
    assert_eq!(
        std::fs::read(bundle.file_path("5001")).expect("5001"),
        b"hello"
    );
    let off_host = &t101.comments[1].attachments[0];
    assert!(!off_host.stored);
    assert!(
        off_host
            .missing_reason
            .as_deref()
            .unwrap_or("")
            .contains("off the Zendesk account"),
        "{off_host:?}"
    );
    assert!(
        off_host.source_url.is_none(),
        "an off-account link isn't kept"
    );
    assert!(!bundle.file_path("5002").exists());
    let gone = &tickets[2].comments[0].attachments[0];
    assert!(!gone.stored);
    assert!(
        gone.missing_reason.as_deref().unwrap_or("").contains("404"),
        "{gone:?}"
    );

    let on_host = s.api.requests("/attachments/5001");
    let expected = format!("Basic {}", base64_encode("admin@acme.test/token:tok"));
    assert_eq!(on_host[0].auth.as_deref(), Some(expected.as_str()));
    assert!(
        s.files.requests("/files/5002").is_empty(),
        "an off-account link is never requested"
    );
    assert!(s
        .api
        .requests("/api/v2/incremental/tickets/cursor.json")
        .iter()
        .all(|r| r.auth.is_some()));

    // The 503 on ticket fields was retried.
    assert_eq!(s.api.requests("/api/v2/ticket_fields.json").len(), 2);

    // Manifest and progress.
    assert_eq!(manifest.source, "zendesk");
    assert_eq!(manifest.source_scope, format!("127.0.0.1:{}", s.api.port));
    let c = |k: &str| manifest.counts.get(k).copied();
    assert_eq!(c("people"), Some(3));
    assert_eq!(c("tickets"), Some(3));
    assert_eq!(c("comments"), Some(4));
    assert_eq!(c("attachments"), Some(3));
    assert_eq!(c("attachments_missing"), Some(2));
    assert_eq!(manifest.counts.len(), 5);
    // Solved time from the sideloaded metrics.
    assert_eq!(
        tickets[2].solved_at.map(|t| t.to_rfc3339()),
        Some("2024-01-04T12:00:00+00:00".to_string())
    );
    assert!(s
        .api
        .requests("/api/v2/incremental/tickets/cursor.json")
        .iter()
        .all(|r| r.query.get("include").map(String::as_str) == Some("metric_sets")));
    assert_eq!(bundle.read_manifest().expect("manifest"), manifest);
    let last = seen
        .lock()
        .expect("progress")
        .last()
        .cloned()
        .expect("progress called");
    assert_eq!(
        (last.people, last.tickets, last.comments, last.attachments),
        (3, 3, 4, 3)
    );
}

#[tokio::test]
async fn waits_out_a_429_with_retry_after() {
    let s = setup().await;
    s.twists.throttle_users_once.store(true, Ordering::SeqCst);
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("oauth".into()));

    let throttled = Arc::new(Mutex::new(0u64));
    let throttled_in = throttled.clone();
    let progress = move |p: &FetchProgress| *throttled_in.lock().expect("t") = p.throttled;
    let started = std::time::Instant::now();
    src.fetch(&bundle, &progress, CancellationToken::new())
        .await
        .expect("fetch");

    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "Retry-After: 1 was honoured"
    );
    assert_eq!(*throttled.lock().expect("t"), 1);
    // The throttled request, both pages, then the look for users created
    // while the tickets were fetched.
    assert_eq!(s.twists.users_calls.load(Ordering::SeqCst), 4);
    assert_eq!(ticket_ids(&bundle).len(), 3);
    let first = &s.api.requests("/api/v2/organizations.json")[0];
    assert_eq!(first.auth.as_deref(), Some("Bearer oauth"));
}

#[tokio::test]
async fn refused_credential_is_unauthorized_and_writes_no_manifest() {
    let s = setup().await;
    s.twists.deny_everything.store(true, Ordering::SeqCst);
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("revoked".into()));

    let err = src
        .fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect_err("refused");
    assert!(matches!(err, SourceError::Unauthorized(_)), "{err:?}");
    assert!(bundle.read_manifest().is_err());
}

#[tokio::test]
async fn interrupted_fetch_resumes_and_writes_each_ticket_once() {
    let s = setup().await;
    s.twists.deny_ticket_103.store(true, Ordering::SeqCst);
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");

    // Run one: the credential is refused midway through ticket page two.
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    let err = src
        .fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect_err("stopped");
    assert!(matches!(err, SourceError::Unauthorized(_)), "{err:?}");
    let state = bundle.read_state().expect("state");
    assert!(state.people_done);
    assert!(!state.tickets_done);
    assert_eq!(state.ticket_cursor.as_deref(), Some("t2"));
    assert!(state.tickets_len > 0);
    assert_eq!(ticket_ids(&bundle), vec!["101", "102"]);
    assert!(
        bundle.read_manifest().is_err(),
        "no manifest on an unfinished bundle"
    );
    let users_calls = s.twists.users_calls.load(Ordering::SeqCst);

    // Run two, with a working credential: carries on from page two.
    s.twists.deny_ticket_103.store(false, Ordering::SeqCst);
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    let manifest = src
        .fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect("resumed");

    assert_eq!(ticket_ids(&bundle), vec!["101", "102", "103"]);
    assert_eq!(
        s.twists.users_calls.load(Ordering::SeqCst),
        users_calls + 1,
        "people not fetched again, only those since"
    );
    let first_page_requests = s
        .api
        .requests("/api/v2/incremental/tickets/cursor.json")
        .iter()
        .filter(|r| r.query.contains_key("start_time"))
        .count();
    assert_eq!(first_page_requests, 1, "page one not fetched again");
    let c = |k: &str| manifest.counts.get(k).copied();
    assert_eq!(c("people"), Some(3));
    assert_eq!(c("tickets"), Some(3));
    assert_eq!(c("comments"), Some(4));
    assert_eq!(c("attachments"), Some(3));
    assert_eq!(c("attachments_missing"), Some(2));
    assert_eq!(
        s.api.requests("/attachments/5001").len(),
        1,
        "page one's files not downloaded again"
    );
}

#[tokio::test]
async fn cancelled_fetch_stops_before_writing_anything() {
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = src
        .fetch(&bundle, &no_progress, cancel)
        .await
        .expect_err("cancelled");
    assert!(matches!(err, SourceError::Cancelled));
    assert_eq!(bundle.read_state().expect("state"), FetchState::default());
}

#[tokio::test]
async fn attachment_already_on_disk_is_not_downloaded_again() {
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    std::fs::write(bundle.file_path("5001"), b"HELLO").expect("seed");
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    src.fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect("fetch");
    assert!(s.api.requests("/attachments/5001").is_empty());
    assert_eq!(
        std::fs::read(bundle.file_path("5001")).expect("5001"),
        b"HELLO"
    );
}

#[tokio::test]
async fn attachment_over_the_cap_is_listed_not_stored() {
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    src.max_attachment_bytes = 4;
    src.fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect("fetch");
    let t: Vec<_> = bundle
        .tickets()
        .expect("tickets")
        .map(|t| t.expect("ticket"))
        .collect();
    let a = &t[0].comments[0].attachments[0];
    assert!(!a.stored);
    assert!(a.missing_reason.as_deref().unwrap_or("").contains("limit"));
    assert!(!bundle.file_path("5001").exists());
}

#[tokio::test]
async fn base_url_must_be_allowlisted() {
    with_local_allowlisted(|| {
        let refused =
            ZendeskSource::with_base_url("http://10.9.8.7:8080", ZendeskAuth::Bearer("t".into()));
        assert!(matches!(
            refused,
            Err(ZendeskConfigError::BaseUrlRefused(_))
        ));
        let refused =
            ZendeskSource::with_base_url("file:///etc/passwd", ZendeskAuth::Bearer("t".into()));
        assert!(refused.is_err());
        assert!(ZendeskSource::with_base_url(
            "http://127.0.0.1:9",
            ZendeskAuth::Bearer("t".into())
        )
        .is_ok());
    });
}

#[tokio::test]
async fn a_private_attachment_host_is_refused_without_the_allowlist() {
    // The source trusts its own (allowlisted) host; a content URL on
    // another private address is refused by the egress policy, not fetched.
    let s = setup().await;
    let src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    let dir = tempfile::tempdir().expect("tempdir");
    let out = src
        .http
        .download(
            "http://10.9.8.7:1/x",
            &dir.path().join("x"),
            10,
            None,
            None,
            &CancellationToken::new(),
        )
        .await
        .expect("not cancelled");
    let missing = out.expect_err("refused");
    assert!(
        missing.reason.contains("off the Zendesk account"),
        "{missing:?}"
    );
    assert!(!missing.retry);
}

#[tokio::test]
async fn a_download_that_breaks_off_is_tried_again_and_late_users_are_fetched() {
    let s = setup().await;
    s.twists.break_5001_once.store(true, Ordering::SeqCst);
    s.twists.late_user.store(true, Ordering::SeqCst);
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    src.fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect("fetch");
    assert_eq!(
        std::fs::read(bundle.file_path("5001")).expect("5001"),
        b"hello",
        "the broken download was tried again, not kept short"
    );
    assert_eq!(s.api.requests("/attachments/5001").len(), 2);
    assert!(!bundle.root().join("files/5001.part").exists());
    // Ticket 103's requester was created after the people were fetched.
    let people: Vec<String> = bundle
        .people()
        .expect("people")
        .map(|p| p.expect("person").source_id)
        .collect();
    assert!(people.contains(&"4".to_string()), "{people:?}");
    assert_eq!(
        bundle.read_state().expect("state").people_cursor.as_deref(),
        Some("u4")
    );
}

#[test]
fn subdomains_are_validated() {
    assert_eq!(normalise_subdomain("acme").expect("ok"), "acme");
    assert_eq!(normalise_subdomain(" ACME ").expect("ok"), "acme");
    assert_eq!(normalise_subdomain("acme-it2").expect("ok"), "acme-it2");
    assert_eq!(normalise_subdomain("Acme.Zendesk.com").expect("ok"), "acme");
    let long = "a".repeat(64);
    for bad in [
        "evil.com/",
        "a.b",
        "",
        "-acme",
        "acme_it",
        "acme.zendesk.com.evil.com",
        "a/b",
        "acme:443",
        &long,
    ] {
        assert!(
            normalise_subdomain(bad).is_err(),
            "{bad:?} should be refused"
        );
    }
    let src = ZendeskSource::new("Acme", ZendeskAuth::Bearer("t".into())).expect("source");
    assert_eq!(src.scope(), "acme");
    assert_eq!(src.source(), "zendesk");
    assert_eq!(
        src.http.url("/api/v2/x.json", &[]).as_str(),
        "https://acme.zendesk.com/api/v2/x.json"
    );
}

#[test]
fn credentials_are_not_printed() {
    let s = format!(
        "{:?}",
        ZendeskAuth::Basic {
            email: "a@b.c".into(),
            token: "secret".into()
        }
    );
    assert!(!s.contains("secret"));
    assert!(!format!("{:?}", ZendeskAuth::Bearer("secret".into())).contains("secret"));
}

fn base64_encode(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(s)
}

#[tokio::test]
async fn reads_a_json_export_zip() {
    use std::io::Write;
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let zip_path = dir.path().join("export.zip");
    let api_base = s.api.base();
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).expect("zip"));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("export_2026_11_22_abc_1", opts)
            .expect("entry");
        let lines = [
            json!({"organization": {"id": 900, "name": "Acme"}}),
            json!({"user": {"id": 3, "name": "Cara", "email": "CARA@example.com", "role": "end-user", "organization_id": 900}}),
            json!({"ticket": {
                "id": 201, "subject": "Printer", "status": "closed", "requester_id": 3,
                "created_at": "2023-05-01T09:00:00Z", "updated_at": "2023-05-02T09:00:00Z",
                "custom_fields": [{"id": 77, "value": "emea"}],
                "comments": [
                    {"id": 2002, "author_id": 2, "public": false, "created_at": "2023-05-01T10:00:00Z", "body": "note"},
                    {"id": 2001, "author_id": 3, "public": true, "created_at": "2023-05-01T09:00:00Z",
                     "html_body": "<p>It jams</p>",
                     "attachments": [{"id": 5001, "file_name": "jam.txt", "size": 5,
                                      "content_url": format!("{api_base}/attachments/5001")}]}
                ]
            }}),
        ];
        for l in lines {
            writeln!(zip, "{l}").expect("line");
        }
        zip.start_file("export_2026_11_22_abc_2", opts)
            .expect("entry");
        // A bare record, recognised by shape, sent without its comments.
        writeln!(
            zip,
            "{}",
            json!({"id": 202, "subject": "Big one", "status": "open", "requester_id": 3,
            "description": "Too long to export", "created_at": "2023-06-01T09:00:00Z"})
        )
        .expect("line");
        writeln!(zip, "{{not json").expect("line");
        writeln!(
            zip,
            "{}",
            json!({"user": {"id": 2, "name": "Alan", "email": "alan@acme.test", "role": "agent"}})
        )
        .expect("line");
        zip.finish().expect("finish");
    }

    let bundle = Bundle::create(dir.path().join("bundle")).expect("bundle");
    let src = source(&s.api.base(), ZendeskAuth::Bearer("t".into()));
    let manifest = read_export(
        &src,
        &zip_path,
        &bundle,
        &no_progress,
        CancellationToken::new(),
    )
    .await
    .expect("export");

    let people: Vec<_> = bundle
        .people()
        .expect("people")
        .map(|p| p.expect("person"))
        .collect();
    assert_eq!(people.len(), 2);
    let cara = people.iter().find(|p| p.source_id == "3").expect("cara");
    assert_eq!(cara.organization.as_deref(), Some("Acme"));
    assert_eq!(cara.email.as_deref(), Some("cara@example.com"));

    let tickets: Vec<_> = bundle
        .tickets()
        .expect("tickets")
        .map(|t| t.expect("ticket"))
        .collect();
    assert_eq!(
        tickets
            .iter()
            .map(|t| t.source_id.as_str())
            .collect::<Vec<_>>(),
        vec!["201", "202"]
    );
    let t = &tickets[0];
    assert_eq!(t.status, StatusKind::Closed);
    assert_eq!(t.fields.get("Region").map(String::as_str), Some("emea"));
    assert_eq!(t.comments[0].source_id, "2001", "oldest first");
    assert!(t.comments[0].attachments[0].stored);
    assert_eq!(
        std::fs::read(bundle.file_path("5001")).expect("file"),
        b"hello"
    );
    assert_eq!(t.comments[1].body.format, BodyFormat::Plain);
    assert_eq!(tickets[1].comments.len(), 1);
    assert_eq!(tickets[1].comments[0].body.value, "Too long to export");

    assert_eq!(manifest.counts.get("tickets"), Some(&2));
    assert_eq!(manifest.counts.get("comments"), Some(&3));
    assert!(
        manifest
            .notes
            .iter()
            .any(|n| n.contains("could not be read")),
        "{:?}",
        manifest.notes
    );
    assert!(
        manifest
            .notes
            .iter()
            .any(|n| n.contains("without their conversation")),
        "{:?}",
        manifest.notes
    );
    let state = bundle.read_state().expect("state");
    assert!(state.people_done && state.tickets_done);
}

#[tokio::test]
async fn export_reads_without_a_credential() {
    use std::io::Write;
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let zip_path = dir.path().join("export.zip");
    let api_base = s.api.base();
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).expect("zip"));
        zip.start_file(
            "export_2026_11_22_abc_1",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("entry");
        let t = json!({"ticket": {
            "id": 301, "subject": "Laptop", "status": "solved", "requester_id": 3,
            "created_at": "2023-05-01T09:00:00Z",
            "custom_fields": [{"id": 77, "value": "emea"}],
            "comments": [{"id": 3001, "author_id": 3, "public": true, "created_at": "2023-05-01T09:00:00Z",
                "body": "see attached",
                "attachments": [
                    {"id": 5001, "file_name": "a.txt", "size": 5, "content_url": format!("{api_base}/attachments/5001")},
                    {"id": 5009, "file_name": "b.txt", "size": 6, "content_url": format!("{api_base}/attachments/5009")}
                ]}]
        }});
        writeln!(zip, "{t}").expect("line");
        zip.finish().expect("finish");
    }
    let bundle = Bundle::create(dir.path().join("bundle")).expect("bundle");
    let src = with_local_allowlisted(|| ZendeskSource::for_export_with_base_url(&api_base))
        .expect("source");
    let manifest = read_export(
        &src,
        &zip_path,
        &bundle,
        &no_progress,
        CancellationToken::new(),
    )
    .await
    .expect("export");

    let tickets: Vec<_> = bundle
        .tickets()
        .expect("tickets")
        .map(|t| t.expect("ticket"))
        .collect();
    assert_eq!(tickets.len(), 1);
    assert_eq!(
        tickets[0].fields.get("Field 77").map(String::as_str),
        Some("emea")
    );
    let atts = &tickets[0].comments[0].attachments;
    assert!(atts[0].stored);
    assert!(!atts[1].stored);
    assert_eq!(
        atts[1].missing_reason.as_deref(),
        Some("needs sign-in; re-run with a Zendesk credential")
    );
    // Nothing was signed, and the field-title lookup was skipped.
    assert!(s
        .api
        .log
        .lock()
        .expect("log")
        .iter()
        .all(|r| r.auth.is_none()));
    assert!(s.api.requests("/api/v2/ticket_fields.json").is_empty());
    assert!(
        manifest.notes.iter().any(|n| n.contains("need API access")),
        "{:?}",
        manifest.notes
    );
    assert_eq!(manifest.counts.get("attachments_missing"), Some(&1));
}

#[tokio::test]
async fn api_fetch_without_a_credential_is_refused_at_once() {
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let bundle = Bundle::create(dir.path()).expect("bundle");
    let mut src = with_local_allowlisted(|| ZendeskSource::for_export_with_base_url(&s.api.base()))
        .expect("source");
    let err = src
        .fetch(&bundle, &no_progress, CancellationToken::new())
        .await
        .expect_err("refused");
    assert!(matches!(err, SourceError::Unauthorized(_)), "{err:?}");
    assert!(s.api.log.lock().expect("log").is_empty(), "no request made");
    assert!(ZendeskSource::for_export("acme").is_ok());
    assert!(ZendeskSource::for_export("evil.com/").is_err());
}

fn export_zip(dir: &std::path::Path, entries: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
    use std::io::Write;
    let path = dir.join("export.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("zip"));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(bytes).expect("bytes");
    }
    zip.finish().expect("finish");
    path
}

async fn read_zip(
    api_base: &str,
    zip: &std::path::Path,
    out: &std::path::Path,
) -> Result<(Bundle, Manifest), SourceError> {
    let bundle = Bundle::create(out).expect("bundle");
    let src = with_local_allowlisted(|| ZendeskSource::for_export_with_base_url(api_base))
        .expect("source");
    let manifest = read_export(&src, zip, &bundle, &no_progress, CancellationToken::new()).await?;
    Ok((bundle, manifest))
}

#[tokio::test]
async fn the_export_reader_takes_odd_files_without_losing_or_inventing_records() {
    let s = setup().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend(
        json!({"ticket": {
            "id": 401, "subject": "BOM", "status": "open", "requester_id": 3,
            "created_at": "2023-05-01T09:00:00Z", "tags": null, "collaborator_ids": null,
            "custom_fields": null,
            "comments": [
                {"id": 1, "author_id": 3, "created_at": "2023-05-01T09:00:00Z", "body": "no public field"},
                {"id": 2, "author_id": 3, "public": null, "attachments": null,
                 "created_at": "2023-05-01T10:00:00Z", "body": "public is null"}
            ]
        }})
        .to_string()
        .into_bytes(),
    );
    bom.push(b'\n');
    let document = serde_json::to_vec_pretty(&json!({"tickets": [{
        "id": 402, "subject": "Pretty", "status": "open", "requester_id": 3,
        "created_at": "2023-05-02T09:00:00Z",
        "comments": [{"id": 3, "author_id": 3, "public": true, "created_at": "2023-05-02T09:00:00Z", "body": "hi"}]
    }]}))
    .expect("doc");
    let unknown = format!(
        "{}\n{}\n",
        json!({"widget": {"id": 1}}),
        json!({"id": 9, "colour": "red"})
    )
    .into_bytes();
    let bad_first = format!(
        "garbage line\n{}\n",
        json!({"ticket": {"id": 404, "subject": "After garbage", "status": "open",
            "requester_id": 3, "created_at": "2023-05-04T09:00:00Z",
            "comments": [{"id": 4, "author_id": 3, "public": true, "created_at": "2023-05-04T09:00:00Z", "body": "x"}]}})
    )
    .into_bytes();
    let zip = export_zip(
        dir.path(),
        &[
            ("export_1", bom),
            ("export_2", document),
            ("export_3", unknown),
            ("export_4", bad_first),
        ],
    );
    let (bundle, manifest) = read_zip(&s.api.base(), &zip, &dir.path().join("b"))
        .await
        .expect("export");
    let tickets: Vec<_> = bundle
        .tickets()
        .expect("tickets")
        .map(|t| t.expect("ticket"))
        .collect();
    let ids: Vec<&str> = tickets.iter().map(|t| t.source_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["401", "402", "404"],
        "a bad first line costs only itself"
    );
    assert!(
        tickets[0].comments.iter().all(|c| !c.public),
        "a comment that doesn't say it's public is an internal note"
    );
    assert!(
        manifest
            .notes
            .iter()
            .any(|n| n.contains("of no kind this reader knows")),
        "{:?}",
        manifest.notes
    );
    assert!(
        manifest
            .notes
            .iter()
            .any(|n| n.contains("could not be read")),
        "{:?}",
        manifest.notes
    );

    // A zip of records none of which is a ticket is refused, not read as
    // an empty helpdesk.
    let only_unknown = export_zip(
        &dir.path().join("b"),
        &[(
            "export_1",
            format!("{}\n", json!({"widget": {"id": 1}})).into_bytes(),
        )],
    );
    let err = read_zip(&s.api.base(), &only_unknown, &dir.path().join("c"))
        .await
        .expect_err("refused");
    assert!(matches!(err, SourceError::Protocol(_)), "{err:?}");
}
