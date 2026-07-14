//! Log-message content scrubbing — the sink-side companion to the field-name
//! allowlist in [`crate::utils::tracing_redact`].
//!
//! The allowlist protects structured *field names*: a field whose name isn't
//! listed is dropped. But `"message"` is always allowlisted and emitted
//! verbatim, so PII interpolated into a free-text message — `info!("... {}",
//! email)` — slips straight through. That's the unguarded channel (it's how a
//! customer email once shipped from `password_reset`).
//!
//! [`scrub`] closes it structurally: every emitted message string passes
//! through here and has high-confidence, never-legitimately-raw tokens (email
//! addresses, JWTs) masked at the output boundary — regardless of call-site
//! discipline. It deliberately does *not* touch dotted-quad IPs (infra IPs in
//! DB/connection errors are operational, not customer PII — blunt scrubbing
//! would hurt debuggability) or opaque IDs (UUIDs, `ticket_id` — safe to log).
//!
//! The CI guardrail (`tests/logging_pii_guardrail.rs`) is the source-side
//! companion: it fails the build if PII is interpolated into a log macro in the
//! first place. Typed redaction at the source, content scrub at the sink, lint
//! against regressions.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::{Captures, Regex};

/// Mask an email for log output: `kyle@nosdesk.com` → `k***@nosdesk.com`. Keeps
/// the domain (triage/filtering) and the first local-part character (enough to
/// tell "same user as the previous line") without leaving a harvestable address.
/// Zero-char / no-`@` inputs return `"***"` rather than risking a panic.
pub fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return "***".to_string();
    };
    let head = local.chars().next().unwrap_or('?');
    format!("{head}***@{domain}")
}

/// Email: local-part + `@` + a dotted domain. The local-part class excludes
/// `*`, so an already-masked `k***@domain` is *not* re-matched (idempotent).
fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap())
}

/// JWT / JWS compact serialization: `eyJ…` (base64url of `{"…`) + two more
/// base64url segments. Catches id_tokens, access/reset/verify tokens that are
/// JWTs — the whole thing is replaced, header included.
fn jwt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+").unwrap())
}

/// Mask PII/secret tokens inside a free-text log message. Borrows unchanged
/// when nothing matches (the overwhelmingly common case), gated by a cheap
/// substring check before any regex runs.
pub fn scrub(input: &str) -> Cow<'_, str> {
    let has_email = input.contains('@');
    let has_jwt = input.contains("eyJ");
    if !has_email && !has_jwt {
        return Cow::Borrowed(input);
    }

    let mut out: Cow<'_, str> = Cow::Borrowed(input);
    if has_email {
        out = apply(out, email_re(), |caps: &Captures| {
            // Reuse the canonical masker so message emails and structured
            // `email` fields would mask identically: `k***@domain`.
            mask_email(caps.get(0).map_or("", |m| m.as_str()))
        });
    }
    if has_jwt {
        out = apply(out, jwt_re(), |_: &Captures| "[REDACTED_JWT]".to_string());
    }
    out
}

/// Apply a regex replacement to a `Cow`, preserving the borrow when the regex
/// makes no change (so the no-match fast path never allocates).
fn apply<'a>(s: Cow<'a, str>, re: &Regex, rep: impl Fn(&Captures) -> String) -> Cow<'a, str> {
    match re.replace_all(&s, rep) {
        Cow::Borrowed(_) => s,
        Cow::Owned(owned) => Cow::Owned(owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_email_in_message() {
        assert_eq!(
            scrub("Password reset requested for kyle@nosdesk.com"),
            "Password reset requested for k***@nosdesk.com"
        );
    }

    #[test]
    fn masks_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc-DEF_123";
        assert_eq!(scrub(&format!("bearer {jwt}")), "bearer [REDACTED_JWT]");
    }

    #[test]
    fn idempotent_and_borrows_when_clean() {
        assert_eq!(scrub("k***@nosdesk.com"), "k***@nosdesk.com");
        assert!(matches!(
            scrub("ticket 42 created workspace_id=7"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn masks_multiple_emails() {
        assert_eq!(
            scrub("merge a@x.com into b@y.org"),
            "merge a***@x.com into b***@y.org"
        );
    }

    #[test]
    fn mask_email_edge_cases() {
        assert_eq!(mask_email("a@b.co"), "a***@b.co");
        assert_eq!(mask_email(""), "***");
        assert_eq!(mask_email("no-at-sign"), "***");
    }
}
