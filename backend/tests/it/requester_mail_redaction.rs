//! Automatic mail about a ticket a stranger may have filed (one an API token
//! created, or a request-form ticket filed without confirmation) names it only
//! by number, to anyone outside the team, until that person writes on the
//! ticket themselves. Proving the address doesn't count, and nor does
//! following a "View request" link. Someone a requester added is held the
//! same way on any ticket. The emails are asserted on their outbound queue
//! rows; nothing is sent.

use std::collections::HashMap;
use std::sync::Arc;

use diesel::prelude::*;
use tokio::sync::RwLock;
use uuid::Uuid;

use backend::models::{NewTicket, OutboundEmail, Ticket};
use backend::services::notifications::channels::email::EmailChannel;
use backend::services::notifications::channels::NotificationDeliveryChannel;
use backend::services::notifications::deriver::{resolve, Intent, SyncActionRow};
use backend::services::notifications::types::{
    DeliverableNotification, NotificationChannel, NotificationPayload,
};
use backend::sync::session::run_in_workspace;
use backend::utils::email::{EmailBranding, EmailConfig, EmailService, SmtpSecurity};

use crate::common::{self, TestDb, TestPool, WorkspaceSeed};
use crate::portal_session::{context_of, link_server};

const REF: &str = "test:requester_mail_redaction";
/// What the stranger typed as the title and as their name.
const TITLE: &str = "Claim your refund at title-marker.example";
const NAME: &str = "Name Marker";
const REQUESTER: &str = "requester@example.com";
const BASE: &str = "https://desk.example.com";

struct Desk {
    db: TestDb,
    pool: TestPool,
    ws: WorkspaceSeed,
    requester: Uuid,
}

/// Workspace A, whose member is the requester: named by the stranger, with an
/// address nobody has proven.
fn desk() -> Desk {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let ws = seeded.a;
    let requester = ws.member_uuid;
    let mut c = db.conn();
    diesel::update(backend::schema::users::table.find(requester))
        .set(backend::schema::users::name.eq(NAME))
        .execute(&mut c)
        .expect("name the requester");
    diesel::insert_into(backend::schema::user_emails::table)
        .values((
            backend::schema::user_emails::user_uuid.eq(requester),
            backend::schema::user_emails::email.eq(REQUESTER),
            backend::schema::user_emails::email_type.eq("personal"),
            backend::schema::user_emails::is_primary.eq(true),
            backend::schema::user_emails::is_verified.eq(false),
        ))
        .execute(&mut c)
        .expect("requester address");
    let pool = db.runtime_pool(4);
    Desk {
        db,
        pool,
        ws,
        requester,
    }
}

