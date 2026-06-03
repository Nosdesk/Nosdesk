//! RFC 3464 delivery-status notification parser.
//!
//! Once `email_imap::detect_bounce` flags an inbound as a DSN
//! (J Pass 2.1), this module pulls the structured payload out of
//! the MIME tree so the pipeline can link the bounce back to its
//! originating `outbound_emails` row.
//!
//! ## DSN structure (per RFC 3464)
//!
//! ```text
//! Content-Type: multipart/report; report-type=delivery-status
//! ├── text/plain                  human-readable explanation
//! ├── message/delivery-status     RFC 3464 structured fields
//! └── message/rfc822[-headers]    original message (or headers)
//! ```
//!
//! The `message/rfc822` part carries the original message's
//! `Message-ID`, which is our key into `outbound_emails`. The
//! `message/delivery-status` part has per-recipient fields with
//! `Final-Recipient` (failed address) and `Diagnostic-Code` (the
//! upstream MTA's reason).
//!
//! ## Robustness
//!
//! Real-world DSNs are sloppy: some servers omit the `message/
//! rfc822` part, others wrap the diagnostic across multiple lines,
//! and a few miss the structured part entirely and only send a
//! human-readable failure notice. This parser does *best effort*:
//!
//! - If there's no embedded original message-id we return `None`
//!   and the pipeline still records the message as a bounce skip
//!   (J Pass 2.1's behaviour) but can't link to the outbound row.
//! - Recipient and diagnostic are both `Option` so a partial DSN
//!   still produces a usable report.

use mailparse::ParsedMail;

/// Structured bounce information extracted from a DSN.
///
/// `original_message_id` is the lookup key into `outbound_emails`;
/// the other fields are best-effort diagnostic detail. Per RFC 3464
/// we keep `status_code` and `diagnostic` separate rather than
/// collapsing them into one string: `status_code` is the canonical
/// classification signal (5.x.y enhanced-status), `diagnostic` is
/// the human-readable reason from the upstream MTA. A line wrap on
/// `Diagnostic-Code` is unfolded into a single logical value before
/// reaching this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BounceReport {
    /// `Message-ID` header from the embedded `message/rfc822` part,
    /// stripped of `<>` brackets. Matches `outbound_emails.message_id`.
    pub original_message_id: String,
    /// The address the remote MTA actually rejected. May differ
    /// from the original recipient if a list expanded.
    pub recipient: Option<String>,
    /// RFC 3463 enhanced status (`x.y.z`), e.g. "5.1.1" / "4.2.2".
    /// The structured classification signal — when present, the
    /// classifier prefers it over scanning the free-text diagnostic.
    pub status_code: Option<String>,
    /// Human-readable upstream reason text from `Diagnostic-Code`,
    /// with the type-prefix (`smtp;`) stripped. Surface in the
    /// admin UI so techs can see *why* the send failed.
    pub diagnostic: Option<String>,
}

impl BounceReport {
    /// Classify the bounce as hard (permanent recipient failure,
    /// auto-suppress) vs not. Prefers the structured `status_code`
    /// because RFC 3464 makes it the canonical classification
    /// signal; falls back to scanning the diagnostic text only
    /// when Status is absent (sloppy DSNs from legacy MTAs).
    ///
    /// See `is_hard_bounce` for the underlying logic + carve-outs
    /// (5.0.0 "undefined" and 5.7.x policy / SPF / DKIM, both of
    /// which look 5xx but should not auto-suppress).
    pub fn is_hard(&self) -> bool {
        is_hard_bounce(self.status_code.as_deref(), self.diagnostic.as_deref())
    }
}

