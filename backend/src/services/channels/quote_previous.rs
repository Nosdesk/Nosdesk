//! Build the "On <date>, <name> wrote:" quoted prelude that mail
//! clients use to visually ground a reply.
//!
//! Most clients (Gmail, Outlook, Apple Mail) thread a ticket's message
//! chain via `References` / `In-Reply-To` headers — which we already
//! set — and render the prior message in a collapsible section. But
//! plenty of other clients (Mutt, console readers, older Thunderbird,
//! Outlook without conversation view) show each message in isolation.
//! Without a quoted prelude a customer replying to our tech's reply
//! would see only the tech's one-liner, disconnected from the context
//! they sent in.
//!
//! Producing the prelude in *both* HTML and plaintext form lets us
//! ship a `multipart/alternative` email where each side is internally
//! consistent: the HTML view uses `<blockquote>` (which mail clients
//! style and collapse), the plaintext view uses the conventional `> `
//! line prefix.

use chrono::{DateTime, Utc};

use super::reply_body::ReplyBody;
use crate::db::DbConnection;
use crate::models::{Channel, Comment, ContentFormat, Ticket};
use crate::repository::{
    channels as channels_repo, comments as comments_repo, users as users_repo,
};
use crate::utils::content::html_to_plaintext;

/// Pull the customer's latest inbound message for this ticket and
/// return the formatted quoted prelude in both representations, or
/// `None` if there's no prior inbound to quote.
pub fn build_for_outbound(
    conn: &mut DbConnection,
    channel: &Channel,
    ticket: &Ticket,
) -> Option<QuotedPrelude> {
    let last = channels_repo::latest_inbound_for_ticket(conn, channel.id, ticket.id).ok()??;
    // The comment body is the authoritative content; `channel_messages`
    // only carries metadata. A missing comment row shouldn't happen in
    // practice but we no-op rather than fabricate.
    let comment = comments_repo::get_comment_by_id(conn, last.comment_id?).ok()?;
    let from_email = last.from_address.as_deref().unwrap_or("customer");
    // Resolve the author's display name from the users table when we
    // can — Gmail / Apple Mail render "On <date>, Jake Ingram <abc@...>
    // wrote:" and the name anchors the quoted block visually. Email-
    // only falls back cleanly when the comment was authored by a
    // placeholder / guest user without a name.
    let display_name = users_repo::get_user_by_uuid(&comment.user_uuid, conn)
        .ok()
        .map(|u| u.name)
        .filter(|n| !n.trim().is_empty() && n != from_email);
    // The ChannelMessage row stores received_at as a NaiveDateTime in
    // server time. We render in UTC under the hood but drop the tz
    // label to match what Gmail / Apple Mail actually emit — hard-coded
    // "UTC" on a customer's screen looks robotic. Absolute time lives
    // in the `Date` header anyway.
    let received = DateTime::<Utc>::from_naive_utc_and_offset(last.received_at, Utc);
    Some(format_quote(
        &comment,
        display_name.as_deref(),
        from_email,
        received,
    ))
}

/// Both representations of the quoted prelude. Always produced as a
/// pair so the surrounding `ReplyBody` invariant — that HTML and text
/// describe the same logical message — holds without callers needing
/// to interleave conversions themselves.
pub struct QuotedPrelude {
    pub html: String,
    pub text: String,
}

fn format_quote(
    comment: &Comment,
    display_name: Option<&str>,
    from_email: &str,
    at: DateTime<Utc>,
) -> QuotedPrelude {
    // Standard "On {short date}, {Name} <{email}> wrote:" intro —
    // matches what Gmail and Apple Mail emit when a user hits Reply.
    // Day-name + short date keeps the line scan-friendly. No tz suffix;
    // see the rationale in `build_for_outbound`.
    let date_str = at.format("%a, %b %-d, %Y at %-I:%M %p").to_string();
    let attribution_text = match display_name {
        Some(name) => format!("{name} <{from_email}>"),
        None => format!("<{from_email}>"),
    };
    // Plaintext side: each line of the prior body gets the `> ` prefix.
    // Whether the source comment is HTML or plaintext, we render it to
    // plaintext first so the `> ` prefix lines up with the email
    // convention.
    let plain_body = match comment.content_format {
        ContentFormat::Html => html_to_plaintext(&comment.content),
        // Markdown isn't produced by any path that writes comments yet.
        // Treat it as plaintext (its bytes are perfectly readable as
        // text) until a renderer lands.
        ContentFormat::Plaintext | ContentFormat::Markdown => comment.content.clone(),
    };
    let mut quoted_text = format!("\n\nOn {date_str}, {attribution_text} wrote:\n");
    for line in plain_body.lines() {
        quoted_text.push_str("> ");
        quoted_text.push_str(line);
        quoted_text.push('\n');
    }

    // HTML side: the prior body lands inside a `<blockquote>` so mail
    // clients style and collapse it. If the source was already HTML we
    // embed it verbatim (it was sanitized at ingest time and we don't
    // want to re-escape the markup); plaintext gets escaped and
    // line-broken so its layout survives.
    let attribution_html = html_escape::encode_safe(&attribution_text);
    let body_html = match comment.content_format {
        ContentFormat::Html => comment.content.clone(),
        ContentFormat::Plaintext | ContentFormat::Markdown => {
            html_escape::encode_safe(&comment.content).replace('\n', "<br>\n")
        }
    };
    let quoted_html = format!(
        "<br><br>On {date_html}, {attribution_html} wrote:<br>\n<blockquote>{body_html}</blockquote>",
        date_html = html_escape::encode_safe(&date_str),
    );

    QuotedPrelude {
        html: quoted_html,
        text: quoted_text,
    }
}

