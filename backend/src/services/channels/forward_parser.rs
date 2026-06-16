//! Extract the original sender from an email that has been forwarded
//! into the helpdesk by a technician.
//!
//! Used by [`super::pipeline::process_event`] when the message's
//! `From:` address belongs to a verified Nosdesk user. In that case
//! the message is almost certainly a tech forwarding a customer's
//! email, and the ticket's requester should be the original sender —
//! not the tech. Parsing the embedded `From:` is the industry-standard
//! approach (Zendesk, Freshdesk, Help Scout, Zammad all do this).
//!
//! # Delimiter shapes we recognise
//!
//! ```text
//! Gmail, Outlook web   ----------- Forwarded message -----------
//! Apple Mail           Begin forwarded message:
//! Thunderbird          -------- Forwarded Message --------
//! Various (reply hdr)  -----Original Message-----
//! Outlook desktop      ________________________________   (underscore rule)
//! ```
//!
//! The parser is deliberately **conservative**: if it can't find a
//! delimiter AND a plausible `From:` line below it, it returns `None`
//! and the pipeline falls through to the impersonation guard. That's
//! the safe default — better to reject a malformed forward and have
//! the tech resend than to mis-attribute a ticket.
//!
//! # Trust boundary
//!
//! The extracted `From:` comes from the **body** of an email, which
//! the sender controls. A tech with a compromised mailbox could in
//! principle fabricate a body that misattributes a ticket to anyone.
//! The pipeline mitigates this by gating forward handling on the
//! envelope sender being a *verified Nosdesk user* — see
//! [`super::pipeline::resolve_identity`]. Callers outside that gate
//! MUST NOT trust the extracted identity for authorization decisions.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::services::channels::InboundMessage;

/// What we managed to pull out of the forwarded section. The email is
/// the important bit; a display name is nice-to-have for auto-
/// provisioning a readable `users.name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedForward {
    pub email: String,
    pub display_name: Option<String>,
}

/// Match any of the forward delimiter shapes above. Anchored to line
/// start (`(?m)^`) so we don't trip on the word "forwarded" inside
/// normal prose.
static DELIMITER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?m)^\s*(?:-{3,}\s*(?:Forwarded message|Forwarded Message|Original Message)\s*-{3,}|Begin forwarded message:|_{10,})\s*$",
    )
    .expect("valid forward delimiter regex")
});

/// Match a `From:` header line with one of:
///
///   From: "Display Name" <email@host>
///   From: Display Name <email@host>
///   From: <email@host>
///   From: email@host
///
/// Capture group 1 is the optional display name (may include quotes,
/// which are stripped by the caller); group 2 is the angle-bracketed
/// email, group 3 is the bare email.
static FROM_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?im)^\s*From:\s*(?:"?([^"<\n]*?)"?\s*<\s*([^\s<>@]+@[^\s<>]+)\s*>|([^\s<>@]+@[^\s<>]+))\s*$"#,
    )
    .expect("valid From-line regex")
});

/// Gmail / Outlook sometimes HTML-encode the `From:` as
/// `From: *Name* <email>` after a Markdown-y italic. We strip leading
/// and trailing asterisks from the display name below.
fn clean_display_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('*').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Entry point: look at the plaintext body and return the extracted
/// forward author, if any. Returns `None` when:
///
/// - No recognised delimiter appears in the body, AND
/// - No `From:` line appears near the top of the body (the no-quote-
///   wrapper case some clients emit when the forward isn't fenced)
///
/// The caller decides what to do with `None` — the pipeline falls
/// back to the impersonation guard, which is the right semantics for
/// a verified tech who sent a plain non-forwarded message.
pub fn extract(msg: &InboundMessage) -> Option<ExtractedForward> {
    let body = &msg.body_text;
    if body.is_empty() {
        return None;
    }

    // Scan from the first delimiter onwards. If there's no delimiter
    // but the message is clearly a forward by subject hint AND a From
    // line sits at the top of the body, accept that too — Apple Mail
    // stripped to text sometimes loses the "Begin forwarded message"
    // line.
    let search_start = DELIMITER_RE.find(body).map(|m| m.end());

    let slice = match search_start {
        Some(idx) => &body[idx..],
        None if subject_looks_forwarded(msg.subject.as_deref()) => body.as_str(),
        None => return None,
    };

    // Multi-hop forwards (A → B → us) produce multiple `From:` blocks
    // in the body. We take the FIRST match after the delimiter, which
    // for a "Fwd: Fwd: ..." chain resolves to the outermost forwarded
    // sender — the one closest to the tech doing the final hop. That
    // matches what Help Scout / Zendesk do and is usually what an
    // operator expects ("the customer who I'm forwarding from").
    let caps = FROM_LINE_RE.captures(slice)?;
    let (email, display_name) = match (caps.get(2), caps.get(3)) {
        (Some(angle), _) => (
            angle.as_str().trim().to_string(),
            caps.get(1).and_then(|m| clean_display_name(m.as_str())),
        ),
        (None, Some(bare)) => (bare.as_str().trim().to_string(), None),
        _ => return None,
    };

    // Defensive: the regex's email fragment is loose enough that
    // garbage like "From: foo@" or "From: foo@.com" could sneak
    // through. Require the domain part to have a dot somewhere
    // in the middle — not at start or end — matching the shape
    // `lettre` would accept.
    if !is_plausible_email(&email) {
        return None;
    }
    Some(ExtractedForward {
        email,
        display_name,
    })
}