/// Classify a bounce as hard (permanent recipient failure) vs not.
///
/// Prefers `status` (the RFC 3463 enhanced status code from the
/// DSN's `Status:` field) when present — that's the canonical
/// per-spec signal. Falls back to scanning `diagnostic` (the
/// `Diagnostic-Code:` text) only when `status` is absent.
///
/// Carve-outs even on 5.x.y:
///   - `5.0.0` is "other / undefined" per RFC 3463 — too vague to
///     act on without risking a real customer.
///   - `5.7.x` is policy / security (SPF / DKIM / DMARC / content
///     filtering / greylisting). These are almost always sender-
///     side problems; suppressing the recipient just because we
///     failed DKIM loses them forever for a configuration issue
///     we own.
///
/// Default-deny: when neither signal can be classified, return
/// false. Better to retry a real address than to permanently block
/// a real customer over a parsing miss.
pub fn is_hard_bounce(status: Option<&str>, diagnostic: Option<&str>) -> bool {
    // Authoritative classification: structured Status code.
    if let Some(code) = status {
        return classify_enhanced(code.trim());
    }
    // Fallback: scan the diagnostic text for 5xx-shaped tokens.
    // Same carve-out rules as the structured path.
    if let Some(diag) = diagnostic {
        return scan_5xx(diag);
    }
    false
}

/// Hard-bounce check for a structured `x.y.z` enhanced status.
///
/// Auto-suppress on the narrow set of RFC 3463 categories that
/// reliably indicate the *recipient* is the problem:
///
///   - `5.1.x` Addressing Status — bad mailbox address (§3.2)
///   - `5.2.x` Mailbox Status — mailbox disabled / full / not
///     accepting (§3.3)
///   - `5.4.7` Delivery time expired (§3.5) — the receiving MTA
///     gave up after exhausting its retry window; this specific
///     code is the only `5.4.x` that's recipient-terminal
///
/// Everything else in the `5.x.y` space — even though RFC 3463
/// classifies it as permanent — is either sender-side (`5.5.x`
/// protocol violations, `5.6.x` content rejection, `5.7.x` policy
/// / SPF / DKIM) or receiver-system-recoverable (`5.3.x` system
/// problems, the rest of `5.4.x` routing). Auto-suppressing on
/// those would lose customers over our own bugs or transient
/// receiver-side infrastructure issues.
///
/// Matches the conservative end of SES / Postmark default behaviour.
/// Admins can always add a suppression manually for an edge case
/// the classifier misses.
fn classify_enhanced(code: &str) -> bool {
    if !code.starts_with("5.") || code.matches('.').count() != 2 {
        return false;
    }
    code.starts_with("5.1.") || code.starts_with("5.2.") || code == "5.4.7"
}

/// Diagnostic-text fallback: scan tokens for any 5xx-shaped pattern,
/// applying the same carve-outs as `classify_enhanced`. Enhanced
/// status takes precedence over basic SMTP code when both appear.
fn scan_5xx(diagnostic: &str) -> bool {
    let mut saw_hard_enhanced = false;
    let mut saw_carve_out_enhanced = false;
    let mut saw_basic_5xx = false;

    for word in diagnostic.split(|c: char| !c.is_ascii_alphanumeric() && c != '.') {
        if word.starts_with("5.") && word.matches('.').count() == 2 {
            if word == "5.0.0" || word.starts_with("5.7.") {
                saw_carve_out_enhanced = true;
            } else {
                saw_hard_enhanced = true;
            }
            continue;
        }
        if word.len() == 3 && word.starts_with('5') && word.chars().all(|c| c.is_ascii_digit()) {
            saw_basic_5xx = true;
        }
    }

    if saw_carve_out_enhanced && !saw_hard_enhanced {
        return false;
    }
    saw_hard_enhanced || saw_basic_5xx
}

/// Parse a DSN message into one or more structured bounce reports.
///
/// RFC 3464 §2.1: a DSN can carry "one or more" per-recipient field
/// groups. Exchange / Postfix / Sendmail all emit a single DSN with
/// multiple recipient blocks when several addresses in a fan-out
/// failed. We return one `BounceReport` per block so the pipeline
/// can suppress each independently.
///
/// Returns an empty `Vec` when the embedded original message-id
/// can't be recovered (no `message/rfc822` part, or that part has
/// no `Message-ID` header). The caller still treats the inbound as
/// a bounce skip — just without the outbound-row linkage.
pub fn parse_bounce(root: &ParsedMail) -> Vec<BounceReport> {
    let mut original_message_id: Option<String> = None;
    let mut per_recipient: Vec<DeliveryStatus> = Vec::new();

    walk(root, &mut |part| {
        let ctype = part.ctype.mimetype.to_ascii_lowercase();
        match ctype.as_str() {
            "message/rfc822" | "message/rfc822-headers" if original_message_id.is_none() => {
                if let Ok(body) = part.get_body() {
                    original_message_id = extract_message_id(&body);
                }
            }
            "message/delivery-status" => {
                if let Ok(body) = part.get_body() {
                    per_recipient.extend(parse_delivery_status_blocks(&body));
                }
            }
            _ => {}
        }
    });

    let Some(mid) = original_message_id else {
        return Vec::new();
    };
    per_recipient
        .into_iter()
        .map(|ds| BounceReport {
            original_message_id: mid.clone(),
            recipient: ds.recipient,
            status_code: ds.status_code,
            diagnostic: ds.diagnostic,
        })
        .collect()
}

