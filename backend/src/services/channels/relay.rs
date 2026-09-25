//! Outbound relay gate.
//!
//! Given a newly-created comment, decide whether — and where — to relay
//! it back through the originating channel. The actual send is wired in
//! task #20 (comment-creation handler); this module only computes the
//! decision so it can be unit-tested in isolation.
//!
//! A ticket that arrived through a channel replies through it. One that
//! didn't (web form, guest portal, API) replies through the workspace's email
//! channel so the requester's answer threads back, or, with no email channel,
//! as plain mail from the default sender.
//!
//! We skip when:
//!
//! - The comment is flagged internal (`is_internal`) — tech-to-tech
//!   notes must never leak back to the requester.
//! - The comment is soft-deleted (`deleted_at`) — nothing to send.
//! - The originating channel has been disabled since the ticket was
//!   opened. Admins disable for a reason; respect it.
//! - We can't build a recipient — the requester has no primary email,
//!   or the ticket has no requester at all. We refuse to guess.
//! - The requester wrote the comment (their own reply from the portal).
//! - A ticket with no origin channel is requested by staff, who follow it
//!   in the app.

use crate::db::DbConnection;
use crate::models::{
    Channel, Comment, Ticket, CHANNEL_PROVIDER_EMAIL_FORWARD, CHANNEL_PROVIDER_EMAIL_IMAP,
    CHANNEL_PROVIDER_EMAIL_MANAGED,
};
use crate::repository::{channels as channels_repo, user_helpers};
use crate::services::channels::threading::format_outbound_subject;
use crate::services::channels::{ExternalIdentity, ThreadContext};

/// Decision returned by [`decide_relay`]. The caller either performs a
/// send for [`Self::Relay`] or logs the skip reason for metrics.
#[derive(Debug)]
pub enum RelayDecision {
    /// Send the comment out via this channel with this thread context.
    Relay {
        channel: Channel,
        thread: ThreadContext,
    },
    /// Internal note — never leaked to the requester.
    SkipInternal,
    /// Comment has been soft-deleted; don't resurrect it on the wire.
    SkipDeleted,
    /// Ticket wasn't opened through a channel and the workspace has no email
    /// channel to reply through: send as plain mail from the default sender.
    Direct { recipient: String, subject: String },
    /// The requester wrote this comment; don't mail them their own words.
    SkipAuthorIsRecipient,
    /// A ticket with no origin channel whose requester is staff.
    SkipStaffRequester,
    /// Channel is disabled (e.g. admin turned off the mailbox after
    /// the ticket was opened). Queueing for a disabled channel would
    /// silently pile up, so we drop.
    SkipChannelDisabled,
    /// Can't determine a recipient for the reply.
    SkipNoRecipient,
}

/// Compute the relay decision for a comment. `ticket` and `comment` are
/// already-loaded models so the caller can pass freshly-inserted rows
/// without re-querying.
pub fn decide_relay(
    conn: &mut DbConnection,
    ticket: &Ticket,
    comment: &Comment,
) -> Result<RelayDecision, diesel::result::Error> {
    if comment.is_internal {
        return Ok(RelayDecision::SkipInternal);
    }
    if comment.deleted_at.is_some() {
        return Ok(RelayDecision::SkipDeleted);
    }
    let channel = match ticket.origin_channel_id {
        Some(channel_id) => {
            let channel = channels_repo::find(conn, channel_id)?;
            if !channel.enabled {
                return Ok(RelayDecision::SkipChannelDisabled);
            }
            Some(channel)
        }
        None => None,
    };

    // Recipient lookup: the requester's primary email. Phase-1 email is
    // the only concrete channel, so email-only is fine here. When chat
    // adapters land they'll pre-populate the recipient via a
    // channel-specific column (e.g. slack_user_id) and this step
    // branches on `channel.provider`.
    let Some(requester_uuid) = ticket.requester_uuid else {
        return Ok(RelayDecision::SkipNoRecipient);
    };
    let Some(recipient_email) = user_helpers::get_primary_email(&requester_uuid, conn) else {
        return Ok(RelayDecision::SkipNoRecipient);
    };
    if comment.user_uuid == requester_uuid {
        return Ok(RelayDecision::SkipAuthorIsRecipient);
    }

    let subject = format_outbound_subject(ticket.id, &ticket.title);
    let channel = match channel {
        Some(channel) => channel,
        None => {
            if user_helpers::workspace_role(conn, requester_uuid).is_some_and(|r| r.is_staff()) {
                return Ok(RelayDecision::SkipStaffRequester);
            }
            match workspace_reply_channel(conn)? {
                Some(channel) => channel,
                None => {
                    return Ok(RelayDecision::Direct {
                        recipient: recipient_email,
                        subject,
                    })
                }
            }
        }
    };

    // Thread context: the latest inbound message (if any) gives us the
    // parent Message-ID to put in In-Reply-To + References. When a tech
    // is the first to speak (no prior inbound), the references chain is
    // empty and the recipient's client will thread on our Subject +
    // Message-ID alone.
    let latest_inbound = channels_repo::latest_inbound_for_ticket(conn, channel.id, ticket.id)?;
    let external_thread_id = latest_inbound.as_ref().map(|m| m.external_id.clone());
    let references = latest_inbound
        .as_ref()
        .map(|m| vec![m.external_id.clone()])
        .unwrap_or_default();

    let thread = ThreadContext {
        ticket_id: ticket.id,
        channel_id: channel.id,
        external_thread_id,
        recipient: ExternalIdentity {
            provider: channel.provider.clone(),
            external_id: recipient_email.clone(),
            display_name: recipient_email.clone(),
            known_email: Some(recipient_email),
        },
        subject: Some(subject),
        references,
    };

    Ok(RelayDecision::Relay { channel, thread })
}

