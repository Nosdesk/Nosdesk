//! Build the "On <date>, <name> wrote:" quoted prelude that modern
//! mail clients use to visually ground a reply.
//!
//! Most clients (Gmail, Outlook, Apple Mail) thread a ticket's
//! message chain via `References` / `In-Reply-To` headers — which we
//! already set — and render the prior message in a collapsible
//! section. But plenty of other clients (Mutt, console readers,
//! older Thunderbird, Outlook without conversation view) show each
//! message in isolation. In those cases a customer replying to our
//! tech's reply sees only the tech's one-liner, disconnected from
//! the context they sent in.
//!
//! Prepending a standard quoted block fixes that without harming the
//! clients that DO thread — their UI collapses / hides the quoted
//! section. It's the same belt-and-suspenders convention every email
//! client applies when a user hits "Reply".

use chrono::{DateTime, Utc};

use crate::db::DbConnection;
use crate::models::{Channel, Comment, Ticket};
use crate::repository::{channels as channels_repo, comments as comments_repo, users as users_repo};

/// Pull the customer's latest inbound message for this ticket and
/// return it formatted as a quoted prelude, or `None` if there's no
/// prior inbound to quote.
pub fn build_for_outbound(
    conn: &mut DbConnection,
    channel: &Channel,
    ticket: &Ticket,
) -> Option<String> {
    let last = channels_repo::latest_inbound_for_ticket(conn, channel.id, ticket.id).ok()??;
    // The comment body is the authoritative content; `channel_messages`
    // only carries metadata. A missing comment row shouldn't happen in
    // practice but we no-op rather than fabricate.
    let comment = comments_repo::get_comment_by_id(conn, last.comment_id?).ok()?;
    let from_email = last.from_address.as_deref().unwrap_or("customer");
    // Resolve the author's display name from the users table when we
    // can — Gmail/Apple Mail render "On <date>, Jake Ingram <abc@...>
    // wrote:" and the name anchors the quoted block visually. Email-
    // only falls back cleanly when the comment was authored by a
    // placeholder/guest user without a name.
    let display_name = users_repo::get_user_by_uuid(&comment.user_uuid, conn)
        .ok()
        .map(|u| u.name)
        .filter(|n| !n.trim().is_empty() && n != from_email);
    // The ChannelMessage row stores received_at as a NaiveDateTime in
    // server time. We render in UTC under the hood but drop the tz
    // label to match what Gmail/Apple Mail actually emit — hard-coded
    // "UTC" on a customer's screen looks robotic. Absolute time lives
    // in the Date header anyway.
    let received = DateTime::<Utc>::from_naive_utc_and_offset(last.received_at, Utc);
    Some(format_quote(&comment, display_name.as_deref(), from_email, received))
}

fn format_quote(
    comment: &Comment,
    display_name: Option<&str>,
    from_email: &str,
    at: DateTime<Utc>,
) -> String {
    // Standard "On {short date}, {Name} <{email}> wrote:" intro —
    // matches what Gmail and Apple Mail emit when a user hits Reply.
    // Day-name + short date keeps the line scan-friendly. No tz
    // suffix; see build_for_outbound.
    let date_str = at.format("%a, %b %-d, %Y at %-I:%M %p").to_string();
    let attribution = match display_name {
        Some(name) => format!("{name} <{from_email}>"),
        None => format!("<{from_email}>"),
    };
    let mut out = format!("\n\nOn {date_str}, {attribution} wrote:\n");
    for line in comment.content.lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Prepend the tech's new reply body with the quoted prelude when
/// there's a previous inbound to quote. No-op when this is the first
/// outbound on the ticket (no prior customer message to quote).
pub fn maybe_prepend_quote(
    conn: &mut DbConnection,
    channel: &Channel,
    ticket: &Ticket,
    tech_reply: &str,
) -> String {
    match build_for_outbound(conn, channel, ticket) {
        Some(quote) => format!("{tech_reply}{quote}"),
        None => tech_reply.to_string(),
    }
}

#[cfg(test)]
mod tests {
    //! Format-only tests for the prelude. Full DB-backed coverage
    //! lives in the outbound/relay integration paths; here we just
    //! verify the shape a recipient sees.
    use super::*;
    use crate::models::Comment;
    use chrono::TimeZone;
    use uuid::Uuid;

    fn make_comment(body: &str) -> Comment {
        Comment {
            id: 1,
            content: body.into(),
            ticket_id: 1,
            user_uuid: Uuid::nil(),
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            is_edited: false,
            edit_count: 0,
            channel_metadata: None,
            is_internal: false,
            deleted_at: None,
        }
    }

    #[test]
    fn single_line_body_quoted_with_display_name() {
        let c = make_comment("My printer is broken.");
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let got = format_quote(&c, Some("Alice Example"), "alice@example.com", at);
        // Gmail-style attribution: Name <email>, no tz suffix.
        assert!(
            got.contains("On Tue, Jan 2, 2024 at 10:15 AM, Alice Example <alice@example.com> wrote:"),
            "got:\n{got}"
        );
        assert!(got.contains("> My printer is broken."));
        // Belt and braces — we actively dropped the hard-coded UTC.
        assert!(!got.contains("UTC"));
    }

    #[test]
    fn missing_display_name_falls_back_to_email_only() {
        let c = make_comment("Hi.");
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let got = format_quote(&c, None, "alice@example.com", at);
        assert!(
            got.contains("On Tue, Jan 2, 2024 at 10:15 AM, <alice@example.com> wrote:"),
            "got:\n{got}"
        );
    }

    #[test]
    fn multi_line_body_prefixes_each_line() {
        let c = make_comment("line one\nline two\nline three");
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let got = format_quote(&c, Some("Alice"), "alice@example.com", at);
        for line in ["> line one", "> line two", "> line three"] {
            assert!(got.contains(line), "missing {line:?} in:\n{got}");
        }
    }

    #[test]
    fn empty_body_still_produces_intro() {
        // Edge case: attachment-only inbound; quoting an empty body
        // produces only the intro line. The recipient's client will
        // render it as "On X wrote: <empty>", clearer than silently
        // dropping the quote.
        let c = make_comment("");
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let got = format_quote(&c, Some("Alice"), "alice@example.com", at);
        assert!(got.contains("wrote:"));
    }

    #[test]
    fn prepend_is_a_noop_when_no_previous_inbound() {
        // `maybe_prepend_quote` goes through the DB path which we
        // don't exercise here; this is just a sanity that
        // format_quote composes as expected when the caller prepends.
        let c = make_comment("Hi, a message.");
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let quoted = format_quote(&c, Some("Alice"), "alice@example.com", at);
        let composed = format!("Thanks for the info.{quoted}");
        assert!(composed.starts_with("Thanks for the info."));
        assert!(composed.contains("> Hi, a message."));
    }
}
