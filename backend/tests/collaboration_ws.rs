//! Integration smoke test for the collaboration WebSocket handler.
//!
//! Purpose: guard the actix-web-actors → actix-ws migration. The tests
//! here connect real `awc` clients (via the `actix-test`-spawned server's
//! `ws_at` helper) and exercise the handshake + broadcast + clean-
//! disconnect paths. They pass against both the current actor-based
//! handler and the planned async-task replacement; if they ever stop
//! passing after the swap, the migration broke something.
//!
//! Scope intentionally narrow: this is the *transport* layer, not the
//! Yjs protocol. We assert that bytes sent by one client reach the
//! other, not that the Yjs document state converges (yrs handles that
//! and has its own tests).
//!
//! Setup is heavy because `YjsAppState` is real (Postgres + Redis +
//! Tantivy). Tests share a single test server inside one
//! `#[actix_web::test]` and serialise their WS conversations.

#![allow(clippy::expect_used)] // tests fail loudly on purpose

use std::sync::{Arc, Once};
use std::time::Duration;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use awc::ws;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};

use backend::extractors::WorkspaceContext;
use backend::handlers::collaboration::{ws_handler, CollabRoutingMode, YjsAppState};
use backend::handlers::sse::SseState;
use backend::services::search::SearchService;
use backend::utils::jwt::JwtUtils;

mod common;

/// Short heartbeat values so the tests don't sit on the 20s/60s
/// production wall clock. Set *before* the `Lazy`s in collaboration.rs
/// resolve (they read env once, on first access). One-shot per test
/// process via `Once`; if a later test wants different values, change
/// them here and add `#[serial_test::serial]` to the affected tests.
fn install_fast_heartbeat() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::env::set_var("NOSDESK_WS_HEARTBEAT_MS", "100");
        std::env::set_var("NOSDESK_WS_CLIENT_TIMEOUT_MS", "500");
        // Maintenance every 100ms and a 300ms quiet period, so a revision
        // cut by quiet lands well inside a test's wait.
        std::env::set_var("NOSDESK_COLLAB_MAINTENANCE_MS", "100");
        std::env::set_var("NOSDESK_REVISION_QUIET_MS", "300");
        // Hold each session for a moment between the access check and
        // joining its room, so a test can change access in between.
        std::env::set_var("NOSDESK_COLLAB_JOIN_DELAY_MS", "300");
        // JWT_SECRET must be stable across the test process; main.rs
        // refuses placeholder secrets in production, but tests run in
        // dev/test mode.
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-32-characters-min-for-tests");
        }
        // Relax the ws_handler Origin requirement. `assume_production` is
        // fail-closed: only "development"/"dev" count as non-production, so a
        // label like "test" would read as production and reject the WS.
        std::env::set_var("ENVIRONMENT", "development");
    });
}

/// Build a real `YjsAppState` against the test sandbox DB and a temp
/// Tantivy index dir.
fn build_app_state(pool_inner: &backend::db::Pool) -> (YjsAppState, tempfile::TempDir) {
    build_app_state_with_sse(pool_inner, web::Data::new(SseState::new()))
}

/// [`build_app_state`] on a given live-event state, for a test that runs the
/// sync outbox into it.
fn build_app_state_with_sse(
    pool_inner: &backend::db::Pool,
    sse_state: web::Data<SseState>,
) -> (YjsAppState, tempfile::TempDir) {
    let pool_data = web::Data::new(pool_inner.clone());
    let tmp_search = tempfile::tempdir().expect("temp dir for search index");
    let search =
        Arc::new(SearchService::new(tmp_search.path(), pool_inner).expect("init search service"));
    // Single-instance: this is the transport-layer test, so no ownership
    // manager and routing is inert (every doc served locally).
    let state = YjsAppState::new(
        pool_data,
        sse_state,
        search,
        None,
        CollabRoutingMode::Single,
    );
    (state, tmp_search)
}

/// Pins `app.workspace_id = 1` on every newly-acquired connection
/// so audit triggers (which require the GUC) don't trip the NOT-NULL
/// constraint on `audit_log.workspace_id`. Same shape as the
/// `WorkspaceGucCustomizer` in `tests/common/mod.rs`, duplicated
/// here because that one is private to the module.
#[derive(Debug)]
struct WorkspaceGuc;

impl diesel::r2d2::CustomizeConnection<diesel::pg::PgConnection, diesel::r2d2::Error>
    for WorkspaceGuc
{
    fn on_acquire(&self, conn: &mut diesel::pg::PgConnection) -> Result<(), diesel::r2d2::Error> {
        use diesel::RunQueryDsl;
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(diesel::r2d2::Error::QueryError)?;
        Ok(())
    }
}

/// Wrap the test sandbox DB url in a real `backend::db::Pool` so
/// `ws_handler` can fetch a connection for JWT validation, and
/// fixtures can insert seed rows.
/// Init the at-rest Keyring once per process. test_helpers' equivalent
/// is `#[cfg(test)]`-gated and thus invisible to integration tests.
fn ensure_test_keyring() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if std::env::var("MFA_KEK_V1").is_err() {
            std::env::set_var(
                "MFA_KEK_V1",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }
        if let Err(e) = backend::utils::encryption::init_keyring() {
            panic!("ensure_test_keyring: init_keyring failed: {e}");
        }
    });
}

fn build_pool(url: &str) -> backend::db::Pool {
    use diesel::r2d2::Pool;
    // YjsAppState construction touches the Keyring via the periodic
    // save loop's encryption paths; init once per process so we
    // never panic with `keyring not initialised`.
    ensure_test_keyring();
    let mgr = backend::db::ResettingManager::new(url);
    Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(WorkspaceGuc))
        // Seeded ambient workspace GUC (outside the request middleware), so
        // keep the production per-checkout scrub off this pool.
        .test_on_check_out(false)
        .build(mgr)
        .expect("build backend Pool")
}

/// Test workspace fixture. Maps to the bootstrap workspace seeded
/// by every fresh template (id = 1), so DB inserts hit a real row.
fn test_workspace() -> WorkspaceContext {
    WorkspaceContext {
        workspace_id: 1,
        workspace_uuid: uuid::Uuid::nil(),
        slug: "default".into(),
        name: "Default".into(),
        organisation_id: None,
        custom_domain: None,
    }
}

/// Seed a ticket in the bootstrap workspace and return its uuid. The
/// collaboration handshake's per-document visibility gate
/// (security-audit-2026-06) calls `can_view_ticket`, which requires the
/// ticket to exist; pointing the doc_id at a real ticket lets the
/// transport smoke test exercise the handshake. The caller (WSAlice) is
/// a platform admin, so any existing ticket is visible to her. Collab
/// doc_ids are keyed by the immutable ticket uuid (the backend resolves
/// it to the integer id), so we return the uuid, not the id.
fn seed_ticket(conn: &mut diesel::pg::PgConnection) -> uuid::Uuid {
    use backend::schema::{tickets, workflow_states};
    use diesel::prelude::*;
    let state_id: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(1))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(conn)
        .expect("default workflow state seeded");
    let t: backend::models::Ticket = diesel::insert_into(tickets::table)
        .values(&backend::models::NewTicket {
            title: "WS collab test ticket".to_string(),
            workflow_state_id: state_id,
            ..Default::default()
        })
        .get_result(conn)
        .expect("insert ticket");
    t.uuid
}

