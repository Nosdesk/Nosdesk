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
use crate::models::{Channel, NewChannelMessage, CHANNEL_DIRECTION_OUTBOUND};
use crate::repository::channels as channels_repo;
use crate::services::channels::email_imap::{
    build_email_imap_adapter, ImapChannelConfig,
};
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

/// Thin wrapper for the common fire-and-forget case used by the
/// comment-creation handler. Errors are logged, not propagated, since
/// a failed outbound relay should not block the user's HTTP response
/// (the comment is already saved; we'll retry on a future send or the
/// admin can resend manually).
pub async fn send_and_record_best_effort(
    channel: &Channel,
    thread: ThreadContext,
    content: OutboundContent,
    comment_id: i32,
    email: Arc<EmailService>,
    pool: crate::db::Pool,
    conn: &mut DbConnection,
) {
    match send_and_record(channel, thread, content, comment_id, email, pool, conn).await {
        Ok(_) => {}
        Err(e) => {
            warn!(
                channel_id = channel.id,
                provider = %channel.provider,
                comment_id,
                error = %e,
                "outbound channel dispatch failed — comment saved, no reply sent"
            );
        }
    }
}

/// Spawn a detached task that runs the full inbound-comment relay
/// pipeline for a freshly-created comment:
///
///   1. obtain a DB connection,
///   2. run [`super::relay::decide_relay`] to classify the comment
///      (skip internal, closed channel, etc.),
///   3. if the decision is `Relay`, build outbound content from the
///      comment body and call [`send_and_record_best_effort`].
///
/// Fire-and-forget on purpose: the HTTP caller should never wait on
/// SMTP, and a failed send doesn't roll back the already-persisted
/// comment. Returns immediately; errors hit the log only.
///
/// Keeping the orchestration here (rather than inline in the comment
/// handler) means the handler stays focused on persistence + client
/// response, and future relay targets (chat channels) plug into one
/// place.
pub fn spawn_relay_for_comment(
    ticket: crate::models::Ticket,
    comment: crate::models::Comment,
    pool: crate::db::Pool,
    email: Arc<EmailService>,
) {
    tokio::spawn(async move {
        let mut conn = match pool.get() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "channel relay: could not obtain db connection");
                return;
            }
        };
        let decision = match super::relay::decide_relay(&mut conn, &ticket, &comment) {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "channel relay: decision failed");
                return;
            }
        };
        let (channel, thread) = match decision {
            super::relay::RelayDecision::Relay { channel, thread } => (channel, thread),
            other => {
                tracing::debug!(decision = ?other, "channel relay: skipped");
                return;
            }
        };
        // Compose the reply in both HTML and plaintext form so the
        // email adapter can ship a real `multipart/alternative`
        // message. Final wire order in either form:
        //   <tech's new reply>
        //   <signature>
        //   <quoted prior message>
        // Matches what `Mail.app` / Gmail produce when a user hits
        // Reply, anchoring the response to its context.
        let body = super::reply_body::ReplyBody::from_comment(&comment);
        let body = super::signature::append_signature_for_user(
            &mut conn,
            comment.user_uuid,
            body,
        );
        let body = super::quote_previous::maybe_prepend_quote(
            &mut conn,
            &channel,
            &ticket,
            body,
        );
        let content = OutboundContent {
            body_markdown: body.text,
            body_html: Some(body.html),
            attachments: vec![],
            external_id_hint: None,
        };
        send_and_record_best_effort(
            &channel,
            thread,
            content,
            comment.id,
            email,
            pool.clone(),
            &mut conn,
        )
        .await;
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
    use crate::test_helpers::setup_test_connection;
    use crate::services::channels::{ExternalIdentity, OutboundContent};

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
