//! Outbound relay gate.
//!
//! Given a newly-created comment, decide whether — and where — to relay
//! it back through the originating channel. The actual send is wired in
//! task #20 (comment-creation handler); this module only computes the
//! decision so it can be unit-tested in isolation.
//!
//! Relay is intentionally conservative. We skip when:
//!
//! - The comment is flagged internal (`is_internal`) — tech-to-tech
//!   notes must never leak back to the requester.
//! - The comment is soft-deleted (`deleted_at`) — nothing to send.
//! - The ticket wasn't opened through a channel (`origin_channel_id`
//!   is null) — nothing to relay to.
//! - The originating channel has been disabled since the ticket was
//!   opened. Admins disable for a reason; respect it.
//! - We can't build a recipient — the requester has no primary email,
//!   or the ticket has no requester at all. We refuse to guess.

use crate::db::DbConnection;
use crate::models::{Channel, Comment, Ticket};
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
    /// Ticket wasn't opened through a channel (web form, API import,
    /// etc.) — there's no thread to reply into.
    SkipNoChannel,
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
    let Some(channel_id) = ticket.origin_channel_id else {
        return Ok(RelayDecision::SkipNoChannel);
    };
    let channel = channels_repo::find(conn, channel_id)?;
    if !channel.enabled {
        return Ok(RelayDecision::SkipChannelDisabled);
    }

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
        subject: Some(format_outbound_subject(ticket.id, &ticket.title)),
        references,
    };

    Ok(RelayDecision::Relay { channel, thread })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NewChannelMessage, TicketUpdate, UserRole, CHANNEL_DIRECTION_INBOUND};
    use crate::repository::{tickets as tickets_repo, user_helpers::create_user_with_email};
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    /// Seed helper: user w/ primary email, channel, ticket opened against
    /// that channel. Returns (channel_id, ticket) so tests can mutate.
    fn seed(conn: &mut DbConnection) -> (Channel, Ticket) {
        let user = crate::models::NewUser {
            uuid: uuid::Uuid::now_v7(),
            name: "Requester".into(),
            role: UserRole::User,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_enabled: false,
            mfa_backup_codes: None,
        };
        let (user, _) = create_user_with_email(
            user,
            "alice@example.com".into(),
            false,
            Some("guest_submission".into()),
            conn,
            None,
        )
        .unwrap();
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
            workspace_id: None,
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
    fn ticket_without_channel_is_not_relayed() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "U", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = make_comment(ticket.id, false);
        let decision = decide_relay(&mut conn, &ticket, &comment).unwrap();
        assert!(matches!(decision, RelayDecision::SkipNoChannel));
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
        let user = TestFixtures::create_user(&mut conn, "NoEmail", UserRole::User);
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
