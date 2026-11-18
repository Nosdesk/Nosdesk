//! Mail a mailbox held before its channel connected is history: it opens its
//! ticket, but nobody is answered for it. A message received since the
//! channel connected still gets its automatic acknowledgement. The
//! acknowledgements here go through the real send path to a local SMTP sink,
//! so a missing one is a message never sent, not a send path left unwired.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use diesel::prelude::*;
use serde_json::json;

use backend::models::{Channel, NewChannel, Ticket, CHANNEL_DIRECTION_OUTBOUND};
use backend::repository::{channels as channels_repo, tickets};
use backend::services::channels::auto_ack::send_auto_ack;
use backend::services::channels::email_imap::{
    build_email_imap_adapter, parse_rfc822_into_inbound_message,
};
use backend::services::channels::pipeline::{self, PipelineContext, PipelineOutcome};
use backend::services::channels::InboundEvent;
use backend::services::outbound_email::OutboundEmailResolver;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::email::{EmailConfig, EmailService, SmtpSecurity};

use crate::common::{self, TestDb, TestPool, WorkspaceSeed};

/// An SMTP server on a local port that accepts every message and records
/// who it was for.
pub struct Sink {
    pub port: u16,
    recipients: Arc<Mutex<Vec<String>>>,
}

impl Sink {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the sink");
        let port = listener.local_addr().expect("sink address").port();
        let recipients = Arc::new(Mutex::new(Vec::new()));
        let seen = recipients.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let seen = seen.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &seen);
                });
            }
        });
        Self { port, recipients }
    }

    /// Every address a message was delivered to, in order.
    pub fn recipients(&self) -> Vec<String> {
        self.recipients.lock().expect("sink lock").clone()
    }
}

fn serve(stream: std::net::TcpStream, seen: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    let mut lines = BufReader::new(stream);
    out.write_all(b"220 sink ESMTP\r\n")?;
    let mut to = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if lines.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let command = line.trim_end().to_ascii_uppercase();
        if command.starts_with("EHLO") || command.starts_with("HELO") {
            out.write_all(b"250 sink\r\n")?;
        } else if command.starts_with("RCPT TO:") {
            let address = line.trim_end()["RCPT TO:".len()..]
                .trim()
                .trim_matches(['<', '>'])
                .to_string();
            to.push(address);
            out.write_all(b"250 OK\r\n")?;
        } else if command == "DATA" {
            out.write_all(b"354 go ahead\r\n")?;
            loop {
                line.clear();
                if lines.read_line(&mut line)? == 0 || line.trim_end() == "." {
                    break;
                }
            }
            seen.lock().expect("sink lock").append(&mut to);
            out.write_all(b"250 queued\r\n")?;
        } else if command == "QUIT" {
            out.write_all(b"221 bye\r\n")?;
            return Ok(());
        } else {
            out.write_all(b"250 OK\r\n")?;
        }
    }
}

/// A workspace with an IMAP channel and a pipeline whose acknowledgements
/// reach the sink.
pub struct Inbox {
    /// Holds the sandbox database for the test's lifetime.
    pub _db: TestDb,
    pub pool: TestPool,
    pub ws: WorkspaceSeed,
    pub channel: Channel,
    pub sink: Sink,
    pub resolver: Arc<OutboundEmailResolver>,
}

impl Inbox {
    pub fn new() -> Self {
        let db = TestDb::new();
        let ws = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn")).a;
        Self::in_workspace(db, ws)
    }