#[actix_web::test]
async fn handshake_broadcast_and_clean_disconnect() {
    install_fast_heartbeat();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());

    // Seed a user so JWT validation finds a real row to attach to.
    let user = common::insert_user(&mut pool.get().expect("conn"), "WSAlice");

    // The collab WS handshake runs the workspace-membership gate, so the user
    // must be a member of the request workspace (id 1 here). Production reaches
    // this handshake only for members; mirror that. Pinned via the actor because
    // workspace_members is RLS-isolated and the pooled conn carries no GUC.
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(user.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                backend::repository::workspaces::add_membership(
                    c,
                    1,
                    user.uuid,
                    "admin",
                    backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                )?;
                Ok(())
            },
        )
        .expect("seed workspace membership");
    }

    // The collab WS authenticates with a sessionless, workspace-bound connection
    // token carried in the query string (a browser WebSocket sets no cookie or
    // Authorization header). Bind it to the doc's workspace so the handler's
    // token-workspace match passes.
    let token = JwtUtils::create_collab_token(
        &user.uuid.to_string(),
        &user.platform_role,
        Some(test_workspace().workspace_uuid),
    )
    .expect("mint collab token");

    // Seed a ticket so the WS visibility gate admits the handshake
    // (see the doc_id construction below).
    let ticket_uuid = seed_ticket(&mut pool.get().expect("conn"));

    let state_pool_inner = pool.clone();

    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        // The TempDir would drop at end of closure and remove the
        // search index. Leak it on purpose for the server's lifetime:
        // the test process is short-lived and the OS reaps it.
        std::mem::forget(_tmp);

        let ws = test_workspace();
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            // Inject WorkspaceContext into request extensions so the
            // extractor in ws_handler resolves. Replaces the real
            // WorkspaceContextMiddleware which depends on
            // subdomain/host parsing we don't want to set up here.
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                srv.call(req)
            })
            .route("/ws/{doc}", web::get().to(ws_handler))
    });

    // doc_id must be in the workspace-namespaced format
    // `ws-{workspace_uuid}_{kind}-{resource_uuid}`; ws_handler rejects
    // bare/integer ids with a 400 and rejects a workspace uuid that
    // doesn't match the request's WorkspaceContext with a 403. Build it
    // from test_workspace()'s uuid (nil) and the seeded ticket's uuid so
    // both checks pass and the backend resolves the uuid to the ticket
    // the per-document visibility gate (security-audit-2026-06) admits.
    let doc_id = format!(
        "ws-{}_ticket-{ticket_uuid}",
        test_workspace().workspace_uuid
    );
    // The connection token rides in the query string (the WS can't send a cookie
    // or Authorization header).
    let url = srv.url(&format!("/ws/{doc_id}?token={token}"));

    let client = awc::Client::new();

    // --- Client A connects ---
    let (_resp_a, mut conn_a) = client
        .ws(&url)
        .connect()
        .await
        .expect("client A WS handshake");

    // ws_handler emits initial SyncStep1 + (optional) awareness on
    // connect. Wait for at least one binary frame to confirm the
    // handshake reached the per-session task.
    let first = tokio::time::timeout(Duration::from_secs(2), conn_a.next())
        .await
        .expect("client A initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("client A initial frame error");
    match first {
        ws::Frame::Binary(_) => {} // expected: SyncStep1
        other => panic!("client A: expected Binary SyncStep1, got {other:?}"),
    }

    // --- Client B connects (same connection token in the URL) ---
    let (_resp_b, mut conn_b) = client
        .ws(&url)
        .connect()
        .await
        .expect("client B WS handshake");
    let _ = tokio::time::timeout(Duration::from_secs(2), conn_b.next())
        .await
        .expect("client B initial frame timeout");

    // --- Broadcast: A sends, B receives ---
    // ws_handler only fans a frame out to peers when it parses via the
    // Yjs sync protocol AND is a real-data frame: SyncStep2, SyncUpdate,
    // or Awareness. SyncStep1 is a point-to-point state-vector request
    // and is deliberately NOT rebroadcast (see process_inbound_binary's
    // should_broadcast). So construct a SyncUpdate (subtype 2) carrying
    // a real yrs update; the server applies it and relays the inbound
    // bytes verbatim to the other client. The payload is opaque to our
    // assertion: we only check the same bytes reach client B.
    let payload: Bytes = {
        use yrs::sync::{Message, SyncMessage};
        use yrs::updates::encoder::Encode;
        use yrs::{Doc, ReadTxn, StateVector, Text, Transact};
        let doc = Doc::new();
        let text = doc.get_or_insert_text("content");
        {
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, 0, "x");
        }
        let update = {
            let txn = doc.transact();
            txn.encode_state_as_update_v1(&StateVector::default())
        };
        let msg = Message::Sync(SyncMessage::Update(update));
        Bytes::from(msg.encode_v1())
    };
    conn_a
        .send(ws::Message::Binary(payload.clone()))
        .await
        .expect("client A send");

    // Drain B until we see our payload or hit the timeout. Other
    // messages (awareness updates, sync responses) may arrive first.
    let saw_payload = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match conn_b.next().await {
                Some(Ok(ws::Frame::Binary(b))) if b.as_ref() == payload.as_ref() => break true,
                Some(Ok(_)) => continue,
                Some(Err(e)) => panic!("client B stream error: {e:?}"),
                None => break false,
            }
        }
    })
    .await
    .expect("client B receive timeout");
    assert!(saw_payload, "client B never received the broadcast payload");

    // --- Clean disconnect: drop A, B should remain alive ---
    drop(conn_a);
    // Give the server a moment to process A's disconnect.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // B sends a Ping and we drain frames until the matching Pong
    // arrives. The server's own heartbeat Pings (at 100ms in tests
    // via install_fast_heartbeat) interleave with our Pong, so we
    // can't take the next frame blindly. The payload tag identifies
    // ours unambiguously.
    let probe = Bytes::from_static(b"ping-after-A-left");
    conn_b
        .send(ws::Message::Ping(probe.clone()))
        .await
        .expect("client B send after A disconnect");
    let got_pong = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match conn_b.next().await {
                Some(Ok(ws::Frame::Pong(b))) if b.as_ref() == probe.as_ref() => break true,
                Some(Ok(_)) => continue, // server heartbeat Ping, intermediate broadcasts
                Some(Err(e)) => panic!("client B stream error after A left: {e:?}"),
                None => break false,
            }
        }
    })
    .await
    .expect("client B Pong timeout after A disconnect");
    assert!(got_pong, "client B never received Pong for its probe Ping");
}

/// Selection-path handshake (Model C, increment 2): with NO Host-derived
/// WorkspaceContext on the request (the single-origin agent app), the collab WS
/// resolves the workspace from the docId's embedded workspace_uuid and
/// membership-gates it. The WS can't send the selection header, so the docId is
/// the carrier. A member of the docId's workspace connects; a non-member is
/// rejected at the handshake.
#[actix_web::test]
async fn handshake_resolves_workspace_from_doc_id_without_host_context() {
    install_fast_heartbeat();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());

    // Bootstrap workspace 1's real uuid is the docId's tenancy anchor.
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;

    // A member of workspace 1 and a stranger who belongs to no workspace.
    let member = common::insert_user(&mut pool.get().expect("conn"), "DocSel Member");
    let stranger = common::insert_user(&mut pool.get().expect("conn"), "DocSel Stranger");
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(member.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                backend::repository::workspaces::add_membership(
                    c,
                    1,
                    member.uuid,
                    "admin",
                    backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                )?;
                Ok(())
            },
        )
        .expect("seed membership");
    }

    // Sessionless, workspace-bound connection token per user; the WS reads it
    // from the query string (no cookie / Authorization header on a browser WS).
    let token_for = |user: &backend::models::User| {
        JwtUtils::create_collab_token(&user.uuid.to_string(), &user.platform_role, Some(ws1_uuid))
            .expect("mint collab token")
    };
    let member_token = token_for(&member);
    let stranger_token = token_for(&stranger);

    let ticket_uuid = seed_ticket(&mut pool.get().expect("conn"));

    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(_tmp);
        // No WorkspaceContext injected: the handler must derive the workspace
        // from the docId, exercising the selection (None-context) branch.
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });

    // docId carries workspace 1's real uuid so find_by_uuid resolves it.
    let doc_id = format!("ws-{ws1_uuid}_ticket-{ticket_uuid}");
    let member_url = srv.url(&format!("/ws/{doc_id}?token={member_token}"));
    let stranger_url = srv.url(&format!("/ws/{doc_id}?token={stranger_token}"));
    let client = awc::Client::new();

    // Member: handshake succeeds and the initial SyncStep1 frame arrives.
    let (_resp, mut conn) = client
        .ws(&member_url)
        .connect()
        .await
        .expect("member handshake should succeed via docId-derived workspace");
    let first = tokio::time::timeout(Duration::from_secs(2), conn.next())
        .await
        .expect("initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("initial frame error");
    assert!(
        matches!(first, ws::Frame::Binary(_)),
        "expected Binary SyncStep1, got {first:?}"
    );

    // Non-member: the socket opens and is closed straight away as "no access".
    assert_eq!(
        refusal_code(&client, &stranger_url).await,
        4403,
        "a non-member is refused at the docId-derived handshake"
    );
}

/// Connect, and return the close code the server ends the connection with
/// before sending anything else.
async fn refusal_code(client: &awc::Client, url: &str) -> u16 {
    let (_resp, mut conn) = client
        .ws(url)
        .connect()
        .await
        .unwrap_or_else(|e| panic!("the upgrade is accepted, then closed: {e}"));
    let frame = tokio::time::timeout(Duration::from_secs(2), conn.next())
        .await
        .expect("close frame timeout")
        .expect("stream ended before a close frame")
        .expect("close frame error");
    match frame {
        ws::Frame::Close(Some(reason)) => u16::from(reason.code),
        other => panic!("expected a close frame with a code, got {other:?}"),
    }
}