impl Desk {
    fn run<T>(&self, f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>) -> T {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, f).expect("pinned work")
    }

    /// A ticket titled [`TITLE`] for the requester, filed `via` this way.
    fn ticket(&self, via: &str, verification_state: Option<&str>) -> Ticket {
        self.run(|c| {
            let open = backend::repository::workflow_states::default_state(c)?;
            backend::repository::tickets::create_ticket(
                c,
                NewTicket {
                    title: TITLE.into(),
                    workflow_state_id: open.id,
                    requester_uuid: Some(self.requester),
                    submitted_via: Some(via.into()),
                    verification_state: verification_state.map(str::to_string),
                    ..Default::default()
                },
            )
        })
    }

    /// The requester proves their address, as a sign-in code does.
    fn prove(&self) {
        backend::repository::user_emails::mark_primary_proven(&mut self.db.conn(), &self.requester)
            .expect("prove the address");
    }

    /// `person` writes a public reply on `ticket`, as from the portal.
    fn reply(&self, ticket: &Ticket, person: Uuid) {
        self.run(|c| {
            backend::repository::comments::create_comment(
                c,
                backend::models::NewComment {
                    content: "Still broken.".into(),
                    user_uuid: person,
                    ticket_id: ticket.id,
                    content_format: backend::models::ContentFormat::Plaintext,
                    ..Default::default()
                },
                None,
            )
        });
    }

    /// Put `person` on `ticket` as `by` would (the acting user is recorded).
    fn add(&self, ticket: &Ticket, person: Uuid, by: Uuid) {
        let actor =
            backend::sync::actor::ActorContext::user(by, None).with_workspace(self.ws.workspace_id);
        backend::sync::session::with_actor_context::<_, diesel::result::Error>(
            &mut self.pool.get().expect("conn"),
            &actor,
            |c| backend::repository::ticket_watchers::add_watcher(c, ticket.id, person, false),
        )
        .expect("add participant");
    }

    /// A non-member with `address` as their primary address.
    fn outsider(&self, name: &str, address: &str) -> Uuid {
        let mut c = self.db.conn();
        let person = common::insert_plain_user(&mut c, name);
        diesel::insert_into(backend::schema::user_emails::table)
            .values((
                backend::schema::user_emails::user_uuid.eq(person),
                backend::schema::user_emails::email.eq(address),
                backend::schema::user_emails::email_type.eq("personal"),
                backend::schema::user_emails::is_primary.eq(true),
                backend::schema::user_emails::is_verified.eq(false),
            ))
            .execute(&mut c)
            .expect("outsider address");
        person
    }

    fn payloads(&self, intent: Intent) -> Vec<NotificationPayload> {
        let row = SyncActionRow {
            sync_id: 1,
            workspace_id: self.ws.workspace_id,
            event_type: "ticket.updated".into(),
            data: serde_json::json!({}),
            actor_uuid: Some(self.ws.admin_uuid),
            actor_kind: "user".into(),
            occurred_at: chrono::Utc::now(),
        };
        self.run(|c| resolve(c, &row, intent))
    }

    /// Deliver `payload` by email; the row it queued.
    async fn deliver(&self, payload: NotificationPayload) -> OutboundEmail {
        let channel = EmailChannel::new(
            Arc::new(svc()),
            self.pool.clone(),
            BASE.into(),
            Arc::new(RwLock::new(HashMap::new())),
        );
        let notification = DeliverableNotification {
            id: None,
            uuid: Uuid::new_v4(),
            payload,
            channels: vec![NotificationChannel::Email],
        };
        channel.deliver(&notification).await.expect("deliver");
        let key = format!("notify:{}:", notification.uuid);
        self.run(|c| {
            use backend::schema::outbound_emails as o;
            o::table
                .filter(o::idempotency_key.like(format!("{key}%")))
                .first(c)
        })
    }

    fn acknowledgement(&self, ticket: &Ticket) -> NotificationPayload {
        let mut payloads = self.payloads(Intent::Created {
            ticket_id: ticket.id,
            ticket_title: ticket.title.clone(),
            requester: self.requester,
        });
        assert_eq!(payloads.len(), 1, "one acknowledgement");
        payloads.remove(0)
    }

    /// The requester's update for a move to a state of another category.
    fn status_update(&self, ticket: &Ticket) -> NotificationPayload {
        let other = self.run(|c| {
            use backend::schema::workflow_states as s;
            s::table
                .filter(s::workspace_id.eq(self.ws.workspace_id))
                .filter(s::category.eq(backend::models::WorkflowStateCategory::Done))
                .select(s::id)
                .first::<i32>(c)
        });
        let mut payloads = self.payloads(Intent::StatusChanged {
            ticket_id: ticket.id,
            ticket_title: ticket.title.clone(),
            requester: self.requester,
            previous_state_id: ticket.workflow_state_id,
            state_id: other,
        });
        assert_eq!(payloads.len(), 1, "one update");
        payloads.remove(0)
    }

    /// Tell `person` at `address` that the requester added them.
    fn participant_added(&self, ticket: &Ticket, person: Uuid, address: &str) -> OutboundEmail {
        let locale: unic_langid::LanguageIdentifier = "en-US".parse().expect("locale");
        self.run(|c| {
            backend::services::transactional_email::enqueue_participant_added(
                c,
                &svc(),
                &EmailBranding::new("Acme".into(), None, BASE.into()),
                address,
                person,
                NAME,
                ticket,
                &format!("{BASE}/request"),
                &locale,
            )
        })
    }
}

fn svc() -> EmailService {
    EmailService::new(EmailConfig {
        smtp_host: "smtp.example.com".into(),
        smtp_port: 587,
        smtp_username: "u".into(),
        smtp_password: "p".into(),
        from_name: "Nosdesk".into(),
        from_email: "noreply@nosdesk.test".into(),
        enabled: true,
        security: SmtpSecurity::StartTls,
    })
}

/// Whether the stranger's text appears anywhere in the email.
fn names_the_ticket(m: &OutboundEmail) -> bool {
    [TITLE, NAME].iter().any(|marker| {
        m.subject.contains(marker)
            || m.body_text.contains(marker)
            || m.body_html.as_deref().is_some_and(|h| h.contains(marker))
    })
}

