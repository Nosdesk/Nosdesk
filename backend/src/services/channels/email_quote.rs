//! Quoted-reply splitter for inbound email comments.
//!
//! Email replies routinely arrive with the entire prior thread
//! inlined below the new content. Rendering the full body verbatim
//! makes the ticket detail view balloon with every reply, so we
//! extract just the new content at ingest time and store the
//! quoted remainder separately. The renderer shows the new content
//! by default and offers a "Show trimmed content" affordance for
//! the quoted block.
//!
//! Algorithm shape borrowed from Discourse's `email_reply_trimmer`
//! and GitHub's `email_reply_parser`: scan the body top-down,
//! pick the earliest boundary where the quoted thread begins, and
//! split there. Boundaries we recognise on the plaintext path:
//!
//!   - Single-line "On ... wrote:" headers, including localised
//!     forms (French, German, Spanish, Dutch).
//!   - Wrapped multi-line versions where the header runs across 2
//!     to 4 lines (Gmail's 80-character wrap).
//!   - Forward delimiters reused from
//!     [`super::forward_parser`] (`----- Original Message -----`
//!     and friends).
//!   - Outlook desktop's header block (`From:` / `Sent:` or
//!     `Date:` / `To:` within four lines).
//!   - A run of `>`-prefixed lines that begins at the start of a
//!     paragraph.
//!
//! On the HTML path we split at the earliest substring match for
//! the well-known quote-container markers used by Gmail, Outlook,
//! Apple Mail, and recent Outlook web. This is a heuristic, not a
//! full DOM walker, and Pass 2 of the email-rendering plan will
//! upgrade it to a proper html5ever-backed splitter when the
//! sanitiser substrate goes in. For now it covers the documented
//! cases without pulling in a new parser dependency.
//!
//! What this module deliberately does *not* do (yet):
//!
//!   - Signature detection / stripping. Out of scope for the
//!     activity-log noise fix that Pass 1 addresses; Pass 2 may
//!     split signatures into their own column once the renderer
//!     is ready to use them.
//!   - Inline-quoted reply preservation. If a customer typed `> `
//!     prefixed lines as an inline rebuttal in the middle of new
//!     content, we'll cut at the first run and lump everything
//!     below into `quoted_content`. The raw source remains
//!     available via "Show original message" as the escape hatch.

use once_cell::sync::Lazy;
use regex::Regex;

/// Result of splitting an inbound email body into the new reply
/// and the quoted prior thread. `quoted_content` is `None` when no
/// boundary was detected — common for first-touch emails and for
/// short messages that don't carry quoted history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSplit {
    pub new_content: String,
    pub quoted_content: Option<String>,
}

/// Single-line "On ... wrote:" headers in English plus the four
/// localised forms that show up most often in our captured mail.
/// Anchored case-insensitively at line start. The 1-400 character
/// cap on the middle avoids runaway matching when "wrote:" appears
/// elsewhere in the body.
///
/// `[ \t]*` rather than `\s*` for the leading and trailing
/// whitespace classes — `\s` includes newlines, which would cause
/// `m.start()` to slide back past the previous line break and
/// produce a boundary offset that's off-by-one when callers split
/// the body. We want the match to start at the first non-newline
/// column of the header line.
static QUOTE_HEADER_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?im)^[ \t]*(?:",
        r"On\b.{1,400}\bwrote\s*:",
        r"|Le\b.{1,400}\ba\s+écrit\s*:",
        r"|Am\b.{1,400}\bschrieb\s+.{1,200}:",
        r"|El\b.{1,400}\bescribió\s*:",
        r"|Op\b.{1,400}\bschreef\s+.{1,200}:",
        r")[ \t]*$",
    ))
    .expect("valid quote-header regex")
});

/// `From:` line detector for the Outlook desktop header block.
/// Anchored at line start (`(?im)`) with horizontal-only leading
/// whitespace to avoid matching `From: ` that appears inside the
/// body of a forwarded narrative.
static OUTLOOK_FROM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?im)^[ \t]*From:[ \t]*\S").expect("valid Outlook From regex"));

/// Companion-header detector for the Outlook block. Any of
/// `Sent:` / `Date:` / `To:` / `Subject:` / `Cc:` at line start.
/// Used alongside `OUTLOOK_FROM_RE` to confirm we're looking at a
/// structured header block rather than the word "From:" in prose.
static OUTLOOK_COMPANION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?im)^[ \t]*(?:Sent|Date|To|Subject|Cc):[ \t]")
        .expect("valid Outlook companion regex")
});

