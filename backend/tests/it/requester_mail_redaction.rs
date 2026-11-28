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

    /// The requester adds `address` to `ticket` from the portal, as the
    /// request page's "Add someone" does (`POST .../participants`).
    async fn portal_add(&self, ticket: &Ticket, address: &str) {
        use actix_web::dev::Service as _;
        use actix_web::{test as http_test, web, App, HttpMessage as _};
        let workspace = context_of(&mut self.pool.get().expect("conn"), self.ws.workspace_id);
        let portal = backend::handlers::portal::PortalContext {
            user_uuid: self.requester,
            workspace_id: self.ws.workspace_id,
            workspace_uuid: self.ws.workspace_uuid,
            scope: backend::handlers::portal::PortalScope::Full,
        };
        let corr = Uuid::now_v7();
        let actor = backend::sync::actor::ActorContext::user(self.requester, Some(corr))
            .with_workspace(self.ws.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(backend::middleware::RequestContext::new(
                            corr,
                            actor.clone(),
                        ));
                    srv.call(req)
                })
                .route(
                    "/api/portal/tickets/{id}/participants",
                    web::post().to(backend::handlers::portal::add_participant),
                ),
        )
        .await;
        let resp = http_test::call_service(
            &app,
            http_test::TestRequest::post()
                .uri(&format!("/api/portal/tickets/{}/participants", ticket.id))
                .set_json(serde_json::json!({ "email": address }))
                .to_request(),
        )
        .await;
        assert!(resp.status().is_success(), "add: {}", resp.status());
    }

    /// The watcher row's `added_by` for `person` on `ticket`.
    fn added_by(&self, ticket: i32, person: Uuid) -> Option<Uuid> {
        use backend::schema::ticket_watchers as w;
        w::table
            .filter(w::ticket_id.eq(ticket))
            .filter(w::user_uuid.eq(person))
            .select(w::added_by)
            .first(&mut self.db.conn())
            .expect("watcher row")
    }

    /// The participant notice to `person`, composed afresh (the notice goes
    /// once per person, so an earlier one is cleared).
    fn fresh_participant_added(
        &self,
        ticket: &Ticket,
        person: Uuid,
        address: &str,
    ) -> OutboundEmail {
        let key = format!("participant_added:{}:{person}", ticket.id);
        diesel::delete(
            backend::schema::outbound_emails::table
                .filter(backend::schema::outbound_emails::idempotency_key.eq(key)),
        )
        .execute(&mut self.db.conn())
        .expect("clear the notice");
        self.participant_added(ticket, person, address)
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
    d.portal_add(&ticket, "victim@example.com").await;
    assert_eq!(d.added_by(ticket.id, victim), Some(d.requester));
    d.add(&ticket, colleague, d.ws.admin_uuid);

    let to_victim = d.fresh_participant_added(&ticket, victim, "victim@example.com");
    assert!(!names_the_ticket(&to_victim), "{to_victim:?}");
    let to_colleague = d.participant_added(&ticket, colleague, "colleague@example.com");
    assert!(
        to_colleague.subject.contains(TITLE),
        "{}",
        to_colleague.subject
    );

    d.reply(&ticket, victim);
    let again = d.fresh_participant_added(&ticket, victim, "victim@example.com");
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

/// A merge moves its sources' participants onto the destination keeping who
/// put them there: someone the requester added stays held, not upgraded to
/// the merging agent's choice.
#[actix_rt::test]
async fn a_merge_keeps_who_added_a_participant() {
    let d = desk();
    let destination = d.ticket("portal", None);
    let source = d.ticket("portal", None);
    let victim = d.outsider("Victim", "victim@example.com");
    d.portal_add(&source, "victim@example.com").await;
    let agent = backend::sync::actor::ActorContext::user(d.ws.admin_uuid, None)
        .with_workspace(d.ws.workspace_id);
    backend::sync::session::with_actor_context::<_, backend::repository::ticket_merge::MergeError>(
        &mut d.pool.get().expect("conn"),
        &agent,
        |c| {
            backend::repository::ticket_merge::execute_merge(
                c,
                backend::repository::ticket_merge::MergeInput {
                    destination_ticket_id: destination.id,
                    source_ticket_ids: vec![source.id],
                    reason: None,
                    notify_customer: false,
                    expected_state: Vec::new(),
                    marker_body: None,
                },
                &agent,
            )
        },
    )
    .expect("merge");
    assert_eq!(d.added_by(destination.id, victim), Some(d.requester));
    let notice = d.fresh_participant_added(&destination, victim, "victim@example.com");
    assert!(!names_the_ticket(&notice), "{notice:?}");
}

/// A person on both tickets keeps the more restrictive record of who put them
/// there: the requester's choice on the source over staff's on the
/// destination.
#[actix_rt::test]
async fn a_merge_keeps_the_more_restrictive_adder() {
    let d = desk();
    let destination = d.ticket("portal", None);
    let source = d.ticket("portal", None);
    let victim = d.outsider("Victim", "victim@example.com");
    d.add(&destination, victim, d.ws.admin_uuid);
    assert_eq!(d.added_by(destination.id, victim), Some(d.ws.admin_uuid));
    d.portal_add(&source, "victim@example.com").await;
    let agent = backend::sync::actor::ActorContext::user(d.ws.admin_uuid, None)
        .with_workspace(d.ws.workspace_id);
    backend::sync::session::with_actor_context::<_, backend::repository::ticket_merge::MergeError>(
        &mut d.pool.get().expect("conn"),
        &agent,
        |c| {
            backend::repository::ticket_merge::execute_merge(
                c,
                backend::repository::ticket_merge::MergeInput {
                    destination_ticket_id: destination.id,
                    source_ticket_ids: vec![source.id],
                    reason: None,
                    notify_customer: false,
                    expected_state: Vec::new(),
                    marker_body: None,
                },
                &agent,
            )
        },
    )
    .expect("merge");
    assert_eq!(d.added_by(destination.id, victim), Some(d.requester));
    let notice = d.fresh_participant_added(&destination, victim, "victim@example.com");
    assert!(!names_the_ticket(&notice), "{notice:?}");
}

/// Someone who put themselves on a request (following it) isn't someone else
/// choosing to mail them: on a ticket the requester filed, they get its text.
/// A stranger's ticket still names itself by number only.
#[actix_rt::test]
async fn a_follower_who_added_themselves_gets_the_ticket() {
    let d = desk();
    let follower = d.outsider("Follower", "follower@example.com");
    let ticket = d.ticket("portal", None);
    d.add(&ticket, follower, follower);
    let notice = d.fresh_participant_added(&ticket, follower, "follower@example.com");
    assert!(notice.subject.contains(TITLE), "{}", notice.subject);

    let strangers = d.ticket("api", None);
    d.add(&strangers, follower, follower);
    let notice = d.fresh_participant_added(&strangers, follower, "follower@example.com");
    assert!(!names_the_ticket(&notice), "{notice:?}");
}

/// A reply that came in by email counts as its sender writing on the ticket
/// only when it passed DMARC: a forged From must not stand for the address it
/// names. A portal reply counts as it is.
#[actix_rt::test]
async fn an_emailed_reply_counts_only_when_it_passed_dmarc() {
    let d = desk();
    let ticket = d.ticket("api", None);
    let emailed = |verdict: &str| {
        d.run(|c| {
            backend::repository::comments::create_comment(
                c,
                backend::models::NewComment {
                    content: "Still broken.".into(),
                    user_uuid: d.requester,
                    ticket_id: ticket.id,
                    content_format: backend::models::ContentFormat::Plaintext,
                    channel_metadata: Some(serde_json::json!({
                        "provider": "email_imap",
                        "sender_auth": verdict,
                    })),
                    ..Default::default()
                },
                None,
            )
        });
    };
    for verdict in ["unknown", "fail"] {
        emailed(verdict);
        let update = d.deliver(d.status_update(&ticket)).await;
        assert!(!names_the_ticket(&update), "{verdict}: {update:?}");
    }
    emailed("pass");
    let update = d.deliver(d.status_update(&ticket)).await;
    assert!(update.subject.contains(TITLE), "{}", update.subject);
}

/// A workspace member (a requester, not staff) with `address` as their primary
/// address.
fn copied_member(d: &Desk, name: &str, address: &str) -> Uuid {
    use backend::schema::workspace_members as m;
    let person = d.outsider(name, address);
    diesel::insert_into(m::table)
        .values((
            m::workspace_id.eq(d.ws.workspace_id),
            m::user_uuid.eq(person),
            m::role.eq("member"),
        ))
        .execute(&mut d.db.conn())
        .expect("seat the member");
    person
}

/// Mail into the inbox from `from`, copying `cc`, with the receiving server's
/// DMARC verdict and the ids it answers.
fn inbound(from: &str, cc: &str, dmarc: &str, id: &str, in_reply_to: &[&str]) -> String {
    let threading = if in_reply_to.is_empty() {
        String::new()
    } else {
        format!(
            "In-Reply-To: {}\r\nReferences: {}\r\n",
            in_reply_to[0],
            in_reply_to.join(" ")
        )
    };
    format!(
        "Authentication-Results: mx.example.com; dmarc={dmarc} header.from=example\r\n\
         From: {from}\r\nTo: support@example.com\r\nCc: {cc}\r\nSubject: Laptop\r\n\
         Message-ID: <{id}>\r\n{threading}Date: Mon, 1 Jan 2024 09:00:00 +0000\r\n\r\n\
         It won't start.\r\n"
    )
}

/// What mail about `ticket` to `person` may show, as every queue path asks.
fn text_for(
    d: &Desk,
    ticket: i32,
    person: Uuid,
) -> backend::services::ticket_mail_redaction::TicketMailText {
    d.run(|c| {
        Ok(
            backend::services::ticket_mail_redaction::ticket_mail_redaction_by_id(
                c,
                ticket,
                backend::services::ticket_mail_redaction::Recipient::User(person),
            ),
        )
    })
}

/// Mail we sent `to`, under `message_id`.
fn sent_to(d: &Desk, to: &str, message_id: &str) {
    d.run(|c| {
        backend::repository::outbound_emails::enqueue(
            c,
            backend::models::NewOutboundEmail {
                channel_id: None,
                ticket_id: None,
                comment_id: None,
                recipient: to.into(),
                subject: "Update".into(),
                body_text: "Update".into(),
                body_html: None,
                message_id: message_id.into(),
                in_reply_to: None,
                references_list: vec![],
                headers_json: serde_json::json!({}),
                correlation_id: None,
                idempotency_key: None,
                sender_identity: backend::models::outbound_email_sender_identity::WORKSPACE.into(),
                mail_class: backend::models::outbound_email_mail_class::TRANSACTIONAL.into(),
            },
        )
        .map(|_| ())
    });
}

/// The workspace's requester sign-in provider, listing `domains` as its
/// organisation's. Written as stored, so a free-mail domain an older admin
/// form let through is there too.
fn org_domains(d: &Desk, domains: &[&str]) {
    sign_in_provider(d, d.ws.workspace_id, domains);
}

/// Workspace `workspace_id`'s requester sign-in provider, listing `domains`.
fn sign_in_provider(d: &Desk, workspace_id: i32, domains: &[&str]) {
    let domains: Vec<String> = domains.iter().map(|d| d.to_string()).collect();
    run_in_workspace(&d.pool, REF, workspace_id, |c| {
        backend::repository::workspace_identity_providers::save(
            c,
            workspace_id,
            &backend::repository::workspace_identity_providers::ProviderInput {
                kind: "oidc",
                display_name: "Acme",
                issuer_url: "https://id.acme.example",
                client_id: "nosdesk",
                client_secret: None,
                allowed_domains: &domains,
                enabled: true,
            },
        )
        .map(|_| ())
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))
    })
    .expect("sign-in provider");
}

