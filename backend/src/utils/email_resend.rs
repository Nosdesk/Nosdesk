//! Resend transport for the pluggable email layer.
//!
//! Implements [`EmailTransport`](super::email::EmailTransport) against the
//! Resend HTTP API (official `resend-rs` SDK). Selected when
//! `EMAIL_PROVIDER=resend` (or `RESEND_API_KEY` is set); SMTP stays the
//! default. Unlike SMTP, a send returns the provider's own message id
//! (`email_id`), surfaced via [`SendOutcome::provider_message_id`] so the
//! delivery/bounce/complaint webhook can correlate back to the row.
//!
//! Composition (subject, html, text, threading headers) is built by
//! `EmailService`; this transport only maps the provider-agnostic
//! [`OutboundEmailMessage`] onto a Resend request.

use async_trait::async_trait;
use resend_rs::{types::CreateEmailBaseOptions, Resend};

use super::email::{EmailTransport, OutboundEmailMessage, SendOutcome};

/// Resend API transport. Holds the SDK client plus the configured
/// sender. `enabled` gates sends the same way SMTP's `is_configured`
/// does, so a half-configured provider refuses rather than silently
/// dropping mail.
pub struct ResendEmailTransport {
    client: Resend,
    /// Pre-formatted `Name <email>` sender string.
    from: String,
    enabled: bool,
}

impl ResendEmailTransport {
    pub fn new(api_key: String, from_name: String, from_email: String) -> Self {
        let enabled = !api_key.is_empty() && !from_email.is_empty();
        Self {
            client: Resend::new(&api_key),
            from: format!("{from_name} <{from_email}>"),
            enabled,
        }
    }
}

#[async_trait]
impl EmailTransport for ResendEmailTransport {
    async fn send(&self, msg: &OutboundEmailMessage<'_>) -> Result<SendOutcome, String> {
        if !self.enabled {
            return Err("Email is not configured".to_string());
        }

        // `to` takes any IntoIterator<Item: Into<String>>; a single-element
        // array is the one-recipient case.
        let mut opts = CreateEmailBaseOptions::new(self.from.clone(), [msg.to], msg.subject)
            .with_text(msg.body_text)
            // Carry the RFC Message-ID so threading + our DSN/bounce
            // correlation keep working alongside Resend's own id.
            .with_header("Message-ID", &format!("<{}>", msg.message_id));

        if let Some(html) = msg.body_html {
            opts = opts.with_html(html);
        }
        if let Some(parent) = msg.in_reply_to {
            opts = opts.with_header("In-Reply-To", parent);
        }
        if !msg.references.is_empty() {
            opts = opts.with_header("References", &msg.references.join(" "));
        }
        // RFC 3834 + Exchange loop-prevention headers for auto-replies.
        if msg.auto_submitted {
            opts = opts
                .with_header("Auto-Submitted", "auto-replied")
                .with_header("X-Auto-Response-Suppress", "All");
        }

        let resp = self
            .client
            .emails
            .send(opts)
            .await
            .map_err(|e| format!("Failed to send via Resend: {e}"))?;

        Ok(SendOutcome {
            provider_message_id: Some(resp.id.to_string()),
        })
    }

    fn is_configured(&self) -> bool {
        self.enabled
    }

    fn provider_name(&self) -> &'static str {
        "resend"
    }
}