/// Forward / "Original Message" delimiters. Mirrors the regex in
/// `forward_parser` so both modules cut at the same line shapes;
/// the difference is the destination, `forward_parser` extracts a
/// new requester, this module cuts the body at the same line and
/// drops everything below into `quoted_content`.
static FORWARD_DELIM_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?im)^[ \t]*(?:",
        r"-{3,}[ \t]*(?:Forwarded\s+message|Original\s+Message)[ \t]*-{3,}",
        r"|Begin\s+forwarded\s+message\s*:",
        r"|_{10,}",
        r")[ \t]*$",
    ))
    .expect("valid forward delimiter regex")
});

/// HTML quote-container markers. Substring search: we look for
/// these literal strings in the source HTML and treat the earliest
/// match as the start of the quoted thread. The strings are
/// distinctive enough to be safe as substring lookups (none appear
/// in normal email body content).
const HTML_QUOTE_MARKERS: &[&str] = &[
    // Gmail wraps replies in `<div class="gmail_quote">`.
    "class=\"gmail_quote\"",
    "class='gmail_quote'",
    // Apple Mail / classic webmail: blockquote with type="cite".
    "<blockquote type=\"cite\"",
    "<blockquote type='cite'",
    // Outlook desktop reply: `<div id="divRplyFwdMsg">`.
    "id=\"divRplyFwdMsg\"",
    "id='divRplyFwdMsg'",
    // Outlook web: a hidden marker div that precedes the quote.
    "id=\"appendonsend\"",
    "id='appendonsend'",
    // Outlook (Word-styled HTML) wraps the original message in a
    // `WordSection1`-classed div.
    "class=\"WordSection1\"",
    "class='WordSection1'",
];

/// Split a plaintext email body into new and quoted parts.
pub fn split_plaintext(body: &str) -> QuoteSplit {
    let trimmed = body.trim_end_matches(['\r', '\n']);

    if let Some(boundary) = find_plaintext_boundary(trimmed) {
        let new_part = trimmed[..boundary].trim_end_matches(['\r', '\n', ' ', '\t']);
        let quoted_part = &trimmed[boundary..];
        QuoteSplit {
            new_content: new_part.to_string(),
            quoted_content: if quoted_part.trim().is_empty() {
                None
            } else {
                Some(quoted_part.to_string())
            },
        }
    } else {
        QuoteSplit {
            new_content: trimmed.to_string(),
            quoted_content: None,
        }
    }
}

/// Find the byte offset of the first quote boundary in a plaintext
/// body, or `None` if the body is all new content. Returns the
/// position of the first character of the boundary line so the
/// caller can split with `body[..pos]` / `body[pos..]`.
fn find_plaintext_boundary(body: &str) -> Option<usize> {
    let mut candidates: Vec<usize> = Vec::new();

    if let Some(m) = QUOTE_HEADER_LINE_RE.find(body) {
        candidates.push(m.start());
    }

    if let Some(pos) = find_wrapped_quote_header(body) {
        candidates.push(pos);
    }

    if let Some(m) = FORWARD_DELIM_RE.find(body) {
        candidates.push(m.start());
    }

    if let Some(pos) = find_outlook_header_block(body) {
        candidates.push(pos);
    }

    if let Some(pos) = find_quoted_run(body) {
        candidates.push(pos);
    }

    candidates.into_iter().min()
}