/// An address copied on inbound mail counts as the team's choice only when
/// the message passed DMARC, the address is on the sender's own domain, and
/// that domain is one the workspace knows as its organisation's (its settings
/// or its sign-in provider list it) and not a shared mail provider. Any other is the
/// sender's choice, and gets the ticket by number until it writes on it,
/// which a reply that passed DMARC does.
#[actix_rt::test]
async fn a_copied_address_gets_the_ticket_only_from_its_senders_authenticated_domain() {
    use backend::services::ticket_mail_redaction::TicketMailText;
    let d = desk();
    org_domains(&d, &["acme.example", "gmail.com"]);
    let inbox = crate::channel_backlog::Inbox::on(None, d.pool.clone(), d.ws.clone());
    let colleague = copied_member(&d, "Colleague", "colleague@acme.example");
    let friend = copied_member(&d, "Friend", "friend@other.example");
    let unchecked = copied_member(&d, "Unchecked", "unchecked@acme.example");
    let victim = copied_member(&d, "Victim", "victim@gmail.com");
    let unlisted = copied_member(&d, "Unlisted", "unlisted@beta.example");
    let at = chrono::Utc::now();

    // A shared mail provider never stands for colleagues, listed or not.
    let shared = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "attacker@gmail.com",
                    "victim@gmail.com",
                    "pass",
                    "cc-5@gmail.com",
                    &[],
                ),
                at,
            )
            .await,
    );
    assert_eq!(text_for(&d, shared, victim), TicketMailText::Redacted);

    // Nor does a domain the workspace doesn't know.
    let unknown = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "sam@beta.example",
                    "unlisted@beta.example",
                    "pass",
                    "cc-6@beta.example",
                    &[],
                ),
                at,
            )
            .await,
    );
    assert_eq!(text_for(&d, unknown, unlisted), TicketMailText::Redacted);

    let same_domain = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "sam@acme.example",
                    "colleague@acme.example",
                    "pass",
                    "cc-1@acme.example",
                    &[],
                ),
                at,
            )
            .await,
    );
    assert_eq!(d.added_by(same_domain, colleague), None);
    assert_eq!(text_for(&d, same_domain, colleague), TicketMailText::Full);

    let cross_domain = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "sam@acme.example",
                    "friend@other.example",
                    "pass",
                    "cc-2@acme.example",
                    &[],
                ),
                at,
            )
            .await,
    );
    let sender = inbox.ticket(cross_domain).requester_uuid;
    assert_eq!(d.added_by(cross_domain, friend), sender);
    assert_eq!(text_for(&d, cross_domain, friend), TicketMailText::Redacted);

    let no_verdict = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "sam@acme.example",
                    "unchecked@acme.example",
                    "none",
                    "cc-3@acme.example",
                    &[],
                ),
                at,
            )
            .await,
    );
    assert_eq!(
        text_for(&d, no_verdict, unchecked),
        TicketMailText::Redacted
    );

    // The copied friend replies, and the reply passed DMARC: they wrote on it.
    let replied = inbox
        .receive(
            &inbound(
                "friend@other.example",
                "sam@acme.example",
                "pass",
                "cc-4@other.example",
                &["<cc-2@acme.example>"],
            ),
            chrono::Utc::now(),
        )
        .await;
    assert!(
        matches!(replied, backend::services::channels::pipeline::PipelineOutcome::ReplyAppended { ticket_id, .. } if ticket_id == cross_domain),
        "{replied:?}"
    );
    assert_eq!(text_for(&d, cross_domain, friend), TicketMailText::Full);
}

