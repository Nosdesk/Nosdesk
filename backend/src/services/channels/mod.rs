//! Multi-channel message ingestion.
//!
//! Phase 1 ships a single concrete adapter (`EmailImapAdapter`). The trait
//! hierarchy and event shapes defined here are deliberately informed by
//! what email actually needs; they accommodate Slack / Teams / Discord /
//! webhook-based adapters without locking any of them into a poll-shaped
//! mold. See `/Users/kylephillips/.claude/plans/email-ingestion.md` for
//! the stress-test that shaped this design.
//!
//! # Layout
//!
//! - [`ChannelAdapter`] — trait every adapter implements. Carries send
//!   and thread-resolution responsibilities.
//! - [`PullAdapter`] — adapters with a poll loop (IMAP, optionally Gmail).
//! - [`PushAdapter`] — adapters that consume HTTP webhooks (Postmark,
//!   Slack Events API, Teams Graph change notifications).
//! - [`StreamAdapter`] — adapters that hold an open connection (Discord
//!   gateway, Slack Socket Mode). Not yet used.
//! - [`InboundEvent`] — normalized inbound event, variants cover
//!   `MessageReceived` (phase 1), `MessageEdited`, and `MessageDeleted`.
//! - [`InboundMessage`] / [`InboundAttachment`] — shared payload shape
//!   across all channels.
//! - [`OutboundContent`] — what a tech's reply looks like before adapters
//!   transform into the target channel's native format.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;

use crate::db::DbConnection;

pub mod auto_ack;
pub mod bounce_parser;
pub mod email_imap;
pub mod email_quote;
pub mod email_sanitise;
pub mod forward_parser;
pub mod outbound;
pub mod pipeline;
pub mod quote_previous;
pub mod reply_body;
pub mod registry;
pub mod relay;
pub mod signature;
pub mod supervisor;
pub mod threading;

// ---------- Error type ----------

/// Unified error surface for adapters. Wraps transient vs. permanent
/// distinctions so the registry can decide whether to back off or disable
/// the channel.
#[derive(Debug)]
pub enum ChannelError {
    /// Retry later — network hiccup, transient upstream error.
    Transient(String),
    /// Adapter needs admin intervention (bad credentials, bad config).
    Configuration(String),
    /// Provider rejected the request in a way that implies throttling.
    /// The registry applies exponential backoff.
    RateLimited { retry_after: Option<Duration> },
    /// Anything else. Logged; treated as transient by default.
    Other(String),
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(m) => write!(f, "transient: {m}"),
            Self::Configuration(m) => write!(f, "configuration: {m}"),
            Self::RateLimited { retry_after } => {
                write!(f, "rate limited (retry_after={retry_after:?})")
            }
            Self::Other(m) => write!(f, "{m}"),
        }
    }
}
impl std::error::Error for ChannelError {}

impl ChannelError {
    /// Closure constructor for the very common
    /// `Result::map_err(|e| ChannelError::Transient(format!("prefix: {e}")))`
    /// pattern. Use as: `.map_err(ChannelError::transient("tcp connect"))?`.
    pub fn transient<E: std::fmt::Display>(
        prefix: &'static str,
    ) -> impl FnOnce(E) -> Self {
        move |e| Self::Transient(format!("{prefix}: {e}"))
    }

    /// Companion to [`Self::transient`] for misconfiguration errors —
    /// things an admin must fix before the worker can proceed.
    pub fn configuration<E: std::fmt::Display>(
        prefix: &'static str,
    ) -> impl FnOnce(E) -> Self {
        move |e| Self::Configuration(format!("{prefix}: {e}"))
    }

    /// Companion to [`Self::transient`] for unclassified errors; the
    /// registry treats these as transient for backoff purposes.
    pub fn other<E: std::fmt::Display>(
        prefix: &'static str,
    ) -> impl FnOnce(E) -> Self {
        move |e| Self::Other(format!("{prefix}: {e}"))
    }
}