/// Look for "On <something> wrote:" header that's wrapped across
/// 2-4 lines because the mail client hard-wrapped at 78-80
/// characters. Returns the start byte of the first line.
///
/// Case-insensitive matching is per-line via `eq_ignore_ascii_case`
/// on the leading verb and `to_ascii_lowercase` on the small scan
/// window. We don't lowercase the whole body — long threads can be
/// hundreds of KB, and the wrap regex is the cheapest pass.
/// `eq_ignore_ascii_case` is ASCII-only, which is fine here because
/// every supported opener verb ("on", "le", "am", "el", "op") is
/// ASCII even when the body contains UTF-8 elsewhere.
fn find_wrapped_quote_header(body: &str) -> Option<usize> {
    // (opener_ascii_lower, closer_ascii_lower) pairs. Openers must
    // match the first word of the header; closers are substrings
    // looked up inside the scan window.
    static OPENERS: &[(&str, &str)] = &[
        ("on ", "wrote:"),
        ("le ", "a écrit :"),
        ("am ", "schrieb"),
        ("el ", "escribió"),
        ("op ", "schreef"),
    ];

    let line_starts = line_start_indices(body);

    for (i, &start) in line_starts.iter().enumerate() {
        let line_end = line_starts.get(i + 1).copied().unwrap_or(body.len());
        let line = body[start..line_end].trim_start();

        for (opener, closer) in OPENERS.iter() {
            // `get(..opener.len())` returns `None` when the line is
            // shorter than the opener OR the slice would land
            // mid-UTF-8-codepoint (e.g. an é at the start of the
            // line). Both cases mean "this isn't our opener," so
            // fall through to the next opener.
            let Some(prefix) = line.get(..opener.len()) else {
                continue;
            };
            if prefix.eq_ignore_ascii_case(opener) {
                // Scan window covers the opener line plus up to 3
                // wrapped continuations. Lowercase only this small
                // window for the closer substring check; closer
                // text may include non-ASCII chars ("écrit"), and
                // `contains` does a byte-substring search, so a
                // proper lowercase is needed for case-insensitive
                // contains.
                let scan_end = line_starts
                    .get(i + 4)
                    .copied()
                    .unwrap_or(body.len());
                let window_lower = body[start..scan_end].to_lowercase();
                if window_lower.contains(closer) {
                    return Some(start);
                }
            }
        }
    }

    None
}

/// Detect the Outlook desktop header block: a `From:` line at
/// column 0 followed within four lines by at least two of `Sent`,
/// `Date`, `To`, `Subject`, `Cc`. The companion-count gate keeps
/// us from falsely cutting on the word "From:" used as a literal
/// in prose.
fn find_outlook_header_block(body: &str) -> Option<usize> {
    let line_starts = line_start_indices(body);

    for from_match in OUTLOOK_FROM_RE.find_iter(body) {
        let from_start = from_match.start();
        // Find the line index containing this match.
        let line_idx = match line_starts.binary_search(&from_start) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let scan_end = line_starts
            .get(line_idx + 5)
            .copied()
            .unwrap_or(body.len());
        let window = &body[from_start..scan_end];
        let companions = OUTLOOK_COMPANION_RE.find_iter(window).count();
        if companions >= 2 {
            return Some(line_starts[line_idx]);
        }
    }

    None
}

/// Detect a run of `>`-prefixed quoted lines. Returns the start of
/// the first such line, but only if the run is at the start of a
/// paragraph (preceded by a blank line or the beginning of the
/// body) and at least two lines deep. The depth gate avoids false
/// positives on programmer email where `> ` is used as a shell
/// prompt in a single example line.
fn find_quoted_run(body: &str) -> Option<usize> {
    let line_starts = line_start_indices(body);

    for (i, &start) in line_starts.iter().enumerate() {
        let end = line_starts.get(i + 1).copied().unwrap_or(body.len());
        let line = &body[start..end].trim_end_matches(['\r', '\n']);

        if !line.starts_with('>') {
            continue;
        }

        // Paragraph-start gate: this line must be at the start of
        // the body, or the previous line must be blank.
        if i > 0 {
            let prev_start = line_starts[i - 1];
            let prev_end = start;
            let prev_line =
                body[prev_start..prev_end].trim_end_matches(['\r', '\n']);
            if !prev_line.trim().is_empty() {
                continue;
            }
        }

        // Depth gate: at least two consecutive quoted lines.
        let next_start = line_starts.get(i + 1).copied().unwrap_or(body.len());
        let next_end = line_starts
            .get(i + 2)
            .copied()
            .unwrap_or(body.len());
        if next_end > next_start {
            let next_line = body[next_start..next_end]
                .trim_end_matches(['\r', '\n']);
            if next_line.starts_with('>') {
                return Some(start);
            }
        }
    }

    None
}

/// Indices into `body` where each line starts. Includes byte 0 if
/// the body is non-empty. Used by the boundary scanners to
/// navigate by line without re-splitting on every check.
fn line_start_indices(body: &str) -> Vec<usize> {
    let mut starts = Vec::with_capacity(body.len() / 40);
    if !body.is_empty() {
        starts.push(0);
    }
    for (idx, b) in body.bytes().enumerate() {
        if b == b'\n' && idx + 1 < body.len() {
            starts.push(idx + 1);
        }
    }
    starts
}

