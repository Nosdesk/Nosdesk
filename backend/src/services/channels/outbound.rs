//! Outbound dispatcher.
//!
//! Counterpart to [`super::pipeline`]: given a
//! [`super::relay::RelayDecision::Relay`] and the comment content,
//! pick the right adapter, call `send_reply`, and persist the
//! outbound `channel_messages` row so the next inbound reply can
//! thread back.
//!
//! The dispatcher is intentionally a plain function rather than a
//! method on a struct — callers (comment-creation handler today,
//! admin test-connection tomorrow) pass the handles they already
//! have on their stack.
//!
//! Provider dispatch is hardcoded here for phase 1. When the registry
//! (task #16) lands this module switches to looking the adapter up
//! by `channel.id` instead of rebuilding it per call.

use std::sync::Arc;

use tracing::{info, warn};

use crate::db::DbConnection;
use crate::models::{
    Channel, NewChannelMessage, CHANNEL_DIRECTION_OUTBOUND, CHANNEL_PROVIDER_EMAIL_FORWARD,
    CHANNEL_PROVIDER_EMAIL_IMAP, INBOUND_ADDRESS_STATUS_ACTIVE,
};
use crate::repository::channels as channels_repo;
use crate::repository::inbound_addresses;
use crate::services::channels::email_imap::{build_email_imap_adapter, ImapChannelConfig};
use crate::services::channels::threading::format_outbound_message_id;
use crate::services::channels::{
    ChannelAdapter, ChannelError, OutboundContent, OutboundMessage, ThreadContext,
};
use crate::utils::email::EmailService;

#[derive(Debug)]
pub enum DispatchError {
    UnsupportedProvider(String),
    BadChannelConfig(String),
    Send(ChannelError),
    Db(diesel::result::Error),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedProvider(p) => write!(f, "unsupported provider: {p}"),
            Self::BadChannelConfig(m) => write!(f, "bad channel config: {m}"),
            Self::Send(e) => write!(f, "send failed: {e}"),
            Self::Db(e) => write!(f, "db error: {e}"),
        }
    }
}
impl std::error::Error for DispatchError {}
impl From<diesel::result::Error> for DispatchError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Db(e)
    }
}
impl From<ChannelError> for DispatchError {
    fn from(e: ChannelError) -> Self {
        Self::Send(e)
    }
}

/// Send an outbound message and record it. Returns the
/// [`OutboundMessage`] as emitted by the adapter (contains the
/// external-id our future inbound reply will thread on).
///
/// `comment_id` threads the Nosdesk comment row through into the
/// `Message-ID` format so inbound replies routed via our own id
/// shape (the third step of the threading cascade) can match back.
pub async fn send_and_record(
    channel: &Channel,
    thread: ThreadContext,
    mut content: OutboundContent,
    comment_id: i32,
    email: Arc<EmailService>,
    pool: crate::db::Pool,
    conn: &mut DbConnection,
) -> Result<OutboundMessage, DispatchError> {
    // Build the provider-specific external-id hint *before* calling the
    // adapter, so the adapter doesn't invent a different one and we
    // record the same id we sent on the wire.
    let adapter: Box<dyn ChannelAdapter> = match channel.provider.as_str() {
        "email_imap" => {
            // Stamp the Message-ID *before* constructing the adapter so
            // the send path doesn't have to reach back into config for
            // the reply domain.
            let config: ImapChannelConfig = serde_json::from_value(channel.config.clone())
                .map_err(|e| DispatchError::BadChannelConfig(e.to_string()))?;
            content.external_id_hint = Some(format_outbound_message_id(
                thread.ticket_id,
                comment_id,
                &config.reply_domain,
            ));
            Box::new(
                build_email_imap_adapter(channel, email, pool)
                    .map_err(DispatchError::BadChannelConfig)?,
            )
        }
        other => return Err(DispatchError::UnsupportedProvider(other.to_string())),
    };

    let sent = adapter.send_reply(&thread, &content).await?;

    channels_repo::record_message(
        conn,
        NewChannelMessage {
            channel_id: channel.id,
            external_id: sent.external_id.clone(),
            direction: CHANNEL_DIRECTION_OUTBOUND.into(),
            ticket_id: Some(thread.ticket_id),
            comment_id: Some(comment_id),
            in_reply_to: thread.external_thread_id.clone(),
            from_address: None,
            author_user_uuid: None,
            raw_metadata: sent.raw_metadata.clone(),
        },
    )?;

    info!(
        channel_id = channel.id,
        provider = %channel.provider,
        ticket_id = thread.ticket_id,
        comment_id,
        external_id = %sent.external_id,
        "dispatched outbound channel message"
    );

    Ok(sent)
}