// ---------- Identity ----------

/// A non-Nosdesk identity reaching us via a channel. Fed to the identity
/// resolver to locate or provision a `User` row.
#[derive(Debug, Clone)]
pub struct ExternalIdentity {
    /// Matches `channels.provider` — `"email_imap"`, `"slack"`, etc.
    pub provider: String,
    /// Stable identifier *within the provider*: email address, Slack
    /// user id, Teams AAD object id, Discord snowflake.
    pub external_id: String,
    /// Human-readable name for display / logging.
    pub display_name: String,
    /// Email address if the adapter has one. For email channels this
    /// equals `external_id`; for chat channels this is the user's email
    /// as reported by the provider (may be absent).
    pub known_email: Option<String>,
}

// ---------- Inbound event shapes ----------

/// Normalized inbound event. Every adapter — whether it pulls from IMAP,
/// receives a webhook, or reads from a websocket — emits this shape.
/// Phase 1 pipeline only handles `MessageReceived`; `MessageEdited` and
/// `MessageDeleted` are logged and skipped until we wire the edit/delete
/// pipeline.
#[derive(Debug, Clone)]
pub enum InboundEvent {
    MessageReceived(InboundMessage),
    MessageEdited {
        external_id: String,
        new_body_text: String,
        new_body_html: Option<String>,
        edited_at: DateTime<Utc>,
    },
    MessageDeleted {
        external_id: String,
        deleted_at: DateTime<Utc>,
    },
    // Future: ReactionAdded / ReactionRemoved for workflow signals.
}

/// A single inbound message. Channel-specific metadata that doesn't fit
/// this shape goes into [`Self::raw_metadata`] so we don't lose
/// information we might want to surface later (Slack blocks, Teams
/// mentions lists, email headers, etc.).
#[derive(Debug, Clone)]
pub struct InboundMessage {
    /// Unique within the channel. Email: RFC 5322 Message-ID. Slack:
    /// `{channel}:{ts}`. Discord: snowflake. Ingested into
    /// `channel_messages.external_id`.
    pub external_id: String,
    /// Sender identity — used for `find_or_create_from_identity` and to
    /// stamp `channel_messages.from_address` / `.author_user_uuid`.
    pub from: ExternalIdentity,
    /// Present for email / forum-style channels; `None` for chat channels
    /// that don't have subject lines.
    pub subject: Option<String>,
    /// Required. HTML variant in [`Self::body_html`] when available.
    pub body_text: String,
    pub body_html: Option<String>,
    pub attachments: Vec<InboundAttachment>,
    /// Parent message external IDs (In-Reply-To + References chain for
    /// email; Slack `thread_ts`; Teams `replyToId`). Walked by the
    /// thread resolver in priority order.
    pub references: Vec<String>,
    pub received_at: DateTime<Utc>,
    pub loop_markers: LoopMarkers,
    /// Channel-specific payload preserved verbatim for audit and for
    /// future features that care about provider-native detail.
    pub raw_metadata: serde_json::Value,
    /// Recipient addresses / routes — used by the plus-addressing
    /// resolver to extract `support+ticket-N@host` from To/Cc/Delivered-To.
    /// Chat channels leave this empty.
    pub recipients: Vec<String>,
    /// Set when the message is a delivery-status notification (DSN) /
    /// hard or soft bounce. The pipeline short-circuits these so they
    /// don't create new tickets or trigger auto-replies; J Pass 2.2
    /// also stamps the matching outbound row via `bounce_report`.
    /// Distinct from `loop_markers` because the downstream handling
    /// diverges (bounces hit outbound state; loops just get logged
    /// and dropped).
    pub is_bounce: bool,
    /// Structured bounce detail when `is_bounce` is set AND the DSN's
    /// MIME structure was parseable (`message/rfc822` part present
    /// with an extractable `Message-ID`). One entry per per-recipient
    /// block in the DSN (RFC 3464 §2.1 allows multiple). Empty when
    /// the DSN was malformed or sender-heuristic-only; the pipeline
    /// still short-circuits but can't link back to the outbound row.
    pub bounce_reports: Vec<bounce_parser::BounceReport>,
    /// Raw RFC 5322 bytes for email channels. Carried through the
    /// pipeline so the persistence layer can save a verbatim copy
    /// of the original message (`comments.raw_source_uri`). Powers
    /// the "Show original message" affordance and lets us re-run
    /// the quote splitter on policy change without re-fetching
    /// from upstream. `None` for chat / webhook channels that
    /// don't have an equivalent.
    pub raw_bytes: Option<Vec<u8>>,
}