/// Cheap sanity check — matches what lettre's address parser enforces
/// without pulling lettre into this module. Rejects `foo@`,
/// `foo@.com`, `foo@com` (no dot), and similar.
fn is_plausible_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    // Domain must contain at least one internal dot, not leading /
    // trailing / consecutive.
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2 && labels.iter().all(|label| !label.is_empty())
}

/// Very light subject check — `Fwd:` / `FW:` / `Tr:` (French) /
/// `WG:` (German) prefixes. Used only as a fallback signal when no
/// body delimiter matched; we never *require* the subject marker
/// because clients are inconsistent about preserving it.
fn subject_looks_forwarded(subject: Option<&str>) -> bool {
    let Some(s) = subject else { return false };
    let trimmed = s.trim_start().to_ascii_lowercase();
    ["fwd:", "fw:", "tr:", "wg:"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    //! Fixtures are real-world shapes observed across the major mail
    //! clients. When adding a new client, pin the sample body here
    //! rather than tweaking the regex without a repro.

    use super::*;
    use crate::services::channels::{ExternalIdentity, LoopMarkers};
    use chrono::Utc;

    fn msg(subject: &str, body: &str) -> InboundMessage {
        InboundMessage {
            external_id: "<x@h>".into(),
            from: ExternalIdentity {
                provider: "email_imap".into(),
                external_id: "tech@yourco.com".into(),
                display_name: "Tech".into(),
                known_email: Some("tech@yourco.com".into()),
            },
            subject: Some(subject.into()),
            body_text: body.into(),
            body_html: None,
            attachments: vec![],
            references: vec![],
            received_at: Utc::now(),
            loop_markers: LoopMarkers::default(),
            raw_metadata: serde_json::json!({}),
            recipients: vec![],
            is_bounce: false,
            bounce_reports: Vec::new(),
            raw_bytes: None,
            content_language: None,
            source_ref: None,
        }
    }

    // ---- Positive cases ----

    #[test]
    fn gmail_forward() {
        let body = "\
Fyi, please handle.

---------- Forwarded message ---------
From: Alice Customer <alice@customer.example>
Date: Mon, Jan 1, 2024 at 10:00 AM
Subject: Printer fire
To: <tech@yourco.com>

My printer is on fire.
";
        let out = extract(&msg("Fwd: Printer fire", body)).unwrap();
        assert_eq!(out.email, "alice@customer.example");
        assert_eq!(out.display_name.as_deref(), Some("Alice Customer"));
    }

    #[test]
    fn outlook_horizontal_rule() {
        let body = "\
Please help this customer.

________________________________
From: Bob Example <bob@customer.example>
Sent: Monday, January 1, 2024 10:00 AM
To: Tech <tech@yourco.com>
Subject: Cannot log in

Hi, my password isn't working.
";
        let out = extract(&msg("FW: Cannot log in", body)).unwrap();
        assert_eq!(out.email, "bob@customer.example");
        assert_eq!(out.display_name.as_deref(), Some("Bob Example"));
    }

    #[test]
    fn apple_mail() {
        let body = "\
Please take this one.

Begin forwarded message:

From: Carol <carol@customer.example>
Subject: Help
Date: 1 Jan 2024 10:00
To: tech@yourco.com

Need assistance please.
";
        let out = extract(&msg("Fwd: Help", body)).unwrap();
        assert_eq!(out.email, "carol@customer.example");
        assert_eq!(out.display_name.as_deref(), Some("Carol"));
    }

    #[test]
    fn thunderbird() {
        let body = "\
-------- Forwarded Message --------
From: Dan <dan@customer.example>
Subject: Crash report

Please fix.
";
        let out = extract(&msg("Fwd: Crash report", body)).unwrap();
        assert_eq!(out.email, "dan@customer.example");
    }

    #[test]
    fn double_quoted_display_name() {
        // Outlook likes to wrap names with odd chars in quotes.
        let body = "\
---------- Forwarded message ---------
From: \"Chen, Lin\" <lin.chen@customer.example>
Subject: Access request
";
        let out = extract(&msg("Fwd: Access", body)).unwrap();
        assert_eq!(out.email, "lin.chen@customer.example");
        assert_eq!(out.display_name.as_deref(), Some("Chen, Lin"));
    }

    #[test]
    fn bare_address_without_display_name() {
        let body = "\
---------- Forwarded message ---------
From: anon@customer.example
Subject: ?
";
        let out = extract(&msg("Fwd: ?", body)).unwrap();
        assert_eq!(out.email, "anon@customer.example");
        assert_eq!(out.display_name, None);
    }

    #[test]
    fn fwd_subject_with_inline_from_but_no_delimiter() {
        // Some clients strip the delimiter when converting to plain
        // text. When the subject says "Fwd:" we still try.
        let body = "\
From: Eve <eve@customer.example>
Subject: issue

please fix
";
        let out = extract(&msg("FWD: issue", body)).unwrap();
        assert_eq!(out.email, "eve@customer.example");
    }

    // ---- Negative cases ----

    #[test]
    fn plain_message_without_markers_returns_none() {
        let body = "Hey team, just checking the mailbox. No forward here.";
        assert!(extract(&msg("Testing", body)).is_none());
    }

    #[test]
    fn quoted_from_word_in_prose_does_not_trigger() {
        // The word "From" in prose must NOT be mistaken for a header.
        let body = "\
I got a message from a customer earlier. No delimiter in this email.
From the logs it looks bad.
";
        assert!(extract(&msg("Testing prose", body)).is_none());
    }

    #[test]
    fn malformed_from_line_below_delimiter_returns_none() {
        let body = "\
---------- Forwarded message ---------
From: not-an-email
Subject: weird
";
        assert!(extract(&msg("Fwd: weird", body)).is_none());
    }

    #[test]
    fn empty_body() {
        assert!(extract(&msg("Fwd: hi", "")).is_none());
    }

    #[test]
    fn fwd_subject_alone_without_from_line_returns_none() {
        // Subject says "Fwd:" but there's no From: anywhere — don't
        // invent a sender.
        let body = "I think you forgot to include the original email.";
        assert!(extract(&msg("Fwd: where", body)).is_none());
    }

    #[test]
    fn malformed_domains_are_rejected() {
        // Each of these has a "From:" line the regex will match but
        // whose domain shape is nonsense — the downstream validator
        // must drop them rather than auto-provisioning a garbage
        // guest account.
        for bad in [
            "From: foo@\n",
            "From: foo@.com\n",
            "From: foo@com\n",
            "From: foo@domain.\n",
            "From: foo@..com\n",
        ] {
            let body = format!("---------- Forwarded message ---------\n{bad}Subject: x\n");
            assert!(
                extract(&msg("Fwd: junk", &body)).is_none(),
                "should have rejected {bad:?}"
            );
        }
    }

    #[test]
    fn is_plausible_email_edge_cases() {
        assert!(is_plausible_email("a@b.co"));
        assert!(is_plausible_email("a.b@sub.domain.example"));
        assert!(!is_plausible_email("no-at-sign"));
        assert!(!is_plausible_email("@domain.com"));
        assert!(!is_plausible_email("user@"));
        assert!(!is_plausible_email("user@nodot"));
        assert!(!is_plausible_email("user@.leading-dot"));
        assert!(!is_plausible_email("user@trailing-dot."));
        assert!(!is_plausible_email("user@double..dot"));
    }

    // ---- Subject detection ----

    #[test]
    fn subject_hints_covered() {
        assert!(subject_looks_forwarded(Some("Fwd: hi")));
        assert!(subject_looks_forwarded(Some("FW: hi")));
        assert!(subject_looks_forwarded(Some(" fw: hi")));
        assert!(subject_looks_forwarded(Some("Tr: bonjour"))); // French
        assert!(subject_looks_forwarded(Some("WG: hallo"))); // German
        assert!(!subject_looks_forwarded(Some("Re: hi"))); // reply, not forward
        assert!(!subject_looks_forwarded(Some("Forward me later")));
        assert!(!subject_looks_forwarded(None));
    }
}
