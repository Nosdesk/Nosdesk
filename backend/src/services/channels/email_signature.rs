//! Inbound signature stripping (B5), plaintext path.
//!
//! Runs AFTER quote-splitting ([`super::email_quote`]) on the reply-only text, to
//! trim the sender's signature/footer so it doesn't land in the ticket comment.
//! A port of Mailgun talon's bruteforce heuristic (no ML): a signature lives in
//! the last few lines and is recognised by a small set of high-confidence
//! signals, applied in confidence order. The raw body is always retained
//! (`Comment.body_text` + `raw_source_uri`), so a mis-strip is recoverable.
//!
//! Bias: UNDER-strip. A leftover signature is cosmetic; a cut sentence loses real
//! content. Each rule below is anchored and bounded to the trailing window so an
//! in-body "Thanks for the help, here's what I found:" is never mistaken for a
//! sign-off.

use once_cell::sync::Lazy;
use regex::Regex;

/// Talon's bound: only the last N non-empty lines are signature candidates.
const SIGNATURE_MAX_LINES: usize = 11;
/// Talon's bound: lines longer than this are unlikely to be signature lines, so
/// a trailing block containing one is treated as content, not a signature.
const TOO_LONG_SIGNATURE_LINE: usize = 60;

/// Result of [`strip_plaintext`]: the content with any signature removed, and
/// the removed signature (kept so callers can stash it in the collapsed region
/// rather than discard it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureSplit {
    pub content: String,
    pub signature: Option<String>,
}

/// A `-- ` / `--` / `__` delimiter line (RFC 3676 §4.3). Clients often strip the
/// trailing space, so match dashes-only too.
static DELIMITER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(-{2,}|_{2,})\s*$").unwrap());

/// Mobile / webmail footers. Always trailing and never followed by real content.
static MOBILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^\s*(sent from my |get outlook for |sent from mailbox|enviado desde mi |envoyé de mon |von meinem .* gesendet)",
    )
    .unwrap()
});

/// A line that is ONLY a closing word (anchored), optionally with trailing
/// punctuation. The anchor is what stops "Thanks for the help:" from matching.
static CLOSING_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^\s*(thanks|thank you|thanks again|many thanks|regards|kind regards|best regards|warm regards|best wishes|all the best|best|cheers|sincerely|cordially|talk soon)[\s,.!-]*$",
    )
    .unwrap()
});

/// Strip a trailing signature from already-quote-stripped plaintext.
pub fn strip_plaintext(body: &str) -> SignatureSplit {
    let lines: Vec<&str> = body.split('\n').collect();
    match signature_start(&lines) {
        Some(i) => SignatureSplit {
            content: lines[..i].join("\n").trim_end().to_string(),
            signature: Some(lines[i..].join("\n").trim().to_string()),
        },
        None => SignatureSplit {
            content: body.to_string(),
            signature: None,
        },
    }
}

/// The earliest line index that begins the signature, or `None`. Constrained to
/// the trailing candidate window and never the first line of the message.
fn signature_start(lines: &[&str]) -> Option<usize> {
    if lines.len() < 2 {
        return None;
    }
    let from = window_start(lines);

    // Rule 1: explicit delimiter (highest confidence). Earliest in-window wins;
    // everything after a `-- ` is signature by convention even if it's long.
    if let Some(i) = (from..lines.len()).find(|&i| DELIMITER_RE.is_match(lines[i])) {
        return Some(i);
    }
    // Rule 2: a mobile footer line; cut from it to the end.
    if let Some(i) = (from..lines.len()).find(|&i| MOBILE_RE.is_match(lines[i])) {
        return Some(i);
    }
    // Rule 3: a closing-word line, but only when everything after it to the end
    // is signature-ish (short / blank) — the over-stripping guard.
    if let Some(i) = (from..lines.len())
        .find(|&i| CLOSING_RE.is_match(lines[i]) && trailing_is_signatureish(&lines[i + 1..]))
    {
        return Some(i);
    }
    None
}

/// Smallest line index inside the last [`SIGNATURE_MAX_LINES`] non-empty lines,
/// clamped to `>= 1` (a signature never starts on the body's first line).
fn window_start(lines: &[&str]) -> usize {
    let mut non_empty = 0;
    let mut start = lines.len();
    for i in (0..lines.len()).rev() {
        if !lines[i].trim().is_empty() {
            non_empty += 1;
            start = i;
            if non_empty == SIGNATURE_MAX_LINES {
                break;
            }
        }
    }
    start.max(1)
}

/// True when every non-empty line in `rest` is short enough to be a contact /
/// sign-off line. A long line means real content follows, so don't cut.
fn trailing_is_signatureish(rest: &[&str]) -> bool {
    rest.iter()
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.trim().chars().count() <= TOO_LONG_SIGNATURE_LINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stripped(body: &str) -> String {
        strip_plaintext(body).content
    }

    #[test]
    fn cuts_at_dash_delimiter() {
        let body = "Yes, that works for me.\n\n-- \nJohn Smith\nAcme Corp\n555-1234";
        assert_eq!(stripped(body), "Yes, that works for me.");
    }

    #[test]
    fn cuts_mobile_footer() {
        let body = "Looks good, ship it.\n\nSent from my iPhone";
        assert_eq!(stripped(body), "Looks good, ship it.");
    }

    #[test]
    fn cuts_closing_word_with_contact_block() {
        let body = "Please reset my password.\n\nThanks,\nJane Doe\nIT Manager\njane@acme.com";
        assert_eq!(stripped(body), "Please reset my password.");
    }

    #[test]
    fn keeps_closing_word_when_real_content_follows() {
        // "Thanks," is not a sign-off here: a long content line follows it.
        let body = "Thanks,\nHere is the detailed reproduction you asked for: it fails when the queue drains.";
        assert_eq!(stripped(body), body);
    }

    #[test]
    fn no_signature_returns_body_unchanged() {
        let body = "Just a plain reply with no sign-off at all.";
        assert_eq!(stripped(body), body);
        assert_eq!(strip_plaintext(body).signature, None);
    }

    #[test]
    fn does_not_cut_on_the_first_line() {
        // A one-line body that happens to look like a closing word stays intact;
        // a signature never starts on the body's first line.
        assert_eq!(stripped("Thanks"), "Thanks");
    }

    #[test]
    fn delimiter_outside_window_is_ignored() {
        // A `--` early in a long body (a markdown rule, say) isn't a signature
        // because it's not within the trailing candidate window.
        let mut lines = vec!["--".to_string()];
        for i in 0..20 {
            lines.push(format!("real content line {i}"));
        }
        let body = lines.join("\n");
        assert_eq!(stripped(&body), body);
    }

    #[test]
    fn captures_the_signature_text() {
        let split = strip_plaintext("Done.\n\n-- \nJohn");
        assert_eq!(split.content, "Done.");
        assert_eq!(split.signature.as_deref(), Some("-- \nJohn"));
    }
}