/// A browser can't see why a WebSocket upgrade failed (it reports close code
/// 1006, and y-websocket retries forever), so a refused connection is
/// upgraded and closed at once with a code that says why: 4401 for no usable
/// token, 4403 for no access, 4404 for a document that doesn't exist.
#[actix_web::test]
async fn a_refused_connection_is_closed_with_why() {
    install_fast_heartbeat();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;

    // An admin of workspace 1, and a requester there who isn't on the ticket
    // (a plain account: `insert_user` mints a platform admin, who sees all).
    let admin = common::insert_user(&mut pool.get().expect("conn"), "Refusal Admin");
    let requester = common::insert_plain_user(&mut pool.get().expect("conn"), "Refusal Requester");
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(admin.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                for (user, role) in [(admin.uuid, "admin"), (requester, "member")] {
                    backend::repository::workspaces::add_membership(
                        c,
                        1,
                        user,
                        role,
                        backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                    )?;
                }
                Ok(())
            },
        )
        .expect("seed memberships");
    }
    let token_for = |user: uuid::Uuid, platform_role: &str| {
        JwtUtils::create_collab_token(&user.to_string(), platform_role, Some(ws1_uuid))
            .expect("mint collab token")
    };
    let ticket_uuid = seed_ticket(&mut pool.get().expect("conn"));

    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let client = awc::Client::new();
    let ticket_doc = format!("ws-{ws1_uuid}_ticket-{ticket_uuid}");
    let missing_doc = format!("ws-{ws1_uuid}_ticket-{}", uuid::Uuid::new_v4());

    for (url, code, what) in [
        (
            srv.url(&format!(
                "/ws/{ticket_doc}?token={}",
                token_for(requester, "user")
            )),
            4403,
            "a member who can't see the ticket",
        ),
        (
            srv.url(&format!(
                "/ws/{missing_doc}?token={}",
                token_for(admin.uuid, &admin.platform_role)
            )),
            4404,
            "a document that doesn't exist",
        ),
        (
            srv.url(&format!("/ws/{ticket_doc}?token=not-a-token")),
            4401,
            "a token that doesn't validate",
        ),
        (srv.url(&format!("/ws/{ticket_doc}")), 4401, "no token"),
    ] {
        assert_eq!(refusal_code(&client, &url).await, code, "{what}");
    }

    // The admin still connects to the ticket's note.
    let (_resp, mut conn) = client
        .ws(srv.url(&format!(
            "/ws/{ticket_doc}?token={}",
            token_for(admin.uuid, &admin.platform_role)
        )))
        .connect()
        .await
        .expect("an admin connects");
    let first = tokio::time::timeout(Duration::from_secs(2), conn.next())
        .await
        .expect("initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("initial frame error");
    assert!(
        matches!(first, ws::Frame::Binary(_)),
        "expected Binary SyncStep1, got {first:?}"
    );
}

/// A user who isn't staff opens a document read-only: their document
/// updates reach neither the document nor anyone else in the room, while
/// staff edits still reach them. Here the requester of a ticket opens its
/// notes beside an agent.
#[actix_web::test]
async fn a_requesters_updates_reach_no_one() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());

    let person = |name: &str| -> backend::models::User {
        diesel::insert_into(backend::schema::users::table)
            .values(&backend::models::NewUser {
                uuid: uuid::Uuid::new_v4(),
                name: name.to_string(),
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
            .get_result(&mut pool.get().expect("conn"))
            .expect("insert user")
    };
    let requester = person("WSRequester");
    let agent = person("WSAgent");
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(agent.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                for (user, role) in [(&requester, "member"), (&agent, "agent")] {
                    backend::repository::workspaces::add_membership(
                        c,
                        1,
                        user.uuid,
                        role,
                        backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                    )?;
                }
                Ok(())
            },
        )
        .expect("seed workspace memberships");
    }

    // The requester's own ticket, so the visibility gate lets them open it.
    let ticket_uuid = {
        use backend::schema::{tickets, workflow_states};
        let mut conn = pool.get().expect("conn");
        let state_id: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(1))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(&mut conn)
            .expect("default workflow state seeded");
        let t: backend::models::Ticket = diesel::insert_into(tickets::table)
            .values(&backend::models::NewTicket {
                title: "Requester's ticket".to_string(),
                workflow_state_id: state_id,
                requester_uuid: Some(requester.uuid),
                ..Default::default()
            })
            .get_result(&mut conn)
            .expect("insert ticket");
        t.uuid
    };

    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(tmp);
        let ws = test_workspace();
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                srv.call(req)
            })
            .route("/ws/{doc}", web::get().to(ws_handler))
    });

    let doc_id = format!(
        "ws-{}_ticket-{ticket_uuid}",
        test_workspace().workspace_uuid
    );
    let client = awc::Client::new();
    let connect = |user: &backend::models::User| {
        let token = JwtUtils::create_collab_token(
            &user.uuid.to_string(),
            &user.platform_role,
            Some(test_workspace().workspace_uuid),
        )
        .expect("mint collab token");
        client
            .ws(srv.url(&format!("/ws/{doc_id}?token={token}")))
            .connect()
    };
    let (_, mut theirs) = connect(&requester).await.expect("requester handshake");
    let (_, mut staff) = connect(&agent).await.expect("agent handshake");
    for conn in [&mut theirs, &mut staff] {
        let _ = tokio::time::timeout(Duration::from_secs(2), conn.next())
            .await
            .expect("initial frame timeout");
    }

    // A SyncUpdate inserting `text`, as a client editing the document sends.
    let update = |text: &str| -> Bytes {
        use yrs::sync::{Message, SyncMessage};
        use yrs::updates::encoder::Encode;
        use yrs::{Doc, ReadTxn, StateVector, Text, Transact};
        let doc = Doc::new();
        let content = doc.get_or_insert_text("content");
        content.insert(&mut doc.transact_mut(), 0, text);
        let update = doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        Bytes::from(Message::Sync(SyncMessage::Update(update)).encode_v1())
    };
    // Whether `frame` arrives on `conn` within `wait`.
    async fn receives<S, E>(conn: &mut S, frame: &Bytes, wait: Duration) -> bool
    where
        S: futures_util::Stream<Item = Result<ws::Frame, E>> + Unpin,
        E: std::fmt::Debug,
    {
        tokio::time::timeout(wait, async {
            loop {
                match conn.next().await {
                    Some(Ok(ws::Frame::Binary(b))) if b.as_ref() == frame.as_ref() => break true,
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => panic!("stream error: {e:?}"),
                    None => break false,
                }
            }
        })
        .await
        .unwrap_or(false)
    }

    let edit = update("from the requester");
    theirs
        .send(ws::Message::Binary(edit.clone()))
        .await
        .expect("requester send");
    assert!(
        !receives(&mut staff, &edit, Duration::from_millis(800)).await,
        "a requester's update must not reach the room"
    );

    // y-sync allows several messages in one frame: presence first, then an
    // edit. The edit still mustn't reach anyone.
    let packed = {
        use yrs::sync::{Awareness, Message};
        use yrs::updates::encoder::Encode;
        let awareness = Awareness::new(yrs::Doc::new());
        awareness.set_local_state("{}").expect("local state");
        let mut frame =
            Message::Awareness(awareness.update().expect("awareness update")).encode_v1();
        frame.extend_from_slice(&update("packed after presence"));
        Bytes::from(frame)
    };
    theirs
        .send(ws::Message::Binary(packed.clone()))
        .await
        .expect("requester send");
    assert!(
        !receives(&mut staff, &packed, Duration::from_millis(800)).await,
        "an edit packed after presence must not reach the room"
    );

    let edit = update("from the agent");
    staff
        .send(ws::Message::Binary(edit.clone()))
        .await
        .expect("agent send");
    assert!(
        receives(&mut theirs, &edit, Duration::from_secs(3)).await,
        "the requester still receives staff edits"
    );
}

/// The revisions stored for a ticket's note, oldest first.
fn ticket_revisions(conn: &mut diesel::pg::PgConnection, ticket_uuid: uuid::Uuid) -> Vec<Vec<u8>> {
    use backend::schema::{article_content_revisions, article_contents, tickets};
    use diesel::prelude::*;
    let ticket_id: i32 = tickets::table
        .filter(tickets::uuid.eq(ticket_uuid))
        .select(tickets::id)
        .first(conn)
        .expect("ticket");
    let Some(article_id) = article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .select(article_contents::id)
        .first::<i32>(conn)
        .optional()
        .expect("article content lookup")
    else {
        return Vec::new();
    };
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_id))
        .order(article_content_revisions::revision_number.asc())
        .select(article_content_revisions::yjs_document_content)
        .load(conn)
        .expect("revisions")
}

/// The `prosemirror` fragment a stored document holds, as XML.
fn fragment_text(document: &[u8]) -> String {
    use yrs::updates::decoder::Decode;
    use yrs::{Doc, GetString, Transact, Update, WriteTxn};
    let doc = Doc::new();
    doc.transact_mut()
        .apply_update(Update::decode_v1(document).expect("decode document"))
        .expect("apply document");
    let mut txn = doc.transact_mut();
    let fragment = txn.get_or_insert_xml_fragment("prosemirror");
    fragment.get_string(&txn)
}

/// The `content` text a stored revision holds.
fn revision_text(revision: &[u8]) -> String {
    use yrs::updates::decoder::Decode;
    use yrs::{Doc, GetString, Transact, Update};
    let doc = Doc::new();
    let text = doc.get_or_insert_text("content");
    doc.transact_mut()
        .apply_update(Update::decode_v1(revision).expect("decode revision"))
        .expect("apply revision");
    let txn = doc.transact();
    text.get_string(&txn)
}

