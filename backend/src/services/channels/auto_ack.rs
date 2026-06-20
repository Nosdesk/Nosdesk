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
use crate::services::outbound_email::OutboundEmailResolver;
use crate::utils::email::OutboundEmailMessage;

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
    resolver: Arc<OutboundEmailResolver>,
    channel: Channel,
    ticket: Ticket,
    in_reply_to: String,
    inbound_locale: Option<String>,
) {
    tokio::spawn(async move {
        if let Err(e) = send_auto_ack(
            &pool,
            &resolver,
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
    resolver: &OutboundEmailResolver,
    channel: &Channel,
    ticket: &Ticket,
    in_reply_to: &str,
    inbound_locale: Option<&str>,
) -> Result<(), String> {
    // Load site_settings (RLS) + recipient lookup, pinned to the ticket's
    // workspace. The auto-ack runs from a detached spawn with no request
    // context, so it must establish the workspace itself; run_in_workspace
    // (non-bypass) both scopes the RLS-isolated site_settings read to THIS
    // workspace and sets the GUC the later record write needs.
    let (settings, recipient_email, customer_name, routing) = {
        let requester_uuid = ticket
            .requester_uuid
            .ok_or_else(|| "ticket has no requester".to_string())?;
        crate::sync::session::run_in_workspace(
            pool,
            "background:auto_ack_prep",
            ticket.workspace_id,
            |conn| {
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
                // Where the customer's reply to this ack should thread back —
                // the same provider-aware routing an agent reply uses.
                let routing = super::outbound::reply_routing(conn, channel);
                Ok::<_, diesel::result::Error>((settings, email, name, routing))
            },
        )
        .map_err(|e| format!("auto-ack prep: {e}"))?
    };

    if !settings.channel_auto_ack_enabled {
        debug!(
            ticket_id = ticket.id,
            "auto-ack disabled in site_settings; skipping"
        );
        return Ok(());
    }
    // No reply route means no email channel to deliver an ack through (chat
    // adapters get their own "we got it" UX). `reply_domain` stamps the
    // Message-ID; `reply_to` threads the customer's reply back.
    let Some((reply_domain, reply_to)) = routing else {
        debug!(provider = %channel.provider, "auto-ack: channel has no reply route; skipping");
        return Ok(());
    };

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
    let message_id = format_outbound_message_id(ticket.id, 0, &reply_domain);
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
        // This IS the auto-acknowledgement: a direct reply, so RFC 3834
        // marks it auto-replied and a customer OOO won't ping-pong with us.
        auto_submitted: Some("auto-replied"),
        // Conversation mail to the customer about their own ticket: transactional.
        mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL,
        // Thread the reply back: the IMAP polled mailbox or the forwarding
        // address, depending on the channel.
        reply_to: reply_to.as_deref(),
        // Direct send (not queued), so there's no outbound row to encode a
        // VERP token against; bounces fall back to Message-ID correlation.
        envelope_from: None,
        list_unsubscribe: None,
    };

    // Don't auto-reply to an address that previously hard-bounced or complained:
    // it would re-bounce and erode the shared relay's reputation. The queued
    // paths check this in the worker; this direct send must too. Silent skip is
    // correct here, auto-ack is best-effort system mail, not a human's reply.
    if crate::services::outbound_email::recipient_is_suppressed(pool, &recipient_email) {
        tracing::info!(
            recipient = %recipient_email,
            ticket = ticket.id,
            "skipping auto-ack: recipient is on the suppression list"
        );
        return Ok(());
    }

    // Resolve the sending identity for this workspace (its own SMTP, or the
    // env fallback). Done here, after the enabled / provider guards, so a
    // disabled auto-ack never resolves and an unconfigured identity surfaces
    // as a logged send failure rather than silently using the wrong From.
    let service = resolver
        .resolve_owned(ticket.workspace_id)
        .map_err(|e| format!("resolve sender identity: {e}"))?;
    service
        .send_ticket_reply(outbound)
        .await
        .map_err(|e| format!("smtp: {e}"))?;

    // Record the outbound so a customer reply threads back. Comment_id
    // is NULL by design: auto-ack is system-authored, not a ticket comment.
    // channel_messages is workspace-isolated and its workspace_id column
    // defaults from app.workspace_id, so this runs pinned to the ticket's
    // workspace (run_in_workspace) or the insert writes a NULL workspace_id.
    crate::sync::session::run_in_workspace(
        pool,
        "background:auto_ack_record",
        ticket.workspace_id,
        |conn| {
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
        },
    )
    .map_err(|e| format!("record channel_messages: {e}"))?;

    info!(
        channel_id = channel.id,
        ticket_id = ticket.id,
        recipient = %recipient_email,
        "auto-ack sent"
    );
    Ok(())
}

/// `{{variable}}` substitution backed by the shared renderer in
/// `utils::template_variables`. Whitespace inside braces is
/// tolerated (`{{ ticket_id }}` works the same as `{{ticket_id}}`),
/// matching canned-response + signature behaviour. Unknown tokens
/// are left intact rather than erroring; admin-saved templates are
/// validated up-front at the handler boundary so the only way an
/// unknown token reaches this renderer is the built-in FTL default,
/// which is curated.
fn render_template(
    template: &str,
    settings: &SiteSettings,
    ticket: &Ticket,
    customer_name: &str,
) -> String {
    let ticket_id = ticket.id.to_string();
    let customer_first_name = crate::utils::template_variables::first_name(customer_name);
    crate::utils::template_variables::substitute(
        template,
        &[
            ("ticket_id", &ticket_id),
            ("ticket_title", &ticket.title),
            ("customer_name", customer_name),
            ("customer_first_name", &customer_first_name),
            ("app_name", &settings.app_name),
        ],
    )
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
            email_security_note_enabled: false,
            email_security_note_template: None,
        }
    }

    fn sample_ticket() -> Ticket {
        Ticket {
            id: 42,
            uuid: uuid::Uuid::nil(),
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
            spam_suspected: false,
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
