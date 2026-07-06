//! Adapter for the push-ingested email channels: `email_forward` (the hosted
//! forwarding inbound path) and `email_managed` (the hosted managed default
//! address `support@<slug>.<tenant_domain>`, routed by slug).
//!
//! Both channel kinds receive mail pushed by the inbound webhook rather than
//! polled, so this adapter exists only to satisfy the [`ChannelAdapter`]
//! surface the parse pipeline needs on the inbound path: a provider tag and
//! thread resolution (the default explicit-reference cascade, identical to
//! email-over-IMAP). Inbound never calls `send_reply`.
//!
//! Outbound dispatch (a tech's reply — for `email_forward` with `Reply-To`
//! pointing back at the forwarding address, for `email_managed` from the
//! directly-receivable managed From) is wired separately from the inbound
//! path via the outbound queue; until then `send_reply` is an explicit error
//! rather than a silent misdelivery.

use async_trait::async_trait;

use crate::services::channels::{
    ChannelAdapter, ChannelError, OutboundContent, OutboundMessage, ThreadContext,
};

pub struct EmailForwardAdapter {
    id: String,
    provider: &'static str,
}

impl EmailForwardAdapter {
    pub fn new(channel_id: i32) -> Self {
        Self {
            id: format!("email_forward:{channel_id}"),
            provider: crate::models::CHANNEL_PROVIDER_EMAIL_FORWARD,
        }
    }

    /// The same push-ingest adapter shape for an `email_managed` channel.
    pub fn managed(channel_id: i32) -> Self {
        Self {
            id: format!("email_managed:{channel_id}"),
            provider: crate::models::CHANNEL_PROVIDER_EMAIL_MANAGED,
        }
    }
}

#[async_trait]
impl ChannelAdapter for EmailForwardAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn provider(&self) -> &'static str {
        self.provider
    }

    async fn send_reply(
        &self,
        _thread: &ThreadContext,
        _content: &OutboundContent,
    ) -> Result<OutboundMessage, ChannelError> {
        Err(ChannelError::Configuration(
            "push-ingested email outbound dispatch is not wired on this path; \
             the inbound webhook never calls send_reply"
                .into(),
        ))
    }

    // resolve_thread: the default explicit-reference cascade (References chain
    // -> plus-addressed recipient -> our Message-ID -> subject [#N]) is exactly
    // right for pushed email; no override.
}
