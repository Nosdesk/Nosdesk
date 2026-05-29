//! Auto-acknowledgement on channel-opened tickets.
//!
//! When a new ticket opens via an inbound channel message, send a
//! single system-authored reply back to the customer:
//!
//! ```text
//! Thanks, we got your message. Reference: #123. Reply to this
//! email if you have more to add; we'll get back to you soon.
//! ```
//!
//! Standard helpdesk behaviour (Zendesk, Freshdesk, Help Scout all
//! ship it on by default). It sets the customer's expectation that
//! the request was received without waiting for a human response.
//!
//! # Design
//!
//! - Enabled / template stored on `site_settings` so an admin can
//!   customise wording without a redeploy. `None` template → the
//!   built-in [`DEFAULT_TEMPLATE`] is used.
//! - Fire-and-forget from the pipeline: a failure to send never
//!   blocks ticket creation.
//! - The auto-ack is NOT a ticket comment — it's a system-authored
//!   outbound that gets recorded in `channel_messages` with
//!   `comment_id = NULL`. This means:
//!     * it doesn't clutter the tech-facing comment timeline,
//!     * the customer's reply still threads back correctly via
//!       References → our auto-ack Message-ID → the ticket.
//! - Template substitution is plain string replacement of
//!   `{{variable}}` tokens — no Handlebars, no HTML injection risk
//!   because we render as plain text.

use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::db::Pool;
use crate::models::{Channel, NewChannelMessage, SiteSettings, Ticket, CHANNEL_DIRECTION_OUTBOUND};
use crate::repository::{
    channels as channels_repo, site_settings as site_settings_repo, user_helpers,
};
use crate::services::channels::threading::{format_outbound_message_id, format_outbound_subject};
use crate::utils::email::{EmailService, OutboundEmailMessage};

/// Built-in fallback template, retained as a compile-time
/// constant for tests that snapshot the wording. At runtime the
/// `auto-ack-default-template` FTL key is used so the message
/// arrives in the customer's language (driven by their inbound
/// Content-Language header). When an admin sets
/// `channel_auto_ack_template` on site_settings, their wording
/// wins outright; localisation is bypassed in that case because
/// the admin's custom copy is the source of truth.
pub const DEFAULT_TEMPLATE: &str = "Your request (#{{ticket_id}}) has been received and is being reviewed by our support team. To add additional comments, reply to this email.";

/// Fire-and-forget entry point used by the pipeline. Detached so a
/// slow SMTP round-trip never blocks the inbound ingestion.
///
/// `in_reply_to` must be the customer's original Message-ID (with
/// angle brackets). It goes into the auto-ack's `In-Reply-To` +
/// `References` headers so the recipient's mail client threads the
/// conversation correctly.
pub fn spawn_auto_ack(
    pool: Pool,
    email: Arc<EmailService>,
    channel: Channel,
    ticket: Ticket,
    in_reply_to: String,
    inbound_locale: Option<String>,
) {
    tokio::spawn(async move {
        if let Err(e) = send_auto_ack(
            &pool,
            &email,
            &channel,
            &ticket,
            &in_reply_to,
            inbound_locale.as_deref(),
        )
        .await
        {
            warn!(
                channel_id = channel.id,
                ticket_id = ticket.id,
                error = %e,
                "auto-ack send failed; ticket is unaffected"
            );
        }
    });
}

