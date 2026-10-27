//! The collaboration server's editor schema gate: a client whose editor
//! predates tables is never sent a document holding one, because
//! y-prosemirror would delete the table for everyone. Each case connects real
//! WebSocket clients (`awc`) to `ws_handler`, the way `tests/collaboration_ws.rs`
//! does, without changing any process-wide setting.

use std::sync::Arc;
use std::time::Duration;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use awc::ws;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{
    Doc, ReadTxn, StateVector, Transact, TransactionMut, WriteTxn, XmlElementPrelim, XmlFragment,
    XmlTextPrelim,
};

use backend::extractors::WorkspaceContext;
use backend::handlers::collaboration::{ws_handler, CollabRoutingMode, YjsAppState};
use backend::handlers::sse::SseState;
use backend::services::search::SearchService;
use backend::utils::jwt::JwtUtils;

use crate::common;

const OUTDATED: u16 = 4426;
const WAIT: Duration = Duration::from_secs(5);

fn workspace() -> WorkspaceContext {
    WorkspaceContext {
        workspace_id: 1,
        workspace_uuid: uuid::Uuid::nil(),
        slug: "default".into(),
        name: "Default".into(),
        organisation_id: None,
        custom_domain: None,
    }
}

/// A collaboration server over `pool`, single-instance, with the bootstrap
/// workspace on every request.
fn start_server(pool: &backend::db::Pool) -> actix_test::TestServer {
    let pool = pool.clone();
    actix_test::start(move || {
        let tmp = tempfile::tempdir().expect("temp dir for search index");
        let search = Arc::new(SearchService::new(tmp.path(), &pool).expect("init search service"));
        // The index lives as long as the server; the OS reaps it.
        std::mem::forget(tmp);
        let state = YjsAppState::new(
            web::Data::new(pool.clone()),
            web::Data::new(SseState::new()),
            search,
            None,
            CollabRoutingMode::Single,
        );
        let ws = workspace();
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                srv.call(req)
            })
            .route("/ws/{doc}", web::get().to(ws_handler))
    })
}

fn add_member(pool: &backend::db::Pool, user: uuid::Uuid, role: &str) {
    let mut conn = pool.get().expect("conn");
    let actor = backend::sync::actor::ActorContext::user(user, None).with_workspace(1);
    backend::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |c| {
            backend::repository::workspaces::add_membership(
                c,
                1,
                user,
                role,
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        },
    )
    .expect("seed workspace membership");
}

/// A ticket in workspace 1, optionally requested by `requester`, with
/// `note` as its stored collaborative document. Returns its doc id.
fn seed_ticket(
    pool: &backend::db::Pool,
    requester: Option<uuid::Uuid>,
    note: Option<Vec<u8>>,
) -> String {
    use backend::schema::{tickets, workflow_states};
    use diesel::prelude::*;
    let mut conn = pool.get().expect("conn");
    let state_id: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(1))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(&mut conn)
        .expect("default workflow state seeded");
    let ticket: backend::models::Ticket = diesel::insert_into(tickets::table)
        .values(&backend::models::NewTicket {
            title: "Schema gate ticket".to_string(),
            workflow_state_id: state_id,
            requester_uuid: requester,
            ..Default::default()
        })
        .get_result(&mut conn)
        .expect("insert ticket");
    if let Some(note) = note {
        backend::repository::create_article_content(
            &mut conn,
            backend::models::NewArticleContent {
                ticket_id: ticket.id,
                yjs_state_vector: None,
                yjs_document: Some(note),
                yjs_client_id: None,
            },
        )
        .expect("insert article content");
    }
    format!("ws-{}_ticket-{}", workspace().workspace_uuid, ticket.uuid)
}

/// Append a one-cell table to the `prosemirror` fragment, element by element
/// as y-prosemirror writes one.
fn push_table(txn: &mut TransactionMut) {
    let fragment = txn.get_or_insert_xml_fragment("prosemirror");
    let table = fragment.push_back(txn, XmlElementPrelim::empty("table"));
    let row = table.push_back(txn, XmlElementPrelim::empty("table_row"));
    let cell = row.push_back(txn, XmlElementPrelim::empty("table_cell"));
    let paragraph = cell.push_back(txn, XmlElementPrelim::empty("paragraph"));
    paragraph.push_back(txn, XmlTextPrelim::new("cell"));
}

