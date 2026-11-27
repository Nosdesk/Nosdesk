//! The merge notice reaches a customer who wrote in through a hosted
//! workspace's own address (an `email_managed` channel) or a forwarding
//! address (`email_forward`), as the acknowledgement does, and the ticket
//! records that it went.
//!
//! Part of the hosted binary rather than `tests/it`: routing those channels
//! reads the process-wide `NOSDESK_TENANT_DOMAIN` and `NOSDESK_INBOUND_DOMAIN`,
//! which `tests/it` must not see set. The hosted binary sets both, as a hosted
//! deployment does.

use crate::common;

use diesel::prelude::*;

use backend::db::DbConnection;
use backend::models::{Channel, NewChannel, NewTicket, Ticket};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

const WS: i32 = 1;

struct Sent {
    notices: usize,
    /// (recipient, message_id, headers) of each queued email.
    queued: Vec<(String, String, serde_json::Value)>,
    /// `ticket.merge_notice_sent` rows on the source.
    recorded: i64,
    /// Each queued email's subject.
    subjects: Vec<String>,
    /// The destination's number.
    destination: i32,
}

/// Merge a ticket that came in on a `provider` channel (after `prepare` has
/// run against it) and send the notice.
fn merge_notice(provider: &str, prepare: impl FnOnce(&mut DbConnection, &Channel)) -> Sent {
    merge_notice_into(provider, None, prepare)
}

/// [`merge_notice`] into a destination filed `destination_via` this way.
fn merge_notice_into(
    provider: &str,
    destination_via: Option<&str>,
    prepare: impl FnOnce(&mut DbConnection, &Channel),
) -> Sent {
    crate::hosted_env();
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    // A customer, not staff: mail to the team always names the ticket.
    let customer = common::insert_plain_user(&mut conn, "Customer");
    let agent = common::insert_user(&mut conn, "Agent");

    let actor = ActorContext::user(agent.uuid, None).with_workspace(WS);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        use backend::schema::{channels, user_emails};
        diesel::insert_into(user_emails::table)
            .values((
                user_emails::user_uuid.eq(customer),
                user_emails::email.eq("customer@example.com"),
                user_emails::email_type.eq("personal"),
                user_emails::is_primary.eq(true),
                user_emails::is_verified.eq(true),
            ))
            .execute(c)?;
        let channel: Channel = diesel::insert_into(channels::table)
            .values(&NewChannel {
                provider: provider.into(),
                name: "Support".into(),
                enabled: true,
                config: serde_json::json!({}),
            })
            .get_result(c)?;
        prepare(c, &channel);
        let state = backend::repository::workflow_states::default_state(c)?;
        let ticket = |c: &mut DbConnection, title: &str, origin: Option<i32>, via: Option<&str>| {
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.into(),
                    workflow_state_id: state.id,
                    requester_uuid: Some(customer),
                    origin_channel_id: origin,
                    submitted_via: via.map(str::to_string),
                    ..Default::default()
                })
                .get_result::<Ticket>(c)
        };
        let dest = ticket(c, "VPN down", None, destination_via)?;
        let source = ticket(c, "Can't connect", Some(channel.id), None)?;
        let notices = backend::repository::ticket_merge::enqueue_merge_notifications(
            c,
            &dest,
            std::slice::from_ref(&source),
            "",
        )?;
        let queued = {
            use backend::schema::outbound_emails as o;
            o::table
                .filter(o::ticket_id.eq(dest.id))
                .select((o::recipient, o::message_id, o::headers_json))
                .load(c)?
        };
        let subjects = {
            use backend::schema::outbound_emails as o;
            o::table
                .filter(o::ticket_id.eq(dest.id))
                .select(o::subject)
                .load(c)?
        };
        let recorded = {
            use backend::schema::sync_actions as s;
            s::table
                .filter(s::event_type.eq("ticket.merge_notice_sent"))
                .filter(s::aggregate_id.eq(source.id.to_string()))
                .count()
                .get_result(c)?
        };
        Ok(Sent {
            notices,
            queued,
            recorded,
            subjects,
            destination: dest.number,
        })
    })
    .expect("merge notice")
}

#[test]
fn a_merge_notice_goes_out_on_a_managed_channel() {
    let sent = merge_notice("email_managed", |_, _| {});
    assert_eq!(sent.notices, 1, "one notice for the one source");
    assert_eq!(sent.queued.len(), 1, "{:?}", sent.queued);
    let (recipient, message_id, headers) = &sent.queued[0];
    assert_eq!(recipient, "customer@example.com");
    // Threaded like the acknowledgement on the same channel: the workspace's
    // own mail host, and no Reply-To beside its From.
    assert!(message_id.ends_with(".nosdesk.test"), "{message_id}");
    assert_eq!(headers.get("Reply-To"), None);
    assert_eq!(sent.recorded, 1, "the source records that the notice went");
}

#[test]
fn a_merge_notice_goes_out_on_a_forwarding_channel() {
    let mut token = String::new();
    let sent = merge_notice("email_forward", |c, channel| {
        token = backend::repository::inbound_addresses::create_for_channel(c, channel.id)
            .expect("forwarding address")
            .token;
    });
    assert_eq!(sent.notices, 1, "one notice for the one source");
    assert_eq!(sent.queued.len(), 1, "{:?}", sent.queued);
    let (recipient, message_id, headers) = &sent.queued[0];
    assert_eq!(recipient, "customer@example.com");
    // A reply goes back to the forwarding address, as the acknowledgement's.
    assert!(
        message_id.ends_with("@inbound.nosdesk.test"),
        "{message_id}"
    );
    assert_eq!(
        headers.get("Reply-To").and_then(|v| v.as_str()),
        Some(format!("{token}@inbound.nosdesk.test").as_str())
    );
    assert_eq!(sent.recorded, 1, "the source records that the notice went");
}

/// Merged into a ticket an API token filed, or one the request form filed
/// without confirmation, the notice's subject is the destination's number
/// alone: its title is a stranger's until its requester proves the address.
#[test]
fn a_merge_into_a_strangers_ticket_leaves_its_title_out() {
    for via in ["api", "guest"] {
        let sent = merge_notice_into("email_managed", Some(via), |_, _| {});
        assert_eq!(
            sent.subjects,
            vec![format!("[#{}]", sent.destination)],
            "{via}"
        );
    }
    let sent = merge_notice_into("email_managed", Some("portal"), |_, _| {});
    assert_eq!(
        sent.subjects,
        vec![format!("[#{}] VPN down", sent.destination)]
    );
}