async fn send_auto_ack(
    pool: &Pool,
    email: &EmailService,
    channel: &Channel,
    ticket: &Ticket,
    in_reply_to: &str,
    inbound_locale: Option<&str>,
) -> Result<(), String> {
    // Load site_settings (RLS) + recipient lookup in one bypass
    // txn. The auto-ack runs from the channel pipeline which has
    // no request-bound workspace pin.
    let (settings, recipient_email, customer_name) = {
        let requester_uuid = ticket
            .requester_uuid
            .ok_or_else(|| "ticket has no requester".to_string())?;
        crate::sync::session::background_run(pool, "background:auto_ack_prep", |conn| {
            let settings = site_settings_repo::get_site_settings(conn)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
            let email =
                user_helpers::get_primary_email(&requester_uuid, conn).ok_or_else(|| {
                    diesel::result::Error::QueryBuilderError(
                        "requester has no primary email".into(),
                    )
                })?;
            let name = crate::repository::users::get_user_by_uuid(&requester_uuid, conn)
                .map(|u| u.name)
                .unwrap_or_else(|_| email.clone());
            Ok::<_, diesel::result::Error>((settings, email, name))
        })
        .map_err(|e| format!("auto-ack prep: {e}"))?
    };

    if !settings.channel_auto_ack_enabled {
        debug!(
            ticket_id = ticket.id,
            "auto-ack disabled in site_settings; skipping"
        );
        return Ok(());
    }
    if channel.provider != "email_imap" {
        // Only email knows how to deliver one today; chat adapters
        // might have their own "we got it" UX (Slack ephemeral, etc).
        debug!(provider = %channel.provider, "auto-ack not supported for this provider");
        return Ok(());
    }

    // Render template. Admin-customised wording wins outright;
    // the inbound's Content-Language only drives the *default*
    // template's localisation (we have one canonical FTL key per
    // locale for the built-in copy, but no machine translation of
    // arbitrary admin text). When no admin override is set,
    // resolve the locale via inbound -> site default -> en-US.
    let body = match settings.channel_auto_ack_template.as_deref() {
        Some(custom) => render_template(custom, &settings, ticket, &customer_name),
        None => {
            let locale =
                crate::utils::locale::effective_locale(inbound_locale, &settings.default_locale);
            let default_localised = crate::utils::i18n::tr_with(
                &locale,
                "auto-ack-default-template",
                &[
                    ("ticket_id", ticket.id.to_string().into()),
                    ("ticket_title", ticket.title.clone().into()),
                    ("customer_name", customer_name.clone().into()),
                    ("app_name", settings.app_name.clone().into()),
                ],
            );
            // The FTL value carries its own placeholders pre-
            // substituted, but admins may have copy/pasted the
            // legacy `{{var}}` syntax into a custom template that
            // later got cleared — running render_template here as
            // a no-op keeps both shapes safe.
            render_template(&default_localised, &settings, ticket, &customer_name)
        }
    };

    // Build outbound email. The Message-ID is stamped by the threading
    // helper so the recipient's reply matches back to this ticket via
    // the References cascade (step 1 — References chain).
    let config: crate::services::channels::email_imap::ImapChannelConfig =
        serde_json::from_value(channel.config.clone())
            .map_err(|e| format!("parse channel.config: {e}"))?;
    let message_id = format_outbound_message_id(ticket.id, 0, &config.reply_domain);
    let subject = format_outbound_subject(ticket.id, &ticket.title);
    let references = vec![in_reply_to.to_string()];

    let outbound = OutboundEmailMessage {
        to: &recipient_email,
        subject: &subject,
        body_text: &body,
        body_html: None,
        message_id: &message_id,
        in_reply_to: Some(in_reply_to),
        references: &references,
        // This IS the auto-acknowledgement — emit RFC 3834 headers so
        // a customer OOO doesn't ping-pong with us.
        auto_submitted: true,
    };
    email
        .send_ticket_reply(outbound)
        .await
        .map_err(|e| format!("smtp: {e}"))?;

    // Record the outbound so a customer reply threads back. Comment_id
    // is NULL by design — auto-ack is system-authored, not a ticket
    // comment. channel_messages is RLS-enabled; same bypass story.
    crate::sync::session::background_run(pool, "background:auto_ack_record", |conn| {
        channels_repo::record_message(
            conn,
            NewChannelMessage {
                channel_id: channel.id,
                external_id: format!("<{message_id}>"),
                direction: CHANNEL_DIRECTION_OUTBOUND.to_string(),
                ticket_id: Some(ticket.id),
                comment_id: None,
                in_reply_to: Some(in_reply_to.to_string()),
                from_address: None,
                author_user_uuid: None,
                raw_metadata: Some(serde_json::json!({ "auto_ack": true })),
            },
        )
    })
    .map_err(|e| format!("record channel_messages: {e}"))?;

    info!(
        channel_id = channel.id,
        ticket_id = ticket.id,
        recipient = %recipient_email,
        "auto-ack sent"
    );
    Ok(())
}