/// Someone keeping a note open (a second tab, a colleague reading along) no
/// longer holds back its revision: once nobody has changed it for the quiet
/// period, the edits since the last revision become one. Revisions used to
/// wait for the room to empty.
#[actix_web::test]
async fn a_quiet_note_gets_a_revision_while_someone_keeps_it_open() {
    use yrs::sync::{Message, SyncMessage};
    use yrs::updates::encoder::Encode;
    use yrs::{Doc, ReadTxn, StateVector, Transact, WriteTxn, XmlFragment, XmlTextPrelim};

    install_fast_heartbeat();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let user = common::insert_user(&mut pool.get().expect("conn"), "WSRevisions");
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(user.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                backend::repository::workspaces::add_membership(
                    c,
                    1,
                    user.uuid,
                    "admin",
                    backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                )?;
                Ok(())
            },
        )
        .expect("seed workspace membership");
    }
    let token = JwtUtils::create_collab_token(
        &user.uuid.to_string(),
        &user.platform_role,
        Some(test_workspace().workspace_uuid),
    )
    .expect("mint collab token");
    let ticket_uuid = seed_ticket(&mut pool.get().expect("conn"));

    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(_tmp);
        let ws = test_workspace();
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                srv.call(req)
            })
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let doc_id = format!(
        "ws-{}_ticket-{ticket_uuid}",
        test_workspace().workspace_uuid
    );
    let url = srv.url(&format!("/ws/{doc_id}?token={token}"));
    let client = awc::Client::new();

    // A has the note open for the whole test.
    let (_resp_a, mut open_tab) = client.ws(&url).connect().await.expect("A handshake");
    // B writes the note, then leaves.
    let (_resp_b, mut writer) = client.ws(&url).connect().await.expect("B handshake");
    let edit: Bytes = {
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment("prosemirror");
            fragment.push_back(&mut txn, XmlTextPrelim::new("revision one"));
        }
        let update = doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        Bytes::from(Message::Sync(SyncMessage::Update(update)).encode_v1())
    };
    writer
        .send(ws::Message::Binary(edit.clone()))
        .await
        .expect("B send");
    // The edit reaching A means the server applied it.
    let applied = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match open_tab.next().await {
                Some(Ok(ws::Frame::Binary(b))) if b.as_ref() == edit.as_ref() => break true,
                Some(Ok(_)) => continue,
                Some(Err(e)) => panic!("A stream error: {e:?}"),
                None => break false,
            }
        }
    })
    .await
    .expect("A never saw B's edit");
    assert!(applied, "A's stream ended before B's edit arrived");
    drop(writer);

    // Only binary frames count as activity, so A keeps asking for the state
    // vector (which changes nothing) to stay in the room past the stale cut.
    async fn keep_open<S, E>(conn: &mut S, wait: Duration)
    where
        S: futures_util::Stream<Item = Result<ws::Frame, E>>
            + futures_util::Sink<ws::Message>
            + Unpin,
        E: std::fmt::Debug,
        <S as futures_util::Sink<ws::Message>>::Error: std::fmt::Debug,
    {
        let state_request =
            Bytes::from(Message::Sync(SyncMessage::SyncStep1(StateVector::default())).encode_v1());
        let until = tokio::time::Instant::now() + wait;
        while tokio::time::Instant::now() < until {
            conn.send(ws::Message::Binary(state_request.clone()))
                .await
                .expect("A keepalive");
            while let Ok(Some(frame)) =
                tokio::time::timeout(Duration::from_millis(20), conn.next()).await
            {
                frame.expect("A stream error");
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    }

    let mut revisions = Vec::new();
    for _ in 0..20 {
        keep_open(&mut open_tab, Duration::from_millis(100)).await;
        revisions = ticket_revisions(&mut pool.get().expect("conn"), ticket_uuid);
        if !revisions.is_empty() {
            break;
        }
    }
    assert_eq!(
        revisions.len(),
        1,
        "the quiet note got a revision while A kept it open"
    );
    assert_eq!(fragment_text(&revisions[0]), "revision one");

    // Quiet with nothing new to record: no further revision.
    keep_open(&mut open_tab, Duration::from_millis(800)).await;
    assert_eq!(
        ticket_revisions(&mut pool.get().expect("conn"), ticket_uuid).len(),
        1,
        "an unchanged note gets no second revision"
    );
}

/// Someone with a page open in the editor who loses access to it is closed
/// out with "no access", which tells the client to stop reconnecting. The
/// change reaches the editor the way it reaches every machine: through the
/// sync feed.
#[actix_web::test]
async fn losing_access_closes_an_open_page_with_no_access() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let admin = common::insert_user(&mut pool.get().expect("conn"), "Revoke Admin");
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Revoke Agent");
    let in_workspace =
        |f: &mut dyn FnMut(&mut backend::db::DbConnection) -> Result<(), diesel::result::Error>| {
            let mut conn = pool.get().expect("conn");
            let actor =
                backend::sync::actor::ActorContext::user(admin.uuid, None).with_workspace(1);
            backend::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &actor,
                |c| f(c),
            )
            .expect("write in workspace 1");
        };
    in_workspace(&mut |c| {
        for (user, role) in [(admin.uuid, "admin"), (agent, "agent")] {
            backend::repository::workspaces::add_membership(
                c,
                1,
                user,
                role,
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
        }
        Ok(())
    });
    let page_uuid = uuid::Uuid::new_v4();
    let mut page_id = 0;
    in_workspace(&mut |c| {
        use backend::schema::documentation_pages as dp;
        page_id = diesel::insert_into(dp::table)
            .values((
                dp::uuid.eq(page_uuid),
                dp::title.eq("Runbook"),
                dp::slug.eq(format!("runbook-{}", page_uuid.simple())),
                dp::created_by.eq(admin.uuid),
                dp::last_edited_by.eq(admin.uuid),
            ))
            .returning(dp::id)
            .get_result(c)?;
        Ok(())
    });

    // The sync outbox feeds the live-event state every machine's editor
    // listens to.
    let sse = Arc::new(SseState::new());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _outbox = backend::services::sync_outbox::spawn(
        test_db.url().to_string(),
        pool.clone(),
        sse.clone(),
        shutdown.clone(),
    );
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) =
            build_app_state_with_sse(&state_pool_inner, web::Data::from(sse.clone()));
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });

    let token = JwtUtils::create_collab_token(&agent.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the agent opens the page");
    let first = tokio::time::timeout(Duration::from_secs(2), conn.next())
        .await
        .expect("initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("initial frame error");
    assert!(
        matches!(first, ws::Frame::Binary(_)),
        "expected Binary SyncStep1, got {first:?}"
    );

    // The page is restricted to the admin. The feed delivers only settled
    // rows (the commit horizon is cluster-wide, so another test's open
    // transaction, even another process's, can hold it back for a while), so
    // the page row is re-sent until it lands.
    in_workspace(&mut |c| {
        backend::repository::set_page_visibility(c, page_id, vec![], vec![admin.uuid], None)
            .map(|_| ())
    });
    let closed_with = closed_within(&mut conn, 60, || {
        in_workspace(&mut |c| {
            backend::repository::emit_page_row(c, page_id, "documentation_page.metadata_changed")
        })
    })
    .await;
    shutdown.cancel();
    assert_eq!(
        closed_with,
        Some(4403),
        "the editor is closed with no access once the page is restricted"
    );
}

/// Access withdrawn between the connect-time check and the session joining
/// its room still closes the session: it checks again once it has joined, so
/// a change the editor heard before the session was in the room isn't missed.
#[actix_web::test]
async fn access_withdrawn_while_connecting_still_closes_the_page() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let admin = common::insert_user(&mut pool.get().expect("conn"), "Race Admin");
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Race Agent");
    let in_workspace =
        |f: &mut dyn FnMut(&mut backend::db::DbConnection) -> Result<(), diesel::result::Error>| {
            let mut conn = pool.get().expect("conn");
            let actor =
                backend::sync::actor::ActorContext::user(admin.uuid, None).with_workspace(1);
            backend::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &actor,
                |c| f(c),
            )
            .expect("write in workspace 1");
        };
    in_workspace(&mut |c| {
        for (user, role) in [(admin.uuid, "admin"), (agent, "agent")] {
            backend::repository::workspaces::add_membership(
                c,
                1,
                user,
                role,
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
        }
        Ok(())
    });
    let page_uuid = uuid::Uuid::new_v4();
    let mut page_id = 0;
    in_workspace(&mut |c| {
        use backend::schema::documentation_pages as dp;
        page_id = diesel::insert_into(dp::table)
            .values((
                dp::uuid.eq(page_uuid),
                dp::title.eq("Rota"),
                dp::slug.eq(format!("rota-{}", page_uuid.simple())),
                dp::created_by.eq(admin.uuid),
                dp::last_edited_by.eq(admin.uuid),
            ))
            .returning(dp::id)
            .get_result(c)?;
        Ok(())
    });

    // No sync outbox here: the only way the session learns of the change is
    // the check it makes once it has joined.
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let token = JwtUtils::create_collab_token(&agent.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the agent passes the connect-time check");
    in_workspace(&mut |c| {
        backend::repository::set_page_visibility(c, page_id, vec![], vec![admin.uuid], None)
            .map(|_| ())
    });

    let mut closed_with = None;
    let wait = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(wait);
    loop {
        tokio::select! {
            _ = &mut wait => break,
            frame = conn.next() => match frame {
                Some(Ok(ws::Frame::Close(reason))) => {
                    closed_with = Some(reason.map_or(0, |r| u16::from(r.code)));
                    break;
                }
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => {
                    closed_with = Some(0);
                    break;
                }
            },
        }
    }
    assert_eq!(
        closed_with,
        Some(4403),
        "the session is closed with no access once it has joined"
    );
}

/// Someone with a page open who leaves the only group it is shared with is
/// closed out with "no access", like a change to the page's own rules.
#[actix_web::test]
async fn leaving_a_group_closes_an_open_page_with_no_access() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let admin = common::insert_user(&mut pool.get().expect("conn"), "Group Admin");
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Group Agent");
    let in_workspace =
        |f: &mut dyn FnMut(&mut backend::db::DbConnection) -> Result<(), diesel::result::Error>| {
            let mut conn = pool.get().expect("conn");
            let actor =
                backend::sync::actor::ActorContext::user(admin.uuid, None).with_workspace(1);
            backend::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &actor,
                |c| f(c),
            )
            .expect("write in workspace 1");
        };
    let page_uuid = uuid::Uuid::new_v4();
    let (mut page_id, mut spare_id, mut group_id) = (0, 0, 0);
    in_workspace(&mut |c| {
        use backend::schema::documentation_pages as dp;
        for (user, role) in [(admin.uuid, "admin"), (agent, "agent")] {
            backend::repository::workspaces::add_membership(
                c,
                1,
                user,
                role,
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
        }
        group_id = backend::repository::groups::create_group(
            c,
            backend::models::NewGroup {
                name: "On call".to_string(),
                description: None,
                color: None,
                created_by: None,
            },
        )?
        .id;
        backend::repository::groups::add_user_to_group(c, agent, group_id, None)?;
        let mut insert = |uuid: uuid::Uuid, title: &str| {
            diesel::insert_into(dp::table)
                .values((
                    dp::uuid.eq(uuid),
                    dp::title.eq(title),
                    dp::slug.eq(format!("{}-{}", title.to_lowercase(), uuid.simple())),
                    dp::created_by.eq(admin.uuid),
                    dp::last_edited_by.eq(admin.uuid),
                ))
                .returning(dp::id)
                .get_result::<i32>(c)
        };
        page_id = insert(page_uuid, "Escalations")?;
        spare_id = insert(uuid::Uuid::new_v4(), "Spare")?;
        backend::repository::set_page_visibility(c, page_id, vec![group_id], vec![], None)?;
        Ok(())
    });

    let sse = Arc::new(SseState::new());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _outbox = backend::services::sync_outbox::spawn(
        test_db.url().to_string(),
        pool.clone(),
        sse.clone(),
        shutdown.clone(),
    );
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) =
            build_app_state_with_sse(&state_pool_inner, web::Data::from(sse.clone()));
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let token = JwtUtils::create_collab_token(&agent.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the agent opens the page");
    // The session has joined its room once it sends its first frame.
    let first = tokio::time::timeout(Duration::from_secs(3), conn.next())
        .await
        .expect("initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("initial frame error");
    assert!(
        matches!(first, ws::Frame::Binary(_)),
        "expected Binary SyncStep1, got {first:?}"
    );

    in_workspace(&mut |c| {
        backend::repository::groups::remove_user_from_group(c, &agent, group_id).map(|_| ())
    });
    let closed_with = closed_within(&mut conn, 60, || {
        in_workspace(&mut |c| {
            backend::repository::emit_page_row(c, spare_id, "documentation_page.metadata_changed")
        })
    })
    .await;
    shutdown.cancel();
    assert_eq!(
        closed_with,
        Some(4403),
        "the editor is closed with no access once the agent leaves the group"
    );
}

/// Wait up to `rounds` half-seconds for the server to close `conn`, running
/// `poke` between rounds (the feed delivers only settled rows, and a later
/// write wakes it again; a test of a re-send pokes with an unrelated page, so
/// only the re-send can close the session). Each round also asks the server for the document,
/// as an editor in use does, so the session isn't swept as idle meanwhile.
/// The close code, or `None` when it stays open.
async fn closed_within<S, E>(conn: &mut S, rounds: usize, mut poke: impl FnMut()) -> Option<u16>
where
    S: futures_util::Stream<Item = Result<ws::Frame, E>> + futures_util::Sink<ws::Message> + Unpin,
{
    use yrs::updates::encoder::Encode;
    let ask = yrs::sync::Message::Sync(yrs::sync::SyncMessage::SyncStep1(
        yrs::StateVector::default(),
    ))
    .encode_v1();
    for _ in 0..rounds {
        let _ = conn
            .send(ws::Message::Binary(Bytes::from(ask.clone())))
            .await;
        let wait = tokio::time::sleep(Duration::from_millis(500));
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                frame = conn.next() => match frame {
                    Some(Ok(ws::Frame::Close(reason))) => {
                        return Some(reason.map_or(0, |r| u16::from(r.code)));
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(_)) | None => return Some(0),
                },
            }
        }
        poke();
    }
    None
}

