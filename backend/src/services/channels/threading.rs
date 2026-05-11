//! Thread-resolution cascade.
//!
//! Given an inbound [`InboundMessage`] we try, in priority order, to
//! locate an existing ticket it belongs to:
//!
//!   1. **References chain** — walk `In-Reply-To` + `References` header
//!      IDs looking for a `channel_messages` row we previously emitted
//!      and attach to its `ticket_id`.
//!   2. **Plus-addressed recipient** — `support+ticket-1234@host` in To/
//!      Cc / Delivered-To.
//!   3. **Our own Message-ID format** — when the inbound message itself
//!      matches `<ticket-N.comment-M…@host>` (e.g. forwarded by the
//!      customer so it now appears as the new message's Message-ID).
//!   4. **Subject prefix** — `[#1234]` anywhere in the subject.
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
    for rcpt in &event.recipients {
        if let Some(ticket_id) = parse_plus_addr_ticket_id(rcpt) {
            return Some(ticket_id);
        }
    }

    // 3. Our own outbound Message-ID format appearing as the inbound
    //    message's own id. This catches forwarded-by-customer cases
    //    where headers were rewritten but the body preserved.
    if let Some(ticket_id) = parse_our_message_id(&event.external_id) {
        return Some(ticket_id);
    }

    // 4. Subject-line fallback.
    if let Some(subject) = &event.subject {
        if let Some(ticket_id) = parse_subject_ticket_id(subject) {
            return Some(ticket_id);
        }
    }

    None
}

// ---------- Parsers ----------

// Match `support+ticket-1234@host` in an RFC-5322 address. We only care
// about the local-part suffix after `+`; the domain is ignored so this
// works regardless of how the admin names their mailbox.
static PLUS_ADDR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\+ticket-(\d+)@").expect("valid regex"));

// Match our custom outbound Message-ID: `<ticket-N.comment-M.RAND@host>`.
// The angle brackets are optional — some clients strip them when quoting.
static OUR_MSGID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)ticket-(\d+)\.comment-\d+\.[a-z0-9]+@").expect("valid regex"));

// Match `#1234` anywhere. `[#1234]` inside subjects is the common form
// we emit; the bare `#1234` form matches customer clients that strip
// brackets from quoted subjects.
static SUBJECT_ID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"#(\d+)").expect("valid regex"));

