//! End-to-end Greenmail test for the IMAP ingestion path.
//!
//! This test is **ignored by default** — it needs a Greenmail container
//! running on the standard GreenMail ports. Start it with:
//!
//! ```text
//! docker compose -f compose.yaml -f compose.dev.yaml --profile email-testing up -d greenmail
//! ```
//!
//! Then run inside the backend container (where `GREENMAIL_HOST`
//! defaults to the `greenmail` service name on the compose network):
//!
//! ```text
//! docker compose -f compose.yaml -f compose.dev.yaml exec backend \
//!   cargo test --test channels_email_imap_integration -- --ignored
//! ```
//!
//! Or from the host shell, pointing at the published ports:
//!
//! ```text
//! cd backend && GREENMAIL_HOST=127.0.0.1 \
//!   cargo test --test channels_email_imap_integration -- --ignored
//! ```
//!
//! Greenmail's IMAPS port presents a self-signed certificate, so the channel
//! config sets `insecure_skip_cert_verify`. The adapter honours that flag
//! automatically here because the run is non-production; it hard-ignores it
//! when `ENVIRONMENT=production` (see `email_imap.rs` — a guard against
//! silently disabling TLS validation in production). Don't set
//! `ENVIRONMENT=production` when running these tests.
//!
//! The first test exercises the adapter's poll cycle (plant via SMTP,
//! poll IMAP, assert the parsed event and `last_seen_uid`). The second
//! drives a full inbound -> threading -> relay -> outbound cycle
//! through the real pipeline.
//!
//! Both need a pool: the adapter writes runtime_state after each poll,
//! and the pipeline persists tickets/comments/channel_messages. We
//! lean on the dedicated `TEST_DATABASE_URL` the library tests use.
//!
//! `build_pool` (a) bootstraps migrations on that DB so the suite is
//! self-contained on a fresh database (e.g. in CI), and (b) seeds
//! `app.workspace_id` on every connection. After the Phase 3 NOT-NULL
//! flip, tenant tables default `workspace_id` from that GUC; without
//! it, `seed_channel` and the manual comment inserts fail the NOT-NULL
//! check. This mirrors `tests/common/mod.rs`'s `WorkspaceGucCustomizer`.

use std::sync::Arc;
use std::time::Duration;

use std::sync::OnceLock;

use backend::db::{Pool, MIGRATIONS};
use backend::models::{Channel, NewChannel, NewComment, CHANNEL_DIRECTION_OUTBOUND};
use backend::repository::{
    channels as channels_repo, comments as comments_repo, tickets as tickets_repo,
};
use backend::services::channels::email_imap::{
    EmailImapAdapter, ImapChannelConfig, ImapRuntimeState,
};
use backend::services::channels::pipeline::{self, PipelineContext, PipelineOutcome};
use backend::services::channels::relay::{self, RelayDecision};
use backend::services::channels::{InboundEvent, PullAdapter};
use backend::utils::email::{EmailConfig, EmailService, SmtpSecurity};
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager};
use diesel_migrations::MigrationHarness;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

// ---- Greenmail defaults (match compose.dev.yaml `email-testing` profile) ----

/// Greenmail's hostname. `greenmail` resolves inside the Docker
/// network used by `compose.dev.yaml`. When running this test from
/// the host shell, override with `GREENMAIL_HOST=127.0.0.1`.
fn greenmail_host() -> String {
    std::env::var("GREENMAIL_HOST").unwrap_or_else(|_| "greenmail".into())
}
const IMAP_PORT: u16 = 3993;
const SMTP_PORT: u16 = 3025;
const API_PORT: u16 = 8080;
const USER: &str = "support@example.com";
const USER_LOGIN: &str = "support";
const PASSWORD: &str = "hunter2";

