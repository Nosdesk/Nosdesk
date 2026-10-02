//! Thread-resolution cascade.
//!
//! Given an inbound [`InboundMessage`] we try, in priority order, to
//! locate an existing ticket it belongs to:
//!
//!   1. **References chain** — walk `In-Reply-To` + `References` header
//!      IDs looking for a `channel_messages` row we previously emitted
//!      and attach to its `ticket_id`.
//!   2. **Plus-addressed recipient** — `support+ticket-12@host` in To/
//!      Cc / Delivered-To, naming the ticket's number.
//!   3. **Subject tag** — `[#12]` anywhere in the subject, the ticket's
//!      number as our outbound subjects carry it.
//!
//! Steps 2 and 3 name a ticket by a number anyone can type, so they only
//! match a ticket in the channel's workspace that the sender is already on
//! (requester, watcher or staff); otherwise the cascade moves on.
//!
//! A message whose own Message-ID is one we sent never gets here: the
//! pipeline drops it as a duplicate of the recorded outbound row.
//!
//! If none hit, the pipeline treats the message as a new ticket.
//!
//! The cascade is deliberately channel-agnostic: Slack and Discord
//! populate `references` with their thread_ts and `subject` with None,
//! so they naturally fall through to the explicit-reference match via
//! step 1. Adapters for channels with different semantics (e.g.
//! WhatsApp's fuzzy sender-window) override `ChannelAdapter::resolve_thread`.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::db::DbConnection;
use crate::repository::channels as channels_repo;
use crate::repository::tickets as tickets_repo;
use crate::services::channels::InboundMessage;

/// Default resolver used by the `ChannelAdapter::resolve_thread` trait
/// method. See module docs for the cascade order.
pub async fn default_explicit_threading(
    event: &InboundMessage,
    channel_id: i32,
    conn: &mut DbConnection,
) -> Option<i32> {
    // 1. References / In-Reply-To chain.
    if !event.references.is_empty() {
        if let Ok(Some(ticket_id)) =
            channels_repo::find_ticket_by_reference_chain(conn, channel_id, &event.references)
        {
            return Some(ticket_id);
        }
    }

    // 2. Plus-addressed recipient. Pick the first match across all
    //    recipient addresses (To / Cc / Delivered-To).
    if let Some(number) = event
        .recipients
        .iter()
        .find_map(|rcpt| parse_plus_addr_ticket_number(rcpt))
    {
        if let Some(ticket_id) = senders_ticket(event, channel_id, number, conn) {
            return Some(ticket_id);
        }
    }

    // 3. Subject tag.
    if let Some(number) = event
        .subject
        .as_deref()
        .and_then(parse_subject_ticket_number)
    {
        if let Some(ticket_id) = senders_ticket(event, channel_id, number, conn) {
            return Some(ticket_id);
        }
    }

    None
}

/// The id of the ticket numbered `number` in the channel's workspace, when
/// the message's sender is on it. The sender resolves as the pipeline's
/// identity step resolves them.
fn senders_ticket(
    event: &InboundMessage,
    channel_id: i32,
    number: i32,
    conn: &mut DbConnection,
) -> Option<i32> {
    let sender = event.from.known_email.as_deref()?;
    let workspace_id = channels_repo::find(conn, channel_id).ok()?.workspace_id;
    let ticket_id = tickets_repo::id_for_number(conn, workspace_id, number).ok()??;
    let user =
        crate::repository::user_helpers::find_verified_user_by_email(sender, conn).ok()??;
    tickets_repo::is_on_ticket(conn, ticket_id, user.uuid)
        .ok()?
        .then_some(ticket_id)
}

// ---------- Parsers ----------

// Match `support+ticket-1234@host` in an RFC-5322 address. We only care
// about the local-part suffix after `+`; the domain is ignored so this
// works regardless of how the admin names their mailbox.
static PLUS_ADDR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\+ticket-(\d+)@").expect("valid regex"));