/// Reply routing for a channel: the Message-ID domain plus the optional
/// `Reply-To` the customer's reply should target so it threads back onto the
/// ticket. `None` means we can't route a reply (bad config / no forwarding
/// address), so the caller skips the enqueue.
///
/// - `email_imap`: thread back via the polled mailbox (`Reply-To` = the IMAP
///   username when it's an address).
/// - `email_forward`: thread back via the generated forwarding address, so the
///   customer's reply re-enters through SES inbound and threads.
/// - `email_managed`: no `Reply-To` — the resolved From
///   (`support@<slug>.<tenant_domain>`) IS the receivable address, and a
///   Reply-To duplicating From is noise some filters penalise. The Message-ID
///   domain is the workspace's mail host so the threading cascade matches.
///
/// Shared with [`super::auto_ack`] so the acknowledgement threads back the same
/// way an agent's reply does.
pub(crate) fn reply_routing(
    conn: &mut DbConnection,
    channel: &Channel,
) -> Option<(String, Option<String>)> {
    if channel.provider == CHANNEL_PROVIDER_EMAIL_IMAP {
        let config: ImapChannelConfig = serde_json::from_value(channel.config.clone()).ok()?;
        let reply_to = config
            .username
            .contains('@')
            .then(|| config.username.clone());
        Some((config.reply_domain, reply_to))
    } else if channel.provider == CHANNEL_PROVIDER_EMAIL_FORWARD {
        let domain = std::env::var("NOSDESK_INBOUND_DOMAIN")
            .ok()
            .filter(|d| !d.is_empty())?;
        let address = inbound_addresses::list_for_channel(conn, channel.id)
            .ok()?
            .into_iter()
            .find(|a| a.status == INBOUND_ADDRESS_STATUS_ACTIVE)?;
        let forwarding_address = format!("{}@{}", address.token, domain);
        Some((domain, Some(forwarding_address)))
    } else if channel.provider == crate::models::CHANNEL_PROVIDER_EMAIL_MANAGED {
        let tenant_domain = crate::utils::tenant_origin::tenant_domain()?;
        let workspace =
            crate::repository::workspaces::find_by_id(conn, channel.workspace_id).ok()??;
        Some((format!("{}.{}", workspace.slug, tenant_domain), None))
    } else {
        None
    }
}