fn push_paragraph(txn: &mut TransactionMut, text: &str) {
    let fragment = txn.get_or_insert_xml_fragment("prosemirror");
    let paragraph = fragment.push_back(txn, XmlElementPrelim::empty("paragraph"));
    paragraph.push_back(txn, XmlTextPrelim::new(text));
}

/// A stored note: a paragraph, and a table when `with_table`.
fn note(with_table: bool) -> Vec<u8> {
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        push_paragraph(&mut txn, "before");
        if with_table {
            push_table(&mut txn);
        }
    }
    let update = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    update
}

/// A SyncUpdate inserting a table, as an editor on schema 2 sends one.
fn table_update() -> Bytes {
    let doc = Doc::new();
    push_table(&mut doc.transact_mut());
    let update = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    Bytes::from(Message::Sync(SyncMessage::Update(update)).encode_v1())
}

/// A SyncStep1 with an empty state vector: "send me everything".
fn sync_step1() -> Bytes {
    Bytes::from(Message::Sync(SyncMessage::SyncStep1(StateVector::default())).encode_v1())
}

fn token(user: &backend::models::User) -> String {
    JwtUtils::create_collab_token(
        &user.uuid.to_string(),
        &user.platform_role,
        Some(workspace().workspace_uuid),
    )
    .expect("mint collab token")
}

/// A client's WebSocket: frames in, messages out.
trait Conn:
    futures_util::Stream<Item = Result<ws::Frame, actix_ws::ProtocolError>>
    + futures_util::Sink<ws::Message, Error = actix_ws::ProtocolError>
    + Unpin
{
}
impl<T> Conn for T where
    T: futures_util::Stream<Item = Result<ws::Frame, actix_ws::ProtocolError>>
        + futures_util::Sink<ws::Message, Error = actix_ws::ProtocolError>
        + Unpin
{
}

