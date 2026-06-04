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
use backend::handlers::collaboration::{ws_handler, YjsAppState};
use backend::handlers::sse::SseState;
use backend::services::search::SearchService;
use backend::utils::cookies::ACCESS_TOKEN_COOKIE;
use backend::utils::jwt::JwtUtils;
use backend::utils::redis_yjs_cache::create_redis_cache;

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
        // JWT_SECRET must be stable across the test process; main.rs
        // refuses placeholder secrets in production, but tests run in
        // dev/test mode.
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-32-characters-min-for-tests");
        }
        // Tests are not "production" — relaxes the Origin requirement
        // in ws_handler.
        std::env::set_var("ENVIRONMENT", "test");
    });
}

/// Build a real `YjsAppState` against the test sandbox DB, a temp
/// Tantivy index dir, and the dev-compose Redis. The compose dev
/// stack publishes Redis at `redis:6379` on the internal network;
/// integration tests run inside that container so the hostname
/// resolves.
fn build_app_state(pool_inner: &backend::db::Pool) -> (YjsAppState, tempfile::TempDir) {
    let pool_data = web::Data::new(pool_inner.clone());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://redis:6379".into());
    let redis_cache = create_redis_cache(&redis_url).expect("connect Redis for YjsAppState");
    let sse_state = web::Data::new(SseState::new());
    let tmp_search = tempfile::tempdir().expect("temp dir for search index");
    let search =
        Arc::new(SearchService::new(tmp_search.path(), pool_inner).expect("init search service"));
    let state = YjsAppState::new(pool_data, redis_cache, sse_state, search);
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
    use diesel::r2d2::{ConnectionManager, Pool};
    // YjsAppState construction touches the Keyring via the periodic
    // save loop's encryption paths; init once per process so we
    // never panic with `keyring not initialised`.
    ensure_test_keyring();
    let mgr = ConnectionManager::<diesel::pg::PgConnection>::new(url);
    Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(WorkspaceGuc))
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
    }
}

#[actix_web::test]
async fn handshake_broadcast_and_clean_disconnect() {
    install_fast_heartbeat();

    let test_db = common::TestDb::new();
    let pool = build_pool(test_db.url());

    // Seed a user so JWT validation finds a real row to attach to.
    let user = common::insert_user(&mut pool.get().expect("conn"), "WSAlice");

    // ws_handler's JWT validation also checks `active_sessions`: the
    // token's `sid` claim must reference a live session row owned by
    // this user. The `session_id` column has a DB-side default, so
    // we insert first and read back the generated UUID, then mint
    // the JWT against it — same shape as the production login path.
    let session_row = backend::repository::active_sessions::create_session(
        &mut pool.get().expect("conn"),
        backend::models::NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("ws-test".into()),
            ip_address: None,
            user_agent: Some("ws-test-client".into()),
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            is_current: true,
        },
    )
    .expect("create active session");
    let token = JwtUtils::create_token(&user, &session_row.session_id).expect("mint JWT");

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
    // `ws-{uuid}_{kind}-{id}`; ws_handler rejects bare ids with a 400
    // and rejects a uuid that doesn't match the request's
    // WorkspaceContext with a 403. Build it from test_workspace()'s
    // uuid (nil) so both checks pass. The ticket row need not exist:
    // the Yjs doc layer starts an empty document for an unseen id.
    let doc_id = format!("ws-{}_ticket-1", test_workspace().workspace_uuid);
    let url = srv.url(&format!("/ws/{doc_id}"));

    // Own our own awc::Client so we can attach the auth cookie.
    // actix-test's `ws_at` convenience method skips cookie support
    // (it uses an internal client we can't reach for chaining).
    let client = awc::Client::new();
    let cookie = awc::cookie::Cookie::new(ACCESS_TOKEN_COOKIE, token.clone());

    // --- Client A connects ---
    let (_resp_a, mut conn_a) = client
        .ws(&url)
        .cookie(cookie.clone())
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

    // --- Client B connects ---
    let (_resp_b, mut conn_b) = client
        .ws(&url)
        .cookie(cookie.clone())
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