/// An admin with a restricted page open who stops being one is closed out
/// with "no access", whichever route set their role.
#[actix_web::test]
async fn losing_admin_closes_an_open_restricted_page() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let owner = common::insert_user(&mut pool.get().expect("conn"), "Owner");
    let admin = common::insert_plain_user(&mut pool.get().expect("conn"), "Workspace Admin");
    let in_workspace =
        |f: &mut dyn FnMut(&mut backend::db::DbConnection) -> Result<(), diesel::result::Error>| {
            let mut conn = pool.get().expect("conn");
            let actor =
                backend::sync::actor::ActorContext::user(owner.uuid, None).with_workspace(1);
            backend::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &actor,
                |c| f(c),
            )
            .expect("write in workspace 1");
        };
    let page_uuid = uuid::Uuid::new_v4();
    let (mut page_id, mut spare_id) = (0, 0);
    in_workspace(&mut |c| {
        use backend::schema::documentation_pages as dp;
        for (user, role) in [(owner.uuid, "admin"), (admin, "admin")] {
            backend::repository::workspaces::add_membership(
                c,
                1,
                user,
                role,
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
        }
        let mut insert = |uuid: uuid::Uuid, title: &str| {
            diesel::insert_into(dp::table)
                .values((
                    dp::uuid.eq(uuid),
                    dp::title.eq(title),
                    dp::slug.eq(format!("{}-{}", title.to_lowercase(), uuid.simple())),
                    dp::created_by.eq(owner.uuid),
                    dp::last_edited_by.eq(owner.uuid),
                ))
                .returning(dp::id)
                .get_result::<i32>(c)
        };
        page_id = insert(page_uuid, "Payroll")?;
        spare_id = insert(uuid::Uuid::new_v4(), "Spare")?;
        backend::repository::set_page_visibility(c, page_id, vec![], vec![owner.uuid], None)?;
        Ok(())
    });

    let sse = Arc::new(SseState::new());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _outbox = backend::services::sync_outbox::spawn(
        test_db.url().to_string(),
        pool.clone(),
        sse.clone(),
        shutdown.clone(),
    );
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) =
            build_app_state_with_sse(&state_pool_inner, web::Data::from(sse.clone()));
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let token = JwtUtils::create_collab_token(&admin.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the admin opens the page");
    let first = tokio::time::timeout(Duration::from_secs(3), conn.next())
        .await
        .expect("initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("initial frame error");
    assert!(matches!(first, ws::Frame::Binary(_)), "got {first:?}");

    // The ordinary self-hosted role edit.
    in_workspace(&mut |c| {
        backend::repository::users::set_user_roles(
            c,
            1,
            admin,
            "user",
            "agent",
            backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .map(|_| ())
    });
    let closed_with = closed_within(&mut conn, 60, || {
        in_workspace(&mut |c| {
            backend::repository::emit_page_row(c, spare_id, "documentation_page.metadata_changed")
        })
    })
    .await;
    shutdown.cancel();
    assert_eq!(closed_with, Some(4403), "closed with no access");
}

/// When the editor can't check access again, it closes the session with a
/// code the client retries rather than keep serving a page it can't vouch
/// for.
#[actix_web::test]
async fn an_access_check_that_fails_closes_the_session_to_retry() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let owner = common::insert_user(&mut pool.get().expect("conn"), "Owner");
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    let page_uuid = uuid::Uuid::new_v4();
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(owner.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                use backend::schema::documentation_pages as dp;
                backend::repository::workspaces::add_membership(
                    c,
                    1,
                    agent,
                    "agent",
                    backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                )?;
                diesel::insert_into(dp::table)
                    .values((
                        dp::uuid.eq(page_uuid),
                        dp::title.eq("Handbook"),
                        dp::slug.eq(format!("handbook-{}", page_uuid.simple())),
                        dp::created_by.eq(owner.uuid),
                        dp::last_edited_by.eq(owner.uuid),
                    ))
                    .execute(c)?;
                Ok(())
            },
        )
        .expect("seed");
    }
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let token = JwtUtils::create_collab_token(&agent.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the agent passes the connect-time check");
    // Between the handshake and the join, the check it makes on joining
    // stops working.
    diesel::sql_query(
        "ALTER TABLE documentation_collection_pages RENAME TO documentation_collection_pages_gone",
    )
    .execute(&mut pool.get().expect("conn"))
    .expect("break the access check");

    let closed_with = closed_within(&mut conn, 6, || {}).await;
    assert_eq!(
        closed_with,
        Some(1013),
        "closed with a code the client retries"
    );
}

