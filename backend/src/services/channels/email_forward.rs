//! Adapter for `email_forward` channels (the hosted forwarding inbound path).
//!
//! Forwarding channels receive mail pushed by the inbound webhook rather than
//! polled, so this adapter exists only to satisfy the [`ChannelAdapter`]
//! surface the parse pipeline needs on the inbound path: a provider tag and
//! thread resolution (the default explicit-reference cascade, identical to
//! email-over-IMAP). Inbound never calls `send_reply`.
//!
//! Outbound dispatch for forwarding channels (a tech's reply, with `Reply-To`
//! pointing back at the forwarding address so the customer's reply threads in)
//! is wired separately from the inbound path; until then `send_reply` is an
//! explicit error rather than a silent misdelivery.

use async_trait::async_trait;

use crate::services::channels::{
    ChannelAdapter, ChannelError, OutboundContent, OutboundMessage, ThreadContext,
};

pub struct EmailForwardAdapter {
    id: String,
}

impl EmailForwardAdapter {
    pub fn new(channel_id: i32) -> Self {
        Self {
            id: format!("email_forward:{channel_id}"),
        }
    }
}

#[async_trait]
impl ChannelAdapter for EmailForwardAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn provider(&self) -> &'static str {
        crate::models::CHANNEL_PROVIDER_EMAIL_FORWARD
    }

    async fn send_reply(
        &self,
        _thread: &ThreadContext,
        _content: &OutboundContent,
    ) -> Result<OutboundMessage, ChannelError> {
        Err(ChannelError::Configuration(
            "email_forward outbound dispatch is not wired on this path; \
             the inbound webhook never calls send_reply"
                .into(),
        ))
    }

    // resolve_thread: the default explicit-reference cascade (References chain
    // -> plus-addressed recipient -> our Message-ID -> subject [#N]) is exactly
    // right for forwarded email; no override.
}