/// The acknowledgement never names the title, for anyone.
#[actix_rt::test]
async fn the_acknowledgement_names_the_request_by_number() {
    let d = desk();
    for ticket in [d.ticket("portal", None), d.ticket("api", None)] {
        let m = d.deliver(d.acknowledgement(&ticket)).await;
        assert_eq!(m.recipient, REQUESTER);
        assert!(
            m.subject
                .ends_with(&format!("We've received your request #{}", ticket.number)),
            "{}",
            m.subject
        );
        assert!(!names_the_ticket(&m), "{m:?}");
    }
}

/// An API ticket and a request-form ticket filed without confirmation: status
/// updates and the participant notice name the ticket by number until the
/// requester writes on it, and then by its title to the requester only. The
/// person they were telling hasn't written on it, so they still get #N.
#[actix_rt::test]
async fn mail_about_a_strangers_ticket_names_it_once_the_requester_writes_on_it() {
    for (via, verification) in [("api", None), ("guest", None), ("guest", Some("pending"))] {
        let d = desk();
        let friend = common::insert_plain_user(&mut d.db.conn(), "Friend");
        let ticket = d.ticket(via, verification);

        let update = d.deliver(d.status_update(&ticket)).await;
        assert!(!names_the_ticket(&update), "{via}: {update:?}");
        assert!(
            update
                .subject
                .contains(&format!("Update on request #{}", ticket.number)),
            "{via}: {}",
            update.subject
        );
        let added = d.participant_added(&ticket, friend, "friend@example.com");
        assert!(!names_the_ticket(&added), "{via}: {added:?}");
        assert!(
            added
                .subject
                .contains(&format!("added to request #{}", ticket.number)),
            "{}",
            added.subject
        );

        d.reply(&ticket, d.requester);
        let update = d.deliver(d.status_update(&ticket)).await;
        assert!(update.subject.contains(TITLE), "{via}: {}", update.subject);
        let other = common::insert_plain_user(&mut d.db.conn(), "Other");
        let added = d.participant_added(&ticket, other, "other@example.com");
        assert!(!names_the_ticket(&added), "{via}: someone else: {added:?}");
    }
}

/// Proving the address isn't about this ticket: a relay can file a ticket for
/// any address, including one that signs in every day, and `proven_at` is the
/// same in every workspace. Before the ticket or after, it unlocks nothing.
#[actix_rt::test]
async fn proving_the_address_doesnt_unlock_a_strangers_ticket() {
    let d = desk();
    d.prove();
    let ticket = d.ticket("api", None);
    let update = d.deliver(d.status_update(&ticket)).await;
    assert!(!names_the_ticket(&update), "{update:?}");
    d.prove();
    let update = d.deliver(d.status_update(&ticket)).await;
    assert!(!names_the_ticket(&update), "after: {update:?}");
}

/// On a ticket the requester filed themselves, a person the requester added
/// gets the number only, with no names, until they write on it themselves;
/// one the team added gets the ticket.
#[actix_rt::test]
async fn people_a_requester_adds_get_the_number_until_they_write() {
    let d = desk();
    let ticket = d.ticket("portal", None);
    let victim = d.outsider("Victim", "victim@example.com");
    let colleague = d.outsider("Colleague", "colleague@example.com");
    d.add(&ticket, victim, d.requester);
    d.add(&ticket, colleague, d.ws.admin_uuid);

    let to_victim = d.participant_added(&ticket, victim, "victim@example.com");
    assert!(!names_the_ticket(&to_victim), "{to_victim:?}");
    let to_colleague = d.participant_added(&ticket, colleague, "colleague@example.com");
    assert!(
        to_colleague.subject.contains(TITLE),
        "{}",
        to_colleague.subject
    );

    d.reply(&ticket, victim);
    // The notice goes once per person; clear it to compose it again.
    diesel::delete(backend::schema::outbound_emails::table.find(to_victim.id))
        .execute(&mut d.db.conn())
        .expect("clear the notice");
    let again = d.participant_added(&ticket, victim, "victim@example.com");
    assert!(
        again.subject.contains(TITLE),
        "after writing: {}",
        again.subject
    );
}