/// Skip the whole test file if Greenmail isn't reachable — keeps
/// `cargo test` from red-lighting on a machine that isn't running it.
///
/// Probes the plaintext SMTP port, not the IMAPS port: GreenMail's
/// mail listeners all bind together, so SMTP liveness implies IMAPS is
/// up too, and a bare TCP connect to a TLS port would make GreenMail
/// log a noisy "Can not handle IMAP connection" when it fails to write
/// its greeting over the unestablished TLS session.
fn greenmail_reachable() -> bool {
    std::net::TcpStream::connect((greenmail_host().as_str(), SMTP_PORT)).is_ok()
}

/// Empty every Greenmail mailbox via the standalone API. The two tests
/// in this file share one `support` mailbox, and Greenmail retains mail
/// across runs of the same container, so without a reset the first poll
/// can return a prior test's messages (and even hit `MAX_FETCH_PER_POLL`
/// on a busy container, leaving a tail that breaks the "second poll is
/// empty" assertion). Call this at the start of each test for a known
/// clean inbox. Tests run `--test-threads=1` so the purge of one test
/// never races another.
async fn purge_greenmail() {
    let url = format!("http://{}:{}/api/mail/purge", greenmail_host(), API_PORT);
    reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .expect("POST greenmail /api/mail/purge")
        .error_for_status()
        .expect("greenmail purge returned non-success status");
}

/// Seeds `app.workspace_id` on every fresh connection so tenant-table
/// inserts (channels, comments, tickets, ...) satisfy the Phase 3
/// NOT-NULL default `NULLIF(current_setting('app.workspace_id', true),
/// '')::int`. Bootstrap workspace id=1 is present after migrations.
/// The pipeline re-sets this transaction-locally for its own writes;
/// the session value here covers the test's direct inserts. Mirrors
/// `tests/common/mod.rs`.
#[derive(Debug)]
struct WorkspaceGucCustomizer;

impl r2d2::CustomizeConnection<diesel::PgConnection, r2d2::Error> for WorkspaceGucCustomizer {
    fn on_acquire(&self, conn: &mut diesel::PgConnection) -> Result<(), r2d2::Error> {
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(r2d2::Error::QueryError)?;
        Ok(())
    }
}

/// Apply embedded migrations to the test DB once per process so the
/// suite is self-contained on a fresh database (CI starts the Postgres
/// service empty). Embedded migrations are idempotent, so re-running on
/// a DB the library tests already migrated is a no-op.
fn ensure_migrated(url: &str) {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let mut conn = diesel::PgConnection::establish(url)
            .expect("connect to TEST_DATABASE_URL for migration bootstrap");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("apply migrations to test DB");
    });
}

/// Initialise the at-rest Keyring once per test process. `MFA_KEK_V1`
/// is a stable test key. Mirrors the helper in `src/test_helpers.rs`
/// which is `#[cfg(test)]`-gated and therefore invisible to integration
/// tests; the `std::sync::Once` lets us call this from every test
/// fixture without tripping `init_keyring`'s "called twice" panic.
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

fn build_pool() -> Pool {
    dotenvy::dotenv().ok();
    // Require a dedicated test DB — see `src/test_helpers.rs` for the
    // non-transactional-sequence rationale. This integration test
    // commits rows (no test transaction) and tears them down in
    // `teardown`, but we still don't want it mingling with dev data.
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set for integration tests");
    ensure_migrated(&url);
    // channel_credentials writes go through the at-rest Keyring;
    // production initialises it in `main.rs::init_keyring`, but
    // integration tests don't run main. test_helpers is
    // `#[cfg(test)]`-gated and so invisible to this binary (integration
    // tests are compiled as external crates against the lib), so we
    // inline the same Once-guarded init here.
    ensure_test_keyring();
    let manager = ConnectionManager::<diesel::PgConnection>::new(url);
    r2d2::Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(WorkspaceGucCustomizer))
        .build(manager)
        .expect("build pool")
}

