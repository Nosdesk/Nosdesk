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
/// the other two fields are best-effort diagnostic detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BounceReport {
    /// `Message-ID` header from the embedded `message/rfc822` part,
    /// stripped of `<>` brackets. Matches `outbound_emails.message_id`.
    pub original_message_id: String,
    /// The address the remote MTA actually rejected. May differ
    /// from the original recipient if a list expanded.
    pub recipient: Option<String>,
    /// Raw `Diagnostic-Code` or `Status` text from the DSN. Kept
    /// verbatim so the admin UI can show the upstream reason
    /// without us inventing a category vocabulary.
    pub diagnostic: Option<String>,
}

/// Parse a DSN message into a structured bounce report.
///
/// Returns `None` when the embedded original message-id can't be
/// recovered (no `message/rfc822` part, or that part has no
/// `Message-ID` header). The caller still treats the inbound as a
/// bounce skip — it just can't link it back to an outbound row.
pub fn parse_bounce(root: &ParsedMail) -> Option<BounceReport> {
    let mut original_message_id: Option<String> = None;
    let mut recipient: Option<String> = None;
    let mut diagnostic: Option<String> = None;

    walk(root, &mut |part| {
        let ctype = part.ctype.mimetype.to_ascii_lowercase();
        match ctype.as_str() {
            "message/rfc822" | "message/rfc822-headers" => {
                if original_message_id.is_none() {
                    if let Ok(body) = part.get_body() {
                        original_message_id = extract_message_id(&body);
                    }
                }
            }
            "message/delivery-status" => {
                if recipient.is_none() || diagnostic.is_none() {
                    if let Ok(body) = part.get_body() {
                        let (r, d) = parse_delivery_status(&body);
                        if recipient.is_none() {
                            recipient = r;
                        }
                        if diagnostic.is_none() {
                            diagnostic = d;
                        }
                    }
                }
            }
            _ => {}
        }
    });

    original_message_id.map(|mid| BounceReport {
        original_message_id: mid,
        recipient,
        diagnostic,
    })
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
            return Some(value.trim_start_matches('<').trim_end_matches('>').to_string());
        }
    }
    None
}

/// Extract `Final-Recipient` (or fallback `Original-Recipient`) and
/// `Diagnostic-Code` (fallback `Status`) from the per-recipient
/// block of a `message/delivery-status` body.
///
/// DSNs can carry multiple per-recipient blocks (one per failed
/// address); we take the first one with diagnostic detail. That's
/// the common case for transactional support email, where the
/// recipient set is one address.
fn parse_delivery_status(body: &str) -> (Option<String>, Option<String>) {
    let mut recipient: Option<String> = None;
    let mut diagnostic_code: Option<String> = None;
    let mut status_code: Option<String> = None;

    for line in body.lines() {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if recipient.is_none()
            && (lower.starts_with("final-recipient:") || lower.starts_with("original-recipient:"))
        {
            // Format: `Final-Recipient: rfc822; user@example.com`
            // The type-prefix (`rfc822`) sits before the semicolon;
            // the address sits after. Take the suffix.
            if let Some((_, value)) = trimmed.split_once(':') {
                let addr = value
                    .split_once(';')
                    .map(|(_, a)| a.trim())
                    .unwrap_or_else(|| value.trim());
                if !addr.is_empty() {
                    recipient = Some(addr.to_string());
                }
            }
        }
        if diagnostic_code.is_none() && lower.starts_with("diagnostic-code:") {
            if let Some((_, value)) = trimmed.split_once(':') {
                let raw = value
                    .split_once(';')
                    .map(|(_, a)| a.trim())
                    .unwrap_or_else(|| value.trim());
                if !raw.is_empty() {
                    diagnostic_code = Some(raw.to_string());
                }
            }
        } else if status_code.is_none() && lower.starts_with("status:") {
            if let Some((_, value)) = trimmed.split_once(':') {
                let raw = value.trim();
                if !raw.is_empty() {
                    status_code = Some(raw.to_string());
                }
            }
        }
    }

    // Prefer `Diagnostic-Code` (the upstream MTA's human-readable
    // reason) over `Status` (just the RFC 3463 numeric code) when
    // both are present. Either alone is acceptable.
    let diagnostic = diagnostic_code.or(status_code);
    (recipient, diagnostic)
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
        let report = parse_bounce(&parsed(raw)).unwrap();
        assert_eq!(report.original_message_id, "out-42@yourco.com");
        assert_eq!(report.recipient.as_deref(), Some("bouncer@example.org"));
        assert_eq!(
            report.diagnostic.as_deref(),
            Some("550 5.1.1 User unknown")
        );
    }

    #[test]
    fn returns_none_without_embedded_original_message() {
        let raw = b"From: postmaster@example.com\r\n\
            Subject: failure notice\r\n\
            Message-ID: <md@example.com>\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            The user does not exist.\r\n";
        assert!(parse_bounce(&parsed(raw)).is_none());
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
        let report = parse_bounce(&parsed(raw)).unwrap();
        assert_eq!(report.original_message_id, "out-99@yourco.com");
        assert_eq!(report.recipient.as_deref(), Some("user@example.org"));
        assert_eq!(report.diagnostic.as_deref(), Some("5.4.7"));
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
        let report = parse_bounce(&parsed(raw)).unwrap();
        assert_eq!(report.original_message_id, "out-7@yourco.com");
        assert_eq!(report.recipient.as_deref(), Some("lonely@example.org"));
        assert!(report.diagnostic.is_none());
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