/// The workspace's own list of organisation domains, written as stored, so a
/// free-mail domain the settings form would refuse is there too.
fn listed_domains(d: &Desk, domains: &[&str]) {
    listed_domains_in(d, d.ws.workspace_id, domains);
}

/// Workspace `workspace_id`'s own list, written as stored.
fn listed_domains_in(d: &Desk, workspace_id: i32, domains: &[&str]) {
    let domains: Vec<Option<String>> = domains.iter().map(|d| Some(d.to_string())).collect();
    run_in_workspace(&d.pool, REF, workspace_id, |c| {
        backend::repository::site_settings::update_site_settings(
            c,
            backend::models::UpdateSiteSettings {
                organisation_domains: Some(domains),
                ..Default::default()
            },
        )
        .map(|_| ())
    })
    .expect("organisation domains");
}

/// Every workspace on the server but the desk's.
fn other_workspaces(d: &Desk) -> Vec<i32> {
    use backend::schema::workspaces as w;
    w::table
        .filter(w::id.ne(d.ws.workspace_id))
        .select(w::id)
        .order(w::id)
        .load(&mut d.db.conn())
        .expect("workspaces")
}

/// With no sign-in provider, a domain the admin listed in the workspace's
/// settings is an organisation domain: a copied colleague there gets the
/// ticket in full from mail that passed DMARC. Before it is listed, the same
/// mail leaves them with the number only. A listed free-mail domain still
/// never stands for colleagues.
#[actix_rt::test]
async fn a_listed_organisation_domain_counts_without_a_sign_in_provider() {
    use backend::services::ticket_mail_redaction::TicketMailText;
    let d = desk();
    let inbox = crate::channel_backlog::Inbox::on(None, d.pool.clone(), d.ws.clone());
    let colleague = copied_member(&d, "Colleague", "colleague@acme.example");
    let victim = copied_member(&d, "Victim", "victim@gmail.com");
    let at = chrono::Utc::now();
    let copied = |from: &str, cc: &str, id: &str| inbound(from, cc, "pass", id, &[]);

    // Nothing listed and no provider: the sender's choice.
    let before = crate::channel_backlog::opened(
        inbox
            .receive(
                &copied(
                    "sam@acme.example",
                    "colleague@acme.example",
                    "org-1@acme.example",
                ),
                at,
            )
            .await,
    );
    let sender = inbox.ticket(before).requester_uuid;
    assert_eq!(d.added_by(before, colleague), sender);
    assert_eq!(text_for(&d, before, colleague), TicketMailText::Redacted);

    listed_domains(&d, &["Acme.example", "gmail.com"]);
    assert_eq!(
        d.run(backend::repository::site_settings::organisation_domains),
        vec!["acme.example".to_string(), "gmail.com".to_string()]
    );

    let after = crate::channel_backlog::opened(
        inbox
            .receive(
                &copied(
                    "sam@acme.example",
                    "colleague@acme.example",
                    "org-2@acme.example",
                ),
                at,
            )
            .await,
    );
    assert_eq!(d.added_by(after, colleague), None);
    assert_eq!(text_for(&d, after, colleague), TicketMailText::Full);

    let shared = crate::channel_backlog::opened(
        inbox
            .receive(
                &copied("attacker@gmail.com", "victim@gmail.com", "org-3@gmail.com"),
                at,
            )
            .await,
    );
    assert_eq!(text_for(&d, shared, victim), TicketMailText::Redacted);
}