async fn connect(
    srv: &actix_test::TestServer,
    doc_id: &str,
    token: &str,
    extra_query: &str,
) -> impl Conn {
    let (_resp, conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/{doc_id}?token={token}{extra_query}")))
        .connect()
        .await
        .expect("WS handshake");
    conn
}

/// What a connection received: binary frames, and its close code if it was
/// closed.
#[derive(Debug, Default)]
struct Received {
    frames: Vec<Bytes>,
    closed: Option<u16>,
}

impl Received {
    /// Whether any frame carries a table's element name. Crude on purpose:
    /// it also catches a table riding in blocks a client couldn't integrate
    /// yet. No test text contains the word.
    fn saw_table(&self) -> bool {
        self.frames
            .iter()
            .any(|f| f.windows(b"table".len()).any(|w| w == b"table"))
    }

    fn saw_sync_step2(&self) -> bool {
        self.frames.iter().any(|f| f.starts_with(&[0, 1]))
    }

    fn hello(&self) -> Option<&Bytes> {
        self.frames.iter().find(|f| f.first() == Some(&100))
    }
}

/// Read from `conn` until it closes or `wait` passes with nothing more.
async fn drain<S, E>(conn: &mut S, wait: Duration) -> Received
where
    S: futures_util::Stream<Item = Result<ws::Frame, E>> + Unpin,
{
    let mut received = Received::default();
    while let Ok(next) = tokio::time::timeout(wait, conn.next()).await {
        match next {
            Some(Ok(ws::Frame::Binary(b))) => received.frames.push(b),
            Some(Ok(ws::Frame::Close(reason))) => {
                received.closed = Some(reason.map_or(0, |r| u16::from(r.code)));
                break;
            }
            Some(Ok(_)) => continue,
            Some(Err(_)) | None => break,
        }
    }
    received
}

struct Fixture {
    _db: common::TestDb,
    pool: backend::db::Pool,
    srv: actix_test::TestServer,
    staff: backend::models::User,
}

fn fixture() -> Fixture {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let staff = common::insert_user(&mut pool.get().expect("conn"), "Gate Staff");
    add_member(&pool, staff.uuid, "admin");
    let srv = start_server(&pool);
    Fixture {
        _db: db,
        pool,
        srv,
        staff,
    }
}

/// A client from before the gate (no `schema`) opening a note that already
/// holds a table is closed with 4426 and sent nothing of it, before and
/// after asking for the document.
#[actix_web::test]
async fn an_old_client_is_refused_a_document_with_a_table() {
    let f = fixture();
    let doc_id = seed_ticket(&f.pool, None, Some(note(true)));

    let mut old = connect(&f.srv, &doc_id, &token(&f.staff), "").await;
    let _ = old.send(ws::Message::Binary(sync_step1())).await;
    let received = drain(&mut old, WAIT).await;
    assert_eq!(received.closed, Some(OUTDATED), "{received:?}");
    assert!(!received.saw_table(), "an old client received the table");
    assert!(
        !received.saw_sync_step2(),
        "an old client received the document"
    );

    // A current client opens the same note and gets the table.
    let mut current = connect(&f.srv, &doc_id, &token(&f.staff), "&schema=2").await;
    current
        .send(ws::Message::Binary(sync_step1()))
        .await
        .expect("send SyncStep1");
    let received = drain(&mut current, Duration::from_secs(1)).await;
    assert_eq!(received.closed, None);
    assert!(received.saw_table(), "a current client gets the table");
}

/// A note without a table serves an old client as before: the document,
/// and no hello it couldn't read.
#[actix_web::test]
async fn an_old_client_still_opens_a_document_without_a_table() {
    let f = fixture();
    let doc_id = seed_ticket(&f.pool, None, Some(note(false)));

    let mut old = connect(&f.srv, &doc_id, &token(&f.staff), "").await;
    old.send(ws::Message::Binary(sync_step1()))
        .await
        .expect("send SyncStep1");
    let received = drain(&mut old, Duration::from_secs(1)).await;
    assert_eq!(received.closed, None);
    assert!(received.saw_sync_step2(), "{received:?}");
    assert!(received.hello().is_none(), "an old client got a hello");
}

/// A session declaring schema 2 or more gets the hello: message type 100
/// and the schema the server accepts from it, capped at the server's own.
/// The upgrade still succeeds with a schema that doesn't parse (it counts
/// as 1) and with a query parameter the server doesn't know, which is how a
/// new client still connects to a server from before the gate.
#[actix_web::test]
async fn a_current_client_gets_the_hello() {
    let f = fixture();
    let doc_id = seed_ticket(&f.pool, None, Some(note(false)));

    for (query, hello) in [
        ("&schema=2", Some(vec![100u8, 2])),
        ("&schema=3", Some(vec![100, 2])),
        ("&schema=1", None),
        ("&schema=two", None),
        ("&schema=2&unknown=1", Some(vec![100, 2])),
    ] {
        let mut conn = connect(&f.srv, &doc_id, &token(&f.staff), query).await;
        let received = drain(&mut conn, Duration::from_millis(600)).await;
        assert_eq!(received.closed, None, "{query}");
        assert!(
            received
                .frames
                .first()
                .is_some_and(|f| f.starts_with(&[0, 0])),
            "{query}: SyncStep1 comes first"
        );
        assert_eq!(
            received.hello().map(|h| h.to_vec()),
            hello,
            "{query}: hello"
        );
    }
}

/// An old client is in the room when a current one inserts a table: it is
/// closed with 4426 and never receives the frame, while the current client
/// carries on.
#[actix_web::test]
async fn an_old_client_in_the_room_is_closed_when_a_table_arrives() {
    let f = fixture();
    let doc_id = seed_ticket(&f.pool, None, Some(note(false)));

    let mut old = connect(&f.srv, &doc_id, &token(&f.staff), "").await;
    let mut current = connect(&f.srv, &doc_id, &token(&f.staff), "&schema=2").await;
    let mut watcher = connect(&f.srv, &doc_id, &token(&f.staff), "&schema=2").await;
    // Each is in the room once its opening frames arrive.
    for conn in [&mut old, &mut current, &mut watcher] {
        let _ = drain(conn, Duration::from_millis(400)).await;
    }

    let insert = table_update();
    current
        .send(ws::Message::Binary(insert.clone()))
        .await
        .expect("send the table");

    let received = drain(&mut old, WAIT).await;
    assert_eq!(received.closed, Some(OUTDATED), "{received:?}");
    assert!(!received.saw_table(), "the old client received the table");

    let received = drain(&mut watcher, Duration::from_secs(1)).await;
    assert_eq!(received.closed, None);
    assert!(
        received.frames.iter().any(|f| f == &insert),
        "a current client still receives the table"
    );

    // And an old client arriving now is refused.
    let mut late = connect(&f.srv, &doc_id, &token(&f.staff), "").await;
    let received = drain(&mut late, WAIT).await;
    assert_eq!(received.closed, Some(OUTDATED));
    assert!(!received.saw_table());
}

/// An old client keeps asking for the document (y-websocket resends
/// SyncStep1 every 20 s) while a table is inserted: whatever it was sent
/// before the insert, no SyncStep2 it receives holds the table, and it ends
/// closed with 4426.
#[actix_web::test]
async fn an_old_clients_resync_never_returns_the_table() {
    let f = fixture();
    let doc_id = seed_ticket(&f.pool, None, Some(note(false)));

    let mut old = connect(&f.srv, &doc_id, &token(&f.staff), "").await;
    let mut current = connect(&f.srv, &doc_id, &token(&f.staff), "&schema=2").await;
    for conn in [&mut old, &mut current] {
        let _ = drain(conn, Duration::from_millis(400)).await;
    }

    let (old_sink, mut old_stream) = old.split();
    let asker = actix_web::rt::spawn(async move {
        let mut sink = old_sink;
        for _ in 0..200 {
            if sink.send(ws::Message::Binary(sync_step1())).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    current
        .send(ws::Message::Binary(table_update()))
        .await
        .expect("send the table");

    let received = drain(&mut old_stream, WAIT).await;
    asker.abort();
    assert!(
        received.saw_sync_step2(),
        "the old client was served before the insert"
    );
    assert!(!received.saw_table(), "a resync returned the table");
    assert_eq!(received.closed, Some(OUTDATED));
}

/// A read-only session (a requester on their own ticket) is gated like any
/// other: it would delete the table locally all the same.
#[actix_web::test]
async fn a_read_only_old_client_is_refused_too() {
    use diesel::prelude::*;
    let f = fixture();
    let requester: backend::models::User = diesel::insert_into(backend::schema::users::table)
        .values(&backend::models::NewUser {
            uuid: uuid::Uuid::new_v4(),
            name: "Gate Requester".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some("user".to_string()),
        })
        .get_result(&mut f.pool.get().expect("conn"))
        .expect("insert requester");
    add_member(&f.pool, requester.uuid, "member");

    let with_table = seed_ticket(&f.pool, Some(requester.uuid), Some(note(true)));
    let mut old = connect(&f.srv, &with_table, &token(&requester), "").await;
    let _ = old.send(ws::Message::Binary(sync_step1())).await;
    let received = drain(&mut old, WAIT).await;
    assert_eq!(received.closed, Some(OUTDATED), "{received:?}");
    assert!(!received.saw_table());

    // The same requester on a ticket without a table reads it as before.
    let without = seed_ticket(&f.pool, Some(requester.uuid), Some(note(false)));
    let mut old = connect(&f.srv, &without, &token(&requester), "").await;
    old.send(ws::Message::Binary(sync_step1()))
        .await
        .expect("send SyncStep1");
    let received = drain(&mut old, Duration::from_secs(1)).await;
    assert_eq!(received.closed, None);
    assert!(received.saw_sync_step2());
}