/// Create a fresh channel row. The test uses a throwaway row and
/// deletes it in `teardown` to keep the real `channels` table tidy.
fn seed_channel(pool: &Pool) -> Channel {
    let mut conn = pool.get().unwrap();
    let cfg = serde_json::json!({
        "host": greenmail_host(),
        "port": IMAP_PORT,
        "username": USER_LOGIN,
        "mailbox": "INBOX",
        "use_tls": true,
        "reply_domain": "example.com",
        "insecure_skip_cert_verify": true,
    });
    let ch = channels_repo::create(
        &mut conn,
        NewChannel {
            provider: "email_imap".into(),
            name: "greenmail-test".into(),
            enabled: true,
            config: cfg,
        },
    )
    .unwrap();
    channels_repo::put_credential(
        &mut conn,
        ch.id,
        backend::models::CRED_TYPE_IMAP_PASSWORD,
        PASSWORD,
        None,
    )
    .unwrap();
    ch
}

fn teardown_channel(pool: &Pool, channel_id: i32) {
    let mut conn = pool.get().unwrap();
    // Delete any messages we recorded during this test so the row
    // delete cascades cleanly.
    use backend::schema::channel_messages::dsl as cm;
    let _ = diesel::delete(cm::channel_messages.filter(cm::channel_id.eq(channel_id)))
        .execute(&mut conn);
    let _ = channels_repo::delete(&mut conn, channel_id);
}

/// RAII guard so the channel + its messages are cleaned up even when
/// an assertion panics mid-test. Without this, a failing run leaves
/// rows in the dev DB that cascade-fail the next attempt.
struct ChannelGuard {
    pool: Pool,
    channel_id: i32,
}

impl Drop for ChannelGuard {
    fn drop(&mut self) {
        teardown_channel(&self.pool, self.channel_id);
    }
}

async fn send_fixture_email(subject: &str, body: &str, message_id: &str) {
    let transport: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(greenmail_host())
            .port(SMTP_PORT)
            .credentials(Credentials::new(USER_LOGIN.into(), PASSWORD.into()))
            .build();

    let msg = Message::builder()
        .from("Alice <alice@customer.test>".parse().unwrap())
        .to(USER.parse().unwrap())
        .subject(subject)
        .header(lettre::message::header::MessageId::from(format!(
            "<{message_id}>"
        )))
        .body(body.to_string())
        .unwrap();

    transport.send(msg).await.expect("send via greenmail SMTP");
}

fn email_service_stub() -> Arc<EmailService> {
    Arc::new(EmailService::new(EmailConfig {
        smtp_host: String::new(),
        smtp_port: 587,
        smtp_username: String::new(),
        smtp_password: String::new(),
        from_name: String::new(),
        from_email: String::new(),
        enabled: false,
        security: SmtpSecurity::StartTls,
    }))
}

/// Pick our message out of a batch that might include Greenmail's
/// retained cross-run noise. Returns `None` if nothing matches — the
/// caller asserts `.expect(...)` with a descriptive message.
fn find_our_message<'a>(
    events: &'a [InboundEvent],
    message_id: &str,
) -> Option<&'a backend::services::channels::InboundMessage> {
    events.iter().find_map(|e| match e {
        InboundEvent::MessageReceived(m) if m.external_id.contains(message_id) => Some(m),
        _ => None,
    })
}

