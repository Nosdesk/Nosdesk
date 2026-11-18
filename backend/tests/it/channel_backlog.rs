//! Mail a mailbox held before its channel connected is history: it opens its
//! ticket as imported, and nobody is answered or notified for it, nor does
//! it start approvals or run rules. A message received since the
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

use backend::models::{
    Channel, NewChannel, NewTicket, RuleTriggerKind as Trigger, Ticket, TicketUpdate,
    WorkflowStateCategory, CHANNEL_DIRECTION_OUTBOUND, EMAIL_BACKLOG,
};
use backend::repository::{channels as channels_repo, tickets, workflow_states};
use backend::schema::{sync_actions, ticket_approvals, ticket_categories};
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
use crate::rules_event_triggers::{marker, Harness, Spec};

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
    /// Holds the sandbox database for the test's lifetime, when the inbox
    /// made it.
    pub _db: Option<TestDb>,
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
        let pool = db.runtime_pool(4);
        Self::on(Some(db), pool, ws)
    }

    /// An IMAP channel in `ws`, reached through `pool`.
    pub fn on(db: Option<TestDb>, pool: TestPool, ws: WorkspaceSeed) -> Self {
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

// ---------- Imported history: approvals, rules, reopen, notifications ----------

/// The ticket ids `rule` has run on.
fn ran_on(h: &Harness, ws: &WorkspaceSeed, rule: i32) -> Vec<i32> {
    h.applications(ws, rule)
        .into_iter()
        .map(|a| a.ticket_id)
        .collect()
}

#[actix_web::test]
async fn old_mail_opens_an_imported_ticket_that_runs_no_rules_while_new_mail_does() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let rule = h.rule(a, Spec::live(Trigger::TicketCreated, marker()));
    let inbox = Inbox::on(None, h.pool.clone(), a.clone());

    let old = opened(
        inbox
            .receive(
                &message("Ada <ada@customer.example>", "Printer", "old-2"),
                years_ago(),
            )
            .await,
    );
    let new = opened(
        inbox
            .receive(
                &message("Bo <bo@customer.example>", "Laptop", "new-2"),
                Utc::now(),
            )
            .await,
    );

    let old_ticket = inbox.ticket(old);
    assert_eq!(old_ticket.submitted_via.as_deref(), Some(EMAIL_BACKLOG));
    assert!(old_ticket.is_imported());
    assert_eq!(old_ticket.approval_state, None, "no approval round");
    let new_ticket = inbox.ticket(new);
    assert_eq!(new_ticket.submitted_via.as_deref(), Some("email_imap"));
    assert_eq!(new_ticket.origin_channel_id, old_ticket.origin_channel_id);

    inbox.wait_for_ack(new).await;
    assert_eq!(inbox.acks(old), 0, "no acknowledgement for old mail");
    h.drain().await;
    assert_eq!(
        ran_on(&h, a, rule),
        vec![new],
        "rules ran for new mail only"
    );
}

/// An inbound ticket has no request type, so it never needs approval; an
/// imported email ticket of a type that does still starts no round.
#[actix_web::test]
async fn an_email_backlog_ticket_starts_no_approval_round() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let ticket = h.as_admin(a, |c| {
        let laptops: i32 = diesel::insert_into(ticket_categories::table)
            .values((
                ticket_categories::name.eq("New laptop"),
                ticket_categories::approval_required.eq(true),
                ticket_categories::approval_rule.eq("any"),
            ))
            .returning(ticket_categories::id)
            .get_result(c)?;
        backend::repository::categories::set_category_approvers(c, laptops, &[a.admin_uuid])?;
        let open = workflow_states::default_state(c)?;
        tickets::create_ticket(
            c,
            NewTicket {
                title: "Laptop from 2019".to_string(),
                workflow_state_id: open.id,
                requester_uuid: Some(a.member_uuid),
                category_id: Some(laptops),
                submitted_via: Some(EMAIL_BACKLOG.to_string()),
                ..Default::default()
            },
        )
    });
    assert_eq!(ticket.approval_state, None);
    let rounds: i64 = h.as_admin(a, |c| {
        ticket_approvals::table
            .filter(ticket_approvals::ticket_id.eq(ticket.id))
            .count()
            .get_result(c)
    });
    assert_eq!(rounds, 0);
}

/// A reply from before the channel connected joins its ticket, but a done
/// ticket stays done, no reply rule runs and the event tells the
/// notification deriver it is history. A reply sent now does all three.
#[actix_web::test]
async fn an_old_reply_reopens_nothing_and_runs_no_reply_rules() {
    let h = Harness::new();
    let a = &h.seeded.a;
    let inbox = Inbox::on(None, h.pool.clone(), a.clone());
    let ticket = opened(
        inbox
            .receive_with(
                &message("Ada <ada@customer.example>", "Printer", "first-3"),
                Utc::now(),
                &PipelineContext::bare(),
            )
            .await,
    );
    let done = h.as_admin(a, |c| {
        workflow_states::first_in_category(c, WorkflowStateCategory::Done)
    });
    h.update(
        a,
        ticket,
        TicketUpdate {
            workflow_state_id: Some(done.id),
            ..Default::default()
        },
    );
    h.drain().await;
    let rule = h.rule(
        a,
        Spec {
            trigger_config: json!({ "reply_kind": "any" }),
            ..Spec::live(Trigger::TicketReplied, marker())
        },
    );

    let reply = |id: &str| {
        format!(
            "From: Ada <ada@customer.example>\r\nTo: support@example.com\r\nSubject: Re: Printer\r\n\
             Message-ID: <{id}@customer.example>\r\nIn-Reply-To: <first-3@customer.example>\r\n\
             References: <first-3@customer.example>\r\n\r\nStill broken.\r\n"
        )
    };
    let old = inbox
        .receive_with(&reply("reply-old"), years_ago(), &PipelineContext::bare())
        .await;
    assert!(
        matches!(old, PipelineOutcome::ReplyAppended { ticket_id, .. } if ticket_id == ticket),
        "{old:?}"
    );
    assert_eq!(
        inbox.ticket(ticket).workflow_state_id,
        done.id,
        "still done"
    );
    let created_via: Vec<serde_json::Value> = h.as_admin(a, |c| {
        sync_actions::table
            .filter(sync_actions::event_type.eq("comment.created"))
            .order(sync_actions::occurred_at)
            .select(sync_actions::data)
            .load::<serde_json::Value>(c)
            .map(|rows| rows.into_iter().map(|d| d["created_via"].clone()).collect())
    });
    assert_eq!(created_via.last().unwrap()["imported"], json!(true));
    h.drain().await;
    assert!(h.statuses(a, rule).is_empty(), "no reply rule for old mail");

    let new = inbox
        .receive_with(&reply("reply-new"), Utc::now(), &PipelineContext::bare())
        .await;
    assert!(
        matches!(new, PipelineOutcome::ReplyAppended { .. }),
        "{new:?}"
    );
    assert_ne!(inbox.ticket(ticket).workflow_state_id, done.id, "reopened");
    h.drain().await;
    assert_eq!(ran_on(&h, a, rule), vec![ticket]);
}