    /// The IMAP channel in `ws` of `db`, already seeded.
    pub fn in_workspace(db: TestDb, ws: WorkspaceSeed) -> Self {
        let pool = db.runtime_pool(4);
        let sink = Sink::start();
        let mail = Arc::new(EmailService::new(EmailConfig {
            smtp_host: "127.0.0.1".into(),
            smtp_port: sink.port,
            smtp_username: String::new(),
            smtp_password: String::new(),
            from_name: "Support".into(),
            from_email: "support@example.com".into(),
            enabled: true,
            security: SmtpSecurity::Plaintext,
        }));
        let resolver = Arc::new(OutboundEmailResolver::new(pool.clone(), Some(mail)));
        let channel = as_admin(&pool, &ws, |c| {
            channels_repo::create(
                c,
                NewChannel {
                    provider: "email_imap".into(),
                    name: "Support".into(),
                    enabled: true,
                    config: json!({
                        "host": "imap.example.com",
                        "username": "support@example.com",
                        "reply_domain": "example.com",
                    }),
                },
            )
        });
        Self {
            _db: db,
            pool,
            ws,
            channel,
            sink,
            resolver,
        }
    }

    pub fn context(&self) -> PipelineContext {
        PipelineContext {
            pool: Some(self.pool.clone()),
            resolver: Some(self.resolver.clone()),
            ..PipelineContext::bare()
        }
    }

    /// Run `raw`, received by the mail server at `received_at`, through the
    /// pipeline as the channel's poll would.
    pub async fn receive(&self, raw: &str, received_at: DateTime<Utc>) -> PipelineOutcome {
        self.receive_with(raw, received_at, &self.context()).await
    }

    pub async fn receive_with(
        &self,
        raw: &str,
        received_at: DateTime<Utc>,
        ctx: &PipelineContext,
    ) -> PipelineOutcome {
        // The adapter is built from the stored row, as the registry builds it.
        let channel = as_admin(&self.pool, &self.ws, |c| {
            channels_repo::find(c, self.channel.id)
        });
        let adapter = build_email_imap_adapter(&channel, self.resolver_mail(), self.pool.clone())
            .expect("adapter");
        let msg =
            parse_rfc822_into_inbound_message(raw.as_bytes(), Some(received_at)).expect("parse");
        let mut conn = self.pool.get().expect("conn");
        let actor =
            ActorContext::system("channels:inbound").with_workspace(self.channel.workspace_id);
        backend::sync::session::elevate_session_role(&mut conn, &actor).expect("elevate");
        let outcome = pipeline::process_event(
            &adapter,
            &channel,
            InboundEvent::MessageReceived(msg),
            &mut conn,
            ctx,
        )
        .await
        .expect("ingest");
        backend::sync::session::reset_session_role(&mut conn);
        outcome
    }

    fn resolver_mail(&self) -> Arc<EmailService> {
        Arc::new(EmailService::new(EmailConfig {
            smtp_host: String::new(),
            smtp_port: 25,
            smtp_username: String::new(),
            smtp_password: String::new(),
            from_name: String::new(),
            from_email: String::new(),
            enabled: false,
            security: SmtpSecurity::StartTls,
        }))
    }

    pub fn ticket(&self, id: i32) -> Ticket {
        as_admin(&self.pool, &self.ws, |c| tickets::get_ticket_by_id(c, id))
    }

    /// The acknowledgements recorded for `ticket`.
    pub fn acks(&self, ticket: i32) -> i64 {
        use backend::schema::channel_messages as m;
        as_admin(&self.pool, &self.ws, |c| {
            m::table
                .filter(m::ticket_id.eq(ticket))
                .filter(m::direction.eq(CHANNEL_DIRECTION_OUTBOUND))
                .count()
                .get_result(c)
        })
    }