/// The email channel a ticket with no origin channel replies through: the
/// first enabled one that can route a reply back, preferring the managed
/// address, then a forwarding address, then a polled mailbox.
fn workspace_reply_channel(
    conn: &mut DbConnection,
) -> Result<Option<Channel>, diesel::result::Error> {
    const PREFERENCE: [&str; 3] = [
        CHANNEL_PROVIDER_EMAIL_MANAGED,
        CHANNEL_PROVIDER_EMAIL_FORWARD,
        CHANNEL_PROVIDER_EMAIL_IMAP,
    ];
    let mut channels = channels_repo::list_enabled(conn)?;
    channels.retain(|c| PREFERENCE.contains(&c.provider.as_str()));
    channels.sort_by_key(|c| PREFERENCE.iter().position(|p| *p == c.provider));
    Ok(channels
        .into_iter()
        .find(|c| super::outbound::reply_routing(conn, c).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewChannelMessage, TicketUpdate, CHANNEL_DIRECTION_INBOUND};
    use crate::repository::{tickets as tickets_repo, user_helpers::create_user_with_email};
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    /// Seed helper: user w/ primary email, channel, ticket opened against
    /// that channel. Returns (channel_id, ticket) so tests can mutate.
    fn requester(conn: &mut DbConnection) -> crate::models::User {
        let user = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Requester".into(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        };
        let (user, _) = create_user_with_email(
            user,
            crate::models::WorkspaceRole::Member,
            "alice@example.com".into(),
            false,
            Some("guest_submission".into()),
            conn,
            None,
            crate::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
        .and_then(|o| o.into_created())
        .unwrap();
        user
    }

    fn seed(conn: &mut DbConnection) -> (Channel, Ticket) {
        let user = requester(conn);
        let channel = TestFixtures::create_channel(conn, "email_imap");
        let ticket = TestFixtures::create_ticket(conn, "Printer fire", Some(user.uuid), None);
        // Point the ticket at the channel — mimics what the pipeline does.
        let ticket = tickets_repo::update_ticket_partial(
            conn,
            ticket.id,
            TicketUpdate {
                origin_channel_id: Some(Some(channel.id)),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        (channel, ticket)
    }

    fn make_comment(ticket_id: i32, is_internal: bool) -> Comment {
        use chrono::Utc;
        Comment {
            id: 999, // unused in relay logic
            content: "body".into(),
            ticket_id,
            user_uuid: uuid::Uuid::nil(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            is_edited: false,
            edit_count: 0,
            channel_metadata: None,
            is_internal,
            deleted_at: None,
            content_format: Default::default(),
            body_text: None,
            body_html: None,
            new_content: None,
            quoted_content: None,
            raw_source_uri: None,
            workspace_id: 1,
            render_kind: None,
        }
    }

    #[test]
    fn internal_comment_is_not_relayed() {
        let mut conn = setup_test_connection();
        let (_channel, ticket) = seed(&mut conn);
        let comment = make_comment(ticket.id, true);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(matches!(decision, RelayDecision::SkipInternal));
    }

    #[test]
    fn deleted_comment_is_not_relayed() {
        let mut conn = setup_test_connection();
        let (_channel, ticket) = seed(&mut conn);
        let mut comment = make_comment(ticket.id, false);
        comment.deleted_at = Some(chrono::Utc::now().naive_utc());
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(matches!(decision, RelayDecision::SkipDeleted));
    }

    #[test]
    fn ticket_without_channel_replies_directly() {
        let mut conn = setup_test_connection();
        let user = requester(&mut conn);
        let ticket = TestFixtures::create_ticket(&mut conn, "Printer", Some(user.uuid), None);
        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        match decision {
            RelayDecision::Direct { recipient, subject } => {
                assert_eq!(recipient, "alice@example.com");
                assert_eq!(subject, format!("[#{}] Printer", ticket.id));
            }
            other => panic!("expected Direct, got {other:?}"),
        }
    }

    #[test]
    fn ticket_without_channel_replies_through_the_workspace_mailbox() {
        let mut conn = setup_test_connection();
        let user = requester(&mut conn);
        // Unroutable (no config) channels are passed over.
        TestFixtures::create_channel(&mut conn, "email_imap");
        let mailbox = channels_repo::create(
            &mut conn,
            crate::models::NewChannel {
                provider: CHANNEL_PROVIDER_EMAIL_IMAP.into(),
                name: "Support".into(),
                enabled: true,
                config: serde_json::json!({
                    "host": "imap.example.com",
                    "username": "support@example.com",
                    "reply_domain": "example.com",
                }),
            },
        )
        .unwrap();
        let ticket = TestFixtures::create_ticket(&mut conn, "Printer", Some(user.uuid), None);
        let comment = make_comment(ticket.id, false);
        match decide_relay(&mut conn, &ticket, &comment).unwrap() {
            RelayDecision::Relay { channel, thread } => {
                assert_eq!(channel.id, mailbox.id);
                assert_eq!(
                    thread.recipient.known_email.as_deref(),
                    Some("alice@example.com")
                );
            }
            other => panic!("expected Relay, got {other:?}"),
        }
    }

    #[test]
    fn staff_requester_without_channel_is_not_mailed() {
        let mut conn = setup_test_connection();
        let agent = TestFixtures::create_user(&mut conn, "A", "technician");
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(agent.uuid), None);
        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(
            matches!(
                decision,
                RelayDecision::SkipStaffRequester | RelayDecision::SkipNoRecipient
            ),
            "{decision:?}"
        );
    }

    #[test]
    fn the_requesters_own_comment_is_not_mailed_back() {
        let mut conn = setup_test_connection();
        let (_channel, ticket) = seed(&mut conn);
        let mut comment = make_comment(ticket.id, false);
        comment.user_uuid = ticket.requester_uuid.unwrap();
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(matches!(decision, RelayDecision::SkipAuthorIsRecipient));
    }

    #[test]
    fn disabled_channel_short_circuits() {
        let mut conn = setup_test_connection();
        let (channel, ticket) = seed(&mut conn);
        channels_repo::update(
            &mut conn,
            channel.id,
            crate::models::ChannelUpdate {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(matches!(decision, RelayDecision::SkipChannelDisabled));
    }

    #[test]
    fn relay_builds_thread_context_with_subject_and_references() {
        let mut conn = setup_test_connection();
        let (channel, ticket) = seed(&mut conn);

        // Prior inbound from the customer gives us a parent Message-ID.
        channels_repo::record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: channel.id,
                external_id: "<customer-msg@ex>".into(),
                direction: CHANNEL_DIRECTION_INBOUND.into(),
                ticket_id: Some(ticket.id),
                comment_id: None,
                in_reply_to: None,
                from_address: Some("alice@example.com".into()),
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();

        let (got_channel, thread) = match decision {
            RelayDecision::Relay { channel, thread } => (channel, thread),
            other => panic!("expected Relay, got {other:?}"),
        };
        assert_eq!(got_channel.id, channel.id);
        assert_eq!(thread.ticket_id, ticket.id);
        assert_eq!(
            thread.subject.as_deref(),
            Some(format!("[#{}] Printer fire", ticket.id).as_str())
        );
        assert_eq!(thread.references, vec!["<customer-msg@ex>".to_string()]);
        assert_eq!(
            thread.external_thread_id.as_deref(),
            Some("<customer-msg@ex>")
        );
        assert_eq!(
            thread.recipient.known_email.as_deref(),
            Some("alice@example.com")
        );
    }

    #[test]
    fn relay_without_prior_inbound_has_empty_references() {
        let mut conn = setup_test_connection();
        let (_channel, ticket) = seed(&mut conn);
        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        let thread = match decision {
            RelayDecision::Relay { thread, .. } => thread,
            other => panic!("expected Relay, got {other:?}"),
        };
        assert!(thread.references.is_empty());
        assert!(thread.external_thread_id.is_none());
    }

    #[test]
    fn requester_without_email_is_skipped() {
        let mut conn = setup_test_connection();
        // User created without email.
        let user = TestFixtures::create_user(&mut conn, "NoEmail", "user");
        let channel = TestFixtures::create_channel(&mut conn, "email_imap");
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let ticket = tickets_repo::update_ticket_partial(
            &mut conn,
            ticket.id,
            TicketUpdate {
                origin_channel_id: Some(Some(channel.id)),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(matches!(decision, RelayDecision::SkipNoRecipient));
    }
}