/// Depth-first walk of a `ParsedMail` tree, invoking `visit` on
/// every node (including the root). Used here so the caller can
/// pluck specific subparts without flattening the tree first.
fn walk<'a>(part: &'a ParsedMail<'a>, visit: &mut dyn FnMut(&'a ParsedMail<'a>)) {
    visit(part);
    for child in &part.subparts {
        walk(child, visit);
    }
}

/// Pull the first `Message-ID:` value from a raw RFC 822 header
/// block. Strips angle brackets so the returned string matches
/// the canonical form we store in `outbound_emails.message_id`.
///
/// The bracket-stripping is mild; we keep the rest of the value
/// verbatim because Message-ID syntax is liberal about what's
/// inside `<>` and we want byte-identical matches.
fn extract_message_id(raw: &str) -> Option<String> {
    for line in raw.lines() {
        // Header continuation lines start with whitespace; treat
        // them as part of the previous field, not a fresh header.
        // For Message-ID specifically the value rarely wraps, so
        // we just look at lines starting with the field name.
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("message-id:") {
            // Re-slice the original (case-preserving) line at the
            // same offset so we keep the value's exact bytes.
            let offset = trimmed.len() - rest.len();
            let value = trimmed[offset..].trim();
            return Some(
                value
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string(),
            );
        }
    }
    None
}

/// Three fields extracted from a `message/delivery-status` body.
///
/// DSNs can carry multiple per-recipient blocks (one per failed
/// address); we take the first one with detail. That's the common
/// case for transactional support email, where the recipient set
/// is one address.
struct DeliveryStatus {
    recipient: Option<String>,
    status_code: Option<String>,
    diagnostic: Option<String>,
}

/// Extract every per-recipient field block from a
/// `message/delivery-status` body.
///
/// RFC 3464 §2.1: the body is one per-message field block (headers
/// like `Reporting-MTA`, `Arrival-Date`) followed by one or more
/// per-recipient field blocks (each with `Final-Recipient`, `Status`,
/// `Diagnostic-Code`). Blocks are separated by a blank line. We
/// split on the blank-line boundary, parse each block, and keep only
/// those that yielded a recipient — that filters out the per-message
/// preamble block automatically without needing to count positions.
fn parse_delivery_status_blocks(body: &str) -> Vec<DeliveryStatus> {
    split_into_blocks(body)
        .iter()
        .map(|block| parse_delivery_status(block))
        .filter(|ds| ds.recipient.is_some())
        .collect()
}