/// Another workspace's domains count for nothing here. Inbound mail runs on a
/// connection elevated past row-level security and pinned to the receiving
/// workspace, so the domains are read for the pinned workspace by name: every
/// other workspace lists the sender's domain and has a sign-in provider for
/// it, the receiving one has neither, and the copied colleague gets the
/// number only. The receiving workspace's settings row is written last, so a
/// read that took the first row it could see would find another workspace's.
#[actix_rt::test]
async fn another_workspaces_organisation_domains_dont_count() {
    use backend::services::ticket_mail_redaction::TicketMailText;
    let d = desk();
    let others = other_workspaces(&d);
    assert!(!others.is_empty(), "the seed has other workspaces");
    for &other in &others {
        listed_domains_in(&d, other, &["acme.example"]);
        sign_in_provider(&d, other, &["acme.example"]);
    }
    listed_domains(&d, &[]);

    // Elevated and pinned, as the inbound paths run.
    let mut conn = d.pool.get().expect("conn");
    let actor = backend::sync::actor::ActorContext::system("channels:inbound")
        .with_workspace(d.ws.workspace_id);
    backend::sync::session::elevate_session_role(&mut conn, &actor).expect("elevate");
    let listed = backend::repository::site_settings::organisation_domains(&mut conn);
    let provider = backend::repository::workspace_identity_providers::get(&mut conn);
    backend::sync::session::reset_session_role(&mut conn);
    drop(conn);
    assert_eq!(listed.expect("organisation domains"), Vec::<String>::new());
    assert!(provider.expect("provider").is_none());

    let inbox = crate::channel_backlog::Inbox::on(None, d.pool.clone(), d.ws.clone());
    let colleague = copied_member(&d, "Colleague", "colleague@acme.example");
    let ticket = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "sam@acme.example",
                    "colleague@acme.example",
                    "pass",
                    "other-1@acme.example",
                    &[],
                ),
                chrono::Utc::now(),
            )
            .await,
    );
    let sender = inbox.ticket(ticket).requester_uuid;
    assert_eq!(d.added_by(ticket, colleague), sender);
    assert_eq!(text_for(&d, ticket, colleague), TicketMailText::Redacted);
}