/// Someone removed from the workspace has every session they hold open there
/// closed, an open page that isn't restricted included: edit rights are fixed
/// when a session connects.
#[actix_web::test]
async fn removal_from_the_workspace_closes_every_open_page() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let owner = common::insert_user(&mut pool.get().expect("conn"), "Owner");
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Leaving Agent");
    let in_workspace =
        |f: &mut dyn FnMut(&mut backend::db::DbConnection) -> Result<(), diesel::result::Error>| {
            let mut conn = pool.get().expect("conn");
            let actor =
                backend::sync::actor::ActorContext::user(owner.uuid, None).with_workspace(1);
            backend::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &actor,
                |c| f(c),
            )
            .expect("write in workspace 1");
        };
    let page_uuid = uuid::Uuid::new_v4();
    let mut spare_id = 0;
    in_workspace(&mut |c| {
        use backend::schema::documentation_pages as dp;
        for (user, role) in [(owner.uuid, "admin"), (agent, "agent")] {
            backend::repository::workspaces::add_membership(
                c,
                1,
                user,
                role,
                backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
            )?;
        }
        let mut insert = |uuid: uuid::Uuid, title: &str| {
            diesel::insert_into(dp::table)
                .values((
                    dp::uuid.eq(uuid),
                    dp::title.eq(title),
                    dp::slug.eq(format!("{}-{}", title.to_lowercase(), uuid.simple())),
                    dp::created_by.eq(owner.uuid),
                    dp::last_edited_by.eq(owner.uuid),
                ))
                .returning(dp::id)
                .get_result::<i32>(c)
        };
        insert(page_uuid, "Handbook")?;
        spare_id = insert(uuid::Uuid::new_v4(), "Spare")?;
        Ok(())
    });

    let sse = Arc::new(SseState::new());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let _outbox = backend::services::sync_outbox::spawn(
        test_db.url().to_string(),
        pool.clone(),
        sse.clone(),
        shutdown.clone(),
    );
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) =
            build_app_state_with_sse(&state_pool_inner, web::Data::from(sse.clone()));
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let token = JwtUtils::create_collab_token(&agent.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the agent opens the page");
    let first = tokio::time::timeout(Duration::from_secs(3), conn.next())
        .await
        .expect("initial frame timeout")
        .expect("stream ended before initial frame")
        .expect("initial frame error");
    assert!(matches!(first, ws::Frame::Binary(_)), "got {first:?}");

    in_workspace(&mut |c| {
        backend::repository::workspaces::remove_membership(
            c,
            1,
            agent,
            backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .map(|_| ())
    });
    let closed_with = closed_within(&mut conn, 60, || {
        in_workspace(&mut |c| {
            backend::repository::emit_page_row(c, spare_id, "documentation_page.metadata_changed")
        })
    })
    .await;
    shutdown.cancel();
    assert_eq!(closed_with, Some(4403), "closed with no access");
}

/// A membership lookup that fails while the editor checks access again is an
/// error, not a removal: the session closes to retry, not for good.
#[actix_web::test]
async fn a_failed_membership_lookup_closes_the_session_to_retry() {
    use diesel::prelude::*;

    install_fast_heartbeat();
    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let ws1_uuid = backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
        .expect("ws lookup")
        .expect("bootstrap workspace exists")
        .uuid;
    let owner = common::insert_user(&mut pool.get().expect("conn"), "Owner");
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Agent");
    let page_uuid = uuid::Uuid::new_v4();
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(owner.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                use backend::schema::documentation_pages as dp;
                backend::repository::workspaces::add_membership(
                    c,
                    1,
                    agent,
                    "agent",
                    backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                )?;
                diesel::insert_into(dp::table)
                    .values((
                        dp::uuid.eq(page_uuid),
                        dp::title.eq("Handbook"),
                        dp::slug.eq(format!("handbook-{}", page_uuid.simple())),
                        dp::created_by.eq(owner.uuid),
                        dp::last_edited_by.eq(owner.uuid),
                    ))
                    .execute(c)?;
                Ok(())
            },
        )
        .expect("seed");
    }
    let state_pool_inner = pool.clone();
    let srv = actix_test::start(move || {
        let (state, _tmp) = build_app_state(&state_pool_inner);
        std::mem::forget(_tmp);
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(state_pool_inner.clone()))
            .route("/ws/{doc}", web::get().to(ws_handler))
    });
    let token = JwtUtils::create_collab_token(&agent.to_string(), "user", Some(ws1_uuid))
        .expect("mint collab token");
    let (_resp, mut conn) = awc::Client::new()
        .ws(srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}")))
        .connect()
        .await
        .expect("the agent passes the connect-time check");
    // Between the handshake and the join, membership stops being readable.
    diesel::sql_query("ALTER TABLE workspace_members RENAME TO workspace_members_gone")
        .execute(&mut pool.get().expect("conn"))
        .expect("break the membership lookup");

    let closed_with = closed_within(&mut conn, 6, || {}).await;
    assert_eq!(
        closed_with,
        Some(1013),
        "closed with a code the client retries"
    );
}

/// The text a stored Yjs document holds in its `content` root, or `None`
/// when nothing is stored.
fn stored_text(document: Option<Vec<u8>>) -> Option<String> {
    document
        .filter(|d| !d.is_empty())
        .map(|d| revision_text(&d))
}

/// Converting a note to documentation copies what the note holds now, not
/// its last save. The room saves on a cadence, so a copy of the saved row
/// missed the last few seconds of typing and the new page came out empty,
/// after which the client replaced the note with an embed of that page.
#[actix_web::test]
async fn converting_a_note_copies_its_live_content() {
    use backend::models::Claims;
    use backend::schema::{article_contents, documentation_pages, tickets};
    use diesel::prelude::*;
    use yrs::sync::{Message, SyncMessage};
    use yrs::updates::decoder::Decode;
    use yrs::updates::encoder::Encode;
    use yrs::{Doc, ReadTxn, StateVector, Text, Transact};

    install_fast_heartbeat();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());
    let user = common::insert_user(&mut pool.get().expect("conn"), "WSConvert");
    {
        let mut conn = pool.get().expect("conn");
        let actor = backend::sync::actor::ActorContext::user(user.uuid, None).with_workspace(1);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |c| {
                backend::repository::workspaces::add_membership(
                    c,
                    1,
                    user.uuid,
                    "admin",
                    backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                )?;
                Ok(())
            },
        )
        .expect("seed workspace membership");
    }
    let token = JwtUtils::create_collab_token(
        &user.uuid.to_string(),
        &user.platform_role,
        Some(test_workspace().workspace_uuid),
    )
    .expect("mint collab token");
    let ticket_uuid = seed_ticket(&mut pool.get().expect("conn"));
    let ticket_id: i32 = tickets::table
        .filter(tickets::uuid.eq(ticket_uuid))
        .select(tickets::id)
        .first(&mut pool.get().expect("conn"))
        .expect("ticket id");

    // One collaboration server shared by the socket and the convert route,
    // as in production.
    let (state, _tmp) = build_app_state(&pool);
    let state = web::Data::new(state);
    let app_pool = pool.clone();
    let caller = user.clone();
    let srv = actix_test::start(move || {
        let ws = test_workspace();
        let caller = caller.clone();
        App::new()
            .app_data(state.clone())
            .app_data(web::Data::new(app_pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                if req.path().starts_with("/api/") {
                    let now = chrono::Utc::now().timestamp();
                    req.extensions_mut().insert(Claims {
                        sub: caller.uuid.to_string(),
                        name: caller.name.clone(),
                        email: "convert@example.com".to_string(),
                        platform_role: caller.platform_role.clone(),
                        scope: "full".to_string(),
                        sid: None,
                        workspace_uuid: None,
                        exp: (now + 3600) as usize,
                        iat: now as usize,
                    });
                    let actor = backend::sync::actor::ActorContext::user(caller.uuid, None)
                        .with_workspace(ws.workspace_id);
                    req.extensions_mut()
                        .insert(backend::middleware::RequestContext::new(
                            uuid::Uuid::now_v7(),
                            actor,
                        ));
                }
                srv.call(req)
            })
            .route("/ws/{doc}", web::get().to(ws_handler))
            .service(web::scope("/api").configure(backend::handlers::documentation::config))
    });
    let doc_id = format!(
        "ws-{}_ticket-{ticket_uuid}",
        test_workspace().workspace_uuid
    );
    let url = srv.url(&format!("/ws/{doc_id}?token={token}"));
    let client = awc::Client::new();

    // A watches the note; B types into it.
    let (_resp_a, mut watcher) = client.ws(&url).connect().await.expect("A handshake");
    let (_resp_b, mut writer) = client.ws(&url).connect().await.expect("B handshake");
    let typed = Doc::new();
    let edit: Bytes = {
        let text = typed.get_or_insert_text("content");
        text.insert(&mut typed.transact_mut(), 0, "steps that fixed it");
        let update = typed
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        Bytes::from(Message::Sync(SyncMessage::Update(update)).encode_v1())
    };
    writer
        .send(ws::Message::Binary(edit.clone()))
        .await
        .expect("B send");
    // The edit reaching A means the server applied it to the room.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match watcher.next().await {
                Some(Ok(ws::Frame::Binary(b))) if b.as_ref() == edit.as_ref() => break,
                Some(Ok(_)) => continue,
                other => panic!("A's stream ended before B's edit arrived: {other:?}"),
            }
        }
    })
    .await
    .expect("A never saw B's edit");

    // The room hasn't saved yet (its first save waits five seconds).
    let saved: Option<Vec<u8>> = article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .select(article_contents::yjs_document)
        .first::<Option<Vec<u8>>>(&mut pool.get().expect("conn"))
        .optional()
        .expect("saved note lookup")
        .flatten();
    assert_eq!(
        stored_text(saved).as_deref().unwrap_or(""),
        "",
        "the note is not saved yet, so only the live room has the text"
    );

    let mut resp = client
        .post(srv.url(&format!("/api/tickets/{ticket_id}/documentation/create")))
        .send_json(&serde_json::json!({ "title": "Converted note", "parent_id": null }))
        .await
        .expect("convert request");
    assert_eq!(resp.status().as_u16(), 201, "a new page is created");
    let page: serde_json::Value = resp.json().await.expect("page json");
    let page_id = page["id"].as_i64().expect("page id") as i32;

    let document: Option<Vec<u8>> = documentation_pages::table
        .find(page_id)
        .select(documentation_pages::yjs_document)
        .first(&mut pool.get().expect("conn"))
        .expect("page row");
    assert_eq!(
        stored_text(document).as_deref(),
        Some("steps that fixed it"),
        "the page holds the text typed into the note"
    );

    // The client checks the page's state vector covers its own before it
    // replaces the note, so the response carries the copy's.
    let state_vector: Vec<u8> =
        serde_json::from_value(page["yjs_state_vector"].clone()).expect("state vector bytes");
    let state_vector = StateVector::decode_v1(&state_vector).expect("decode state vector");
    let typed_clock = typed.transact().state_vector();
    assert!(
        typed_clock
            .iter()
            .all(|(client, clock)| state_vector.get(client) >= *clock),
        "the page's state vector covers everything typed"
    );
}