#[tokio::test]
#[ignore = "requires Greenmail on 127.0.0.1:3993/3025 — see file header"]
async fn poll_fetches_pending_email_and_advances_uid() {
    if !greenmail_reachable() {
        let host = greenmail_host();
        panic!("Greenmail not reachable on {host}:{SMTP_PORT} — start via --profile email-testing");
    }
    purge_greenmail().await;

    let pool = build_pool();
    let channel = seed_channel(&pool);
    let channel_id = channel.id;
    // Cleanup runs even if the test panics below.
    let _guard = ChannelGuard {
        pool: pool.clone(),
        channel_id,
    };

    // Arrange: drop a test email into the inbox.
    let message_id = format!("integration-test-{}@example.com", channel_id);
    send_fixture_email("Printer fire", "Please help.", &message_id).await;

    // Greenmail SMTP delivery is synchronous, but give the server a
    // moment to flush to the mailbox before polling.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Act: drive one poll cycle directly against the adapter.
    let config: ImapChannelConfig = serde_json::from_value(channel.config.clone()).unwrap();
    let mut adapter = EmailImapAdapter::new(
        channel_id,
        channel.workspace_id,
        config,
        email_service_stub(),
        pool.clone(),
        ImapRuntimeState::default(),
    );
    let events = adapter.poll().await.expect("poll");

    // Greenmail may retain messages from earlier test runs (same
    // mailbox across container restarts), so filter by OUR message-id
    // rather than asserting a total count.
    let ours = find_our_message(&events, &message_id).expect("our inbound message in fetch");
    assert_eq!(
        ours.from.known_email.as_deref(),
        Some("alice@customer.test")
    );
    assert_eq!(ours.subject.as_deref(), Some("Printer fire"));
    assert!(ours.body_text.contains("Please help"));

    // Runtime state advanced past UID 0.
    assert!(
        adapter.runtime_state().last_seen_uid > 0,
        "expected last_seen_uid > 0"
    );

    // A second poll has nothing new to deliver: we purged on entry and
    // sent exactly one message, which the first poll consumed. poll()
    // is a long-poll — against an IDLE-capable server (Greenmail
    // advertises IDLE) it drains (empty), then blocks in IDLE waiting
    // for a server push rather than re-delivering the message we
    // already saw. Wrap it in a short timeout: either it returns empty
    // promptly (polled-only server), or it parks in IDLE and we time
    // out. Both prove no new events; a re-delivered message would come
    // back before the deadline and fail the Ok branch.
    match tokio::time::timeout(Duration::from_secs(3), adapter.poll()).await {
        Ok(Ok(events)) => assert!(
            events.is_empty(),
            "second poll must not re-deliver an already-seen message: {events:?}"
        ),
        Ok(Err(e)) => panic!("second poll errored: {e:?}"),
        Err(_elapsed) => { /* parked in IDLE awaiting new mail — correct */ }
    }

    // _guard runs teardown on scope exit (including panics above).
}

// ---------- Full cycle: inbound -> threading -> internal skip -> outbound ----------

/// Build an [`EmailService`] pointed at Greenmail's SMTP. Used so the
/// outbound dispatch side of the cycle actually hits a real server
/// and exercises lettre's `Message-ID` / `In-Reply-To` / `References`
/// header writes.
fn greenmail_email_service() -> Arc<EmailService> {
    Arc::new(EmailService::new(EmailConfig {
        smtp_host: greenmail_host(),
        smtp_port: SMTP_PORT,
        smtp_username: USER_LOGIN.into(),
        smtp_password: PASSWORD.into(),
        from_name: "Nosdesk Support".into(),
        from_email: USER.into(),
        enabled: true,
        // Greenmail's SMTP on 3025 is plaintext; use SMTPS on 3465
        // would work too but this keeps the test simpler.
        security: SmtpSecurity::Plaintext,
    }))
}