// Match the `[#12]` tag our outbound subjects carry. A bare `#12` isn't
// matched: subjects quote order and invoice numbers that way.
static SUBJECT_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[#(\d+)\]").expect("valid regex"));

/// Parse `support+ticket-N@domain` → `Some(N)`, a ticket number.
pub fn parse_plus_addr_ticket_number(address: &str) -> Option<i32> {
    PLUS_ADDR_RE
        .captures(address)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// Parse a subject line for the `[#N]` tag → `Some(N)`, a ticket number.
///
/// Uses the first hit only — a subject like `Re: [#12] re: [#34] ...`
/// attaches to `12`, which is correct because `12` is the older ticket
/// the customer's client echoed from their reply chain.
pub fn parse_subject_ticket_number(subject: &str) -> Option<i32> {
    SUBJECT_TAG_RE
        .captures(subject)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// Format a `Message-ID` for an outbound email. Stored by
/// `channel_messages` so future inbound replies can match back via the
/// References chain.
///
/// Returned string does NOT include the `<…>` angle brackets; the caller
/// (lettre) adds them when writing the header.
pub fn format_outbound_message_id(ticket_id: i32, comment_id: i32, domain: &str) -> String {
    // A short random suffix keeps the ID unique even if the same
    // (ticket, comment) pair is retried.
    let random: u32 = rand::random();
    format!("ticket-{ticket_id}.comment-{comment_id}.{random:08x}@{domain}")
}

/// Format a subject line for outbound replies: `[#N] original`, N the
/// ticket's number. The pipeline calls this once per outbound message;
/// idempotent if the prefix is already present.
pub fn format_outbound_subject(ticket_number: i32, original: &str) -> String {
    let tag = format!("[#{ticket_number}]");
    let trimmed = original.trim();
    if trimmed.contains(&tag) {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        tag
    } else {
        format!("{tag} {trimmed}")
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_plus_addr_ticket_number ----

    #[test]
    fn plus_addr_matches_standard_form() {
        assert_eq!(
            parse_plus_addr_ticket_number("support+ticket-1234@yourco.com"),
            Some(1234)
        );
    }

    #[test]
    fn plus_addr_is_case_insensitive_on_prefix() {
        assert_eq!(
            parse_plus_addr_ticket_number("Support+Ticket-42@yourco.com"),
            Some(42)
        );
    }

    #[test]
    fn plus_addr_extracts_even_from_angle_bracketed_header() {
        assert_eq!(
            parse_plus_addr_ticket_number("<support+ticket-7@yourco.com>"),
            Some(7)
        );
    }

    #[test]
    fn plus_addr_ignores_non_numeric_suffix() {
        assert_eq!(
            parse_plus_addr_ticket_number("support+ticket-abc@yourco.com"),
            None
        );
    }

    #[test]
    fn plus_addr_ignores_non_ticket_suffix() {
        // `+newsletter@` must not parse as a ticket — our suffix is literal.
        assert_eq!(
            parse_plus_addr_ticket_number("support+newsletter@yourco.com"),
            None
        );
    }

    #[test]
    fn plus_addr_returns_none_when_absent() {
        assert_eq!(parse_plus_addr_ticket_number("support@yourco.com"), None);
        assert_eq!(parse_plus_addr_ticket_number(""), None);
    }

    // ---- parse_subject_ticket_number ----

    #[test]
    fn subject_bracketed_hash_number() {
        assert_eq!(
            parse_subject_ticket_number("[#1234] Printer is on fire"),
            Some(1234)
        );
    }

    #[test]
    fn subject_bare_hash_number_is_not_a_tag() {
        assert_eq!(
            parse_subject_ticket_number("Re: Printer is on fire (#1234)"),
            None
        );
        assert_eq!(
            parse_subject_ticket_number("The #1 reason printers break"),
            None
        );
    }

    #[test]
    fn subject_no_hash_returns_none() {
        assert_eq!(parse_subject_ticket_number("help"), None);
        assert_eq!(parse_subject_ticket_number(""), None);
    }

    #[test]
    fn subject_picks_first_hash_when_multiple() {
        // Re-re-forwarded thread with nested references. Oldest wins —
        // that's the ticket the customer actually thinks they're replying to.
        assert_eq!(
            parse_subject_ticket_number("Re: Re: [#12] re: [#34] Printer"),
            Some(12)
        );
    }

    // ---- format_outbound_message_id ----

    #[test]
    fn outbound_message_id_has_unique_random_suffix() {
        let a = format_outbound_message_id(1, 1, "x.com");
        let b = format_outbound_message_id(1, 1, "x.com");
        // Collisions would require two u32::rand() calls returning the same
        // value. Vanishingly unlikely; asserting not-equal guards against
        // accidentally hard-coding the suffix later.
        assert_ne!(a, b);
    }

    // ---- format_outbound_subject ----

    #[test]
    fn outbound_subject_prepends_tag() {
        assert_eq!(
            format_outbound_subject(42, "Printer is on fire"),
            "[#42] Printer is on fire"
        );
    }

    #[test]
    fn outbound_subject_is_idempotent_when_tag_present() {
        assert_eq!(
            format_outbound_subject(42, "[#42] Printer is on fire"),
            "[#42] Printer is on fire"
        );
    }

    #[test]
    fn outbound_subject_handles_empty_original() {
        assert_eq!(format_outbound_subject(42, ""), "[#42]");
        assert_eq!(format_outbound_subject(42, "   "), "[#42]");
    }

    // ---- default_explicit_threading integration ----
    //
    // End-to-end cascade tests. Each one sets up the minimum fixtures
    // (user + ticket + channel + maybe a prior message) and verifies
    // the resolver picks the right step.

    use crate::models::{NewChannelMessage, CHANNEL_DIRECTION_INBOUND, CHANNEL_DIRECTION_OUTBOUND};
    use crate::repository::channels as channels_repo;
    use crate::services::channels::{ExternalIdentity, InboundMessage, LoopMarkers, SenderAuth};
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use chrono::Utc;
    use serde_json::json;
    // CHANNEL_DIRECTION_INBOUND is unused in this file today but kept
    // imported so future tests (pipeline-edge cases, authored-by-tech
    // replay) have the constant in scope.
    const _: &str = CHANNEL_DIRECTION_INBOUND;

    fn make_inbound(
        external_id: &str,
        references: Vec<String>,
        subject: Option<&str>,
        recipients: Vec<String>,
    ) -> InboundMessage {
        InboundMessage {
            external_id: external_id.to_string(),
            from: ExternalIdentity {
                provider: "email_imap".into(),
                external_id: "alice@example.com".into(),
                display_name: "Alice".into(),
                known_email: Some("alice@example.com".into()),
            },
            subject: subject.map(|s| s.to_string()),
            body_text: "hi".into(),
            body_html: None,
            attachments: vec![],
            references,
            received_at: Utc::now(),
            loop_markers: LoopMarkers::default(),
            raw_metadata: json!({}),
            recipients,
            is_bounce: false,
            bounce_reports: Vec::new(),
            raw_bytes: None,
            content_language: None,
            source_ref: None,
            spam_suspected: false,
            sender_auth: SenderAuth::Unknown,
        }
    }

    /// A channel, a ticket, and the address its requester sends from.
    fn setup_channel_and_ticket(
        conn: &mut crate::db::DbConnection,
    ) -> (i32, crate::models::Ticket, String) {
        let ch = TestFixtures::create_channel(conn, "email_imap");
        let user = TestFixtures::create_user(conn, "u", "user");
        let ticket = TestFixtures::create_ticket(conn, "T", Some(user.uuid), None);
        let ticket = TestFixtures::renumber_ticket(conn, ticket);
        (ch.id, ticket, email_for(conn, &user))
    }

    /// A fresh address of `user`'s.
    fn email_for(conn: &mut crate::db::DbConnection, user: &crate::models::User) -> String {
        let email = format!("{}@example.com", user.uuid);
        TestFixtures::create_user_email(conn, user.uuid, &email, true);
        email
    }

    fn sent_by(mut msg: InboundMessage, email: &str) -> InboundMessage {
        msg.from.external_id = email.to_string();
        msg.from.known_email = Some(email.to_string());
        msg
    }

    #[tokio::test]
    async fn resolver_finds_ticket_via_references_chain() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket, _) = setup_channel_and_ticket(&mut conn);
        let ticket_id = ticket.id;

        // Prior outbound we emitted: `<parent@host>`.
        channels_repo::record_message(
            &mut conn,
            NewChannelMessage {
                channel_id,
                external_id: "<parent@host>".into(),
                direction: CHANNEL_DIRECTION_OUTBOUND.into(),
                ticket_id: Some(ticket_id),
                comment_id: None,
                in_reply_to: None,
                from_address: None,
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        let inbound = make_inbound(
            "<reply@customer>",
            vec!["<parent@host>".into()],
            Some("Re: something"),
            vec!["support@yourco.com".into()],
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, Some(ticket_id));
    }

    #[tokio::test]
    async fn resolver_finds_ticket_via_plus_addressed_recipient() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket, requester) = setup_channel_and_ticket(&mut conn);

        let inbound = sent_by(
            make_inbound(
                "<reply@customer>",
                vec![], // no references
                None,
                vec![format!("support+ticket-{}@yourco.com", ticket.number)],
            ),
            &requester,
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, Some(ticket.id));
    }

    #[tokio::test]
    async fn resolver_ignores_a_ticket_id_in_the_messages_own_message_id() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket, _) = setup_channel_and_ticket(&mut conn);

        let inbound = make_inbound(
            &format!("<ticket-{}.comment-1.deadbeef@yourco.com>", ticket.id),
            vec![],
            None,
            vec!["someone@elsewhere.com".into()],
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn resolver_finds_ticket_via_subject_tag() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket, requester) = setup_channel_and_ticket(&mut conn);
        let staff = TestFixtures::create_user(&mut conn, "agent", "technician");
        let staff = email_for(&mut conn, &staff);

        for sender in [&requester, &staff] {
            let inbound = sent_by(
                make_inbound(
                    "<reply@customer>",
                    vec![],
                    Some(&format!("Re: [#{}] Printer fire", ticket.number)),
                    vec!["someone@elsewhere.com".into()],
                ),
                sender,
            );
            let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
            assert_eq!(result, Some(ticket.id));
        }
    }

    #[tokio::test]
    async fn resolver_ignores_a_number_from_someone_not_on_the_ticket() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket, _) = setup_channel_and_ticket(&mut conn);
        let stranger = TestFixtures::create_user(&mut conn, "stranger", "user");
        let stranger = email_for(&mut conn, &stranger);

        let inbound = sent_by(
            make_inbound(
                "<new@stranger>",
                vec![],
                Some(&format!("[#{}] Hello", ticket.number)),
                vec![format!("support+ticket-{}@yourco.com", ticket.number)],
            ),
            &stranger,
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn resolver_ignores_a_bare_number_in_the_subject() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket, requester) = setup_channel_and_ticket(&mut conn);

        let inbound = sent_by(
            make_inbound(
                "<new@customer>",
                vec![],
                Some(&format!("Order #{} hasn't arrived", ticket.number)),
                vec!["support@yourco.com".into()],
            ),
            &requester,
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn resolver_returns_none_for_genuinely_new_ticket() {
        let mut conn = setup_test_connection();
        let (channel_id, _, _) = setup_channel_and_ticket(&mut conn);

        let inbound = make_inbound(
            "<totally-new@customer>",
            vec![],
            Some("Hi I need help"),
            vec!["support@yourco.com".into()],
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn resolver_prefers_references_over_subject_when_both_present() {
        // Customer's client kept the [#1] subject but also threaded
        // via References pointing at a different ticket's outbound.
        // References should win — it's the authoritative signal.
        let mut conn = setup_test_connection();
        let ch = TestFixtures::create_channel(&mut conn, "email_imap");
        let user = TestFixtures::create_user(&mut conn, "u", "user");
        let ticket_a = TestFixtures::create_ticket(&mut conn, "A", Some(user.uuid), None);
        let ticket_b = TestFixtures::create_ticket(&mut conn, "B", Some(user.uuid), None);

        channels_repo::record_message(
            &mut conn,
            NewChannelMessage {
                channel_id: ch.id,
                external_id: "<out-for-b@host>".into(),
                direction: CHANNEL_DIRECTION_OUTBOUND.into(),
                ticket_id: Some(ticket_b.id),
                comment_id: None,
                in_reply_to: None,
                from_address: None,
                author_user_uuid: None,
                raw_metadata: None,
            },
        )
        .unwrap();

        let inbound = make_inbound(
            "<reply@customer>",
            vec!["<out-for-b@host>".into()],
            Some(&format!("Re: [#{}] stale quote", ticket_a.number)),
            vec![],
        );

        let result = default_explicit_threading(&inbound, ch.id, &mut conn).await;
        assert_eq!(result, Some(ticket_b.id));
    }
}