/// A draft page written the way an import writes one, its body then revision
/// 1, and a collaboration server whose editors can open it.
struct ImportedPage {
    pool: backend::db::Pool,
    admin: backend::models::User,
    page_id: i32,
    url: String,
    _srv: actix_test::TestServer,
}

/// What the backing table holds for a page.
struct Stored {
    document: Option<Vec<u8>>,
    updated_at: chrono::NaiveDateTime,
    revisions: i64,
    content_saved_events: i64,
}

impl ImportedPage {
    fn new(test_db: &common::TestDb) -> Self {
        use backend::schema::documentation_pages as dp;
        use diesel::prelude::*;
        use yrs::updates::encoder::Encode;
        use yrs::{
            ReadTxn, StateVector, Transact, WriteTxn, XmlElementPrelim, XmlFragment, XmlTextPrelim,
        };

        install_fast_heartbeat();
        let pool = build_pool(test_db.url());
        let ws1_uuid =
            backend::repository::workspaces::find_by_id(&mut pool.get().expect("conn"), 1)
                .expect("ws lookup")
                .expect("bootstrap workspace exists")
                .uuid;
        let admin = common::insert_user(&mut pool.get().expect("conn"), "Import Viewer");
        let imported = {
            let doc = yrs::Doc::new();
            {
                let mut txn = doc.transact_mut();
                let fragment = txn.get_or_insert_xml_fragment("prosemirror");
                let paragraph = fragment.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
                paragraph.push_back(&mut txn, XmlTextPrelim::new("Connect with WireGuard."));
            }
            let txn = doc.transact();
            (
                txn.encode_state_as_update_v1(&StateVector::default()),
                txn.state_vector().encode_v1(),
            )
        };
        let page_uuid = uuid::Uuid::new_v4();
        let page_id = {
            let mut conn = pool.get().expect("conn");
            let actor =
                backend::sync::actor::ActorContext::user(admin.uuid, None).with_workspace(1);
            backend::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &actor,
                |c| {
                    backend::repository::workspaces::add_membership(
                        c,
                        1,
                        admin.uuid,
                        "admin",
                        backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
                    )?;
                    let page_id: i32 = diesel::insert_into(dp::table)
                        .values((
                            dp::uuid.eq(page_uuid),
                            dp::title.eq("VPN setup"),
                            dp::slug.eq(format!("vpn-setup-{}", page_uuid.simple())),
                            dp::status.eq(backend::models::DocumentationStatus::Draft),
                            dp::created_by.eq(admin.uuid),
                            dp::last_edited_by.eq(admin.uuid),
                        ))
                        .returning(dp::id)
                        .get_result(c)?;
                    backend::repository::update_documentation_yjs_state(
                        c,
                        page_id,
                        imported.0.clone(),
                        None,
                        None,
                    )?;
                    backend::repository::create_revision_with_summary(
                        c,
                        page_id,
                        imported.1.clone(),
                        imported.0.clone(),
                        admin.uuid,
                        "Imported",
                    )?;
                    Ok(page_id)
                },
            )
            .expect("seed the imported page")
        };

        let state_pool_inner = pool.clone();
        let srv = actix_test::start(move || {
            let (state, _tmp) = build_app_state(&state_pool_inner);
            std::mem::forget(_tmp);
            App::new()
                .app_data(web::Data::new(state))
                .app_data(web::Data::new(state_pool_inner.clone()))
                .route("/ws/{doc}", web::get().to(ws_handler))
        });
        let token = JwtUtils::create_collab_token(
            &admin.uuid.to_string(),
            &admin.platform_role,
            Some(ws1_uuid),
        )
        .expect("mint collab token");
        let url = srv.url(&format!("/ws/ws-{ws1_uuid}_doc-{page_uuid}?token={token}"));
        Self {
            pool,
            admin,
            page_id,
            url,
            _srv: srv,
        }
    }

    fn stored(&self) -> Stored {
        use backend::schema::{
            documentation_pages as dp, documentation_revisions as dr, sync_actions as sa,
        };
        use diesel::prelude::*;
        let mut conn = self.pool.get().expect("conn");
        let (document, updated_at) = dp::table
            .find(self.page_id)
            .select((dp::yjs_document, dp::updated_at))
            .first::<(Option<Vec<u8>>, chrono::NaiveDateTime)>(&mut conn)
            .expect("page row");
        let revisions = dr::table
            .filter(dr::page_id.eq(self.page_id))
            .count()
            .get_result(&mut conn)
            .expect("revision count");
        let content_saved_events = sa::table
            .filter(sa::event_type.eq("documentation.content_saved"))
            .filter(sa::aggregate_id.eq(self.page_id.to_string()))
            .count()
            .get_result(&mut conn)
            .expect("content-saved events");
        Stored {
            document,
            updated_at,
            revisions,
            content_saved_events,
        }
    }

    /// The stored state once `done` holds for it, polling for up to `secs`.
    async fn stored_once(&self, secs: u64, done: impl Fn(&Stored) -> bool) -> Stored {
        let until = tokio::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let stored = self.stored();
            if done(&stored) || tokio::time::Instant::now() >= until {
                return stored;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Whether a stored document's content holds `text`.
fn holds(document: &Option<Vec<u8>>, text: &str) -> bool {
    document
        .as_deref()
        .is_some_and(|d| fragment_text(d).contains(text))
}

/// Ask the room for its document and return it.
async fn room_document<S, E>(conn: &mut S) -> Vec<u8>
where
    S: futures_util::Stream<Item = Result<ws::Frame, E>> + futures_util::Sink<ws::Message> + Unpin,
    E: std::fmt::Debug,
    <S as futures_util::Sink<ws::Message>>::Error: std::fmt::Debug,
{
    use yrs::sync::{Message, MessageReader, SyncMessage};
    use yrs::updates::decoder::DecoderV1;
    use yrs::updates::encoder::Encode;
    use yrs::StateVector;
    let request =
        Bytes::from(Message::Sync(SyncMessage::SyncStep1(StateVector::default())).encode_v1());
    conn.send(ws::Message::Binary(request))
        .await
        .expect("send state request");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let Some(Ok(frame)) = conn.next().await else {
                panic!("the stream ended before the document arrived");
            };
            let ws::Frame::Binary(bytes) = frame else {
                continue;
            };
            let mut decoder = DecoderV1::from(bytes.as_ref());
            for message in MessageReader::new(&mut decoder).flatten() {
                if let Message::Sync(SyncMessage::SyncStep2(update)) = message {
                    return update;
                }
            }
        }
    })
    .await
    .expect("the room never sent its document")
}