    /// Wait for `ticket`'s acknowledgement to be sent and recorded.
    pub async fn wait_for_ack(&self, ticket: i32) {
        for _ in 0..100 {
            if self.acks(ticket) > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("ticket {ticket} was never acknowledged");
    }
}

pub fn as_admin<T>(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    with_actor_context(&mut conn, &actor, f).expect("as the workspace admin")
}

/// A customer's message with its own Message-ID.
pub fn message(from: &str, subject: &str, id: &str) -> String {
    format!(
        "From: {from}\r\nTo: support@example.com\r\nSubject: {subject}\r\n\
         Message-ID: <{id}@customer.example>\r\nDate: Mon, 1 Jan 2024 09:00:00 +0000\r\n\r\n\
         Hello, can you help?\r\n"
    )
}

pub fn opened(outcome: PipelineOutcome) -> i32 {
    match outcome {
        PipelineOutcome::TicketOpened { ticket_id, .. } => ticket_id,
        other => panic!("expected a new ticket, got {other:?}"),
    }
}

/// Long enough before the channel connected to be its mailbox's history.
pub fn years_ago() -> DateTime<Utc> {
    Utc::now() - TimeDelta::days(700)
}

#[actix_web::test]
async fn old_mail_opens_a_ticket_without_an_acknowledgement_and_new_mail_gets_one() {
    let inbox = Inbox::new();

    let old = opened(
        inbox
            .receive(
                &message("Ada <ada@customer.example>", "Printer", "old-1"),
                years_ago(),
            )
            .await,
    );
    let new = opened(
        inbox
            .receive(
                &message("Bo <bo@customer.example>", "Laptop", "new-1"),
                Utc::now(),
            )
            .await,
    );

    // The new message's acknowledgement goes out, which proves the send path
    // works here; the old one never asked for one.
    inbox.wait_for_ack(new).await;
    assert_eq!(inbox.acks(old), 0, "no acknowledgement for old mail");
    assert_eq!(inbox.sink.recipients(), vec!["bo@customer.example"]);
}

/// Mail a few minutes older than the channel is clock skew, not history.
#[actix_web::test]
async fn mail_within_the_clock_skew_is_new() {
    let inbox = Inbox::new();
    let ticket = opened(
        inbox
            .receive(
                &message("Cy <cy@customer.example>", "VPN", "skew-1"),
                Utc::now() - TimeDelta::minutes(2),
            )
            .await,
    );
    inbox.wait_for_ack(ticket).await;
}

/// The connection time the channel's first poll records counts, not the
/// row's creation: a channel saved long before it first reached its mailbox
/// treats what arrived in between as history too.
#[actix_web::test]
async fn the_recorded_connection_time_counts() {
    let inbox = Inbox::new();
    as_admin(&inbox.pool, &inbox.ws, |c| {
        channels_repo::update_runtime_state(
            c,
            inbox.channel.id,
            json!({ "connected_at": Utc::now() + TimeDelta::hours(1) }),
        )
    });
    let ticket = opened(
        inbox
            .receive(
                &message("Di <di@customer.example>", "Email", "gap-1"),
                Utc::now() - TimeDelta::minutes(30),
            )
            .await,
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(inbox.acks(ticket), 0);
    assert!(inbox.sink.recipients().is_empty());
}

/// The acknowledgement refuses mail from before its channel connected even
/// when it is asked to send one.
#[actix_web::test]
async fn the_acknowledgement_refuses_old_mail_itself() {
    let inbox = Inbox::new();
    // A ticket opened with no acknowledgement wired up.
    let ticket = opened(
        inbox
            .receive_with(
                &message("Ed <ed@customer.example>", "Monitor", "guard-1"),
                Utc::now(),
                &PipelineContext::bare(),
            )
            .await,
    );
    let ticket = inbox.ticket(ticket);

    let refused = send_auto_ack(
        &inbox.pool,
        &inbox.resolver,
        &inbox.channel,
        &ticket,
        "<guard-1@customer.example>",
        None,
        years_ago(),
    )
    .await;
    assert_eq!(refused, Ok(()));
    assert_eq!(inbox.acks(ticket.id), 0);
    assert!(inbox.sink.recipients().is_empty(), "nothing sent");

    // The same call for mail received now sends.
    let sent = send_auto_ack(
        &inbox.pool,
        &inbox.resolver,
        &inbox.channel,
        &ticket,
        "<guard-1@customer.example>",
        None,
        Utc::now(),
    )
    .await;
    assert_eq!(sent, Ok(()));
    assert_eq!(inbox.acks(ticket.id), 1);
    assert_eq!(inbox.sink.recipients(), vec!["ed@customer.example"]);
}