#[tokio::test]
#[ignore = "requires Greenmail on 127.0.0.1:3993/3025 — see file header"]
async fn full_cycle_inbound_internal_outbound() {
    if !greenmail_reachable() {
        let host = greenmail_host();
        panic!("Greenmail not reachable on {host}:{SMTP_PORT} — start via --profile email-testing");
    }
    purge_greenmail().await;

    let pool = build_pool();
    let channel = seed_channel(&pool);
    let channel_id = channel.id;
    let _guard = ChannelGuard {
        pool: pool.clone(),
        channel_id,
    };

    // ============== STEP 1: Inbound ==============
    // Send a fresh email; poll runs end-to-end through the pipeline so
    // we exercise: IMAP fetch → parse → threading (new ticket) →
    // identity resolve → persist ticket + comment + channel_messages.
    let inbound_msg_id = format!("cycle-inbound-{}@customer.test", channel_id);
    send_fixture_email("Server down", "Can't log in.", &inbound_msg_id).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let config: ImapChannelConfig = serde_json::from_value(channel.config.clone()).unwrap();
    let mut adapter = EmailImapAdapter::new(
        channel_id,
        channel.workspace_id,
        config.clone(),
        email_service_stub(),
        pool.clone(),
        ImapRuntimeState::default(),
    );
    let events = adapter.poll().await.expect("initial poll");
    // Filter to our message — Greenmail may have retained messages
    // from earlier runs in the same container.
    let our_event = events
        .into_iter()
        .find(|e| matches!(e, InboundEvent::MessageReceived(m) if m.external_id.contains(&inbound_msg_id)))
        .expect("expected our inbound message among the fetched events");

    // Run the event through the real pipeline (not just the adapter
    // parser) so we cover: identity resolution, ticket creation, and
    // channel_messages linkage.
    {
        let mut conn = pool.get().expect("pool get");
        let ctx = PipelineContext::bare();
        let outcome = pipeline::process_event(&adapter, &channel, our_event, &mut conn, &ctx)
            .await
            .expect("pipeline process_event");
        assert!(
            matches!(outcome, PipelineOutcome::TicketOpened { .. }),
            "expected TicketOpened, got {outcome:?}"
        );
    }

    // Grab the ticket for the rest of the flow.
    let ticket = {
        let mut conn = pool.get().unwrap();
        let recorded = channels_repo::find_by_external_id(
            &mut conn,
            channel_id,
            &format!("<{inbound_msg_id}>"),
        )
        .unwrap()
        .expect("inbound channel_messages row");
        tickets_repo::get_ticket_by_id(&mut conn, recorded.ticket_id.unwrap()).unwrap()
    };
    assert_eq!(ticket.origin_channel_id, Some(channel_id));
    assert_eq!(ticket.submitted_via.as_deref(), Some("email_imap"));

    // ============== STEP 2: Internal comment is NOT relayed ==============
    // An internal note must produce `RelayDecision::SkipInternal`
    // regardless of channel / requester state.
    let internal_comment = {
        let mut conn = pool.get().unwrap();
        let commenter = ticket.requester_uuid.expect("ticket has requester");
        comments_repo::create_comment(
            &mut conn,
            NewComment {
                content: "internal note, don't email the customer".into(),
                ticket_id: ticket.id,
                user_uuid: commenter,
                channel_metadata: None,
                is_internal: true,
                content_format: Default::default(),
                body_text: None,
                body_html: None,
                new_content: None,
                quoted_content: None,
                raw_source_uri: None,
                render_kind: None,
            },
            None,
        )
        .expect("insert internal comment")
    };
    {
        let mut conn = pool.get().unwrap();
        let decision = relay::decide_relay(&mut conn, &ticket, &internal_comment)
            .expect("decide_relay on internal comment");
        assert!(
            matches!(decision, RelayDecision::SkipInternal),
            "expected SkipInternal, got {decision:?}"
        );
    }

    // ============== STEP 3: Public comment -> outbound via Greenmail ==============
    let public_comment = {
        let mut conn = pool.get().unwrap();
        let commenter = ticket.requester_uuid.unwrap();
        comments_repo::create_comment(
            &mut conn,
            NewComment {
                content: "We've restarted the node, try again.".into(),
                ticket_id: ticket.id,
                user_uuid: commenter,
                channel_metadata: None,
                is_internal: false,
                content_format: Default::default(),
                body_text: None,
                body_html: None,
                new_content: None,
                quoted_content: None,
                raw_source_uri: None,
                render_kind: None,
            },
            None,
        )
        .expect("insert public comment")
    };

    // `decide_relay` should pick the channel and produce a ThreadContext.
    let (relay_channel, thread) = {
        let mut conn = pool.get().unwrap();
        match relay::decide_relay(&mut conn, &ticket, &public_comment)
            .expect("decide_relay on public comment")
        {
            RelayDecision::Relay { channel, thread } => (channel, thread),
            other => panic!("expected Relay, got {other:?}"),
        }
    };
    assert_eq!(relay_channel.id, channel_id);
    // References chain should point at the customer's original message —
    // that's what we recorded as the inbound `channel_messages` row.
    assert_eq!(
        thread.external_thread_id.as_deref(),
        Some(format!("<{inbound_msg_id}>").as_str())
    );

    // Fire through the outbound dispatcher against Greenmail's SMTP.
    {
        let mut conn = pool.get().unwrap();
        let content = backend::services::channels::OutboundContent {
            body_markdown: public_comment.content.clone(),
            body_html: None,
            attachments: vec![],
            external_id_hint: None,
        };
        let result = backend::services::channels::outbound::send_and_record(
            &relay_channel,
            thread,
            content,
            public_comment.id,
            greenmail_email_service(),
            pool.clone(),
            &mut conn,
        )
        .await
        .expect("send_and_record via Greenmail");

        // The outbound external_id must follow our Message-ID format
        // so the customer's reply threads back via the cascade's step 3.
        assert!(
            result.external_id.starts_with(&format!(
                "<ticket-{}.comment-{}.",
                ticket.id, public_comment.id
            )),
            "unexpected outbound external_id {}",
            result.external_id
        );

        // A channel_messages row should now exist with direction=outbound
        // linking the comment.
        let outbound =
            channels_repo::find_by_external_id(&mut conn, channel_id, &result.external_id)
                .unwrap()
                .expect("outbound channel_messages row");
        assert_eq!(outbound.direction, CHANNEL_DIRECTION_OUTBOUND);
        assert_eq!(outbound.ticket_id, Some(ticket.id));
        assert_eq!(outbound.comment_id, Some(public_comment.id));
    }

    // ============== STEP 4: Reply from customer threads back ==============
    // Send a second email that references our outbound Message-ID. The
    // adapter should fetch it and the pipeline's threading cascade
    // (References chain, step 1) should attach the comment to the
    // existing ticket rather than opening a new one.
    let outbound_msg_id = {
        let mut conn = pool.get().unwrap();
        use backend::schema::channel_messages::dsl as cm;
        cm::channel_messages
            .filter(cm::channel_id.eq(channel_id))
            .filter(cm::direction.eq(CHANNEL_DIRECTION_OUTBOUND))
            .filter(cm::comment_id.eq(public_comment.id))
            .select(cm::external_id)
            .first::<String>(&mut conn)
            .expect("find outbound external_id")
    };

    let reply_msg_id = format!("cycle-reply-{}@customer.test", channel_id);
    send_threaded_reply(
        "Re: Server down",
        "Still broken.",
        &reply_msg_id,
        &outbound_msg_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let reply_events = adapter.poll().await.expect("reply poll");
    let our_reply = reply_events
        .into_iter()
        .find(|e| matches!(e, InboundEvent::MessageReceived(m) if m.external_id.contains(&reply_msg_id)))
        .expect("expected our reply among the fetched events");

    {
        let mut conn = pool.get().unwrap();
        let ctx = PipelineContext::bare();
        let outcome = pipeline::process_event(&adapter, &channel, our_reply, &mut conn, &ctx)
            .await
            .expect("pipeline process_event on reply");
        match outcome {
            PipelineOutcome::ReplyAppended { ticket_id, .. } => {
                assert_eq!(
                    ticket_id, ticket.id,
                    "reply should attach to existing ticket"
                )
            }
            other => panic!("expected ReplyAppended, got {other:?}"),
        }
    }
}

/// Send a reply email with `In-Reply-To` + `References` set so the
/// threading cascade's step 1 can match it back to the outbound
/// message we just stored.
async fn send_threaded_reply(subject: &str, body: &str, message_id: &str, in_reply_to: &str) {
    let transport: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(greenmail_host())
            .port(SMTP_PORT)
            .credentials(Credentials::new(USER_LOGIN.into(), PASSWORD.into()))
            .build();

    let msg = Message::builder()
        .from("Alice <alice@customer.test>".parse().unwrap())
        .to(USER.parse().unwrap())
        .subject(subject)
        .header(lettre::message::header::MessageId::from(format!(
            "<{message_id}>"
        )))
        .header(lettre::message::header::InReplyTo::from(
            in_reply_to.to_string(),
        ))
        .header(lettre::message::header::References::from(
            in_reply_to.to_string(),
        ))
        .body(body.to_string())
        .unwrap();

    transport.send(msg).await.expect("send threaded reply");
}