/// Either bytes we already have (IMAP) or a URL we fetch later (Slack
/// file_share, Teams hostedContent). The pipeline calls
/// `materialize_attachment()` which resolves both shapes into bytes
/// before writing through `utils::storage`.
#[derive(Debug, Clone)]
pub enum InboundAttachment {
    Inline {
        filename: String,
        mime_type: String,
        bytes: Vec<u8>,
    },
    External {
        filename: String,
        mime_type: String,
        url: String,
        /// Optional header pair (name, value) the fetcher sets — Slack
        /// requires `Authorization: Bearer {token}` on file downloads.
        auth_header: Option<(String, String)>,
        size_bytes: Option<u64>,
    },
}

/// Loop / auto-reply markers gathered at ingestion time. The pipeline
/// short-circuits on any of these so our auto-reply doesn't ping-pong
/// with the customer's out-of-office or feed back into a mailing list.
///
/// Field names describe the *semantic signal* rather than the
/// underlying transport header so chat / webhook adapters can reuse
/// the same struct with their own detection logic. Email's mapping:
///   - `is_auto_reply`  ← `Auto-Submitted` header per RFC 3834
///   - `is_bulk`        ← `Precedence: bulk | list | junk`
///   - `is_suppressed`  ← `X-Loop` / `X-Auto-Response-Suppress`
/// Chat / webhook adapters that don't carry equivalents just leave
/// them all false and rely on their own duplicate-event handling.
#[derive(Debug, Clone, Default)]
pub struct LoopMarkers {
    pub is_auto_reply: bool,
    pub is_bulk: bool,
    pub is_suppressed: bool,
}

impl LoopMarkers {
    pub fn any(&self) -> bool {
        self.is_auto_reply || self.is_bulk || self.is_suppressed
    }
}

// ---------- Outbound shapes ----------

/// What a tech's reply looks like before the adapter transforms it into
/// the target channel's native representation. Markdown is the canonical
/// format — email adapter HTML-renders it, Slack adapter mrkdwn-izes it,
/// etc. Pre-rendered `body_html` is available for adapters (like email)
/// that prefer it.
#[derive(Debug, Clone)]
pub struct OutboundContent {
    pub body_markdown: String,
    pub body_html: Option<String>,
    pub attachments: Vec<OutboundAttachment>,
    /// Optional external-id the caller wants stamped on this outgoing
    /// message. Email uses it to control the `Message-ID` header so the
    /// subsequent inbound reply can be threaded back via the References
    /// cascade. Chat adapters typically ignore this — the provider
    /// assigns ids server-side and we learn them from the response.
    pub external_id_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OutboundAttachment {
    pub filename: String,
    pub mime_type: String,
    /// Storage path relative to `uploads/` — the adapter reads it back
    /// through `utils::storage` when composing the outbound message.
    pub storage_path: String,
}

/// Context the pipeline hands to the adapter when asking it to send a
/// reply. Holds everything the adapter needs to address the message and
/// thread it with its predecessors.
#[derive(Debug, Clone)]
pub struct ThreadContext {
    pub ticket_id: i32,
    pub channel_id: i32,
    /// Provider-native pointer to the thread: the parent Message-ID for
    /// email, `thread_ts` for Slack, etc. Adapters that don't have an
    /// explicit thread handle can fall back to the recipient address.
    pub external_thread_id: Option<String>,
    /// Where to send the reply — single recipient for email, channel or
    /// DM id for chat.
    pub recipient: ExternalIdentity,
    /// Optional subject line. Email uses this; chat adapters ignore.
    pub subject: Option<String>,
    /// Full ancestor chain (References header for email). Later adapters
    /// may ignore or collapse this.
    pub references: Vec<String>,
}

/// Result of a successful outbound send. The pipeline records it as an
/// `outbound` channel_message row so the next inbound reply can thread.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub external_id: String,
    pub sent_at: DateTime<Utc>,
    pub raw_metadata: Option<serde_json::Value>,
}