/// The domains the Cc rule counts are both lists together: the admin's and
/// the enabled sign-in provider's, each once.
#[actix_rt::test]
async fn organisation_domains_join_the_list_and_the_sign_in_provider() {
    let d = desk();
    listed_domains(&d, &["acme.example", "shared.example"]);
    org_domains(&d, &["shared.example", "sso.example"]);
    assert_eq!(
        d.run(backend::repository::site_settings::organisation_domains),
        vec![
            "acme.example".to_string(),
            "shared.example".to_string(),
            "sso.example".to_string()
        ]
    );
}

/// An emailed reply with no DMARC verdict still counts when it answers mail we
/// sent that same address: only a recipient knows the Message-ID. Naming one
/// we sent someone else doesn't count.
#[actix_rt::test]
async fn a_reply_to_mail_we_sent_the_address_counts_without_dmarc() {
    use backend::services::ticket_mail_redaction::TicketMailText;
    let d = desk();
    let inbox = crate::channel_backlog::Inbox::on(None, d.pool.clone(), d.ws.clone());
    let unchecked = copied_member(&d, "Unchecked", "unchecked@acme.example");
    let ticket = crate::channel_backlog::opened(
        inbox
            .receive(
                &inbound(
                    "sam@acme.example",
                    "unchecked@acme.example",
                    "none",
                    "ask-1@acme.example",
                    &[],
                ),
                chrono::Utc::now(),
            )
            .await,
    );
    assert_eq!(text_for(&d, ticket, unchecked), TicketMailText::Redacted);
    let fresh = "0123456789abcdef0123456789abcdef";
    let theirs = format!("theirs.{fresh}@nosdesk.test");
    let old = "old.0002abcd@nosdesk.test";
    let ours = format!("ours.{fresh}@nosdesk.test");
    sent_to(&d, "someone-else@acme.example", &theirs);
    sent_to(&d, "unchecked@acme.example", old);
    sent_to(&d, "unchecked@acme.example", &ours);
    let mut n = 1;
    let replies = &inbox;
    let mut reply = |refs: Vec<String>| {
        n += 1;
        let refs: Vec<&str> = refs.iter().map(String::as_str).collect();
        let raw = inbound(
            "unchecked@acme.example",
            "sam@acme.example",
            "none",
            &format!("ask-{n}@acme.example"),
            &refs,
        );
        async move { replies.receive(&raw, chrono::Utc::now()).await }
    };
    let thread = "<ask-1@acme.example>".to_string();

    // Mail we sent someone else; an old 32-bit id about no ticket; ours, but
    // past the first 20 ids.
    reply(vec![format!("<{theirs}>"), thread.clone()]).await;
    reply(vec![format!("<{old}>"), thread.clone()]).await;
    let mut long: Vec<String> = vec![thread.clone()];
    long.extend((0..19).map(|i| format!("<filler-{i}@acme.example>")));
    long.push(format!("<{ours}>"));
    reply(long).await;
    assert_eq!(text_for(&d, ticket, unchecked), TicketMailText::Redacted);

    inbox
        .receive(
            &inbound(
                "Unchecked@Acme.example",
                "sam@acme.example",
                "none",
                "ask-9@acme.example",
                &[&format!("<{ours}>"), "<ask-1@acme.example>"],
            ),
            chrono::Utc::now(),
        )
        .await;
    assert_eq!(text_for(&d, ticket, unchecked), TicketMailText::Full);
}