/// A ticket from the portal, or from the request form after confirmation, is
/// the requester's own: its mail names it. So does mail to the team.
#[actix_rt::test]
async fn a_vouched_ticket_and_mail_to_the_team_name_the_ticket() {
    let d = desk();
    for ticket in [
        d.ticket("portal", None),
        d.ticket("guest", Some("verified")),
    ] {
        let update = d.deliver(d.status_update(&ticket)).await;
        assert!(update.subject.contains(TITLE), "{}", update.subject);
    }
    let ticket = d.ticket("api", None);
    let mut c = d.db.conn();
    diesel::insert_into(backend::schema::user_emails::table)
        .values((
            backend::schema::user_emails::user_uuid.eq(d.ws.admin_uuid),
            backend::schema::user_emails::email.eq("admin@a.example"),
            backend::schema::user_emails::email_type.eq("work"),
            backend::schema::user_emails::is_primary.eq(true),
            backend::schema::user_emails::is_verified.eq(true),
        ))
        .execute(&mut c)
        .expect("admin address");
    let staff = d.participant_added(&ticket, d.ws.admin_uuid, "admin@a.example");
    assert!(staff.subject.contains(TITLE), "{}", staff.subject);
}

/// Following the "View request" link signs the requester in but proves
/// nothing: mail scanners open every link. The address stays unverified and
/// the ticket's mail stays redacted.
#[actix_web::test]
async fn following_the_view_request_link_is_not_proof() {
    let d = desk();
    let ticket = d.ticket("api", None);
    let mut c = d.pool.get().expect("conn");
    let token =
        backend::utils::portal_ticket_link::sign(&mut c, d.ws.workspace_id, d.requester, ticket.id)
            .expect("sign");
    let srv = link_server(&d.pool, context_of(&mut c, d.ws.workspace_id));
    drop(c);
    let resp = awc::Client::builder()
        .disable_redirects()
        .finish()
        .get(srv.url(&format!("/api/portal/auth/ticket?t={token}")))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 302, "the link still signs in");
    assert!(
        resp.headers().get_all("set-cookie").count() > 0,
        "with a session"
    );

    let (verified, proven): (bool, Option<chrono::DateTime<chrono::Utc>>) = {
        use backend::schema::user_emails as e;
        e::table
            .filter(e::user_uuid.eq(d.requester))
            .select((e::is_verified, e::proven_at))
            .first(&mut d.db.conn())
            .expect("address")
    };
    assert!(!verified, "the link doesn't verify the address");
    assert_eq!(proven, None);
    let update = d.deliver(d.status_update(&ticket)).await;
    assert!(!names_the_ticket(&update), "{update:?}");
}

/// The reply an API token writes on a stranger's ticket reaches its requester,
/// who hasn't written on it, as a notice with the link and none of its words:
/// a token may be relaying a stranger's text. A reply from the team in the app
/// goes as written, under the number-only subject.
#[actix_rt::test]
async fn a_token_written_reply_reaches_an_unengaged_requester_as_a_notice() {
    const WORDS: &str = "Claim it at reply-marker.example";
    let d = desk();
    let ticket = d.ticket("api", None);
    let sent_for = |by_token: bool| {
        let comment = d.run(|c| {
            backend::repository::comments::create_comment(
                c,
                backend::models::NewComment {
                    content: WORDS.into(),
                    user_uuid: d.ws.admin_uuid,
                    ticket_id: ticket.id,
                    content_format: backend::models::ContentFormat::Plaintext,
                    ..Default::default()
                },
                None,
            )
        });
        backend::services::channels::outbound::enqueue_for_comment(
            ticket.clone(),
            comment.clone(),
            d.pool.clone(),
            by_token,
        );
        comment.id
    };
    let reply_row = |comment_id: i32| {
        d.run(|c| {
            use backend::schema::outbound_emails as o;
            o::table
                .filter(o::comment_id.eq(comment_id))
                .first::<OutboundEmail>(c)
                .optional()
        })
    };
    let settle = |comment_id: i32| async move {
        for _ in 0..50 {
            if let Some(row) = reply_row(comment_id) {
                return row;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("the reply was never queued");
    };

    let by_token = settle(sent_for(true)).await;
    assert_eq!(by_token.recipient, REQUESTER);
    assert_eq!(by_token.subject, format!("Re: [#{}]", ticket.number));
    assert!(
        !by_token.body_text.contains(WORDS),
        "{}",
        by_token.body_text
    );
    assert!(
        by_token
            .body_text
            .contains(&format!("new reply on request #{}", ticket.number)),
        "{}",
        by_token.body_text
    );

    let by_team = settle(sent_for(false)).await;
    assert!(by_team.body_text.contains(WORDS), "{}", by_team.body_text);
    assert_eq!(by_team.subject, format!("Re: [#{}]", ticket.number));
}