// ---------- Trait hierarchy ----------

#[async_trait]
pub trait ChannelAdapter: Send + Sync + 'static {
    /// Stable identifier for logs and metrics —
    /// `format!("{}:{}", provider, channel.id)` is the convention.
    fn id(&self) -> &str;

    /// Matches `channels.provider`.
    fn provider(&self) -> &'static str;

    /// Send a tech's reply out through this channel.
    async fn send_reply(
        &self,
        thread: &ThreadContext,
        content: &OutboundContent,
    ) -> Result<OutboundMessage, ChannelError>;

    /// Find the existing ticket this inbound event belongs to, if any.
    /// Default implementation runs the explicit-reference cascade:
    /// References chain → plus-addressed recipient → our Message-ID
    /// format → subject line `[#N]`. Adapters override for fuzzy
    /// matching (WhatsApp: "last ticket from this phone in 24h").
    async fn resolve_thread(
        &self,
        event: &InboundMessage,
        channel_id: i32,
        conn: &mut DbConnection,
    ) -> Option<i32> {
        threading::default_explicit_threading(event, channel_id, conn).await
    }
}

/// Poll-based adapters (IMAP, Gmail fallback). The registry calls
/// [`Self::poll`] on [`Self::poll_interval`] cadence.
#[async_trait]
pub trait PullAdapter: ChannelAdapter {
    async fn poll(&mut self) -> Result<Vec<InboundEvent>, ChannelError>;
    fn poll_interval(&self) -> Duration;
}

/// Webhook-based adapters (Postmark, Slack Events, Teams Graph change
/// notifications). The handler at `/api/channels/{provider}/webhook`
/// hands the raw request body here for signature verification + parsing.
#[async_trait]
pub trait PushAdapter: ChannelAdapter {
    async fn parse_webhook(
        &self,
        headers: &actix_web::http::header::HeaderMap,
        body: &[u8],
    ) -> Result<Vec<InboundEvent>, ChannelError>;
}

/// Long-lived connection adapters (Discord gateway, Slack Socket Mode).
/// Not yet used. The registry spawns one task per stream adapter and
/// sends events onto the shared pipeline channel.
#[async_trait]
pub trait StreamAdapter: ChannelAdapter {
    async fn run(
        &mut self,
        tx: tokio::sync::mpsc::Sender<InboundEvent>,
    ) -> Result<(), ChannelError>;
}

// ---------- Channel direction constants re-exported for ergonomics ----------

pub use crate::models::{CHANNEL_DIRECTION_INBOUND, CHANNEL_DIRECTION_OUTBOUND};

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_markers_any_is_false_by_default() {
        assert!(!LoopMarkers::default().any());
    }

    #[test]
    fn loop_markers_any_detects_each_flag() {
        assert!(LoopMarkers { is_auto_reply: true, ..Default::default() }.any());
        assert!(LoopMarkers { is_bulk: true, ..Default::default() }.any());
        assert!(LoopMarkers { is_suppressed: true, ..Default::default() }.any());
    }
}