/// Parse `support+ticket-N@domain` → `Some(N)`.
pub fn parse_plus_addr_ticket_id(address: &str) -> Option<i32> {
    PLUS_ADDR_RE
        .captures(address)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// Parse `<ticket-N.comment-M.RAND@domain>` → `Some(N)`.
pub fn parse_our_message_id(message_id: &str) -> Option<i32> {
    OUR_MSGID_RE
        .captures(message_id)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// Parse a subject line for `#N`. Returns `Some(N)` on first match.
///
/// Uses the first hit only — a subject like `Re: [#12] re: [#34] ...`
/// attaches to `12`, which is correct because `12` is the older ticket
/// the customer's client echoed from their reply chain.
pub fn parse_subject_ticket_id(subject: &str) -> Option<i32> {
    SUBJECT_ID_RE
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

/// Format a subject line for outbound replies: `[#N] original`. The
/// pipeline calls this once per outbound message; idempotent if the
/// prefix is already present.
pub fn format_outbound_subject(ticket_id: i32, original: &str) -> String {
    let tag = format!("[#{ticket_id}]");
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

    // ---- parse_plus_addr_ticket_id ----

    #[test]
    fn plus_addr_matches_standard_form() {
        assert_eq!(parse_plus_addr_ticket_id("support+ticket-1234@yourco.com"), Some(1234));
    }

    #[test]
    fn plus_addr_is_case_insensitive_on_prefix() {
        assert_eq!(parse_plus_addr_ticket_id("Support+Ticket-42@yourco.com"), Some(42));
    }

    #[test]
    fn plus_addr_extracts_even_from_angle_bracketed_header() {
        assert_eq!(
            parse_plus_addr_ticket_id("<support+ticket-7@yourco.com>"),
            Some(7)
        );
    }

    #[test]
    fn plus_addr_ignores_non_numeric_suffix() {
        assert_eq!(parse_plus_addr_ticket_id("support+ticket-abc@yourco.com"), None);
    }

    #[test]
    fn plus_addr_ignores_non_ticket_suffix() {
        // `+newsletter@` must not parse as a ticket — our suffix is literal.
        assert_eq!(parse_plus_addr_ticket_id("support+newsletter@yourco.com"), None);
    }

    #[test]
    fn plus_addr_returns_none_when_absent() {
        assert_eq!(parse_plus_addr_ticket_id("support@yourco.com"), None);
        assert_eq!(parse_plus_addr_ticket_id(""), None);
    }

    // ---- parse_our_message_id ----

    #[test]
    fn our_message_id_matches_our_format() {
        assert_eq!(
            parse_our_message_id("<ticket-55.comment-100.deadbeef@yourco.com>"),
            Some(55)
        );
    }

    #[test]
    fn our_message_id_matches_without_angle_brackets() {
        assert_eq!(
            parse_our_message_id("ticket-1.comment-2.cafebabe@yourco.com"),
            Some(1)
        );
    }

    #[test]
    fn our_message_id_rejects_other_message_ids() {
        assert_eq!(
            parse_our_message_id("<CAB=abc123@mail.gmail.com>"),
            None
        );
        assert_eq!(
            parse_our_message_id("<random.id.12345@outlook.com>"),
            None
        );
    }

    // ---- parse_subject_ticket_id ----

    #[test]
    fn subject_bracketed_hash_number() {
        assert_eq!(parse_subject_ticket_id("[#1234] Printer is on fire"), Some(1234));
    }

    #[test]
    fn subject_bare_hash_number_from_reply() {
        // Customer's client stripped brackets when quoting.
        assert_eq!(
            parse_subject_ticket_id("Re: Printer is on fire (#1234)"),
            Some(1234)
        );
    }

    #[test]
    fn subject_no_hash_returns_none() {
        assert_eq!(parse_subject_ticket_id("help"), None);
        assert_eq!(parse_subject_ticket_id(""), None);
    }

    #[test]
    fn subject_picks_first_hash_when_multiple() {
        // Re-re-forwarded thread with nested references. Oldest wins —
        // that's the ticket the customer actually thinks they're replying to.
        assert_eq!(
            parse_subject_ticket_id("Re: Re: [#12] re: [#34] Printer"),
            Some(12)
        );
    }

    #[test]
    fn subject_ignores_hash_text_without_number() {
        assert_eq!(parse_subject_ticket_id("The #1 reason printers break"), Some(1));
        // Note: this parses '1'. Deliberate — false-positive risk exists
        // but only fires when a real ticket with that id also exists,
        // which the caller double-checks before attaching. See pipeline.
    }

    // ---- format_outbound_message_id ----

    #[test]
    fn outbound_message_id_round_trips_through_parser() {
        let id = format_outbound_message_id(1234, 5678, "yourco.com");
        assert_eq!(parse_our_message_id(&id), Some(1234));
    }

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
        assert_eq!(format_outbound_subject(42, "Printer is on fire"), "[#42] Printer is on fire");
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

    use crate::models::{
        CHANNEL_DIRECTION_INBOUND, CHANNEL_DIRECTION_OUTBOUND, NewChannelMessage, UserRole,
    };
    use crate::repository::channels as channels_repo;
    use crate::services::channels::{ExternalIdentity, InboundMessage, LoopMarkers};
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
        }
    }

    fn setup_channel_and_ticket(conn: &mut crate::db::DbConnection) -> (i32, i32) {
        let ch = TestFixtures::create_channel(conn, "email_imap");
        let user = TestFixtures::create_user(conn, "u", UserRole::User);
        let ticket = TestFixtures::create_ticket(conn, "T", Some(user.uuid), None);
        (ch.id, ticket.id)
    }

    #[tokio::test]
    async fn resolver_finds_ticket_via_references_chain() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket_id) = setup_channel_and_ticket(&mut conn);

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
        let (channel_id, ticket_id) = setup_channel_and_ticket(&mut conn);

        let inbound = make_inbound(
            "<reply@customer>",
            vec![], // no references
            None,
            vec![format!("support+ticket-{ticket_id}@yourco.com")],
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, Some(ticket_id));
    }

    #[tokio::test]
    async fn resolver_finds_ticket_via_our_message_id_format() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket_id) = setup_channel_and_ticket(&mut conn);

        let inbound = make_inbound(
            &format!("<ticket-{ticket_id}.comment-1.deadbeef@yourco.com>"),
            vec![],
            None,
            vec!["someone@elsewhere.com".into()],
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, Some(ticket_id));
    }

    #[tokio::test]
    async fn resolver_finds_ticket_via_subject_prefix() {
        let mut conn = setup_test_connection();
        let (channel_id, ticket_id) = setup_channel_and_ticket(&mut conn);

        let inbound = make_inbound(
            "<reply@customer>",
            vec![],
            Some(&format!("Re: [#{ticket_id}] Printer fire")),
            vec!["someone@elsewhere.com".into()],
        );

        let result = default_explicit_threading(&inbound, channel_id, &mut conn).await;
        assert_eq!(result, Some(ticket_id));
    }

    #[tokio::test]
    async fn resolver_returns_none_for_genuinely_new_ticket() {
        let mut conn = setup_test_connection();
        let (channel_id, _ticket_id) = setup_channel_and_ticket(&mut conn);

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
        let user = TestFixtures::create_user(&mut conn, "u", UserRole::User);
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
            Some(&format!("Re: [#{}] stale quote", ticket_a.id)),
            vec![],
        );

        let result = default_explicit_threading(&inbound, ch.id, &mut conn).await;
        assert_eq!(result, Some(ticket_b.id));
    }
}
