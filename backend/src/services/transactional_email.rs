//! Enqueue transactional emails (password reset, invitation,
//! notification) onto the outbound queue rather than firing them
//! synchronously.
//!
//! These three sends are user-blocking and must not be lost on
//! a transient SMTP failure or a process restart. The Pass-1
//! outbound queue gives us retry-with-backoff, circuit breaker,
//! suppression-list integration, and bounce reconciliation; this
//! module just builds the right `NewOutboundEmail` for each
//! template and hands it to the queue with an idempotency key
//! that collapses re-enqueues from the same logical request.
//!
//! Why a free-standing module rather than methods on
//! `EmailService`: the existing `send_*_email` methods predate
//! the queue and do the SMTP send themselves. Keeping the
//! enqueue path here makes the cutover obvious in handlers
//! (callers that want at-least-once delivery call `enqueue_*`;
//! callers that genuinely want fire-and-forget still have the
//! old methods). Once the migration settles, the old async-send
//! methods become deprecated.

use diesel::result::Error as DieselError;
use ring::digest;

use crate::db::DbConnection;
use crate::models::{NewOutboundEmail, OutboundEmail};
use crate::repository::outbound_emails;
use crate::utils::email::{EmailBranding, EmailService};

/// Generate a globally-unique Message-ID for a transactional send.
/// Domain is derived from `SMTP_FROM_EMAIL` (the address the email
/// will be sent from) so inbound bounce DSNs can correlate via
/// the same domain. A short random tail keeps the ID unique even
/// when the idempotency key collapses a retry — the queue row
/// stamps the Message-ID at enqueue and reuses it on every send
/// attempt, so the recipient's MUA dedupes correctly.
fn make_message_id(prefix: &str, domain: &str) -> String {
    let random: u32 = rand::random();
    format!("{prefix}.{random:08x}@{domain}")
}

/// Extract the domain part of `from_email`. Falls back to
/// `nosdesk.local` so Message-IDs always parse, even on a
/// misconfigured dev instance.
fn from_email_domain(svc: &EmailService) -> String {
    svc.config()
        .from_email
        .rsplit_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_else(|| "nosdesk.local".to_string())
}