/// Split an HTML email body at the earliest known quote container.
///
/// Substring search rather than DOM-aware: the markers are
/// distinctive (`class="gmail_quote"`, etc.) and don't appear in
/// natural email body text. When a match is found, we walk
/// backwards to the opening `<` of that tag so the new / quoted
/// split lands on a tag boundary, not in the middle of an
/// attribute. Pass 2 of the email-rendering plan will upgrade this
/// to a proper html5ever walker when the ammonia sanitiser goes
/// in.
pub fn split_html(html: &str) -> QuoteSplit {
    let trimmed = html.trim_end_matches(['\r', '\n', ' ', '\t']);

    let mut earliest: Option<usize> = None;
    for marker in HTML_QUOTE_MARKERS {
        if let Some(pos) = trimmed.find(marker) {
            // If the marker itself starts with `<` (e.g.
            // `<blockquote type="cite"`), `pos` already points at
            // the opening tag. Otherwise it points inside an
            // attribute and we need to walk back to the opening
            // `<` of the containing tag.
            let tag_start = if marker.starts_with('<') {
                pos
            } else {
                trimmed[..pos].rfind('<').unwrap_or(pos)
            };
            earliest = Some(match earliest {
                Some(existing) if existing < tag_start => existing,
                _ => tag_start,
            });
        }
    }

    match earliest {
        Some(boundary) => {
            let new_part = trimmed[..boundary]
                .trim_end_matches(['\r', '\n', ' ', '\t'])
                .to_string();
            let quoted_part = trimmed[boundary..].to_string();
            QuoteSplit {
                new_content: new_part,
                quoted_content: if quoted_part.trim().is_empty() {
                    None
                } else {
                    Some(quoted_part)
                },
            }
        }
        None => QuoteSplit {
            new_content: trimmed.to_string(),
            quoted_content: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_split(input: &str, expected_new: &str, expected_quoted: Option<&str>) {
        let split = split_plaintext(input);
        assert_eq!(split.new_content, expected_new, "new_content mismatch");
        assert_eq!(
            split.quoted_content.as_deref(),
            expected_quoted,
            "quoted_content mismatch"
        );
    }

    #[test]
    fn gmail_single_line_on_wrote() {
        let body = "Yes please, that works for me.\n\
                    \n\
                    On Tue, May 13, 2026 at 3:42 PM Kyle <kyle@example.com> wrote:\n\
                    > Can you confirm by Friday?\n\
                    > Thanks";
        assert_split(
            body,
            "Yes please, that works for me.",
            Some(
                "On Tue, May 13, 2026 at 3:42 PM Kyle <kyle@example.com> wrote:\n\
                 > Can you confirm by Friday?\n\
                 > Thanks",
            ),
        );
    }

    #[test]
    fn apple_mail_comma_variant() {
        // Apple Mail's header has a comma between the date and time:
        // "On 13 May 2026, at 15:42, Kyle <a@b> wrote:". GitHub's
        // original regex misses this; ours catches it because the
        // regex doesn't pin "wrote:" to the same word boundary.
        let body = "Got it, will do.\n\
                    \n\
                    On 13 May 2026, at 15:42, Kyle <kyle@example.com> wrote:\n\
                    Anything I can do to help?";
        assert_split(
            body,
            "Got it, will do.",
            Some(
                "On 13 May 2026, at 15:42, Kyle <kyle@example.com> wrote:\n\
                 Anything I can do to help?",
            ),
        );
    }

    #[test]
    fn gmail_eighty_char_wrap() {
        // Gmail hard-wraps the "On ... wrote:" header at 80 chars.
        // The closer "wrote:" lands on a different line from "On ".
        let body = "Sounds good.\n\
                    \n\
                    On Tue, May 13, 2026 at 3:42 PM Kyle Phillips\n\
                    <kyle.phillips@verylongdomain.example.com> wrote:\n\
                    Original message body here.";
        assert_split(
            body,
            "Sounds good.",
            Some(
                "On Tue, May 13, 2026 at 3:42 PM Kyle Phillips\n\
                 <kyle.phillips@verylongdomain.example.com> wrote:\n\
                 Original message body here.",
            ),
        );
    }

    #[test]
    fn outlook_header_block() {
        let body = "Acknowledged.\n\
                    \n\
                    From: Kyle Phillips <kyle@example.com>\n\
                    Sent: Tuesday, May 13, 2026 3:42 PM\n\
                    To: Support <support@example.com>\n\
                    Subject: Re: Tickets list slow\n\
                    \n\
                    Original message body.";
        assert_split(
            body,
            "Acknowledged.",
            Some(
                "From: Kyle Phillips <kyle@example.com>\n\
                 Sent: Tuesday, May 13, 2026 3:42 PM\n\
                 To: Support <support@example.com>\n\
                 Subject: Re: Tickets list slow\n\
                 \n\
                 Original message body.",
            ),
        );
    }

    #[test]
    fn outlook_header_block_with_date_instead_of_sent() {
        let body = "Thanks.\n\
                    \n\
                    From: Kyle <kyle@example.com>\n\
                    Date: 13 May 2026 15:42\n\
                    To: Support <support@example.com>\n\
                    \n\
                    Original.";
        assert_split(
            body,
            "Thanks.",
            Some(
                "From: Kyle <kyle@example.com>\n\
                 Date: 13 May 2026 15:42\n\
                 To: Support <support@example.com>\n\
                 \n\
                 Original.",
            ),
        );
    }

    #[test]
    fn quoted_run_at_paragraph_start() {
        let body = "Sure, that works.\n\
                    \n\
                    > Can you confirm by Friday?\n\
                    > Thanks!";
        assert_split(
            body,
            "Sure, that works.",
            Some("> Can you confirm by Friday?\n> Thanks!"),
        );
    }

    #[test]
    fn single_quoted_line_is_not_a_boundary() {
        // Programmer emails: "I ran `> foo` and got..." should not
        // cut at the example line. The depth gate (>= 2 consecutive)
        // protects us.
        let body = "I tried the command, then ran:\n\
                    \n\
                    > example.sh --verbose\n\
                    \n\
                    and the output looked off.";
        assert_split(body, body.trim_end(), None);
    }

    #[test]
    fn no_quote_returns_full_body_as_new() {
        let body = "Hi there,\n\nFirst-touch email, nothing to strip.";
        assert_split(body, body.trim_end(), None);
    }

    #[test]
    fn forward_delimiter_is_a_boundary() {
        let body = "Forwarding to support:\n\
                    \n\
                    ---------- Forwarded message ----------\n\
                    From: Kyle <k@x.com>\n\
                    Original body";
        assert_split(
            body,
            "Forwarding to support:",
            Some(
                "---------- Forwarded message ----------\n\
                 From: Kyle <k@x.com>\n\
                 Original body",
            ),
        );
    }

    #[test]
    fn localised_french() {
        let body = "Reçu, merci.\n\
                    \n\
                    Le mar. 13 mai 2026 à 15:42, Kyle <k@x.com> a écrit :\n\
                    Message original.";
        assert_split(
            body,
            "Reçu, merci.",
            Some(
                "Le mar. 13 mai 2026 à 15:42, Kyle <k@x.com> a écrit :\n\
                 Message original.",
            ),
        );
    }

    #[test]
    fn html_gmail_quote() {
        let html = "<div>Yes please.</div>\
                    <div class=\"gmail_quote\">\
                    <blockquote>Prior thread here</blockquote>\
                    </div>";
        let split = split_html(html);
        assert_eq!(split.new_content, "<div>Yes please.</div>");
        assert!(split.quoted_content.unwrap().contains("gmail_quote"));
    }

    #[test]
    fn html_apple_blockquote_cite() {
        let html = "<p>Got it.</p>\
                    <blockquote type=\"cite\">Original message</blockquote>";
        let split = split_html(html);
        assert_eq!(split.new_content, "<p>Got it.</p>");
        assert!(
            split
                .quoted_content
                .as_deref()
                .unwrap()
                .starts_with("<blockquote type=\"cite\"")
        );
    }

    #[test]
    fn html_outlook_div_rply_fwd_msg() {
        let html = "<p>Acknowledged.</p>\
                    <div id=\"divRplyFwdMsg\" dir=\"ltr\">\
                    <font face=\"Calibri\">Original message body</font>\
                    </div>";
        let split = split_html(html);
        assert_eq!(split.new_content, "<p>Acknowledged.</p>");
        assert!(split.quoted_content.is_some());
    }

    #[test]
    fn html_no_marker_returns_full_body() {
        let html = "<p>First-touch email.</p><p>No quote here.</p>";
        let split = split_html(html);
        assert_eq!(split.new_content, html);
        assert_eq!(split.quoted_content, None);
    }

    #[test]
    fn html_earliest_marker_wins() {
        // If both Gmail and Outlook markers appear, split at the
        // earlier one.
        let html = "<p>Reply.</p>\
                    <div class=\"gmail_quote\">A</div>\
                    <div id=\"appendonsend\"></div>\
                    <div class=\"WordSection1\">B</div>";
        let split = split_html(html);
        assert_eq!(split.new_content, "<p>Reply.</p>");
        assert!(
            split
                .quoted_content
                .as_deref()
                .unwrap()
                .starts_with("<div class=\"gmail_quote\"")
        );
    }
}