/// Split a `message/delivery-status` body on blank-line boundaries.
/// Per RFC 3464 §2.1, blank lines separate the per-message block from
/// each per-recipient block (and between recipient blocks).
fn split_into_blocks(body: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Extract per-recipient fields from a single delivery-status block.
///
/// RFC 5322 §2.2.3 header folding: a long header value can wrap
/// across multiple physical lines, with continuation lines starting
/// with whitespace. Exchange's DSNs routinely fold `Diagnostic-Code`
/// across two lines:
///
/// ```text
/// Diagnostic-Code: smtp;550 5.1.10 RESOLVER.ADR.RecipientNotFound;
///  Recipient not found by SMTP address lookup
/// ```
///
/// We unfold first so the full value reaches the caller. The
/// type-prefix (`smtp;` / `rfc822;`) gets stripped from
/// `Diagnostic-Code` and `Final-Recipient` since downstream consumers
/// (admin UI, classifier) want just the human-readable suffix.
fn parse_delivery_status(body: &str) -> DeliveryStatus {
    let unfolded = unfold_headers(body);

    let mut recipient: Option<String> = None;
    let mut status_code: Option<String> = None;
    let mut diagnostic: Option<String> = None;

    for line in &unfolded {
        let lower = line.to_ascii_lowercase();
        if recipient.is_none()
            && (lower.starts_with("final-recipient:") || lower.starts_with("original-recipient:"))
        {
            recipient = strip_typed_value(line);
        } else if diagnostic.is_none() && lower.starts_with("diagnostic-code:") {
            diagnostic = strip_typed_value(line);
        } else if status_code.is_none() && lower.starts_with("status:") {
            if let Some((_, value)) = line.split_once(':') {
                // RFC 3464 §2.3.4 says the value is `class.subject.detail`
                // with no trailing text. Real MTAs sometimes append a
                // human-readable suffix ("Status: 5.1.1 message rejected").
                // Take only the leading token so the classifier reads a
                // clean `x.y.z`.
                if let Some(code) = value.split_whitespace().next() {
                    if !code.is_empty() {
                        status_code = Some(code.to_string());
                    }
                }
            }
        }
    }

    DeliveryStatus {
        recipient,
        status_code,
        diagnostic,
    }
}

/// Fold RFC 5322 continuation lines (those starting with whitespace)
/// into the preceding logical header line. The output is a list of
/// unfolded logical headers, one per element, in source order. Blank
/// separator lines (between per-recipient blocks) are dropped.
fn unfold_headers(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in body.lines() {
        if raw.trim().is_empty() {
            // Blank lines separate per-recipient blocks; they end the
            // current header but don't carry a value themselves.
            continue;
        }
        let starts_with_ws = raw.starts_with(' ') || raw.starts_with('\t');
        if starts_with_ws {
            if let Some(prev) = out.last_mut() {
                // Replace the fold whitespace with a single space, the
                // canonical unfolded form per RFC 5322.
                prev.push(' ');
                prev.push_str(raw.trim_start());
                continue;
            }
            // Continuation with no header above it — treat as fresh
            // line. Shouldn't happen on real DSNs but defensive.
        }
        out.push(raw.to_string());
    }
    out
}

/// Extract the value after the semicolon in a `Header: type; value`
/// line. Falls back to the full value when no semicolon is present
/// (some MTAs omit the type prefix on Diagnostic-Code).
///
/// Runs the result through RFC 2047 encoded-word decoding so values
/// like `=?utf-8?Q?Caf=C3=A9?=` reach the admin UI as readable text.
fn strip_typed_value(line: &str) -> Option<String> {
    let (_, value) = line.split_once(':')?;
    let stripped = value
        .split_once(';')
        .map(|(_, suffix)| suffix.trim())
        .unwrap_or_else(|| value.trim());
    if stripped.is_empty() {
        None
    } else {
        Some(decode_encoded_words(stripped))
    }
}

/// Decode any RFC 2047 encoded-words in a header value to UTF-8.
///
/// Reuses `mailparse`'s header parser via a synthetic header line —
/// mailparse implements the full encoding-vs-charset matrix already
/// and exposes the result on `MailHeader::get_value`. The fast-path
/// check avoids the synthetic-header round-trip for plain ASCII
/// values (the overwhelming majority of DSN diagnostics in practice).
fn decode_encoded_words(raw: &str) -> String {
    if !raw.contains("=?") {
        return raw.to_string();
    }
    let synthetic = format!("X: {}\r\n", raw);
    match mailparse::parse_header(synthetic.as_bytes()) {
        Ok((header, _)) => header.get_value(),
        Err(_) => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &[u8]) -> mailparse::ParsedMail<'_> {
        mailparse::parse_mail(raw).unwrap()
    }

    #[test]
    fn parses_canonical_dsn_to_full_report() {
        let raw = b"From: MAILER-DAEMON@example.com\r\n\
            To: support@yourco.com\r\n\
            Subject: Delivery Status Notification (Failure)\r\n\
            Message-ID: <dsn-1@example.com>\r\n\
            Content-Type: multipart/report; report-type=delivery-status; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            This is a delivery status notification.\r\n\
            --B\r\n\
            Content-Type: message/delivery-status\r\n\
            \r\n\
            Reporting-MTA: dns; mail.example.com\r\n\
            \r\n\
            Final-Recipient: rfc822; bouncer@example.org\r\n\
            Action: failed\r\n\
            Status: 5.1.1\r\n\
            Diagnostic-Code: smtp; 550 5.1.1 User unknown\r\n\
            \r\n\
            --B\r\n\
            Content-Type: message/rfc822\r\n\
            \r\n\
            Message-ID: <out-42@yourco.com>\r\n\
            From: support@yourco.com\r\n\
            To: bouncer@example.org\r\n\
            Subject: Re: ticket\r\n\
            \r\n\
            original body\r\n\
            --B--\r\n";
        let reports = parse_bounce(&parsed(raw));
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.original_message_id, "out-42@yourco.com");
        assert_eq!(report.recipient.as_deref(), Some("bouncer@example.org"));
        assert_eq!(report.status_code.as_deref(), Some("5.1.1"));
        assert_eq!(report.diagnostic.as_deref(), Some("550 5.1.1 User unknown"));
        // Hard bounce per the structured status, suppressable.
        assert!(report.is_hard());
    }

    #[test]
    fn returns_empty_without_embedded_original_message() {
        let raw = b"From: postmaster@example.com\r\n\
            Subject: failure notice\r\n\
            Message-ID: <md@example.com>\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            The user does not exist.\r\n";
        assert!(parse_bounce(&parsed(raw)).is_empty());
    }

    #[test]
    fn falls_back_to_original_recipient_when_final_missing() {
        let raw = b"From: MAILER-DAEMON@example.com\r\n\
            Subject: failure\r\n\
            Message-ID: <dsn-2@example.com>\r\n\
            Content-Type: multipart/report; report-type=delivery-status; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: message/delivery-status\r\n\
            \r\n\
            Original-Recipient: rfc822; user@example.org\r\n\
            Action: failed\r\n\
            Status: 5.4.7\r\n\
            \r\n\
            --B\r\n\
            Content-Type: message/rfc822-headers\r\n\
            \r\n\
            Message-ID: <out-99@yourco.com>\r\n\
            \r\n\
            --B--\r\n";
        let reports = parse_bounce(&parsed(raw));
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.original_message_id, "out-99@yourco.com");
        assert_eq!(report.recipient.as_deref(), Some("user@example.org"));
        // Status was present; diagnostic-code wasn't — they live in
        // their own field, no longer collapsed into the diagnostic.
        assert_eq!(report.status_code.as_deref(), Some("5.4.7"));
        assert!(report.diagnostic.is_none());
        // 5.4.7 is "Delivery time expired" — the receiving MTA gave
        // up retrying. Permanent per RFC 3463, classified as hard.
        assert!(report.is_hard());
    }

    #[test]
    fn partial_dsn_without_diagnostic_returns_recipient_only() {
        let raw = b"Subject: failure\r\n\
            Message-ID: <dsn-3@example.com>\r\n\
            Content-Type: multipart/report; report-type=delivery-status; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: message/delivery-status\r\n\
            \r\n\
            Final-Recipient: rfc822; lonely@example.org\r\n\
            \r\n\
            --B\r\n\
            Content-Type: message/rfc822\r\n\
            \r\n\
            Message-ID: <out-7@yourco.com>\r\n\
            \r\n\
            --B--\r\n";
        let reports = parse_bounce(&parsed(raw));
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.original_message_id, "out-7@yourco.com");
        assert_eq!(report.recipient.as_deref(), Some("lonely@example.org"));
        assert!(report.status_code.is_none());
        assert!(report.diagnostic.is_none());
        // Neither signal present — default-deny: not hard.
        assert!(!report.is_hard());
    }

    #[test]
    fn multi_recipient_dsn_yields_one_report_per_block() {
        // RFC 3464 §2.1: a DSN can carry multiple per-recipient
        // field groups separated by blank lines. The parser must
        // return one BounceReport per block, all linked to the same
        // original Message-ID, so the pipeline can suppress every
        // failed address independently.
        let raw = b"From: MAILER-DAEMON@list.example.com\r\n\
            Subject: Multiple failures\r\n\
            Message-ID: <multi-dsn@example.com>\r\n\
            Content-Type: multipart/report; report-type=delivery-status; boundary=\"M\"\r\n\
            \r\n\
            --M\r\n\
            Content-Type: message/delivery-status\r\n\
            \r\n\
            Reporting-MTA: dns; list.example.com\r\n\
            \r\n\
            Final-Recipient: rfc822; alice@example.org\r\n\
            Status: 5.1.1\r\n\
            Diagnostic-Code: smtp; 550 5.1.1 No such user (alice)\r\n\
            \r\n\
            Final-Recipient: rfc822; carol@example.org\r\n\
            Status: 5.1.1\r\n\
            Diagnostic-Code: smtp; 550 5.1.1 No such user (carol)\r\n\
            \r\n\
            --M\r\n\
            Content-Type: message/rfc822\r\n\
            \r\n\
            Message-ID: <out-multi@yourco.com>\r\n\
            \r\n\
            --M--\r\n";
        let reports = parse_bounce(&parsed(raw));
        assert_eq!(
            reports.len(),
            2,
            "expected one report per per-recipient block"
        );
        // Both reports share the same original Message-ID since
        // they describe failures of the same outbound row.
        assert_eq!(reports[0].original_message_id, "out-multi@yourco.com");
        assert_eq!(reports[1].original_message_id, "out-multi@yourco.com");
        assert_eq!(reports[0].recipient.as_deref(), Some("alice@example.org"));
        assert_eq!(reports[1].recipient.as_deref(), Some("carol@example.org"));
        assert!(reports[0].is_hard());
        assert!(reports[1].is_hard());
    }

    #[test]
    fn unfolds_continuation_lines_on_diagnostic_code() {
        // Exchange-style: Diagnostic-Code wraps across two lines.
        // RFC 5322 §2.2.3 says the continuation line begins with
        // whitespace and joins to the previous logical header.
        //
        // We assemble the fixture via `concat!` because Rust's `\`
        // line-continuation in string literals eats the leading
        // whitespace on the next line, which is exactly the byte
        // we need to preserve to trigger the unfold path.
        let body = concat!(
            "Final-Recipient: rfc822; jane@example.org\r\n",
            "Status: 5.1.10\r\n",
            "Diagnostic-Code: smtp;550 5.1.10 RESOLVER.ADR.RecipientNotFound;\r\n",
            " Recipient not found by SMTP address lookup\r\n",
        );
        let ds = parse_delivery_status(body);
        assert_eq!(ds.status_code.as_deref(), Some("5.1.10"));
        assert!(
            ds.diagnostic
                .as_deref()
                .map(|d| d.contains("RESOLVER.ADR.RecipientNotFound")
                    && d.contains("Recipient not found by SMTP address lookup"))
                .unwrap_or(false),
            "expected unfolded diagnostic to contain both lines, got {:?}",
            ds.diagnostic,
        );
    }

    #[test]
    fn is_hard_bounce_prefers_status_over_diagnostic() {
        // Status says 5.7.1 (policy carve-out, should NOT suppress);
        // diagnostic accidentally contains 5.1.1 (would look hard if
        // we scanned it). The structured status MUST win.
        assert!(!is_hard_bounce(
            Some("5.7.1"),
            Some("smtp; 550 reference 5.1.1 in legacy header"),
        ));
    }

    #[test]
    fn is_hard_bounce_status_carve_outs() {
        // Recipient-side terminals — auto-suppress.
        assert!(is_hard_bounce(Some("5.1.1"), None)); // bad mailbox address
        assert!(is_hard_bounce(Some("5.2.1"), None)); // mailbox disabled
        assert!(is_hard_bounce(Some("5.2.2"), None)); // mailbox full
        assert!(is_hard_bounce(Some("5.4.7"), None)); // delivery time expired

        // RFC 3463 §3.1 "Other / undefined" — too vague to act on.
        assert!(!is_hard_bounce(Some("5.0.0"), None));

        // RFC 3463 §3.4 system status — receiver-side recoverable
        // infrastructure problems. Suppressing locks customers out
        // when the receiver fixes their disk-full or restarts.
        assert!(!is_hard_bounce(Some("5.3.0"), None));
        assert!(!is_hard_bounce(Some("5.3.5"), None));

        // RFC 3463 §3.5 network / routing — only 5.4.7 (gave up
        // after retries) is a recipient-terminal. The rest are
        // either sender-side or transient.
        assert!(!is_hard_bounce(Some("5.4.0"), None));
        assert!(!is_hard_bounce(Some("5.4.1"), None));
        assert!(!is_hard_bounce(Some("5.4.4"), None));

        // RFC 3463 §3.6 SMTP protocol — sender-side bugs. Our MTA
        // is misconfigured; blocking the recipient is wrong.
        assert!(!is_hard_bounce(Some("5.5.0"), None));
        assert!(!is_hard_bounce(Some("5.5.2"), None));

        // RFC 3463 §3.7 content / media — sender-side. The
        // receiver rejected our payload format / encoding.
        assert!(!is_hard_bounce(Some("5.6.0"), None));
        assert!(!is_hard_bounce(Some("5.6.3"), None));

        // RFC 3463 §3.8 security / policy — SPF, DKIM, DMARC,
        // greylisting, content filtering. Mostly sender-side fixes
        // (or transient receiver-side policy).
        assert!(!is_hard_bounce(Some("5.7.1"), None));
        assert!(!is_hard_bounce(Some("5.7.26"), None));

        // 4.x.x soft bounces are always soft regardless of subcode.
        assert!(!is_hard_bounce(Some("4.2.2"), None));
    }

    #[test]
    fn diagnostic_decodes_rfc_2047_encoded_words() {
        // Non-ASCII upstream reason wrapped in an encoded-word.
        // The admin UI shouldn't show `=?utf-8?Q?...?=` literals.
        let body = concat!(
            "Final-Recipient: rfc822; user@example.org\r\n",
            "Status: 5.1.1\r\n",
            "Diagnostic-Code: smtp; 550 =?utf-8?Q?Caf=C3=A9_ferm=C3=A9?=\r\n",
        );
        let ds = parse_delivery_status(body);
        assert_eq!(
            ds.diagnostic.as_deref(),
            Some("550 Café fermé"),
            "encoded-word should decode to UTF-8 plain text",
        );
    }

    #[test]
    fn diagnostic_plain_ascii_passes_through_unchanged() {
        // Fast-path: no `=?` marker means no decode round-trip.
        let body = concat!(
            "Final-Recipient: rfc822; user@example.org\r\n",
            "Diagnostic-Code: smtp; 550 5.1.1 User unknown\r\n",
        );
        let ds = parse_delivery_status(body);
        assert_eq!(ds.diagnostic.as_deref(), Some("550 5.1.1 User unknown"));
    }

    #[test]
    fn status_field_drops_trailing_human_text() {
        // RFC 3464 §2.3.4 says the value is `x.y.z` with no trailing
        // text, but some MTAs append a human-readable suffix. The
        // classifier needs a clean code to work.
        let body = concat!(
            "Final-Recipient: rfc822; user@example.org\r\n",
            "Status: 5.1.1 message rejected\r\n",
        );
        let ds = parse_delivery_status(body);
        assert_eq!(ds.status_code.as_deref(), Some("5.1.1"));
    }

    #[test]
    fn is_hard_bounce_falls_back_to_diagnostic_when_status_absent() {
        // No Status field — scan the diagnostic.
        assert!(is_hard_bounce(None, Some("smtp; 550 5.1.1 User unknown")));
        assert!(!is_hard_bounce(None, Some("smtp; 421 4.7.0 Try again")));
        assert!(!is_hard_bounce(None, Some("smtp; 550 5.7.1 greylisted")));
        // Default-deny when neither signal is present.
        assert!(!is_hard_bounce(None, None));
    }

    #[test]
    fn extracts_message_id_strips_angle_brackets() {
        assert_eq!(
            extract_message_id("From: a@b\r\nMessage-ID: <abc@host>\r\n"),
            Some("abc@host".to_string())
        );
    }

    #[test]
    fn extracts_message_id_case_insensitive() {
        assert_eq!(
            extract_message_id("message-id: <lowercase@host>\r\n"),
            Some("lowercase@host".to_string())
        );
    }
}