/// SHA-256 of `input`, truncated to the first 16 hex chars (64
/// bits — plenty of entropy to avoid collisions across the
/// lifetime of any one transactional flow). Used to derive
/// idempotency keys from sensitive material (reset tokens,
/// invitation tokens) without persisting the raw value as the
/// key.
fn hash16(input: &str) -> String {
    let digest = digest::digest(&digest::SHA256, input.as_bytes());
    let bytes = digest.as_ref();
    let mut out = String::with_capacity(16);
    for &b in &bytes[..8] {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Build the `NewOutboundEmail` row for a password-reset send
/// without touching the database. Split out from
/// `enqueue_password_reset` so the snapshot tests below can lock
/// the headers + idempotency-key + body shape in place without
/// needing a live connection.
pub fn prepare_password_reset(
    svc: &EmailService,
    branding: &EmailBranding,
    recipient: &str,
    user_name: &str,
    reset_token: &str,
    locale: &unic_langid::LanguageIdentifier,
) -> NewOutboundEmail {
    let (subject, body_html, body_text) = svc.compose_password_reset(
        user_name,
        reset_token,
        branding,
        locale,
    );
    let message_id = make_message_id("password-reset", &from_email_domain(svc));

    // Auto-Submitted signals well-behaved auto-responders (out-of-
    // office, vacation mail) to ignore us, breaking mail loops
    // without affecting spam scoring (RFC 3834).
    let headers_json = serde_json::json!({
        "Auto-Submitted": "auto-generated",
    });

    NewOutboundEmail {
        channel_id: None,
        ticket_id: None,
        comment_id: None,
        recipient: recipient.to_string(),
        subject,
        body_text,
        body_html: Some(body_html),
        message_id,
        in_reply_to: None,
        references_list: vec![],
        headers_json,
        correlation_id: None,
        idempotency_key: Some(format!("password_reset:{}", hash16(reset_token))),
    }
}

/// Enqueue a password-reset email. Caller passes the raw reset
/// token; we derive an idempotency key from its hash so a network
/// blip between the handler and the DB doesn't deliver two
/// emails carrying the same link.
pub fn enqueue_password_reset(
    conn: &mut DbConnection,
    svc: &EmailService,
    branding: &EmailBranding,
    recipient: &str,
    user_name: &str,
    reset_token: &str,
    locale: &unic_langid::LanguageIdentifier,
) -> Result<OutboundEmail, DieselError> {
    let row = prepare_password_reset(svc, branding, recipient, user_name, reset_token, locale);
    outbound_emails::enqueue_idempotent(conn, row)
}

/// Build the `NewOutboundEmail` row for an invitation send.
/// See `prepare_password_reset` for the rationale.
pub fn prepare_invitation(
    svc: &EmailService,
    branding: &EmailBranding,
    recipient: &str,
    user_name: &str,
    invitation_token: &str,
    invited_by: &str,
    locale: &unic_langid::LanguageIdentifier,
) -> NewOutboundEmail {
    let (subject, body_html, body_text) = svc.compose_invitation(
        user_name,
        invitation_token,
        branding,
        invited_by,
        locale,
    );
    let message_id = make_message_id("invitation", &from_email_domain(svc));
    let headers_json = serde_json::json!({
        "Auto-Submitted": "auto-generated",
    });

    NewOutboundEmail {
        channel_id: None,
        ticket_id: None,
        comment_id: None,
        recipient: recipient.to_string(),
        subject,
        body_text,
        body_html: Some(body_html),
        message_id,
        in_reply_to: None,
        references_list: vec![],
        headers_json,
        correlation_id: None,
        idempotency_key: Some(format!("invitation:{}", hash16(invitation_token))),
    }
}

/// Enqueue a user-invitation email. The key derives from the
/// invitation token, so admin "resend invitation" actions
/// (which mint a new token) produce a new send; idempotency only
/// catches enqueue retries inside one request.
pub fn enqueue_invitation(
    conn: &mut DbConnection,
    svc: &EmailService,
    branding: &EmailBranding,
    recipient: &str,
    user_name: &str,
    invitation_token: &str,
    invited_by: &str,
    locale: &unic_langid::LanguageIdentifier,
) -> Result<OutboundEmail, DieselError> {
    let row = prepare_invitation(
        svc,
        branding,
        recipient,
        user_name,
        invitation_token,
        invited_by,
        locale,
    );
    outbound_emails::enqueue_idempotent(conn, row)
}

/// Build the `NewOutboundEmail` row for a notification send.
/// See `prepare_password_reset` for the rationale.
#[allow(clippy::too_many_arguments)]
pub fn prepare_notification(
    svc: &EmailService,
    branding: &EmailBranding,
    recipient: &str,
    subject: &str,
    title: &str,
    body: &str,
    actor_name: &str,
    cta_url: &str,
    event_id: &str,
    recipient_uuid: &str,
    locale: &unic_langid::LanguageIdentifier,
) -> NewOutboundEmail {
    let (body_html, body_text) = svc.compose_notification(
        title,
        body,
        actor_name,
        cta_url,
        branding,
        locale,
    );
    let message_id = make_message_id("notify", &from_email_domain(svc));
    // Notification emails are system-generated but represent a
    // human-authored underlying event (a comment a person wrote).
    // The research recommendation is NOT to mark these
    // Auto-Submitted: doing so makes Gmail treat them as bot
    // traffic and reduces engagement scoring. Keep them
    // person-to-person-shaped.
    let headers_json = serde_json::json!({});

    NewOutboundEmail {
        channel_id: None,
        ticket_id: None,
        comment_id: None,
        recipient: recipient.to_string(),
        subject: subject.to_string(),
        body_text,
        body_html: Some(body_html),
        message_id,
        in_reply_to: None,
        references_list: vec![],
        headers_json,
        correlation_id: None,
        idempotency_key: Some(format!("notify:{event_id}:{recipient_uuid}")),
    }
}

/// Enqueue a ticket-activity notification email. The key includes
/// the recipient so multi-recipient fanout (a comment with N
/// watchers) produces N rows rather than collapsing; it also
/// includes the source event id so the same (event, recipient)
/// pair is exactly-once even across retries.
#[allow(clippy::too_many_arguments)]
pub fn enqueue_notification(
    conn: &mut DbConnection,
    svc: &EmailService,
    branding: &EmailBranding,
    recipient: &str,
    subject: &str,
    title: &str,
    body: &str,
    actor_name: &str,
    cta_url: &str,
    event_id: &str,
    recipient_uuid: &str,
    locale: &unic_langid::LanguageIdentifier,
) -> Result<OutboundEmail, DieselError> {
    let row = prepare_notification(
        svc,
        branding,
        recipient,
        subject,
        title,
        body,
        actor_name,
        cta_url,
        event_id,
        recipient_uuid,
        locale,
    );
    outbound_emails::enqueue_idempotent(conn, row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::email::{EmailBranding, EmailConfig, EmailService, SmtpSecurity};
    use std::str::FromStr;
    use unic_langid::LanguageIdentifier;

    fn en_us() -> LanguageIdentifier {
        LanguageIdentifier::from_str("en-US").unwrap()
    }

    fn test_svc() -> EmailService {
        EmailService::new(EmailConfig {
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_username: "u".into(),
            smtp_password: "p".into(),
            from_name: "Nosdesk".into(),
            from_email: "noreply@nosdesk.test".into(),
            enabled: true,
            security: SmtpSecurity::StartTls,
        })
    }

    fn test_branding() -> EmailBranding {
        EmailBranding::new(
            "Nosdesk".to_string(),
            None,
            Some("#2563eb".to_string()),
            "https://desk.example.com".to_string(),
        )
    }

    #[test]
    fn hash16_is_stable_and_short() {
        let h = hash16("hello");
        assert_eq!(h.len(), 16);
        assert_eq!(h, hash16("hello"));
        assert_ne!(h, hash16("world"));
    }

    #[test]
    fn hash16_only_returns_hex() {
        let h = hash16("any input string here");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn message_id_is_unique_per_call() {
        let a = make_message_id("password-reset", "example.com");
        let b = make_message_id("password-reset", "example.com");
        assert_ne!(a, b, "random tail must keep IDs unique per call");
        assert!(a.starts_with("password-reset."));
        assert!(a.ends_with("@example.com"));
    }

    // ---------- snapshot assertions on transactional row shape ----------
    //
    // These tests lock the externally-visible properties of each
    // transactional email so the producer side of the queue can't
    // silently regress on deliverability fundamentals. Specifically:
    //   - hand-authored plain-text body present (so the SMTP send
    //     path can emit multipart/alternative; an empty text part
    //     scores as spam at Gmail/Yahoo)
    //   - branded HTML body with a preview-text (preheader) div so
    //     the inbox snippet isn't the raw "<!DOCTYPE html>..."
    //   - Auto-Submitted: auto-generated on system-shaped mail
    //     (password reset, invitation) per RFC 3834, but absent on
    //     notification mail (which represents a human event)
    //   - idempotency key namespaced and derived from a non-leaky
    //     token hash where a token is the source
    //   - channel_id is None (these aren't channel replies)
    //
    // List-Unsubscribe and dark-mode color-scheme meta land in
    // Phase 2 / 3 of the polish plan; assertions for those will
    // be added alongside their implementation.

    #[test]
    fn password_reset_row_has_loop_break_header_and_idempotency() {
        let row = prepare_password_reset(
            &test_svc(),
            &test_branding(),
            "alice@example.com",
            "Alice",
            "raw-token-abc123",
            &en_us(),
        );

        assert_eq!(row.recipient, "alice@example.com");
        assert!(row.channel_id.is_none(), "transactional rows have no channel");
        assert!(row.subject.contains("Reset"));
        assert!(
            row.subject.contains("Nosdesk"),
            "subject should carry workspace name: {}",
            row.subject
        );

        assert_eq!(
            row.headers_json.get("Auto-Submitted").and_then(|v| v.as_str()),
            Some("auto-generated"),
            "password reset must carry RFC 3834 loop-break header"
        );

        let key = row.idempotency_key.as_ref().expect("idempotency key set");
        assert!(key.starts_with("password_reset:"), "namespaced: {key}");
        assert!(
            !key.contains("raw-token-abc123"),
            "raw token must not appear in the key (privacy)"
        );

        assert!(row.message_id.starts_with("password-reset."));
        assert!(row.message_id.ends_with("@nosdesk.test"));

        let html = row.body_html.as_ref().expect("html body set");
        assert!(
            html.contains("display: none"),
            "preheader (hidden preview text) must be present"
        );
        assert!(html.contains("reset-password?token=raw-token-abc123"));
        assert!(html.contains("Alice"), "user name appears in greeting");

        assert!(!row.body_text.trim().is_empty(), "plain-text alt non-empty");
        assert!(
            row.body_text.contains("reset-password?token=raw-token-abc123"),
            "plain-text alt carries the link as a bare URL: {}",
            row.body_text
        );
        assert!(row.body_text.contains("Alice"));
    }

    #[test]
    fn invitation_row_has_loop_break_header_and_idempotency() {
        let row = prepare_invitation(
            &test_svc(),
            &test_branding(),
            "bob@example.com",
            "Bob",
            "invite-token-xyz",
            "Kyle",
            &en_us(),
        );

        assert!(row.channel_id.is_none());
        assert!(row.subject.contains("Invited"));

        assert_eq!(
            row.headers_json.get("Auto-Submitted").and_then(|v| v.as_str()),
            Some("auto-generated"),
            "invitations must carry RFC 3834 loop-break header"
        );

        let key = row.idempotency_key.as_ref().expect("idempotency key set");
        assert!(key.starts_with("invitation:"));
        assert!(!key.contains("invite-token-xyz"));

        assert!(row.message_id.starts_with("invitation."));

        let html = row.body_html.as_ref().expect("html body set");
        assert!(html.contains("display: none"), "preheader present");
        assert!(html.contains("accept-invitation?token=invite-token-xyz"));
        assert!(html.contains("Bob"));
        assert!(html.contains("Kyle"), "inviter name appears");

        assert!(!row.body_text.trim().is_empty());
        assert!(row.body_text.contains("accept-invitation?token=invite-token-xyz"));
    }

    #[test]
    fn notification_row_omits_loop_break_header() {
        let row = prepare_notification(
            &test_svc(),
            &test_branding(),
            "carol@example.com",
            "[Nosdesk] New comment on: Printer fire",
            "New comment on: Printer fire",
            "It's still burning.",
            "Kyle",
            "https://desk.example.com/tickets/42",
            "notif-uuid-1",
            "user-uuid-9",
            &en_us(),
        );

        assert!(row.channel_id.is_none());
        assert_eq!(row.subject, "[Nosdesk] New comment on: Printer fire");

        // Notifications carry a real human event — sending Auto-
        // Submitted makes Gmail rank them as bot traffic.
        assert!(
            row.headers_json.get("Auto-Submitted").is_none(),
            "notification mail must NOT carry Auto-Submitted: {:?}",
            row.headers_json
        );

        let key = row.idempotency_key.as_ref().expect("idempotency key set");
        assert_eq!(key, "notify:notif-uuid-1:user-uuid-9");

        assert!(row.message_id.starts_with("notify."));

        let html = row.body_html.as_ref().expect("html body set");
        assert!(html.contains("display: none"), "preheader present");
        assert!(html.contains("https://desk.example.com/tickets/42"));
        assert!(html.contains("Kyle"), "actor name appears");
        assert!(html.contains("It&#x27;s still burning.") || html.contains("It's still burning."),
            "body content rendered (possibly HTML-escaped)");

        assert!(!row.body_text.trim().is_empty());
        assert!(row.body_text.contains("It's still burning."));
        assert!(row.body_text.contains("https://desk.example.com/tickets/42"));
    }

    #[test]
    fn password_reset_idempotency_collapses_repeat_enqueues() {
        // Same token → same key. The DB unique index catches the
        // dupe; this just confirms the producer side hands the
        // queue identical keys for identical tokens.
        let r1 = prepare_password_reset(
            &test_svc(),
            &test_branding(),
            "alice@example.com",
            "Alice",
            "same-token",
            &en_us(),
        );
        let r2 = prepare_password_reset(
            &test_svc(),
            &test_branding(),
            "alice@example.com",
            "Alice",
            "same-token",
            &en_us(),
        );
        assert_eq!(r1.idempotency_key, r2.idempotency_key);
        // ...but message_id rotates so a retried send doesn't
        // confuse the recipient's MUA dedupe.
        assert_ne!(r1.message_id, r2.message_id);
    }
}
