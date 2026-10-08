//! The merge notice reaches a customer who wrote in through a hosted
//! workspace's own address (an `email_managed` channel), as the
//! acknowledgement does, and the ticket records that it went.
//!
//! Its own binary: routing a managed channel reads the process-wide
//! `NOSDESK_TENANT_DOMAIN`, which no other test may see set.

#![allow(clippy::expect_used)]

mod common;

use diesel::prelude::*;

use backend::models::{NewChannel, NewTicket, Ticket};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

const WS: i32 = 1;

#[test]
fn a_merge_notice_goes_out_on_a_managed_channel() {
    std::env::set_var("NOSDESK_TENANT_DOMAIN", "nosdesk.test");
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let mut conn = pool.get().expect("conn");
    let customer = common::insert_user(&mut conn, "Customer");
    let agent = common::insert_user(&mut conn, "Agent");

    let actor = ActorContext::user(agent.uuid, None).with_workspace(WS);
    let queued = with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        use backend::schema::{channels, user_emails};
        diesel::insert_into(user_emails::table)
            .values((
                user_emails::user_uuid.eq(customer.uuid),
                user_emails::email.eq("customer@example.com"),
                user_emails::email_type.eq("personal"),
                user_emails::is_primary.eq(true),
                user_emails::is_verified.eq(true),
            ))
            .execute(c)?;
        let channel: backend::models::Channel = diesel::insert_into(channels::table)
            .values(&NewChannel {
                provider: "email_managed".into(),
                name: "Support".into(),
                enabled: true,
                config: serde_json::json!({}),
            })
            .get_result(c)?;
        let state = backend::repository::workflow_states::default_state(c)?;
        let ticket = |c: &mut backend::db::DbConnection, title: &str, origin: Option<i32>| {
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.into(),
                    workflow_state_id: state.id,
                    requester_uuid: Some(customer.uuid),
                    origin_channel_id: origin,
                    ..Default::default()
                })
                .get_result::<Ticket>(c)
        };
        let dest = ticket(c, "VPN down", None)?;
        let source = ticket(c, "Can't connect", Some(channel.id))?;
        let n = backend::repository::ticket_merge::enqueue_merge_notifications(
            c,
            &dest,
            std::slice::from_ref(&source),
            "",
        )?;
        let queued: Vec<(String, String, serde_json::Value)> = {
            use backend::schema::outbound_emails as o;
            o::table
                .filter(o::ticket_id.eq(dest.id))
                .select((o::recipient, o::message_id, o::headers_json))
                .load(c)?
        };
        let recorded: i64 = {
            use backend::schema::sync_actions as s;
            s::table
                .filter(s::event_type.eq("ticket.merge_notice_sent"))
                .filter(s::aggregate_id.eq(source.id.to_string()))
                .count()
                .get_result(c)?
        };
        Ok((n, queued, recorded))
    })
    .expect("merge notice");

    let (n, queued, recorded) = queued;
    assert_eq!(n, 1, "one notice for the one source");
    assert_eq!(queued.len(), 1, "{queued:?}");
    assert_eq!(queued[0].0, "customer@example.com");
    // Threaded like the acknowledgement on the same channel: the workspace's
    // own mail host, and no Reply-To beside its From.
    assert!(queued[0].1.ends_with(".nosdesk.test"), "{}", queued[0].1);
    assert_eq!(queued[0].2.get("Reply-To"), None);
    assert_eq!(recorded, 1, "the source records that the notice went");
}