/// Spawn a detached task that composes the outbound reply for a
/// freshly-created comment and enqueues it on the durable
/// `outbound_emails` queue (Item J Pass 1). The actual SMTP send
/// happens later in `services::email_queue::worker`, with retry,
/// idempotency, and crash recovery.
///
/// What changed vs. the old `spawn_relay_for_comment`:
///   - This task no longer talks to SMTP. It composes the body, builds
///     the queue row (including the deterministic Message-ID stamped
///     once and reused on retry), and inserts it. The worker drains
///     the queue and dispatches.
///   - Failures during composition still hit the log only — the
///     comment is already persisted; we don't roll back on a relay
///     hiccup.
///
/// Why still `tokio::spawn`: composition involves DB reads (signature
/// lookup, quote-previous, channel config). Doing it synchronously in
/// the HTTP handler would slow down comment posting. The spawn body is
/// now milliseconds (no SMTP roundtrip), but it's still off the
/// critical path.
pub fn enqueue_for_comment(
    ticket: crate::models::Ticket,
    comment: crate::models::Comment,
    pool: crate::db::Pool,
) {
    let workspace_id = ticket.workspace_id;
    tokio::spawn(async move {
        // Everything inside this spawn is sync DB work — no awaits
        // between pool.get and the enqueue write — so the whole
        // channels, tickets, signatures (user prefs), outbound_emails are all
        // RLS-enabled. Run pinned as the RLS-enforced runtime role: the relay
        // runs from the comment-handler spawn with no request-bound pin, and
        // decide_relay reads those tenant tables — scoping them to the ticket's
        // workspace keeps the channel/thread resolution from seeing another
        // tenant's row, and the pin also supplies outbound_emails.workspace_id's
        // app.workspace_id default (else the insert writes NULL and fails NOT NULL).
        let result = crate::sync::session::run_in_workspace(
            &pool,
            "background:channel_relay_enqueue",
            workspace_id,
            |conn| {
                let decision = super::relay::decide_relay(conn, &ticket, &comment)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
                let (channel, thread) = match decision {
                    super::relay::RelayDecision::Relay { channel, thread } => (channel, thread),
                    other => {
                        tracing::debug!(decision = ?other, "channel relay: skipped");
                        return Ok(None);
                    }
                };

                // Compose the reply in both HTML and plaintext form
                // so the email worker can ship a real
                // multipart/alternative message. Final wire order
                // in either form:
                //   <tech's new reply>
                //   <signature>
                //   <quoted prior message>
                let body = super::reply_body::ReplyBody::from_comment(&comment);
                let body =
                    super::signature::append_signature_for_user(conn, comment.user_uuid, body);
                let body =
                    super::quote_previous::maybe_prepend_quote(conn, &channel, &ticket, body);

                let Some((reply_domain, reply_to)) = reply_routing(conn, &channel) else {
                    warn!(
                        channel_id = channel.id,
                        provider = %channel.provider,
                        "channel relay: no reply routing for channel; skipping enqueue"
                    );
                    return Ok(None);
                };
                let message_id =
                    format_outbound_message_id(thread.ticket_id, comment.id, &reply_domain);
                let subject = super::threading::format_outbound_subject(
                    thread.ticket_id,
                    thread.subject.as_deref().unwrap_or(""),
                );
                let recipient = thread
                    .recipient
                    .known_email
                    .clone()
                    .unwrap_or_else(|| thread.recipient.external_id.clone());

                // Point the customer's reply where it threads back: the IMAP
                // polled mailbox, or the forwarding address (re-ingested via
                // SES). Set even when the workspace sends From a different
                // (verified-domain) identity. Absent when there's no route.
                let headers_json = match &reply_to {
                    Some(addr) => serde_json::json!({ "Reply-To": addr }),
                    None => serde_json::json!({}),
                };

                let new_row = crate::models::NewOutboundEmail {
                    channel_id: Some(channel.id),
                    ticket_id: Some(thread.ticket_id),
                    comment_id: Some(comment.id),
                    recipient,
                    subject,
                    body_text: body.text,
                    body_html: Some(body.html),
                    message_id,
                    in_reply_to: thread.external_thread_id,
                    references_list: thread.references.into_iter().map(Some).collect(),
                    headers_json,
                    // Item S correlation_id flows in once the per-
                    // request context propagates through this far.
                    correlation_id: None,
                    idempotency_key: None,
                    sender_identity: crate::models::outbound_email_sender_identity::WORKSPACE
                        .to_string(),
                    // The agent's reply is conversation mail: workspace identity,
                    // but transactional (no List-Unsubscribe on a human reply).
                    mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL.to_string(),
                };

                let row = crate::repository::outbound_emails::enqueue_or_suppress(conn, new_row)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
                Ok::<_, diesel::result::Error>(Some((row.id, thread.ticket_id)))
            },
        );

        match result {
            Ok(Some((queue_id, ticket_id))) => {
                tracing::debug!(
                    queue_id,
                    ticket_id,
                    comment_id = comment.id,
                    "channel relay: enqueued for outbound dispatch"
                );
            }
            Ok(None) => {
                // skipped or bad-config — already logged inside the closure
            }
            Err(e) => {
                warn!(
                    error = %e,
                    comment_id = comment.id,
                    "channel relay: enqueue failed; comment saved but no reply sent"
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    //! The dispatcher is exercised via the end-to-end relay tests
    //! (task #23). Pure unit coverage here would require stubbing the
    //! SMTP send path, which the existing `email::tests` already do at
    //! the message-build level — duplicating that without real SMTP
    //! wouldn't add signal.
    //!
    //! What we *can* cover cheaply is the error branch: rejecting an
    //! unknown provider before any network work. This catches config
    //! typos in channel rows and keeps the dispatch function honest.

    use super::*;
    use crate::services::channels::{ExternalIdentity, OutboundContent};
    use crate::test_helpers::setup_test_connection;

    fn email_service_stub() -> Arc<EmailService> {
        Arc::new(EmailService::new(crate::utils::email::EmailConfig {
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_password: String::new(),
            from_name: String::new(),
            from_email: String::new(),
            enabled: false,
            security: crate::utils::email::SmtpSecurity::StartTls,
        }))
    }

    #[tokio::test]
    async fn rejects_unknown_provider() {
        let mut conn = setup_test_connection();
        let channel = channels_repo::create(
            &mut conn,
            crate::models::NewChannel {
                provider: "slack".into(),
                name: "fake-slack".into(),
                enabled: true,
                config: serde_json::json!({}),
            },
        )
        .unwrap();

        let thread = ThreadContext {
            ticket_id: 1,
            channel_id: channel.id,
            external_thread_id: None,
            recipient: ExternalIdentity {
                provider: "slack".into(),
                external_id: "U123".into(),
                display_name: "u".into(),
                known_email: None,
            },
            subject: None,
            references: vec![],
        };
        let content = OutboundContent {
            body_markdown: "hi".into(),
            body_html: None,
            attachments: vec![],
            external_id_hint: None,
        };

        // Pool is passed but unused — the unsupported-provider branch
        // short-circuits before any pool access. A separate pool keeps
        // the shared test-transaction from being exhausted.
        let pool = crate::test_helpers::setup_test_pool();
        let result = send_and_record(
            &channel,
            thread,
            content,
            1,
            email_service_stub(),
            pool,
            &mut conn,
        )
        .await;
        match result {
            Err(DispatchError::UnsupportedProvider(p)) => assert_eq!(p, "slack"),
            other => panic!("expected UnsupportedProvider, got {other:?}"),
        }
    }
}