/// Minimal `{{variable}}` substitution. Not Handlebars — we only need
/// 4 tokens and the template is plain text. Unknown tokens are left
/// intact rather than erroring; admins editing the template will
/// spot their own typos.
fn render_template(
    template: &str,
    settings: &SiteSettings,
    ticket: &Ticket,
    customer_name: &str,
) -> String {
    template
        .replace("{{ticket_id}}", &ticket.id.to_string())
        .replace("{{ticket_title}}", &ticket.title)
        .replace("{{customer_name}}", customer_name)
        .replace("{{app_name}}", &settings.app_name)
}

#[cfg(test)]
mod tests {
    //! Template render coverage; the SMTP + DB integration path is
    //! exercised in the Greenmail E2E tests via the outbound relay.
    //! Here we just verify the substitution is correct.
    use super::*;
    use chrono::Utc;

    fn sample_settings() -> SiteSettings {
        SiteSettings {
            id: 1,
            app_name: "Nosdesk".into(),
            logo_url: None,
            logo_light_url: None,
            favicon_url: None,
            primary_color: None,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            updated_by: None,
            guest_tickets_enabled: false,
            guest_public_docs_enabled: false,
            guest_kb_search_enabled: false,
            guest_ticket_lookup_enabled: false,
            guest_help_page_enabled: false,
            guest_ticket_default_priority: None,
            guest_ticket_rate_limit_per_hour: 5,
            guest_ticket_email_verification: false,
            guest_ticket_attachments_enabled: false,
            guest_ticket_intro_message: None,
            channel_auto_ack_enabled: true,
            channel_auto_ack_template: None,
            feature_flags: serde_json::json!({}),
            default_locale: "en-US".into(),
            default_timezone: "UTC".into(),
            workspace_id: 1,
            signature_default: None,
        }
    }

    fn sample_ticket() -> Ticket {
        Ticket {
            id: 42,
            title: "Printer on fire".into(),
            workflow_state_id: 2, // seeded Backlog row
            priority: crate::models::TicketPriority::Medium,
            requester_uuid: None,
            assignee_uuid: None,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            created_by: None,
            closed_at: None,
            closed_by: None,
            category_id: None,
            submitted_via: Some("email_imap".into()),
            guest_lookup_token: None,
            verification_state: None,
            origin_channel_id: None,
            triage_state: None,
            due_date: None,
            recurrence_rule: None,
            recurrence_template_id: None,
            resolution_notes: None,
            workspace_id: 1,
            first_response_at: None,
            sla_response_target_at: None,
            sla_response_breached_at: None,
            sla_resolution_target_at: None,
            sla_resolution_breached_at: None,
        }
    }

    #[test]
    fn default_template_renders_all_tokens() {
        let settings = sample_settings();
        let ticket = sample_ticket();
        let out = render_template(DEFAULT_TEMPLATE, &settings, &ticket, "Alice");
        // Default is Zendesk-style terse: single sentence, ticket
        // reference up front, reply-by-email hint.
        assert!(out.contains("#42"));
        assert!(out.to_lowercase().contains("reply to this email"));
        // No unsubstituted tokens left.
        assert!(!out.contains("{{"));
    }

    #[test]
    fn custom_template_honoured() {
        let settings = sample_settings();
        let ticket = sample_ticket();
        let out = render_template(
            "#{{ticket_id}} — {{ticket_title}} from {{customer_name}}",
            &settings,
            &ticket,
            "Bob",
        );
        assert_eq!(out, "#42 — Printer on fire from Bob");
    }

    #[test]
    fn unknown_tokens_left_intact() {
        // Admin typos shouldn't explode the send — we render whatever
        // they wrote so they notice it in their inbox test.
        let settings = sample_settings();
        let ticket = sample_ticket();
        let out = render_template(
            "ref={{ticket_id}} unknown={{does_not_exist}}",
            &settings,
            &ticket,
            "Alice",
        );
        assert_eq!(out, "ref=42 unknown={{does_not_exist}}");
    }
}