/// Append the quoted prelude to `body` when there's a previous inbound
/// to quote. No-op when this is the first outbound on the ticket (no
/// prior customer message to quote).
pub fn maybe_prepend_quote(
    conn: &mut DbConnection,
    channel: &Channel,
    ticket: &Ticket,
    body: ReplyBody,
) -> ReplyBody {
    match build_for_outbound(conn, channel, ticket) {
        Some(prelude) => body.append(&prelude.html, &prelude.text),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    //! Format-only tests for the prelude. Full DB-backed coverage
    //! lives in the outbound / relay integration paths; here we just
    //! verify the shape a recipient sees.
    use super::*;
    use chrono::TimeZone;
    use uuid::Uuid;

    fn make_comment(body: &str, format: ContentFormat) -> Comment {
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
            content_format: format,
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
    fn plaintext_side_uses_arrow_prefixes_and_gmail_attribution() {
        let c = make_comment("My printer is broken.", ContentFormat::Plaintext);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, Some("Alice Example"), "alice@example.com", at);
        assert!(
            q.text.contains(
                "On Tue, Jan 2, 2024 at 10:15 AM, Alice Example <alice@example.com> wrote:"
            ),
            "got:\n{}",
            q.text
        );
        assert!(q.text.contains("> My printer is broken."));
        // Belt and braces — we actively dropped the hard-coded UTC.
        assert!(!q.text.contains("UTC"));
    }

    #[test]
    fn missing_display_name_falls_back_to_email_only() {
        let c = make_comment("Hi.", ContentFormat::Plaintext);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, None, "alice@example.com", at);
        assert!(
            q.text
                .contains("On Tue, Jan 2, 2024 at 10:15 AM, <alice@example.com> wrote:"),
            "got:\n{}",
            q.text
        );
    }

    #[test]
    fn multi_line_plaintext_prefixes_each_line() {
        let c = make_comment("line one\nline two\nline three", ContentFormat::Plaintext);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, Some("Alice"), "alice@example.com", at);
        for line in ["> line one", "> line two", "> line three"] {
            assert!(q.text.contains(line), "missing {line:?} in:\n{}", q.text);
        }
    }

    #[test]
    fn html_side_wraps_prior_body_in_blockquote() {
        let c = make_comment("My printer is broken.", ContentFormat::Plaintext);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, Some("Alice"), "alice@example.com", at);
        assert!(q.html.contains("<blockquote>"));
        assert!(q.html.contains("</blockquote>"));
        // Plaintext source got escaped + line-broken on the HTML side.
        assert!(q.html.contains("My printer is broken."));
    }

    #[test]
    fn html_attribution_escapes_recipient_email() {
        // Defence in depth: the email address comes from an external
        // header, so even though we don't currently allow `<>` in
        // addresses, the HTML side must escape what it embeds.
        let c = make_comment("body", ContentFormat::Plaintext);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, Some("Alice"), "weird<at>example.com", at);
        assert!(q.html.contains("&lt;at&gt;"), "got:\n{}", q.html);
        assert!(!q.html.contains("<at>"));
    }

    #[test]
    fn html_source_embeds_verbatim_inside_blockquote() {
        // Source was HTML — must not be re-escaped, but must live
        // inside a blockquote so mail clients style and collapse it.
        let c = make_comment("<p>printer <strong>dead</strong></p>", ContentFormat::Html);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, Some("Alice"), "alice@example.com", at);
        assert!(q
            .html
            .contains("<blockquote><p>printer <strong>dead</strong></p></blockquote>"));
        // Plaintext side rendered the HTML through html2text.
        assert!(q.text.contains("> "), "got:\n{}", q.text);
    }

    #[test]
    fn empty_body_still_produces_intro() {
        // Edge case: attachment-only inbound; quoting an empty body
        // produces only the intro line. The recipient's client renders
        // it as "On X wrote:" with nothing under it — clearer than
        // silently dropping the prelude.
        let c = make_comment("", ContentFormat::Plaintext);
        let at = Utc.with_ymd_and_hms(2024, 1, 2, 10, 15, 0).unwrap();
        let q = format_quote(&c, Some("Alice"), "alice@example.com", at);
        assert!(q.text.contains("wrote:"));
        assert!(q.html.contains("wrote:"));
    }
}