/// The room's document, as a client that has synced it.
async fn synced_doc<S, E>(conn: &mut S) -> yrs::Doc
where
    S: futures_util::Stream<Item = Result<ws::Frame, E>> + futures_util::Sink<ws::Message> + Unpin,
    E: std::fmt::Debug,
    <S as futures_util::Sink<ws::Message>>::Error: std::fmt::Debug,
{
    use yrs::updates::decoder::Decode;
    use yrs::{Transact, Update};
    let doc = yrs::Doc::new();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&room_document(conn).await).expect("decode"))
        .expect("apply");
    doc
}

/// `text` appended to the document's content, as the update an editor sends.
fn append_text(doc: &yrs::Doc, text: &str) -> Bytes {
    use yrs::sync::{Message, SyncMessage};
    use yrs::updates::encoder::Encode;
    use yrs::{ReadTxn, Transact, WriteTxn, XmlFragment, XmlTextPrelim};
    let sv = doc.transact().state_vector();
    {
        let mut txn = doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment("prosemirror");
        fragment.push_back(&mut txn, XmlTextPrelim::new(text));
    }
    let update = doc.transact().encode_state_as_update_v1(&sv);
    Bytes::from(Message::Sync(SyncMessage::Update(update)).encode_v1())
}

/// Opening a page and leaving it changes nothing: no save, no revision, the
/// same `updated_at`. An imported page that was only looked at stays one
/// "nobody has changed since", which is what removing an import moves to
/// Trash. Editors used to map their client to their user in the `users` map
/// on open, which reached the server as the viewer's change: the page was
/// saved and got a revision in the viewer's name.
#[actix_web::test]
async fn opening_a_page_without_editing_changes_nothing() {
    use yrs::sync::{Message, SyncMessage};
    use yrs::updates::encoder::Encode;
    use yrs::{Map, MapPrelim, ReadTxn, Transact, WriteTxn};

    let test_db = common::TestDb::new();
    let page = ImportedPage::new(&test_db);
    let before = page.stored();
    assert_eq!(before.revisions, 1, "the import wrote revision 1");
    let client = awc::Client::new();

    // A reader opens the page with an editor that maps its client to its
    // user on open, as editors did, and leaves without typing.
    let (_resp, mut reader) = client
        .ws(&page.url)
        .connect()
        .await
        .expect("reader connects");
    let opened = synced_doc(&mut reader).await;
    let mapping: Bytes = {
        let sv = opened.transact().state_vector();
        {
            let mut txn = opened.transact_mut();
            let users = txn.get_or_insert_map("users");
            users.insert(&mut txn, page.admin.uuid.to_string(), MapPrelim::default());
        }
        let update = opened.transact().encode_state_as_update_v1(&sv);
        Bytes::from(Message::Sync(SyncMessage::Update(update)).encode_v1())
    };
    reader
        .send(ws::Message::Binary(mapping))
        .await
        .expect("send mapping");
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(reader);

    // The disconnect's final save and the maintenance loop have had their
    // turn by now.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let after = page.stored();
    assert_eq!(after.revisions, 1, "a view adds no revision");
    assert_eq!(
        after.updated_at, before.updated_at,
        "a view leaves updated_at alone"
    );
    assert_eq!(after.document, before.document, "a view saves nothing");
    assert_eq!(after.content_saved_events, 0, "a view announces no save");

    // The room did take the mapping: an editor opening the page now gets it.
    let (_resp, mut editor) = client
        .ws(&page.url)
        .connect()
        .await
        .expect("editor connects");
    let room = synced_doc(&mut editor).await;
    assert!(
        room.transact()
            .get_map("users")
            .is_some_and(|users| users
                .contains_key(&room.transact(), &page.admin.uuid.to_string())),
        "the reader's mapping reached the room"
    );

    // An edit is saved and recorded, in its author's name. The save, the
    // revision and the event are separate writes, so wait for all three.
    editor
        .send(ws::Message::Binary(append_text(&room, "Edited.")))
        .await
        .expect("send edit");
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(editor);
    let edited = page
        .stored_once(5, |s| {
            s.revisions == 2 && holds(&s.document, "Edited.") && s.content_saved_events == 1
        })
        .await;
    assert_eq!(edited.revisions, 2, "the edit is a revision");
    assert!(holds(&edited.document, "Edited."), "the edit is saved");
    assert_ne!(
        edited.updated_at, before.updated_at,
        "the edit moves updated_at"
    );
    // The session's end tells the other machines to index the new body.
    assert_eq!(
        edited.content_saved_events, 1,
        "only the edit's session announces a save"
    );
}

/// An edit saved while its editor is still there tells the other machines to
/// index it, even when a quiet spell already made it a revision and nothing is
/// left to save when the editor leaves.
#[actix_web::test]
async fn an_edit_saved_mid_session_is_announced() {
    let test_db = common::TestDb::new();
    let page = ImportedPage::new(&test_db);
    let client = awc::Client::new();
    let (_resp, mut editor) = client
        .ws(&page.url)
        .connect()
        .await
        .expect("editor connects");
    let room = synced_doc(&mut editor).await;
    editor
        .send(ws::Message::Binary(append_text(&room, "Saved while open.")))
        .await
        .expect("send edit");

    // The quiet period cuts the revision, then the room's regular save
    // stores the edit, both while the editor is still there.
    let saved = page
        .stored_once(10, |s| {
            s.revisions == 2 && holds(&s.document, "Saved while open.")
        })
        .await;
    assert!(
        holds(&saved.document, "Saved while open."),
        "saved mid-session"
    );
    assert_eq!(saved.content_saved_events, 1, "the save announces itself");

    drop(editor);
    tokio::time::sleep(Duration::from_millis(3000)).await;
    assert_eq!(
        page.stored().content_saved_events,
        1,
        "leaving with nothing new to save announces nothing more"
    );
}

/// An edit whose save fails is saved once saving works again. The editor
/// stays until the quiet period has made the edit a revision, so its leaving
/// is not the end of a session with changes; the room still knows the
/// content isn't stored and keeps trying.
#[actix_web::test]
async fn an_edit_whose_save_failed_is_saved_later() {
    use diesel::prelude::*;

    let test_db = common::TestDb::new();
    let page = ImportedPage::new(&test_db);
    // Every save of this page's body fails while it is listed.
    diesel::connection::SimpleConnection::batch_execute(
        &mut *page.pool.get().expect("conn"),
        "CREATE TABLE test_failing_saves (page_id int PRIMARY KEY);
         GRANT SELECT ON test_failing_saves TO PUBLIC;
         CREATE FUNCTION test_fail_save() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF EXISTS (SELECT 1 FROM test_failing_saves WHERE page_id = NEW.id) THEN
                 RAISE EXCEPTION 'this save fails for the test';
             END IF;
             RETURN NEW;
         END $$;
         CREATE TRIGGER test_fail_save BEFORE UPDATE OF yjs_document ON documentation_pages
             FOR EACH ROW EXECUTE FUNCTION test_fail_save();",
    )
    .expect("install the failing save");
    let fail_saves = |on: bool| {
        let sql = if on {
            "INSERT INTO test_failing_saves VALUES ($1)"
        } else {
            "DELETE FROM test_failing_saves WHERE page_id = $1"
        };
        diesel::sql_query(sql)
            .bind::<diesel::sql_types::Integer, _>(page.page_id)
            .execute(&mut page.pool.get().expect("conn"))
            .expect("toggle the failing save");
    };
    fail_saves(true);

    let client = awc::Client::new();
    let (_resp, mut editor) = client
        .ws(&page.url)
        .connect()
        .await
        .expect("editor connects");
    let room = synced_doc(&mut editor).await;
    editor
        .send(ws::Message::Binary(append_text(&room, "Kept anyway.")))
        .await
        .expect("send edit");
    // The quiet period cuts the revision while the editor is still there.
    let revised = page.stored_once(5, |s| s.revisions == 2).await;
    assert_eq!(revised.revisions, 2, "the quiet edit is a revision");
    drop(editor);

    // The save on leaving failed.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        !holds(&page.stored().document, "Kept anyway."),
        "the save on leaving failed"
    );

    // Saving works again: the room's next pass stores the edit.
    fail_saves(false);
    let saved = page
        .stored_once(15, |s| {
            holds(&s.document, "Kept anyway.") && s.content_saved_events == 1
        })
        .await;
    assert!(
        holds(&saved.document, "Kept anyway."),
        "the edit is saved later"
    );
    assert_eq!(
        saved.content_saved_events, 1,
        "the save that ends the session announces it"
    );
}
