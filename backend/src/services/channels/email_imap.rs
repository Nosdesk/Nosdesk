//! IMAP-based email ingestion adapter.
//!
//! Responsibilities:
//!
//! 1. **Persistence schema** — [`ImapRuntimeState`] shape for
//!    `channels.runtime_state`.
//! 2. **Config shape** — [`ImapChannelConfig`] pulled from
//!    `channels.config` JSON; adapter instances are constructed from
//!    `(channel, credential)` pairs in the registry (task #16).
//! 3. **Pure parser** — [`parse_rfc822_into_inbound_message`] turns raw
//!    bytes into an [`InboundMessage`]. No I/O, fully unit-testable.
//!    This is the function Greenmail / the live IMAP fetcher hand bytes
//!    to; the rest of the pipeline doesn't care whether those bytes came
//!    from a real mailbox or a fixture file.
//! 4. **Adapter glue** — [`EmailImapAdapter`] implements
//!    [`ChannelAdapter`]; `send_reply` delegates to
//!    [`crate::utils::email::EmailService`] for SMTP + threading headers.
//!
//! The IMAP poll loop itself (UID-based fetch, UIDVALIDITY pinning,
//! marking `\Seen`) is deliberately *not* in this file — it lives in
//! [`super::registry`] (task #16) so the adapter stays testable without
//! spinning up a Greenmail container. The E2E Greenmail test will live
//! in `tests/channels_email_imap.rs` once registry is wired.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use std::time::Duration;

use futures::TryStreamExt;

use crate::models::CRED_TYPE_IMAP_PASSWORD;
use crate::repository::channels as channels_repo;
use crate::services::channels::threading::format_outbound_message_id;
use crate::services::channels::{
    ChannelAdapter, ChannelError, ExternalIdentity, InboundAttachment, InboundEvent,
    InboundMessage, LoopMarkers, OutboundContent, OutboundMessage, PullAdapter, ThreadContext,
};
use crate::utils::email::{EmailService, OutboundEmailMessage};

// ---------- Persistence shapes ----------

/// Shape of `channels.runtime_state` for the `email_imap` provider.
///
/// UIDVALIDITY pinning is what lets us use `last_seen_uid` safely across
/// restarts: IMAP UIDs are only monotonic within a given UIDVALIDITY, so
/// if that value changes (rare — mailbox restore, reprovision) we need
/// to rescan. Without it we'd risk re-ingesting or skipping messages after
/// any backend operation that rotates UIDVALIDITY.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ImapRuntimeState {
    #[serde(default)]
    pub last_seen_uid: u32,
    #[serde(default)]
    pub uid_validity: Option<u32>,
    #[serde(default)]
    pub last_error: Option<String>,
    /// Messages whose ingest (the pipeline) failed and are awaiting a
    /// bounded retry. The poll cursor advances past them so the channel
    /// keeps making progress; these are re-fetched by UID each poll until
    /// they ingest or exhaust [`MAX_INGEST_ATTEMPTS`]. Keyed implicitly by
    /// `uid` (one entry per UID).
    #[serde(default)]
    pub retry_queue: Vec<IngestRetry>,
    /// Messages that exhausted their ingest retries, kept for operator
    /// visibility (the channel admin surface reads runtime_state). Capped
    /// at [`DEAD_LETTER_CAP`], oldest pruned first.
    #[serde(default)]
    pub dead_letters: Vec<IngestDeadLetter>,
    /// When the channel first reached this mailbox. Its first poll starts
    /// after the newest message (or at the backlog date), so mail older
    /// than this is the mailbox's history. Absent on a channel connected
    /// before this was recorded, which counts from its creation.
    #[serde(default)]
    pub connected_at: Option<DateTime<Utc>>,
    /// The mailbox the cursor belongs to ([`mailbox_key`]). Settings that
    /// point the channel at another mailbox connect it afresh.
    #[serde(default)]
    pub mailbox: Option<String>,
    /// The earliest backlog date whose mail has been searched for.
    #[serde(default)]
    pub backlog_from: Option<NaiveDate>,
}

/// A message awaiting an ingest retry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRetry {
    pub uid: u32,
    /// RFC 5322 Message-ID, carried for operator correlation and the
    /// dead-letter record.
    pub external_id: String,
    pub attempts: u32,
    pub last_error: String,
    pub first_failed_at: DateTime<Utc>,
}

/// A message that exhausted its ingest retries and was given up on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestDeadLetter {
    pub uid: u32,
    pub external_id: String,
    pub attempts: u32,
    pub last_error: String,
    pub dead_lettered_at: DateTime<Utc>,
}

/// How many poll cycles a failed-to-ingest message is retried before it is
/// dead-lettered. A transient blip clears well within this; a persistently
/// bad message stops blocking after it.
pub const MAX_INGEST_ATTEMPTS: u32 = 5;

/// Cap on the retained dead-letter list in runtime_state.
const DEAD_LETTER_CAP: usize = 50;

impl ImapRuntimeState {
    /// A message ingested successfully: drop any pending retry for its UID.
    fn record_ingest_success(&mut self, uid: u32) {
        self.retry_queue.retain(|r| r.uid != uid);
    }

    /// A message failed to ingest. Schedule (or advance) its retry; returns the
    /// dead-letter entry when this failure exhausts [`MAX_INGEST_ATTEMPTS`].
    fn record_ingest_failure(
        &mut self,
        uid: u32,
        external_id: &str,
        error: &str,
    ) -> Option<IngestDeadLetter> {
        let exhausted = match self.retry_queue.iter_mut().find(|r| r.uid == uid) {
            Some(entry) => {
                entry.attempts += 1;
                entry.last_error = error.to_string();
                entry.attempts >= MAX_INGEST_ATTEMPTS
            }
            None => {
                self.retry_queue.push(IngestRetry {
                    uid,
                    external_id: external_id.to_string(),
                    attempts: 1,
                    last_error: error.to_string(),
                    first_failed_at: Utc::now(),
                });
                false
            }
        };
        exhausted.then(|| self.dead_letter(uid, error.to_string()))
    }

    /// The server refused to fetch a message (a NO or BAD reply) while the
    /// session still answered. It is retried on later polls like a message
    /// that failed to ingest, and dead-lettered after
    /// [`MAX_INGEST_ATTEMPTS`]; the poll moves on past it meanwhile.
    fn record_fetch_refused(&mut self, uid: u32, error: &str) -> Option<IngestDeadLetter> {
        self.record_ingest_failure(uid, "", error)
    }

    /// Move a UID out of the retry queue into the capped dead-letter list and
    /// return the recorded entry. Carries the queued external_id / attempts
    /// when present.
    fn dead_letter(&mut self, uid: u32, error: String) -> IngestDeadLetter {
        let (external_id, attempts) = self
            .retry_queue
            .iter()
            .find(|r| r.uid == uid)
            .map(|r| (r.external_id.clone(), r.attempts))
            .unwrap_or_default();
        self.retry_queue.retain(|r| r.uid != uid);
        let entry = IngestDeadLetter {
            uid,
            external_id,
            attempts,
            last_error: error,
            dead_lettered_at: Utc::now(),
        };
        self.dead_letters.push(entry.clone());
        let len = self.dead_letters.len();
        if len > DEAD_LETTER_CAP {
            self.dead_letters.drain(0..len - DEAD_LETTER_CAP);
        }
        entry
    }
}

/// Shape of `channels.config` for the `email_imap` provider. Populated
/// by the admin UI; password is resolved separately from
/// `channel_credentials`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapChannelConfig {
    pub host: String,
    #[serde(default = "default_imap_port")]
    pub port: u16,
    pub username: String,
    /// Mailbox to poll. `INBOX` for most providers; Gmail uses
    /// `"[Gmail]/All Mail"` if admins want to ingest from the
    /// all-mail folder instead.
    #[serde(default = "default_mailbox")]
    pub mailbox: String,
    /// Toggle implicit TLS (port 993) vs STARTTLS (port 143). We don't
    /// support plaintext — credentials always ride TLS.
    #[serde(default = "default_use_tls")]
    pub use_tls: bool,
    /// Domain used when synthesizing outbound Message-IDs. Usually the
    /// same as `username`'s domain but can diverge when a relay rewrites
    /// the From address.
    pub reply_domain: String,
    /// Skip TLS certificate validation. Only safe for Greenmail /
    /// self-hosted test servers; refuse in production. Defaults to
    /// `false` so an admin has to explicitly enable it.
    #[serde(default)]
    pub insecure_skip_cert_verify: bool,
    /// Also bring in the mail the mailbox received since this date. Without
    /// it a new channel takes only mail that arrives after it connects. Mail
    /// from before the connection comes in quietly: no acknowledgement, no
    /// rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog_since: Option<NaiveDate>,
}

fn default_imap_port() -> u16 {
    993
}
fn default_mailbox() -> String {
    "INBOX".into()
}
fn default_use_tls() -> bool {
    true
}

/// The connection time recorded in a stored `runtime_state`, if any.
pub fn stored_connected_at(runtime_state: &serde_json::Value) -> Option<DateTime<Utc>> {
    runtime_state
        .get("connected_at")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
}

/// Names the mailbox a config polls, so the cursor can tell when the
/// settings point it somewhere else.
fn mailbox_key(config: &ImapChannelConfig) -> String {
    format!(
        "{}@{}/{}",
        config.username.trim(),
        config.host.trim().to_ascii_lowercase(),
        config.mailbox.trim()
    )
}

impl ImapChannelConfig {
    /// Quick fail-fast validation before we try to use this config to
    /// stand up a worker. Catches the obvious "admin saved an empty
    /// string" mistakes that would otherwise show up as cryptic TCP /
    /// IMAP errors once per poll, forever.
    pub fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("host", &self.host),
            ("username", &self.username),
            ("reply_domain", &self.reply_domain),
            ("mailbox", &self.mailbox),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty"));
            }
        }
        if self.port == 0 {
            return Err("port must be in 1..=65535".into());
        }
        // Not a full RFC 1035 check — just enough to reject whitespace
        // or quoted junk that would otherwise fail deep in the TCP
        // stack. Alphanumerics + `.-_` cover both DNS names and
        // dotted-quad IPs.
        if !self
            .host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err("host contains characters outside a-z, 0-9, and `.-_`".into());
        }
        Ok(())
    }
}

// ---------- Adapter ----------

/// Concrete [`ChannelAdapter`] for `provider = "email_imap"`. Holds the
/// resolved config + a shared [`EmailService`] for outbound SMTP, plus a
/// [`Pool`] the adapter uses to:
///
///   - fetch the IMAP password from `channel_credentials` each poll
///     (so a rotated password picks up on the next cycle with no
///     worker restart),
///   - persist `last_seen_uid` / `uid_validity` progress into
///     `channels.runtime_state` after each successful fetch.
pub struct EmailImapAdapter {
    id: String,
    channel_id: i32,
    /// The channel's workspace. The poll path reads credentials and writes
    /// runtime state on tenant tables (`channel_credentials`, `channels`),
    /// both workspace-isolated, so those raw-conn touches pin to this id.
    workspace_id: i32,
    config: ImapChannelConfig,
    /// SMTP handle used by [`ChannelAdapter::send_reply`]. Not touched
    /// by the poll path.
    email: Arc<EmailService>,
    /// Pool used by the poll path to read credentials and persist
    /// runtime state. [`ChannelAdapter::send_reply`] ignores it.
    pool: crate::db::Pool,
    state: ImapRuntimeState,
    /// When the channel was created: its connection time until its first
    /// poll records one.
    created_at: Option<DateTime<Utc>>,
    /// Cached `IDLE` capability. `None` = not yet probed — we will
    /// check `CAPABILITY` on the next successful session. `Some(true)`
    /// = advertised, we use IDLE. `Some(false)` = not advertised (or
    /// advertised-but-broken), we fall back to polled cadence without
    /// further attempts. Latches to `Some(false)` on IDLE init failure
    /// so a server that *says* it supports IDLE but rejects the
    /// command doesn't fill the logs with per-poll warnings.
    idle_supported: Option<bool>,
}

impl EmailImapAdapter {
    pub fn new(
        channel_id: i32,
        workspace_id: i32,
        config: ImapChannelConfig,
        email: Arc<EmailService>,
        pool: crate::db::Pool,
        initial_state: ImapRuntimeState,
    ) -> Self {
        Self {
            id: format!("email_imap:{channel_id}"),
            channel_id,
            workspace_id,
            config,
            email,
            pool,
            state: initial_state,
            created_at: None,
            idle_supported: None,
        }
    }

    /// Set the channel's creation time, its connection time until the first
    /// poll records one.
    pub fn with_created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = Some(created_at);
        self
    }

    pub fn config(&self) -> &ImapChannelConfig {
        &self.config
    }

    /// Current in-memory runtime state (last_seen_uid, uid_validity,
    /// last_error). Tests and admin diagnostics read this.
    pub fn runtime_state(&self) -> &ImapRuntimeState {
        &self.state
    }
}

/// Shared constructor used by the registry worker *and* the outbound
/// dispatcher — both need to hand `ChannelAdapter::send_reply` / the
/// poll loop the same adapter shape. Kept here so any future config
/// parsing, credential warming, or runtime-state hydration touches one
/// place instead of two.
pub fn build_email_imap_adapter(
    channel: &crate::models::Channel,
    email: Arc<EmailService>,
    pool: crate::db::Pool,
) -> Result<EmailImapAdapter, String> {
    let config: ImapChannelConfig = serde_json::from_value(channel.config.clone())
        .map_err(|e| format!("invalid email_imap config: {e}"))?;
    config.validate()?;
    let state: ImapRuntimeState =
        serde_json::from_value(channel.runtime_state.clone()).unwrap_or_default();
    Ok(
        EmailImapAdapter::new(channel.id, channel.workspace_id, config, email, pool, state)
            .with_created_at(channel.created_at.and_utc()),
    )
}

/// Authenticated IMAP session type used by both the test-connection
/// probe and the real poll loop. Hoisted here as a type alias so the
/// signature is one place to update when lettre/tokio-native-tls rev.
type ImapSession = async_imap::Session<tokio_native_tls::TlsStream<tokio::net::TcpStream>>;

/// Open a fresh TLS-wrapped IMAP session and authenticate. Errors are
/// wrapped into [`ChannelError`] so both call sites can propagate them
/// through the normal error-classification path (transient for
/// network, configuration for bad creds).
async fn open_session(
    config: &ImapChannelConfig,
    password: &str,
) -> Result<ImapSession, ChannelError> {
    use tokio::net::TcpStream;

    if !config.use_tls {
        // Plaintext IMAP would leak creds on the wire. Refuse up front;
        // admins who need STARTTLS can open a feature request once we
        // add the UPGRADE sequence.
        return Err(ChannelError::Configuration(
            "plaintext IMAP is not supported — enable TLS".into(),
        ));
    }

    // Resolve and SSRF-vet the admin-supplied host before dialing, then
    // connect to a validated address (not the hostname) so a rebinding
    // DNS record can't slip an internal IP between the check and the
    // connect. An allowlisted host (NOSDESK_OUTBOUND_ALLOWED_HOSTS) skips
    // the routability filter for same-VPC self-host relays. The hostname
    // is still used for the TLS handshake below, so certificate
    // verification is unaffected.
    let addrs = crate::utils::egress::resolve_and_validate(&config.host, config.port)
        .await
        .map_err(|e| match e {
            crate::utils::egress::EgressError::DnsLookup { .. } => {
                ChannelError::Transient(format!("imap host: {e}"))
            }
            _ => ChannelError::Configuration(format!("imap host rejected: {e}")),
        })?;
    let tcp = timed("tcp connect", TcpStream::connect(&*addrs)).await?;

    // Skipping TLS verification is a development-only affordance (local
    // Greenmail / self-signed test servers). Honour it ONLY outside
    // production — a hard ceiling that no other config can override, so an
    // admin who enables it and forgets, or a stale config copied to prod,
    // can never silently disable TLS validation (an MITM vector for mailbox
    // credentials). The save path also refuses to store it in production.
    let cert_verify_disabled =
        config.insecure_skip_cert_verify && !crate::config_utils::assume_production();
    if config.insecure_skip_cert_verify && !cert_verify_disabled {
        tracing::warn!(
            host = %config.host,
            "channel has insecure_skip_cert_verify=true but the deployment is production; \
             validating certificate normally"
        );
    }
    let native_connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(cert_verify_disabled)
        .danger_accept_invalid_hostnames(cert_verify_disabled)
        .build()
        .map_err(ChannelError::configuration("tls connector init"))?;
    let tls = tokio_native_tls::TlsConnector::from(native_connector);
    let tls_stream = timed("tls handshake", tls.connect(&config.host, tcp)).await?;

    let client = async_imap::Client::new(tls_stream);
    // async_imap's login returns `(Error, Client)` on failure — we only
    // care about the error; drop the client. Sub-classify so a
    // network blip doesn't permanently disable the worker:
    //   - Io / ConnectionLost / Parse → Transient (retry on backoff)
    //   - No / Bad / Validate → Configuration (admin must fix)
    //   - Anything else → Configuration as a safer default
    // A timeout is Transient since that's an availability signal, not
    // a credential problem.
    match tokio::time::timeout(IMAP_OP_TIMEOUT, client.login(&config.username, password)).await {
        Ok(Ok(session)) => Ok(session),
        Ok(Err((e, _))) => Err(classify_login_error(e)),
        Err(_) => Err(ChannelError::Transient(format!(
            "login: timed out after {}s",
            IMAP_OP_TIMEOUT.as_secs()
        ))),
    }
}

/// Split `async_imap::error::Error` from a LOGIN attempt into the
/// `ChannelError` severity tiers. Kept here next to `open_session` so
/// any change to the classification logic lives with the code that
/// invokes LOGIN; no other callers need this specific mapping.
fn classify_login_error(e: async_imap::error::Error) -> ChannelError {
    use async_imap::error::Error;
    let msg = format!("login: {e}");
    match e {
        // Network-level failures are transient — a full reconnect on
        // the next poll typically recovers.
        Error::Io(_) | Error::ConnectionLost => ChannelError::Transient(msg),
        // Parse errors are a server-side quirk or upstream client bug;
        // either way, spinning backoff is preferable to permanent
        // disable because the LOGIN might succeed cleanly later.
        Error::Parse(_) => ChannelError::Transient(msg),
        // NO ("wrong creds"), BAD ("rejected command"), Validate
        // ("config had an invalid IMAP string"): admin has to fix
        // something, so stop the worker and write last_error.
        Error::No(_) | Error::Bad(_) | Error::Validate(_) => ChannelError::Configuration(msg),
        // Append is about writing messages, not login; if it somehow
        // surfaces here it's a protocol mismatch we can't recover
        // from automatically. `async_imap::error::Error` is marked
        // `#[non_exhaustive]`, so any future variant falls into the
        // same Configuration bucket — safer to stop and alert than to
        // spin on an unknown failure mode.
        _ => ChannelError::Configuration(msg),
    }
}

/// Why a probe failed: a stage the admin form maps to one fix, and the
/// server's own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapProbeFailure {
    /// `dns`, `egress_blocked`, `connect`, `tls`, `auth`, `mailbox`,
    /// `timeout` or `invalid`.
    pub code: &'static str,
    pub detail: String,
}

impl ImapProbeFailure {
    /// Classify an `open_session` error by the stage labels this file gives
    /// it: `imap host` (DNS), `imap host rejected` (egress), `tcp connect`,
    /// `tls handshake`, `login`.
    fn from_session_error(e: &ChannelError) -> Self {
        let detail = match e {
            ChannelError::Transient(m)
            | ChannelError::Configuration(m)
            | ChannelError::Other(m) => m.clone(),
            other => other.to_string(),
        };
        let code = if detail.contains("timed out") {
            "timeout"
        } else if detail.starts_with("imap host rejected") {
            "egress_blocked"
        } else if detail.starts_with("imap host") {
            "dns"
        } else if detail.starts_with("tcp connect") {
            "connect"
        } else if detail.starts_with("tls") {
            "tls"
        } else if detail.starts_with("login") {
            // A dropped connection during LOGIN is the network, not the password.
            if matches!(e, ChannelError::Transient(_)) {
                "connect"
            } else {
                "auth"
            }
        } else {
            "invalid"
        };
        Self { code, detail }
    }
}

/// Probe an IMAP server with the given config and password. Connects,
/// authenticates, examines the configured mailbox, logs out.
///
/// Only exists here (not on `EmailImapAdapter`) because the admin test runs
/// *before* the channel row is saved — there's no adapter yet.
pub async fn test_imap_connection(
    config: &ImapChannelConfig,
    password: &str,
) -> Result<(), ImapProbeFailure> {
    let mut session = open_session(config, password)
        .await
        .map_err(|e| ImapProbeFailure::from_session_error(&e))?;

    // EXAMINE is read-only — it verifies the mailbox exists without
    // touching `\Seen` flags. SELECT would also work but leaves a
    // server-side session state we don't need.
    let mailbox_failure = |detail: String| ImapProbeFailure {
        code: "mailbox",
        detail,
    };
    match tokio::time::timeout(IMAP_OP_TIMEOUT, session.examine(&config.mailbox)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            return Err(mailbox_failure(format!(
                "mailbox '{}' inaccessible: {e}",
                config.mailbox
            )))
        }
        Err(_) => {
            return Err(ImapProbeFailure {
                code: "timeout",
                detail: format!(
                    "mailbox '{}' inaccessible: timed out after {}s",
                    config.mailbox,
                    IMAP_OP_TIMEOUT.as_secs()
                ),
            })
        }
    }

    // A failed LOGOUT after a good EXAMINE still proves the settings work.
    let _ = tokio::time::timeout(IMAP_OP_TIMEOUT, session.logout()).await;
    Ok(())
}

#[async_trait]
impl ChannelAdapter for EmailImapAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn provider(&self) -> &'static str {
        "email_imap"
    }

    fn connected_at(&self) -> Option<DateTime<Utc>> {
        self.state.connected_at.or(self.created_at)
    }

    async fn send_reply(
        &self,
        thread: &ThreadContext,
        content: &OutboundContent,
    ) -> Result<OutboundMessage, ChannelError> {
        // Prefer the caller-supplied id when present; that's how the
        // outbound dispatcher stamps the correct comment id into the
        // Message-ID. Fall back to generating one — useful for ad-hoc
        // sends (e.g. admin test-connection) where there's no comment.
        let message_id = content.external_id_hint.clone().unwrap_or_else(|| {
            format_outbound_message_id(thread.ticket_id, 0, &self.config.reply_domain)
        });

        let recipient = thread
            .recipient
            .known_email
            .as_deref()
            .ok_or_else(|| ChannelError::Configuration("recipient has no email".into()))?;

        // Don't send to an address that previously hard-bounced or complained.
        // Unlike the auto-ack path, a technician deliberately authored this, so
        // surface it as an error (they can clear the suppression in admin) rather
        // than silently dropping their reply.
        if crate::services::outbound_email::recipient_is_suppressed(
            &self.pool,
            self.workspace_id,
            recipient,
        ) {
            return Err(ChannelError::Other(format!(
                "recipient {recipient} is on the suppression list (prior hard bounce or \
                 complaint); remove them from suppressions to send"
            )));
        }

        let subject = thread.subject.as_deref().unwrap_or("(no subject)");
        let in_reply_to = thread.external_thread_id.as_deref();

        let outbound = OutboundEmailMessage {
            to: recipient,
            subject,
            body_text: &content.body_markdown,
            body_html: content.body_html.as_deref(),
            message_id: &message_id,
            in_reply_to,
            references: &thread.references,
            // Tech-authored reply: a human wrote it, so no auto headers.
            auto_submitted: None,
            // Conversation mail to the customer: transactional, never opt-out-able.
            mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL,
            // B3: reply to the channel's polled mailbox so it threads back in.
            reply_to: self
                .config
                .username
                .contains('@')
                .then(|| self.config.username.as_str()),
            // Direct send (not queued): no outbound row id for a VERP token.
            envelope_from: None,
            list_unsubscribe: None,
        };

        let delivered = self
            .email
            .send_ticket_reply(outbound)
            .await
            .map_err(ChannelError::Other)?;

        Ok(OutboundMessage {
            external_id: format!("<{}>", delivered.unwrap_or(message_id)),
            sent_at: Utc::now(),
            raw_metadata: None,
        })
    }
}

/// Polling cadence between IDLE cycles. Each poll call internally
/// uses IMAP IDLE to wait for server-pushed notifications (up to
/// `IDLE_WAIT`), so the registry-level sleep is only a small
/// anti-tight-loop pause after a poll returns. Sub-second responsiveness
/// comes from IDLE, not from this interval.
const DEFAULT_POLL_SECS: u64 = 1;

/// Upper bound on a single IDLE wait, per RFC 2177's guidance that
/// clients SHOULD re-issue IDLE at least every 29 minutes to stay
/// ahead of server-side inactivity timeouts. `async-imap`'s
/// `wait_with_timeout` resets this on any server message (including
/// keep-alive `* OK Still here`), so for a well-behaved server the
/// wait only terminates on real activity.
const IDLE_WAIT: Duration = Duration::from_secs(29 * 60);

/// Ceiling for any single IMAP operation. A well-behaved server
/// completes any of our calls (SELECT, SEARCH, FETCH, LOGIN, LOGOUT) in
/// well under a second. 30s is generous for a bad-network day and
/// aggressive enough that a half-open TCP session or a wedged server
/// can't stall the worker indefinitely. A timeout bubbles up as a
/// `Transient` error so the normal backoff path applies.
const IMAP_OP_TIMEOUT: Duration = Duration::from_secs(30);

/// Ceiling on messages fetched per poll. A UIDVALIDITY rollover on a
/// 100k-message mailbox would otherwise have us SEARCH → N × FETCH
/// single-threaded in one poll, tying up the worker for an hour and
/// holding an IMAP session hostage the whole time. Chunking the rescan
/// across polls at this cap keeps each poll bounded; the cursor
/// catches up within a few iterations for any realistic backlog.
const MAX_FETCH_PER_POLL: usize = 200;

/// IMAP fetch spec. `RFC822` gives us the full raw message,
/// `INTERNALDATE` gives the server's stamp (preferred over the Date
/// header, which clients can forge or clock-skew), `UID` echoes the id
/// so we can advance `last_seen_uid` safely.
const FETCH_SPEC: &str = "(UID INTERNALDATE BODY.PEEK[])";

/// Wrap any fallible async operation with the shared timeout. Both the
/// operation's error and a timeout become `ChannelError::Transient`
/// with a `{label}:` prefix so log lines identify which operation
/// tripped. Covers TCP connect, TLS handshake, and every IMAP call
/// whose error type implements `Display`.
async fn timed<F, T, E>(label: &'static str, fut: F) -> Result<T, ChannelError>
where
    F: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match tokio::time::timeout(IMAP_OP_TIMEOUT, fut).await {
        Ok(r) => r.map_err(ChannelError::transient(label)),
        Err(_) => Err(ChannelError::Transient(format!(
            "{label}: timed out after {}s",
            IMAP_OP_TIMEOUT.as_secs()
        ))),
    }
}

#[async_trait]
impl PullAdapter for EmailImapAdapter {
    /// Fetch all messages with UID greater than `last_seen_uid`, parse
    /// them into [`InboundEvent`]s, and advance persisted state.
    ///
    /// A new channel starts after the newest message (or at its backlog
    /// date), and a changed UIDVALIDITY rescans from the day the channel
    /// connected; see [`plan_start`].
    ///
    /// `BODY.PEEK[]` (rather than `BODY[]`) deliberately leaves the
    /// `\Seen` flag untouched. Our dedup is UID-based, not flag-based,
    /// and admins often keep unrelated mail filters that care about
    /// unread-count.
    async fn poll(&mut self) -> Result<Vec<InboundEvent>, ChannelError> {
        // Credentials are loaded every poll so a rotation picked up by
        // the admin UI takes effect on the next cycle without a worker
        // restart. The DB hit is negligible at this cadence.
        //
        // A DB failure during the lookup is Transient (retry next poll) —
        // using Configuration here would stop the worker permanently for
        // what might be a connection blip. Only the "no credential row
        // exists" case is a genuine Configuration problem.
        let channel_id = self.channel_id;
        let password = crate::sync::session::run_in_workspace(
            &self.pool,
            "channels:imap_credential",
            self.workspace_id,
            |conn| {
                // channel_credentials is workspace-isolated; pin so the read
                // resolves under the runtime (NOBYPASSRLS) role, not just the
                // superuser dev profile. get_credential's CredentialError is
                // flattened into the closure's diesel error channel (all such
                // failures stay Transient, as before).
                channels_repo::get_credential(conn, channel_id, CRED_TYPE_IMAP_PASSWORD)
                    .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
            },
        )
        .map_err(ChannelError::transient("credential lookup"))?
        .ok_or_else(|| ChannelError::Configuration("no IMAP password stored for channel".into()))?;

        let mut session = open_session(&self.config, &password).await?;

        // Probe IDLE support once per adapter. Servers that don't
        // advertise IDLE get polled-only behaviour — without this
        // check we'd send IDLE, fail, and warn on every poll forever.
        if self.idle_supported.is_none() {
            self.idle_supported = Some(probe_idle_support(&mut session).await);
            debug!(
                channel = self.id,
                supported = self.idle_supported.unwrap_or(false),
                "probed IDLE capability"
            );
        }

        // 1. Drain any unseen messages up front. On a fresh connect this
        //    catches up anything that arrived while the worker was down.
        let mut events = match self.fetch_new_messages(&mut session).await {
            Ok(v) => v,
            Err(e) => {
                let _ = tokio::time::timeout(IMAP_OP_TIMEOUT, session.logout()).await;
                return Err(e);
            }
        };

        // 2. If there's something to deliver, or the server can't IDLE,
        //    return now. Keeping sessions short during active delivery
        //    (or when we have to fall back to polled cadence) lets the
        //    registry control backoff and shutdown tightly.
        if !events.is_empty() || self.idle_supported != Some(true) {
            let _ = tokio::time::timeout(IMAP_OP_TIMEOUT, session.logout()).await;
            return Ok(events);
        }

        // 3. Nothing new yet and IDLE is available. Block until the
        //    server pushes a notification (or the 29-min re-IDLE
        //    deadline hits, or the worker is cancelled via the outer
        //    `select!`). On a genuine notification we do ONE more fetch
        //    in the same session so events land in this poll's return
        //    rather than waiting another cycle.
        match idle_wait(session).await {
            Ok((mut session, had_notification)) => {
                if had_notification {
                    events = self
                        .fetch_new_messages(&mut session)
                        .await
                        .unwrap_or_default();
                }
                let _ = tokio::time::timeout(IMAP_OP_TIMEOUT, session.logout()).await;
                Ok(events)
            }
            Err(e) => {
                // Server advertised IDLE but rejected it. Latch the
                // capability to false so we don't try again; we'll keep
                // ingesting through the drain in step 1 on every poll.
                warn!(
                    channel = self.id,
                    error = %e,
                    "IDLE failed despite CAPABILITY advertisement; falling back to polled cadence"
                );
                self.idle_supported = Some(false);
                Ok(events)
            }
        }
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_secs(DEFAULT_POLL_SECS)
    }

    fn record_ingest_outcome(
        &mut self,
        source_ref: Option<u64>,
        external_id: &str,
        success: bool,
        error: Option<&str>,
    ) {
        let Some(uid) = source_ref.and_then(|r| u32::try_from(r).ok()) else {
            return;
        };
        if success {
            self.state.record_ingest_success(uid);
            return;
        }
        let err = error.unwrap_or("ingest failed");
        if let Some(dl) = self.state.record_ingest_failure(uid, external_id, err) {
            self.log_dead_letter(&dl);
        }
    }

    async fn flush_runtime_state(&self) -> Result<(), ChannelError> {
        self.persist_state().await
    }
}

impl EmailImapAdapter {
    /// Inner loop of [`Self::poll`] once a session is authenticated.
    /// Separated for readability — the session lifetime management
    /// stays in `poll` so there's one pairing of `open` / `logout`.
    ///
    /// Two-step fetch pattern:
    ///
    ///   1. `UID SEARCH UID {since+1}:*` returns the list of candidate
    ///      UIDs as a concrete set. Cheap round-trip, no message bodies.
    ///   2. `UID FETCH <uid>` once per UID so one unparseable message
    ///      (or IMAP-layer parser desync on a specific body) doesn't
    ///      blow up the whole batch.
    ///
    /// This replaces an earlier `UID FETCH {since+1}:*` that pulled
    /// everything in one stream. That shape fails atomically on the
    /// first bad message — leaving the channel stuck re-fetching the
    /// same poison UID forever until an operator intervenes. Per-UID
    /// fetch is a few extra round-trips, but the poll completes and
    /// advances `last_seen_uid` past each message as soon as it has
    /// been either ingested or logged off as unparseable. A failed
    /// session ends the batch with the cursor before the message it
    /// was fetching, so the next poll fetches it again ([`Step`]).
    async fn fetch_new_messages(
        &mut self,
        session: &mut ImapSession,
    ) -> Result<Vec<InboundEvent>, ChannelError> {
        let mailbox = timed("select", session.select(&self.config.mailbox)).await?;

        // Where this poll starts: on from the cursor, or, for a mailbox the
        // channel is reaching for the first time or whose UIDs were
        // renumbered, from a point worked out here ([`plan_start`]). The
        // searches run before the state changes, so a failed one leaves the
        // next poll to plan the same start again.
        let key = mailbox_key(&self.config);
        let connected_on = self.connected_at().unwrap_or_else(Utc::now).date_naive();
        let plan = plan_start(
            &self.state,
            mailbox.uid_validity,
            &key,
            self.config.backlog_since,
            connected_on,
        );
        let first_since = match plan.start {
            Start::Since(date) => {
                let query = format!("SINCE {}", date.format("%-d-%b-%Y"));
                timed("uid_search", session.uid_search(&query))
                    .await?
                    .into_iter()
                    .min()
            }
            _ => None,
        };
        let end = match plan.start {
            Start::Cursor => 0,
            _ => match mailbox.uid_next {
                Some(next) => next.saturating_sub(1),
                None => timed("uid_search", session.uid_search("ALL"))
                    .await?
                    .into_iter()
                    .max()
                    .unwrap_or(0),
            },
        };
        let start = start_cursor(&plan, self.state.last_seen_uid, first_since, end);
        let recorded = self.state.mailbox.as_deref() == Some(key.as_str());

        if plan.reset {
            debug!(
                channel = self.id,
                previous = ?self.state.uid_validity,
                current = ?mailbox.uid_validity,
                mode = ?plan.start,
                "mailbox cursor reset"
            );
            self.state.uid_validity = mailbox.uid_validity;
            // Pending retry UIDs were numbered under the old UIDVALIDITY (or
            // in another mailbox) and are meaningless now.
            self.state.retry_queue.clear();
        }
        if plan.connects {
            self.state.connected_at = Some(Utc::now());
            self.state.backlog_from = None;
        }
        if let Start::Since(date) = plan.start {
            self.state.backlog_from = Some(self.state.backlog_from.map_or(date, |f| f.min(date)));
        }

        // Persist the start right away. Even if the fetches below fail, the
        // next poll must not redo it: a reset that keeps failing would loop,
        // and a new channel would move its start point.
        if plan.reset || plan.start != Start::Cursor || !recorded {
            self.state.last_seen_uid = start;
            self.state.mailbox = Some(key);
            self.persist_state().await?;
        }

        // Re-fetch previously-failed messages (bounded ingest retry) before
        // the new-UID scan, so they're re-ingested first. These UIDs sit
        // below `last_seen_uid`, so the scan below won't pick them up.
        let mut events = self.fetch_retry_queue(session).await;

        let since = self.state.last_seen_uid;
        let search_query = format!("UID {}:*", since.saturating_add(1));
        let uid_set = timed("uid_search", session.uid_search(&search_query)).await?;
        let mut uids: Vec<u32> = uid_set.into_iter().filter(|&u| u > since).collect();
        uids.sort_unstable();

        // Cap the per-poll batch so a massive backlog (UIDVALIDITY
        // rollover on a huge mailbox, a channel that's been offline for
        // a while) catches up across several polls instead of holding
        // the session hostage for minutes on a single call.
        let total = uids.len();
        if total > MAX_FETCH_PER_POLL {
            debug!(
                channel = self.id,
                total,
                batch = MAX_FETCH_PER_POLL,
                "backlog exceeds per-poll cap; remaining UIDs will be picked up on subsequent polls"
            );
            uids.truncate(MAX_FETCH_PER_POLL);
        }

        events.reserve(uids.len());
        let mut cursor = UidCursor::new(self.state.last_seen_uid);
        let mut broken: Option<String> = None;

        for uid in uids {
            let fetched = fetch_single_uid(session, uid).await;
            let step = Step::after(&fetched);
            match fetched {
                Ok(Some(msg)) => events.push(InboundEvent::MessageReceived(msg)),
                Ok(None) => warn!(channel = self.id, uid, "fetch returned no body, skipping"),
                Err(FetchError::Transport(e)) => {
                    warn!(
                        channel = self.id,
                        uid,
                        error = %e,
                        "IMAP session failed; the message is fetched again next poll"
                    );
                    broken = Some(e);
                }
                Err(FetchError::Refused(e)) => {
                    warn!(
                        channel = self.id,
                        uid,
                        error = %e,
                        "server refused the message; retrying it on later polls"
                    );
                    if let Some(dl) = self.state.record_fetch_refused(uid, &e) {
                        self.log_dead_letter(&dl);
                    }
                }
                Err(FetchError::Garbled(e)) => {
                    warn!(
                        channel = self.id,
                        uid,
                        error = %e,
                        "skipping unparseable message; ending the batch"
                    );
                    broken = Some(e);
                }
                Err(FetchError::Unparseable(e)) => warn!(
                    channel = self.id,
                    uid,
                    error = %e,
                    "skipping unparseable message"
                ),
            }
            if !cursor.record(uid, step) {
                break;
            }
        }

        if cursor.uid() > self.state.last_seen_uid {
            self.state.last_seen_uid = cursor.uid();
            self.persist_state().await?;
        }

        // A batch that ended on a failed session before anything was fetched
        // is the poll's error, so the caller logs out and polls again rather
        // than idling on that session. With messages in hand, they go through
        // first.
        match broken {
            Some(e) if cursor.ended_early() && events.is_empty() => Err(ChannelError::Transient(e)),
            _ => Ok(events),
        }
    }

    async fn persist_state(&self) -> Result<(), ChannelError> {
        let blob =
            serde_json::to_value(&self.state).map_err(ChannelError::other("runtime_state json"))?;
        let channel_id = self.channel_id;
        // `channels` is workspace-isolated; pin so the UPDATE matches the row
        // under the runtime role. Unpinned, the RLS USING clause matches no
        // rows and the cursor write silently no-ops (reprocessing on restart).
        crate::sync::session::run_in_workspace(
            &self.pool,
            "channels:imap_persist_state",
            self.workspace_id,
            |conn| channels_repo::update_runtime_state(conn, channel_id, blob),
        )
        .map(|_| ())
        .map_err(ChannelError::transient("persist runtime_state"))
    }

    /// Re-fetch the messages whose ingest previously failed (the retry
    /// queue), in UID order, returning them as events for the registry to
    /// re-ingest. A queued message that can no longer be fetched (deleted
    /// upstream, or now unparseable) is dead-lettered here, since it can
    /// never produce a successful ingest. The mailbox is assumed already
    /// selected by the caller.
    async fn fetch_retry_queue(&mut self, session: &mut ImapSession) -> Vec<InboundEvent> {
        let mut uids: Vec<u32> = self.state.retry_queue.iter().map(|r| r.uid).collect();
        uids.sort_unstable();
        let mut events = Vec::new();
        let mut unfetchable = Vec::new();
        for uid in uids {
            match fetch_single_uid(session, uid).await {
                Ok(Some(msg)) => events.push(InboundEvent::MessageReceived(msg)),
                // The session failed, not the message: it and the rest of
                // the queue stay queued for the next poll.
                Err(FetchError::Transport(e)) => {
                    warn!(
                        channel = self.id,
                        uid,
                        error = %e,
                        "IMAP session failed during retries; retrying next poll"
                    );
                    break;
                }
                // Refused again: one more attempt counted, dead-lettered at
                // the cap.
                Err(FetchError::Refused(e)) => {
                    if let Some(dl) = self.state.record_fetch_refused(uid, &e) {
                        self.log_dead_letter(&dl);
                    }
                }
                // Skipped like any unreadable message, but the session may
                // be out of step: the rest wait for the next poll.
                Err(FetchError::Garbled(_)) => {
                    unfetchable.push(uid);
                    break;
                }
                Ok(None) | Err(FetchError::Unparseable(_)) => unfetchable.push(uid),
            }
        }
        for uid in unfetchable {
            // A queued message we can no longer fetch can never ingest; give up.
            if self.state.retry_queue.iter().any(|r| r.uid == uid) {
                let entry = self
                    .state
                    .dead_letter(uid, "message no longer fetchable for retry".to_string());
                self.log_dead_letter(&entry);
            }
        }
        events
    }

    fn log_dead_letter(&self, dl: &IngestDeadLetter) {
        warn!(
            channel = self.id,
            uid = dl.uid,
            external_id = %dl.external_id,
            attempts = dl.attempts,
            error = %dl.last_error,
            "inbound message dead-lettered; it will not be retried"
        );
    }
}

/// Ask the server for its CAPABILITY list and return whether `IDLE`
/// is advertised. Any failure (network glitch, unexpected response)
/// is treated as "not supported" so the adapter falls back safely to
/// polled cadence rather than spinning on a broken probe. Callers
/// cache the result so this only runs once per adapter lifetime.
async fn probe_idle_support(session: &mut ImapSession) -> bool {
    match timed("capabilities", session.capabilities()).await {
        Ok(caps) => caps.has_str("IDLE"),
        Err(e) => {
            // RFC 2177 compliance detail: capabilities can change across
            // LOGIN (and some servers report different sets before vs
            // after). If the probe fails we'd rather assume no IDLE
            // than try it and log noisily every poll.
            debug!(error = %e, "CAPABILITY probe failed; assuming IDLE unsupported");
            false
        }
    }
}

/// Enter IMAP IDLE and block until the server pushes a notification,
/// `IDLE_WAIT` elapses, or the caller drops this future (shutdown).
///
/// Returns the session so the caller can use it for a follow-up fetch
/// if a notification actually arrived; the bool is `true` when the
/// IDLE loop saw server data (as opposed to a timeout or keep-alive),
/// which is the only case where an immediate fetch is worth it.
async fn idle_wait(session: ImapSession) -> Result<(ImapSession, bool), ChannelError> {
    use async_imap::extensions::idle::IdleResponse;

    let mut handle = session.idle();
    timed("idle init", handle.init()).await?;

    // `wait_with_timeout` returns a `(future, StopSource)` pair. The
    // StopSource binding is **load-bearing**: the wait future internally
    // checks a token derived from it, and dropping the StopSource
    // cancels the wait. If a future refactor removes the `_stop`
    // binding as "unused", the wait will cancel immediately on every
    // call. Keep the binding. Dropping naturally at function exit (or
    // earlier, on tokio cancellation) tears down the IDLE cleanly.
    let (wait_fut, _stop) = handle.wait_with_timeout(IDLE_WAIT);
    let response = wait_fut
        .await
        .map_err(ChannelError::transient("idle wait"))?;

    let had_notification = matches!(response, IdleResponse::NewData(_));
    let session = timed("idle done", handle.done()).await?;
    Ok((session, had_notification))
}

/// Why a UID's fetch produced no message.
#[derive(Debug)]
enum FetchError {
    /// The session failed (a timeout, a dropped connection, an IO error), so
    /// nothing is known about the message. It is fetched again next poll.
    Transport(String),
    /// The server answered the fetch with NO or BAD. A reply means the
    /// session still works, so this is the message's failure: it goes to the
    /// retry queue and the batch goes on. (A BAD can't be told apart from a
    /// session out of step, so it is treated the same way rather than holding
    /// the channel at this message.)
    Refused(String),
    /// The server's response for this message couldn't be read. The message
    /// is skipped, and the batch ends because the session may be out of
    /// step.
    Garbled(String),
    /// The message was fetched but isn't a readable RFC 5322 message, and
    /// never will be. It is skipped.
    Unparseable(String),
}

impl FetchError {
    fn from_imap(label: &str, e: async_imap::error::Error) -> Self {
        use async_imap::error::Error;
        match e {
            Error::No(_) | Error::Bad(_) => Self::Refused(format!("{label}: {e}")),
            Error::Parse(_) => Self::Garbled(format!("{label}: {e}")),
            _ => Self::Transport(format!("{label}: {e}")),
        }
    }

    fn timed_out(label: &str) -> Self {
        Self::Transport(format!(
            "{label}: timed out after {}s",
            IMAP_OP_TIMEOUT.as_secs()
        ))
    }
}

/// What a UID's fetch does to the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Handled (fetched, gone, unparseable, or refused and queued for retry):
    /// the cursor moves past it and the batch goes on.
    Next,
    /// Skipped, but the session can't be trusted: the cursor moves past it
    /// and the batch ends.
    SkipAndStop,
    /// Not fetched: the cursor stays before it and the batch ends.
    Stop,
}

impl Step {
    fn after(fetched: &Result<Option<InboundMessage>, FetchError>) -> Self {
        match fetched {
            Ok(_) | Err(FetchError::Unparseable(_) | FetchError::Refused(_)) => Self::Next,
            Err(FetchError::Garbled(_)) => Self::SkipAndStop,
            Err(FetchError::Transport(_)) => Self::Stop,
        }
    }
}

/// Where a poll's search starts before it goes on from the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Start {
    /// On from the stored cursor.
    Cursor,
    /// After the newest message in the mailbox.
    End,
    /// Before the first message the server finds since this date.
    Since(NaiveDate),
}

/// What a poll does with the stored cursor before it searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartPlan {
    /// The stored cursor and retry queue don't describe this mailbox: the
    /// channel is reaching it for the first time, or its UIDs were
    /// renumbered.
    reset: bool,
    /// This poll connects the channel to the mailbox and records when.
    connects: bool,
    start: Start,
}

/// Plan where a poll starts, from the stored state, the UIDVALIDITY the
/// server just reported for `mailbox` ([`mailbox_key`]), the channel's
/// backlog date and the day it connected.
///
/// - A channel that has never reached a mailbox, or whose settings now
///   name another one, connects: it starts after the newest message, or at
///   its backlog date. Nothing older comes in.
/// - Renumbered UIDs (a changed UIDVALIDITY) mean a rescan, from the day
///   the channel connected or its backlog date, whichever is earlier. The
///   mail before that was never ingested, so a rescan from UID 1 would
///   bring the mailbox's whole history in. Mail already ingested is skipped
///   by its Message-ID.
/// - A server that reports no UIDVALIDITY gives nothing to compare, so the
///   cursor stands.
/// - A backlog date set earlier than any searched so far, on a channel
///   already connected, moves the cursor back to bring that mail in.
fn plan_start(
    state: &ImapRuntimeState,
    uid_validity: Option<u32>,
    mailbox: &str,
    backlog_since: Option<NaiveDate>,
    connected_on: NaiveDate,
) -> StartPlan {
    let never_polled =
        state.connected_at.is_none() && state.uid_validity.is_none() && state.last_seen_uid == 0;
    let elsewhere = state.mailbox.as_deref().is_some_and(|m| m != mailbox);
    if never_polled || elsewhere {
        return StartPlan {
            reset: true,
            connects: true,
            start: backlog_since.map_or(Start::End, Start::Since),
        };
    }
    let renumbered = match (state.uid_validity, uid_validity) {
        (Some(before), Some(now)) => before != now,
        (None, None) => false,
        _ => true,
    };
    if renumbered {
        let from = [Some(connected_on), backlog_since, state.backlog_from]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(connected_on);
        return StartPlan {
            reset: true,
            connects: false,
            start: Start::Since(from),
        };
    }
    let start = match backlog_since {
        Some(date) if date < connected_on && state.backlog_from.is_none_or(|from| date < from) => {
            Start::Since(date)
        }
        _ => Start::Cursor,
    };
    StartPlan {
        reset: false,
        connects: false,
        start,
    }
}

/// The cursor a planned start leaves, given the stored cursor, the first
/// UID the server found since the planned date (if it searched one) and
/// the newest UID in the mailbox. Moving back for a backlog never moves
/// the cursor forward past mail not yet fetched.
fn start_cursor(plan: &StartPlan, last_seen_uid: u32, first_since: Option<u32>, end: u32) -> u32 {
    match (plan.start, first_since) {
        (Start::Cursor, _) => last_seen_uid,
        (Start::End, _) => end,
        (Start::Since(_), Some(first)) if plan.reset => first.saturating_sub(1),
        (Start::Since(_), Some(first)) => last_seen_uid.min(first.saturating_sub(1)),
        (Start::Since(_), None) if plan.reset => end,
        (Start::Since(_), None) => last_seen_uid,
    }
}

/// The highest UID a batch has handled, which is where the next poll's
/// search starts, and whether the batch ended before its last UID.
#[derive(Debug)]
struct UidCursor {
    uid: u32,
    ended_early: bool,
}

impl UidCursor {
    fn new(last_seen_uid: u32) -> Self {
        Self {
            uid: last_seen_uid,
            ended_early: false,
        }
    }

    /// Record `uid`'s step; `false` when the batch ends here.
    fn record(&mut self, uid: u32, step: Step) -> bool {
        if step != Step::Stop {
            self.uid = self.uid.max(uid);
        }
        self.ended_early = step != Step::Next;
        !self.ended_early
    }

    fn uid(&self) -> u32 {
        self.uid
    }

    fn ended_early(&self) -> bool {
        self.ended_early
    }
}

/// Fetch a single UID and parse it. Errors say whether the session or the
/// message failed ([`FetchError`]), so the caller can retry the one and skip
/// the other. `Ok(None)` means the fetch succeeded but the server didn't
/// return a body for this UID (message deleted between SEARCH and FETCH,
/// typically).
async fn fetch_single_uid(
    session: &mut ImapSession,
    uid: u32,
) -> Result<Option<InboundMessage>, FetchError> {
    let stream = match tokio::time::timeout(
        IMAP_OP_TIMEOUT,
        session.uid_fetch(uid.to_string(), FETCH_SPEC),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(FetchError::from_imap("uid_fetch", e)),
        Err(_) => return Err(FetchError::timed_out("uid_fetch")),
    };
    let fetches: Vec<_> = match tokio::time::timeout(IMAP_OP_TIMEOUT, stream.try_collect()).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(FetchError::from_imap("fetch stream", e)),
        Err(_) => return Err(FetchError::timed_out("fetch stream")),
    };

    let Some(fetch) = fetches.into_iter().find(|f| f.uid == Some(uid)) else {
        return Ok(None);
    };
    let Some(body) = fetch.body() else {
        return Ok(None);
    };
    let internal_date = fetch.internal_date().map(|d| d.with_timezone(&Utc));
    parse_rfc822_into_inbound_message(body, internal_date)
        .map(|mut m| {
            // Stamp the IMAP UID so the registry can report this message's
            // ingest outcome for bounded retry / dead-lettering.
            m.source_ref = Some(uid as u64);
            Some(m)
        })
        .map_err(|e| FetchError::Unparseable(format!("parse rfc822: {e}")))
}

// ---------- Pure parser ----------

/// Stable stand-in for a missing Message-ID: a digest of the raw message.
fn synthesized_message_id(raw: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(raw));
    format!("<{}@no-message-id.nosdesk.invalid>", &digest[..32])
}

/// Parse raw RFC 5322 bytes into the channel-agnostic [`InboundMessage`]
/// shape. Called once per fetched message by the future poll loop.
///
/// The `received_at` parameter lets the caller stamp the message with
/// IMAP-reported INTERNALDATE rather than the Date header (Date can be
/// forged or clock-skewed). When the caller doesn't have INTERNALDATE,
/// we fall back to the Date header, then to `now()`.
pub fn parse_rfc822_into_inbound_message(
    raw: &[u8],
    internal_date: Option<DateTime<Utc>>,
) -> Result<InboundMessage, ChannelError> {
    let parsed = mailparse::parse_mail(raw).map_err(ChannelError::other("mailparse"))?;

    let headers = &parsed.headers;

    // Message-ID is only RECOMMENDED (RFC 5322 3.6.4) and some senders omit
    // it. Derive one from the raw bytes so the message still ingests and a
    // redelivery of the same bytes still dedupes.
    let external_id = header_first(headers, "Message-ID")
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| synthesized_message_id(raw));

    let from_raw = header_first(headers, "From")
        .ok_or_else(|| ChannelError::Other("message has no From header".into()))?;
    let (display_name, from_email) = parse_mailbox(&from_raw);
    let from = ExternalIdentity {
        provider: "email_imap".into(),
        external_id: from_email.clone(),
        display_name,
        known_email: Some(from_email),
    };

    let subject = header_first(headers, "Subject").map(|s| s.trim().to_string());

    let (body_text, body_html) = extract_bodies(&parsed);
    let attachments = extract_attachments(&parsed);
    let references = extract_references_chain(headers);
    let recipients = collect_recipients(headers);
    let loop_markers = detect_loop_markers(headers);
    let is_bounce = detect_bounce(&parsed, headers);
    // Parse the DSN MIME structure only when we've flagged the
    // message as a bounce; saves the walk on every normal email.
    // Returns one report per per-recipient block per RFC 3464 §2.1.
    let bounce_reports = if is_bounce {
        crate::services::channels::bounce_parser::parse_bounce(&parsed)
    } else {
        Vec::new()
    };

    let received_at = internal_date
        .or_else(|| header_first(headers, "Date").and_then(parse_rfc2822_to_utc))
        .unwrap_or_else(Utc::now);

    // RFC 3282 Content-Language is comma-separated when multi-
    // lingual; take the first tag. Trimmed and lower-cased so the
    // downstream locale parser ("en-au" -> "en-AU") doesn't have
    // to deal with whatever casing the sender produced.
    let content_language = header_first(headers, "Content-Language")
        .and_then(|raw| raw.split(',').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());

    // Keep *all* headers verbatim so debugging, audit, or future features
    // (e.g. DKIM/SPF extraction) never need a re-fetch.
    let raw_metadata = serde_json::json!({
        "headers": headers
            .iter()
            .map(|h| (h.get_key(), h.get_value()))
            .collect::<Vec<_>>(),
    });

    // Self-host From-authentication: DMARC result from the receiving server's
    // Authentication-Results (topmost trusted). The SES forwarding path
    // overrides this from the receipt after parse. `INBOUND_TRUSTED_AUTHSERV_ID`
    // pins which boundary's header we trust; unset falls back to the topmost.
    let auth_results: Vec<String> = headers
        .iter()
        .filter(|h| h.get_key().eq_ignore_ascii_case("Authentication-Results"))
        .map(|h| h.get_value())
        .collect();
    let trusted_authserv_id = std::env::var("INBOUND_TRUSTED_AUTHSERV_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let sender_auth = super::mail_auth::sender_auth(&auth_results, trusted_authserv_id.as_deref());

    Ok(InboundMessage {
        external_id,
        from,
        subject,
        body_text,
        body_html,
        attachments,
        references,
        received_at,
        loop_markers,
        raw_metadata,
        recipients,
        is_bounce,
        bounce_reports,
        // Stamp the raw bytes so the pipeline can persist the .eml
        // for "Show original message" and re-parsing on policy
        // change. One copy at parse time, cheap; alternative would
        // be to pass the bytes through alongside the parsed
        // message and that complicates every adapter signature.
        raw_bytes: Some(raw.to_vec()),
        content_language,
        // Set by the caller (fetch_single_uid) from the IMAP UID; the parser
        // itself has no transport context.
        source_ref: None,
        // Spam is a transport-level verdict (SES on the forwarding path), not
        // something the MIME parser decides; the caller sets it.
        spam_suspected: false,
        sender_auth,
    })
}

/// Detect whether a parsed inbound email is a delivery-status
/// notification (a bounce). Three signals, any of which is taken as
/// sufficient because spammy / malformed bounces routinely satisfy
/// only one:
///
/// 1. Top-level `Content-Type` is `multipart/report` with the
///    `report-type=delivery-status` parameter. This is RFC 3464's
///    structured form and the most reliable signal when present.
/// 2. `Auto-Submitted` header carries `auto-replied` plus a
///    parenthesised `(delivery-status)` annotation. Less common in
///    practice but specified in RFC 3834.
/// 3. The envelope sender looks like a postmaster bounce address —
///    `mailer-daemon@*` or `postmaster@*` (case-insensitive). Falls
///    in last because spam can spoof these; combined with the
///    other signals it's directional, alone it's just a hint.
fn detect_bounce(parsed: &mailparse::ParsedMail, headers: &[mailparse::MailHeader]) -> bool {
    let ctype = parsed.ctype.mimetype.to_ascii_lowercase();
    if ctype == "multipart/report" {
        if let Some(rt) = parsed.ctype.params.get("report-type") {
            if rt.eq_ignore_ascii_case("delivery-status") {
                return true;
            }
        }
    }

    if let Some(auto) = header_first(headers, "Auto-Submitted") {
        let lc = auto.to_ascii_lowercase();
        if lc.contains("delivery-status") || lc.contains("auto-replied") && lc.contains("dsn") {
            return true;
        }
    }

    if let Some(from) = header_first(headers, "From") {
        let (_, addr) = parse_mailbox(&from);
        let lc = addr.to_ascii_lowercase();
        if lc.starts_with("mailer-daemon@") || lc.starts_with("postmaster@") {
            return true;
        }
    }

    false
}

// ---------- Header helpers ----------

fn header_first(headers: &[mailparse::MailHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|h| h.get_key_ref().eq_ignore_ascii_case(name))
        .map(|h| h.get_value())
}

fn header_all(headers: &[mailparse::MailHeader], name: &str) -> Vec<String> {
    headers
        .iter()
        .filter(|h| h.get_key_ref().eq_ignore_ascii_case(name))
        .map(|h| h.get_value())
        .collect()
}

/// Split `Name <email@host>` into (name, email). Without a name, the name
/// comes from the address (its local part, without a `+tag`).
fn parse_mailbox(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    let (name, email) = match (trimmed.rfind('<'), trimmed.rfind('>')) {
        (Some(lt), Some(gt)) if lt < gt => (
            trimmed[..lt].trim().trim_matches('"').trim().to_string(),
            trimmed[lt + 1..gt].trim().to_string(),
        ),
        _ => (String::new(), trimmed.to_string()),
    };
    if !name.is_empty() {
        return (name, email);
    }
    let name = crate::utils::name_from_email(&email).unwrap_or_else(|| email.clone());
    (name, email)
}

fn extract_bodies(mail: &mailparse::ParsedMail) -> (String, Option<String>) {
    // Walk every part. First text/plain wins for body_text; first
    // text/html wins for body_html. Attachments are excluded via
    // Content-Disposition check. When the chosen text/plain part
    // advertises `format=flowed` (RFC 3676 — Apple Mail and
    // Thunderbird are the common emitters), we unfold soft-wrapped
    // lines before storage so downstream consumers (quote splitter,
    // search indexer, plaintext renderer) all see a single logical
    // paragraph per paragraph rather than 78-column fragments.
    let mut text: Option<String> = None;
    let mut html: Option<String> = None;

    walk(mail, &mut |part| {
        if is_attachment(part) {
            return;
        }
        let ctype = part.ctype.mimetype.to_ascii_lowercase();
        match ctype.as_str() {
            "text/plain" if text.is_none() => {
                let body = part.get_body().ok();
                text = body.map(|raw| maybe_unfold_flowed(&part.ctype.params, raw));
            }
            "text/html" if html.is_none() => {
                html = part.get_body().ok();
            }
            _ => {}
        }
    });

    (text.unwrap_or_default(), html)
}

/// Apply the RFC 3676 unfolder when the part's Content-Type
/// parameters say `format=flowed`. `delsp=yes` triggers the
/// space-strip variant most useful for CJK content. Non-flowed
/// parts pass through unmodified so we don't disturb plaintext
/// from mail clients that emit hard-wrapped bodies.
fn maybe_unfold_flowed(
    params: &std::collections::BTreeMap<String, String>,
    body: String,
) -> String {
    let format_is_flowed = params
        .get("format")
        .map(|v| v.eq_ignore_ascii_case("flowed"))
        .unwrap_or(false);
    if !format_is_flowed {
        return body;
    }
    let delsp = params
        .get("delsp")
        .map(|v| v.eq_ignore_ascii_case("yes"))
        .unwrap_or(false);
    super::email_format_flowed::unfold(&body, delsp)
}

fn extract_attachments(mail: &mailparse::ParsedMail) -> Vec<InboundAttachment> {
    let mut out = Vec::new();
    walk(mail, &mut |part| {
        if !is_attachment(part) {
            return;
        }
        let mime_type = part.ctype.mimetype.clone();
        let filename = attachment_filename(part).unwrap_or_else(|| "attachment.bin".into());
        let bytes = match part.get_body_raw() {
            Ok(b) => b,
            Err(_) => return,
        };
        out.push(InboundAttachment::Inline {
            filename,
            mime_type,
            bytes,
        });
    });
    out
}

fn attachment_filename(part: &mailparse::ParsedMail) -> Option<String> {
    // Prefer Content-Disposition `filename=`, fall back to
    // Content-Type `name=`. mailparse exposes both via params.
    if let Some(name) = part.ctype.params.get("name") {
        return Some(name.clone());
    }
    // Content-Disposition params are parsed from the header.
    if let Some(raw) = header_first(&part.headers, "Content-Disposition") {
        // Very light parse: look for `filename="..."` or `filename=...`
        if let Some(start) = raw.to_ascii_lowercase().find("filename=") {
            let rest = &raw[start + "filename=".len()..];
            let trimmed = rest.trim_start();
            let unquoted = trimmed
                .trim_start_matches('"')
                .split(['"', ';'])
                .next()?
                .trim();
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

fn is_attachment(part: &mailparse::ParsedMail) -> bool {
    // Explicit attachment disposition.
    if let Some(disp) = header_first(&part.headers, "Content-Disposition") {
        let lc = disp.to_ascii_lowercase();
        if lc.contains("attachment") {
            return true;
        }
        if lc.contains("inline") && attachment_filename(part).is_some() {
            // Inline image with a filename — still worth saving as an
            // attachment (e.g. pasted screenshots from Outlook).
            return true;
        }
    }
    // Non-text, non-multipart parts we treat as attachments.
    let mt = part.ctype.mimetype.to_ascii_lowercase();
    !mt.starts_with("text/") && !mt.starts_with("multipart/")
}

fn walk<'a, F: FnMut(&'a mailparse::ParsedMail<'a>)>(
    mail: &'a mailparse::ParsedMail<'a>,
    f: &mut F,
) {
    f(mail);
    for sub in &mail.subparts {
        walk(sub, f);
    }
}

/// Extract In-Reply-To + References IDs in the order expected by the
/// threading cascade: most-recent first. The References header can
/// contain multiple space-separated IDs; `In-Reply-To` gets pushed
/// first so the direct parent beats older ancestors.
fn extract_references_chain(headers: &[mailparse::MailHeader]) -> Vec<String> {
    let mut out = Vec::new();

    for raw in header_all(headers, "In-Reply-To") {
        for id in tokenize_message_ids(&raw) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    for raw in header_all(headers, "References") {
        for id in tokenize_message_ids(&raw) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// Tokenize a header value into `<id@host>` pieces. Some clients emit
/// multiple IDs space-separated; some emit them on folded lines. We
/// strip whitespace between `<>` blocks and keep each intact.
fn tokenize_message_ids(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '<' {
            let mut buf = String::new();
            for ch in chars.by_ref() {
                buf.push(ch);
                if ch == '>' {
                    break;
                }
            }
            if buf.ends_with('>') {
                out.push(buf);
            }
        } else {
            chars.next();
        }
    }
    out
}

fn collect_recipients(headers: &[mailparse::MailHeader]) -> Vec<String> {
    let mut out = Vec::new();
    for name in [
        "To",
        "Cc",
        "Bcc",
        "Delivered-To",
        "Envelope-To",
        "X-Original-To",
    ] {
        for raw in header_all(headers, name) {
            for (_, email) in split_address_list(&raw) {
                if !out.iter().any(|e: &String| e.eq_ignore_ascii_case(&email)) {
                    out.push(email);
                }
            }
        }
    }
    out
}

fn split_address_list(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_mailbox)
        .collect()
}

/// Detect RFC 3834 auto-reply markers and map them onto the channel-
/// neutral `LoopMarkers` signals. Mapping per `LoopMarkers` docs:
///   - `is_auto_reply` ← `Auto-Submitted` header != "no"
///   - `is_bulk`       ← `Precedence: bulk | list | junk`
///   - `is_suppressed` ← `X-Loop` or `X-Auto-Response-Suppress`
fn detect_loop_markers(headers: &[mailparse::MailHeader]) -> LoopMarkers {
    let is_auto_reply = header_first(headers, "Auto-Submitted")
        .map(|v| !v.trim().eq_ignore_ascii_case("no"))
        .unwrap_or(false);

    let is_bulk = header_first(headers, "Precedence")
        .map(|v| {
            let lc = v.trim().to_ascii_lowercase();
            lc == "bulk" || lc == "list" || lc == "junk"
        })
        .unwrap_or(false);

    let is_suppressed = headers.iter().any(|h| {
        let k = h.get_key_ref();
        k.eq_ignore_ascii_case("X-Loop")
            || k.eq_ignore_ascii_case("X-Auto-Response-Suppress")
            || k.eq_ignore_ascii_case("X-Autoreply")
            || k.eq_ignore_ascii_case("X-Autorespond")
    });

    LoopMarkers {
        is_auto_reply,
        is_bulk,
        is_suppressed,
    }
}

fn parse_rfc2822_to_utc(raw: String) -> Option<DateTime<Utc>> {
    match chrono::DateTime::parse_from_rfc2822(raw.trim()) {
        Ok(dt) => Some(dt.with_timezone(&Utc)),
        Err(e) => {
            debug!(error = %e, raw = %raw, "failed to parse Date header");
            None
        }
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &[u8]) -> mailparse::ParsedMail<'_> {
        mailparse::parse_mail(raw).unwrap()
    }

    #[test]
    fn detect_bounce_via_content_type_report() {
        let raw = b"From: postmaster@example.com\r\n\
                    To: support@yourco.com\r\n\
                    Subject: Delivery Status Notification (Failure)\r\n\
                    Message-ID: <bounce@example.com>\r\n\
                    Content-Type: multipart/report; report-type=delivery-status; boundary=\"b\"\r\n\
                    \r\n\
                    --b\r\nContent-Type: text/plain\r\n\r\nThis is a bounce.\r\n--b--\r\n";
        let p = parsed(raw);
        assert!(detect_bounce(&p, &p.headers));
    }

    #[test]
    fn a_message_without_message_id_still_parses_with_a_stable_id() {
        let raw = b"From: Guest <guest@example.com>\r\n\
                    To: token@inbound.example.com\r\n\
                    Subject: Re: [#51] Printer\r\n\
                    Content-Type: text/plain\r\n\
                    \r\nStill broken.\r\n";
        let first = parse_rfc822_into_inbound_message(raw, None).expect("parses");
        let again = parse_rfc822_into_inbound_message(raw, None).expect("parses");
        assert!(first
            .external_id
            .ends_with("@no-message-id.nosdesk.invalid>"));
        assert_eq!(first.external_id, again.external_id, "redelivery dedupes");
    }

    #[test]
    fn detect_bounce_via_mailer_daemon_sender() {
        let raw = b"From: MAILER-DAEMON@hosting.example.com\r\n\
                    To: support@yourco.com\r\n\
                    Subject: failure notice\r\n\
                    Message-ID: <md@example.com>\r\n\
                    Content-Type: text/plain\r\n\
                    \r\nUndelivered.\r\n";
        let p = parsed(raw);
        assert!(detect_bounce(&p, &p.headers));
    }

    #[test]
    fn detect_bounce_via_postmaster_sender() {
        let raw = b"From: \"Postmaster\" <postmaster@isp.example.net>\r\n\
                    To: support@yourco.com\r\n\
                    Subject: Returned mail\r\n\
                    Message-ID: <pm@example.com>\r\n\
                    Content-Type: text/plain\r\n\
                    \r\nUser unknown.\r\n";
        let p = parsed(raw);
        assert!(detect_bounce(&p, &p.headers));
    }

    #[test]
    fn detect_bounce_ignores_normal_reply() {
        let raw = b"From: alice@example.com\r\n\
                    To: support@yourco.com\r\n\
                    Subject: Re: ticket #42\r\n\
                    Message-ID: <reply@example.com>\r\n\
                    Content-Type: text/plain\r\n\
                    \r\nThanks for the update.\r\n";
        let p = parsed(raw);
        assert!(!detect_bounce(&p, &p.headers));
    }

    #[test]
    fn detect_bounce_ignores_auto_reply_without_dsn_marker() {
        // Out-of-office: Auto-Submitted is set but not as a DSN.
        let raw = b"From: bob@example.com\r\n\
                    To: support@yourco.com\r\n\
                    Subject: Out of office\r\n\
                    Message-ID: <ooo@example.com>\r\n\
                    Auto-Submitted: auto-replied\r\n\
                    Content-Type: text/plain\r\n\
                    \r\nI'm away.\r\n";
        let p = parsed(raw);
        assert!(!detect_bounce(&p, &p.headers));
    }

    #[test]
    fn runtime_state_roundtrips_through_json() {
        let state = ImapRuntimeState {
            last_seen_uid: 42,
            uid_validity: Some(123),
            last_error: None,
            retry_queue: Vec::new(),
            dead_letters: Vec::new(),
            ..Default::default()
        };
        let v = serde_json::to_value(&state).unwrap();
        let back: ImapRuntimeState = serde_json::from_value(v).unwrap();
        assert_eq!(back.last_seen_uid, 42);
        assert_eq!(back.uid_validity, Some(123));
    }

    #[test]
    fn ingest_retry_escalates_to_dead_letter_at_the_cap() {
        let mut s = ImapRuntimeState::default();
        // First failure schedules a retry (attempts = 1, not yet dead).
        assert!(s.record_ingest_failure(7, "<m7@ex>", "db blip").is_none());
        assert_eq!(s.retry_queue.len(), 1);
        assert_eq!(s.retry_queue[0].attempts, 1);
        // Failures advance attempts; nothing dead-letters before the cap.
        for _ in 0..(MAX_INGEST_ATTEMPTS - 2) {
            assert!(s.record_ingest_failure(7, "<m7@ex>", "db blip").is_none());
        }
        assert_eq!(s.retry_queue[0].attempts, MAX_INGEST_ATTEMPTS - 1);
        // The attempt that reaches the cap dead-letters it and clears the queue.
        let dl = s
            .record_ingest_failure(7, "<m7@ex>", "db blip")
            .expect("should dead-letter at the cap");
        assert_eq!(dl.uid, 7);
        assert_eq!(dl.attempts, MAX_INGEST_ATTEMPTS);
        assert_eq!(dl.external_id, "<m7@ex>");
        assert!(s.retry_queue.is_empty());
        assert_eq!(s.dead_letters.len(), 1);
    }

    #[test]
    fn ingest_success_clears_a_pending_retry() {
        let mut s = ImapRuntimeState::default();
        s.record_ingest_failure(9, "<m9@ex>", "blip");
        assert_eq!(s.retry_queue.len(), 1);
        s.record_ingest_success(9);
        assert!(s.retry_queue.is_empty());
        assert!(s.dead_letters.is_empty());
    }

    #[test]
    fn dead_letters_are_capped_keeping_the_most_recent() {
        let mut s = ImapRuntimeState::default();
        for uid in 0..(DEAD_LETTER_CAP as u32 + 10) {
            s.dead_letter(uid, "gone".to_string());
        }
        assert_eq!(s.dead_letters.len(), DEAD_LETTER_CAP);
        assert_eq!(s.dead_letters.first().unwrap().uid, 10);
        assert_eq!(
            s.dead_letters.last().unwrap().uid,
            DEAD_LETTER_CAP as u32 + 9
        );
    }

    #[test]
    fn a_dropped_session_is_transport_and_a_garbled_response_is_not() {
        use async_imap::error::{Error, ParseError};
        let transport = |e: Error| {
            matches!(
                FetchError::from_imap("uid_fetch", e),
                FetchError::Transport(_)
            )
        };
        assert!(transport(Error::ConnectionLost));
        assert!(transport(Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset"
        ))));
        let refused = |e: Error| {
            matches!(
                FetchError::from_imap("uid_fetch", e),
                FetchError::Refused(_)
            )
        };
        assert!(refused(Error::No(
            "Some messages could not be FETCHed".into()
        )));
        assert!(refused(Error::Bad("command unknown".into())));
        assert!(matches!(
            FetchError::timed_out("fetch stream"),
            FetchError::Transport(_)
        ));
        assert!(matches!(
            FetchError::from_imap(
                "fetch stream",
                Error::Parse(ParseError::Invalid(b"*".to_vec()))
            ),
            FetchError::Garbled(_)
        ));
    }

    #[test]
    fn only_a_session_failure_keeps_the_cursor_before_the_message() {
        let step = |r: Result<Option<InboundMessage>, FetchError>| Step::after(&r);
        assert_eq!(step(Ok(None)), Step::Next);
        assert_eq!(
            step(Err(FetchError::Unparseable("parse rfc822".into()))),
            Step::Next
        );
        assert_eq!(
            step(Err(FetchError::Refused("uid_fetch: no response".into()))),
            Step::Next
        );
        assert_eq!(
            step(Err(FetchError::Garbled("fetch stream".into()))),
            Step::SkipAndStop
        );
        assert_eq!(
            step(Err(FetchError::Transport("uid_fetch".into()))),
            Step::Stop
        );
    }

    /// The batch loop's cursor over a run of fetches: where it ends, how many
    /// UIDs were tried, and whether the batch ended early.
    fn batch(start: u32, steps: &[(u32, Step)]) -> (u32, usize, bool) {
        let mut cursor = UidCursor::new(start);
        let mut tried = 0;
        for &(uid, step) in steps {
            tried += 1;
            if !cursor.record(uid, step) {
                break;
            }
        }
        (cursor.uid(), tried, cursor.ended_early())
    }

    #[test]
    fn a_connection_error_stops_the_batch_before_the_message() {
        use Step::*;
        // 512's fetch hits a dead session: 512 onward is fetched next poll.
        assert_eq!(
            batch(510, &[(511, Next), (512, Stop), (513, Next), (514, Next)]),
            (511, 2, true)
        );
        // On the first UID, the cursor doesn't move at all.
        assert_eq!(batch(510, &[(511, Stop), (512, Next)]), (510, 1, true));
    }

    /// A NO for one UID queues it for retry and the batch moves on; it is
    /// dead-lettered once it has been refused [`MAX_INGEST_ATTEMPTS`] times.
    #[test]
    fn a_refused_message_is_retried_without_holding_the_channel() {
        let mut state = ImapRuntimeState::default();
        let mut cursor = UidCursor::new(510);
        for (uid, fetched) in [
            (511, Ok(None)),
            (
                512,
                Err(FetchError::Refused("uid_fetch: no response".into())),
            ),
            (513, Ok(None)),
        ] {
            if let Err(FetchError::Refused(e)) = &fetched {
                assert!(state.record_fetch_refused(uid, e).is_none());
            }
            assert!(cursor.record(uid, Step::after(&fetched)));
        }
        assert_eq!(cursor.uid(), 513, "the cursor moves past the refused UID");
        assert!(!cursor.ended_early());
        assert_eq!(
            state.retry_queue.iter().map(|r| r.uid).collect::<Vec<_>>(),
            [512]
        );

        // Refused again on each later poll's retry, up to the cap.
        for _ in 2..MAX_INGEST_ATTEMPTS {
            assert!(state.record_fetch_refused(512, "no response").is_none());
        }
        let dl = state
            .record_fetch_refused(512, "no response")
            .expect("dead-lettered at the cap");
        assert_eq!((dl.uid, dl.attempts), (512, MAX_INGEST_ATTEMPTS));
        assert!(state.retry_queue.is_empty());
        assert_eq!(state.dead_letters.len(), 1);
    }

    #[test]
    fn a_poison_message_is_skipped() {
        use Step::*;
        // Unparseable: skipped, and the batch goes on.
        assert_eq!(
            batch(510, &[(511, Next), (512, Next), (513, Next)]),
            (513, 3, false)
        );
        // Garbled: skipped, and the batch ends on a session out of step.
        assert_eq!(
            batch(510, &[(511, Next), (512, SkipAndStop), (513, Next)]),
            (512, 2, true)
        );
        // Garbled on the first UID: the batch fetched nothing and ended early,
        // which the poll reports as its error.
        assert_eq!(
            batch(510, &[(511, SkipAndStop), (512, Next)]),
            (511, 1, true)
        );
    }

    const MAILBOX: &str = "support@example.com@imap.example.com/INBOX";

    fn day(d: &str) -> NaiveDate {
        NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
    }

    /// A channel that has polled `MAILBOX` under UIDVALIDITY 7.
    fn connected(last_seen_uid: u32) -> ImapRuntimeState {
        ImapRuntimeState {
            last_seen_uid,
            uid_validity: Some(7),
            connected_at: Some(Utc::now()),
            mailbox: Some(MAILBOX.to_string()),
            ..Default::default()
        }
    }

    /// Where a poll starts: the plan and the cursor it leaves, given the
    /// first UID found since the planned date and the newest UID (500).
    fn start(
        state: &ImapRuntimeState,
        uid_validity: Option<u32>,
        backlog_since: Option<NaiveDate>,
        first_since: Option<u32>,
    ) -> (StartPlan, u32) {
        let plan = plan_start(
            state,
            uid_validity,
            MAILBOX,
            backlog_since,
            day("2026-11-01"),
        );
        let cursor = start_cursor(&plan, state.last_seen_uid, first_since, 500);
        (plan, cursor)
    }

    #[test]
    fn a_new_channel_starts_after_the_newest_message() {
        let (plan, cursor) = start(&ImapRuntimeState::default(), Some(7), None, None);
        assert_eq!(
            plan,
            StartPlan {
                reset: true,
                connects: true,
                start: Start::End,
            }
        );
        assert_eq!(cursor, 500, "nothing already in the mailbox comes in");
    }

    #[test]
    fn a_channel_that_only_ever_failed_is_still_new() {
        // `last_error` is written into runtime_state before any poll works.
        let state: ImapRuntimeState =
            serde_json::from_value(serde_json::json!({ "last_error": "login: NO" })).unwrap();
        let (plan, cursor) = start(&state, Some(7), None, None);
        assert!(plan.connects);
        assert_eq!(cursor, 500);
    }

    #[test]
    fn a_new_channel_with_a_backlog_date_starts_at_that_date() {
        let since = day("2026-01-01");
        let (plan, cursor) = start(
            &ImapRuntimeState::default(),
            Some(7),
            Some(since),
            Some(120),
        );
        assert_eq!(plan.start, Start::Since(since));
        assert!(plan.connects);
        assert_eq!(cursor, 119, "from the first message since the date");

        // No mail since then: the same as no backlog date.
        let (_, cursor) = start(&ImapRuntimeState::default(), Some(7), Some(since), None);
        assert_eq!(cursor, 500);
    }

    #[test]
    fn a_connected_channel_goes_on_from_its_cursor() {
        let (plan, cursor) = start(&connected(42), Some(7), None, None);
        assert_eq!(
            plan,
            StartPlan {
                reset: false,
                connects: false,
                start: Start::Cursor,
            }
        );
        assert_eq!(cursor, 42);

        // A channel polled before connection times were recorded, with its
        // progress and UIDVALIDITY stored, is not new.
        let legacy = ImapRuntimeState {
            last_seen_uid: 42,
            uid_validity: Some(7),
            ..Default::default()
        };
        assert_eq!(start(&legacy, Some(7), None, None), (plan, 42));
    }

    #[test]
    fn its_own_backlog_date_is_searched_once() {
        let since = day("2026-01-01");
        let state = ImapRuntimeState {
            backlog_from: Some(since),
            ..connected(42)
        };
        assert_eq!(
            start(&state, Some(7), Some(since), None).0.start,
            Start::Cursor
        );
    }

    #[test]
    fn an_earlier_backlog_date_set_later_moves_the_cursor_back() {
        let state = ImapRuntimeState {
            backlog_from: Some(day("2026-06-01")),
            ..connected(300)
        };
        let since = day("2026-01-01");
        let (plan, cursor) = start(&state, Some(7), Some(since), Some(120));
        assert_eq!(
            plan,
            StartPlan {
                reset: false,
                connects: false,
                start: Start::Since(since),
            }
        );
        assert_eq!(cursor, 119);
        // Never forward past mail not yet fetched.
        assert_eq!(start(&state, Some(7), Some(since), Some(400)).1, 300);
        assert_eq!(start(&state, Some(7), Some(since), None).1, 300);
        // A date after the channel connected brings nothing older in.
        let (plan, _) = start(&connected(300), Some(7), Some(day("2026-11-10")), None);
        assert_eq!(plan.start, Start::Cursor);
    }

    #[test]
    fn a_changed_uidvalidity_rescans_from_the_connection_day() {
        let (plan, cursor) = start(&connected(300), Some(8), None, Some(250));
        assert_eq!(
            plan,
            StartPlan {
                reset: true,
                connects: false,
                start: Start::Since(day("2026-11-01")),
            }
        );
        assert_eq!(
            cursor, 249,
            "a rescan, but not of the mail before the channel"
        );

        // Or from an earlier backlog date.
        let state = ImapRuntimeState {
            backlog_from: Some(day("2026-01-01")),
            ..connected(300)
        };
        assert_eq!(
            start(&state, Some(8), None, None).0.start,
            Start::Since(day("2026-01-01"))
        );
        // Nothing since then: the rescan starts after the newest message.
        assert_eq!(start(&connected(300), Some(8), None, None).1, 500);
        // A UIDVALIDITY that disappears is a change too.
        assert!(start(&connected(300), None, None, None).0.reset);
    }

    #[test]
    fn a_server_without_uidvalidity_keeps_its_cursor() {
        let state = ImapRuntimeState {
            uid_validity: None,
            ..connected(42)
        };
        let (plan, cursor) = start(&state, None, None, None);
        assert_eq!(plan.start, Start::Cursor);
        assert!(!plan.reset);
        assert_eq!(cursor, 42);
    }

    #[test]
    fn another_mailbox_connects_afresh() {
        let state = ImapRuntimeState {
            mailbox: Some("old@imap.example.com/INBOX".to_string()),
            ..connected(300)
        };
        let (plan, cursor) = start(&state, Some(7), None, None);
        assert!(plan.reset && plan.connects);
        assert_eq!(cursor, 500);
        // A channel connected before the mailbox was recorded keeps going.
        let state = ImapRuntimeState {
            mailbox: None,
            ..connected(300)
        };
        assert_eq!(start(&state, Some(7), None, None).1, 300);
    }

    #[test]
    fn mailbox_key_names_the_user_server_and_folder() {
        let cfg: ImapChannelConfig = serde_json::from_value(serde_json::json!({
            "host": "IMAP.Example.com ",
            "username": "support@example.com",
            "reply_domain": "example.com",
        }))
        .unwrap();
        assert_eq!(mailbox_key(&cfg), MAILBOX);
    }

    #[test]
    fn the_backlog_date_is_optional_and_read_from_config() {
        let cfg: ImapChannelConfig = serde_json::from_value(serde_json::json!({
            "host": "imap.example.com",
            "username": "support@example.com",
            "reply_domain": "example.com",
            "backlog_since": "2026-01-01",
        }))
        .unwrap();
        assert_eq!(cfg.backlog_since, Some(day("2026-01-01")));
        assert_eq!(
            day("2026-01-01").format("%-d-%b-%Y").to_string(),
            "1-Jan-2026"
        );
        let without = serde_json::to_value(ImapChannelConfig {
            backlog_since: None,
            ..cfg
        })
        .unwrap();
        assert!(without.get("backlog_since").is_none());
    }

    #[test]
    fn the_stored_connection_time_is_read_back() {
        let at = Utc::now();
        let state = ImapRuntimeState {
            connected_at: Some(at),
            ..Default::default()
        };
        let stored = serde_json::to_value(&state).unwrap();
        assert_eq!(stored_connected_at(&stored), Some(at));
        assert_eq!(stored_connected_at(&serde_json::json!({})), None);
    }

    #[test]
    fn runtime_state_defaults_gracefully_from_empty_object() {
        let state: ImapRuntimeState = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(state.last_seen_uid, 0);
        assert!(state.uid_validity.is_none());
    }

    #[test]
    fn validate_accepts_reasonable_config() {
        let cfg = ImapChannelConfig {
            host: "mail.example.com".into(),
            port: 993,
            username: "u@example.com".into(),
            mailbox: "INBOX".into(),
            use_tls: true,
            reply_domain: "example.com".into(),
            insecure_skip_cert_verify: false,
            backlog_since: None,
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_host() {
        let cfg = ImapChannelConfig {
            host: "".into(),
            port: 993,
            username: "u@example.com".into(),
            mailbox: "INBOX".into(),
            use_tls: true,
            reply_domain: "example.com".into(),
            insecure_skip_cert_verify: false,
            backlog_since: None,
        };
        assert!(cfg.validate().unwrap_err().contains("host"));
    }

    #[test]
    fn validate_rejects_zero_port() {
        let cfg = ImapChannelConfig {
            host: "mail.example.com".into(),
            port: 0,
            username: "u@example.com".into(),
            mailbox: "INBOX".into(),
            use_tls: true,
            reply_domain: "example.com".into(),
            insecure_skip_cert_verify: false,
            backlog_since: None,
        };
        assert!(cfg.validate().unwrap_err().contains("port"));
    }

    #[test]
    fn validate_rejects_whitespace_only_reply_domain() {
        let cfg = ImapChannelConfig {
            host: "mail.example.com".into(),
            port: 993,
            username: "u@example.com".into(),
            mailbox: "INBOX".into(),
            use_tls: true,
            reply_domain: "   ".into(),
            insecure_skip_cert_verify: false,
            backlog_since: None,
        };
        assert!(cfg.validate().unwrap_err().contains("reply_domain"));
    }

    #[test]
    fn validate_rejects_host_with_spaces() {
        let cfg = ImapChannelConfig {
            host: "mail example com".into(),
            port: 993,
            username: "u@example.com".into(),
            mailbox: "INBOX".into(),
            use_tls: true,
            reply_domain: "example.com".into(),
            insecure_skip_cert_verify: false,
            backlog_since: None,
        };
        assert!(cfg.validate().unwrap_err().contains("host"));
    }

    #[test]
    fn channel_config_defaults_applied() {
        let cfg: ImapChannelConfig = serde_json::from_value(serde_json::json!({
            "host": "mail.example.com",
            "username": "support@example.com",
            "reply_domain": "example.com"
        }))
        .unwrap();
        assert_eq!(cfg.port, 993);
        assert_eq!(cfg.mailbox, "INBOX");
        assert!(cfg.use_tls);
    }

    // ---------- parse_mailbox ----------

    #[test]
    fn parse_mailbox_with_display_name() {
        let (name, email) = parse_mailbox("Alice Example <alice@example.com>");
        assert_eq!(name, "Alice Example");
        assert_eq!(email, "alice@example.com");
    }

    #[test]
    fn parse_mailbox_strips_display_quotes() {
        let (name, email) = parse_mailbox("\"Alice, The Example\" <alice@example.com>");
        assert_eq!(name, "Alice, The Example");
        assert_eq!(email, "alice@example.com");
    }

    #[test]
    fn parse_mailbox_bare_address_uses_localpart_as_name() {
        let (name, email) = parse_mailbox("bob@example.com");
        assert_eq!(name, "bob");
        assert_eq!(email, "bob@example.com");
    }

    #[test]
    fn parse_mailbox_without_a_name_drops_the_tag() {
        for raw in [
            "bob+news@example.com",
            "<bob+news@example.com>",
            "\"\" <bob+news@example.com>",
        ] {
            let (name, email) = parse_mailbox(raw);
            assert_eq!(name, "bob", "{raw}");
            assert_eq!(email, "bob+news@example.com", "{raw}");
        }
    }

    // ---------- tokenize_message_ids ----------

    #[test]
    fn tokenize_message_ids_single() {
        assert_eq!(
            tokenize_message_ids("<abc@host>"),
            vec!["<abc@host>".to_string()]
        );
    }

    #[test]
    fn tokenize_message_ids_multiple_space_separated() {
        assert_eq!(
            tokenize_message_ids("<a@h> <b@h> <c@h>"),
            vec![
                "<a@h>".to_string(),
                "<b@h>".to_string(),
                "<c@h>".to_string()
            ]
        );
    }

    #[test]
    fn tokenize_message_ids_tolerates_folded_whitespace() {
        assert_eq!(
            tokenize_message_ids("<a@h>\r\n\t<b@h>"),
            vec!["<a@h>".to_string(), "<b@h>".to_string()]
        );
    }

    #[test]
    fn tokenize_message_ids_ignores_unterminated() {
        assert_eq!(tokenize_message_ids("<no-end@h"), Vec::<String>::new());
    }

    // ---------- detect_loop_markers ----------

    fn parse_headers(raw: &[u8]) -> Vec<mailparse::MailHeader<'_>> {
        let parsed = mailparse::parse_mail(raw).unwrap();
        parsed.headers
    }

    #[test]
    fn loop_markers_detects_auto_submitted() {
        let raw = b"From: a@b\r\nAuto-Submitted: auto-replied\r\nSubject: x\r\n\r\nbody";
        let headers = parse_headers(raw);
        let lm = detect_loop_markers(&headers);
        assert!(lm.is_auto_reply);
        assert!(!lm.is_bulk);
    }

    #[test]
    fn loop_markers_ignores_auto_submitted_no() {
        let raw = b"From: a@b\r\nAuto-Submitted: no\r\nSubject: x\r\n\r\nbody";
        let headers = parse_headers(raw);
        assert!(!detect_loop_markers(&headers).is_auto_reply);
    }

    #[test]
    fn loop_markers_detects_precedence_bulk() {
        let raw = b"From: a@b\r\nPrecedence: bulk\r\nSubject: x\r\n\r\nbody";
        let headers = parse_headers(raw);
        assert!(detect_loop_markers(&headers).is_bulk);
    }

    #[test]
    fn loop_markers_detects_x_loop_headers() {
        for name in [
            "X-Loop",
            "X-Auto-Response-Suppress",
            "X-Autoreply",
            "X-Autorespond",
        ] {
            let raw = format!("From: a@b\r\n{name}: 1\r\nSubject: x\r\n\r\nbody");
            let headers = parse_headers(raw.as_bytes());
            assert!(
                detect_loop_markers(&headers).is_suppressed,
                "expected {name} to be detected"
            );
        }
    }

    // ---------- parse_rfc822_into_inbound_message (end-to-end) ----------

    const SIMPLE: &[u8] = b"\
From: \"Alice\" <alice@example.com>\r\n\
To: support+ticket-42@yourco.com\r\n\
Cc: cc@example.com\r\n\
Subject: Re: [#42] Printer fire\r\n\
Message-ID: <abc@customer>\r\n\
In-Reply-To: <out-1@host>\r\n\
References: <thread-start@host> <out-1@host>\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Thanks for the help!\r\n";

    #[test]
    fn content_language_header_is_extracted() {
        const WITH_LANG: &[u8] = b"\
From: alice@example.com\r\n\
To: support@yourco.com\r\n\
Subject: Bonjour\r\n\
Message-ID: <fr-msg@customer>\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
Content-Language: fr-FR\r\n\
\r\n\
Bonjour.\r\n";
        let msg = parse_rfc822_into_inbound_message(WITH_LANG, None).unwrap();
        assert_eq!(msg.content_language.as_deref(), Some("fr-FR"));
    }

    #[test]
    fn content_language_takes_first_when_comma_separated() {
        // RFC 3282 permits a comma-separated list (multilingual
        // documents). We pick the first tag — that's the sender's
        // primary language, the one the auto-ack should match.
        const MULTI: &[u8] = b"\
From: alice@example.com\r\n\
To: support@yourco.com\r\n\
Subject: x\r\n\
Message-ID: <multi@customer>\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
Content-Language: en-AU, fr-FR\r\n\
\r\n\
hi\r\n";
        let msg = parse_rfc822_into_inbound_message(MULTI, None).unwrap();
        assert_eq!(msg.content_language.as_deref(), Some("en-AU"));
    }

    #[test]
    fn content_language_absent_yields_none() {
        let msg = parse_rfc822_into_inbound_message(SIMPLE, None).unwrap();
        assert!(msg.content_language.is_none());
    }

    #[test]
    fn parses_headers_into_inbound_message() {
        let msg = parse_rfc822_into_inbound_message(SIMPLE, None).unwrap();
        assert_eq!(msg.external_id, "<abc@customer>");
        assert_eq!(msg.from.external_id, "alice@example.com");
        assert_eq!(msg.from.display_name, "Alice");
        assert_eq!(msg.subject.as_deref(), Some("Re: [#42] Printer fire"));
        assert!(msg.body_text.contains("Thanks for the help"));
        assert_eq!(
            msg.references,
            vec![
                "<out-1@host>".to_string(), // In-Reply-To comes first.
                "<thread-start@host>".to_string(),
            ]
        );
        assert!(msg
            .recipients
            .iter()
            .any(|r| r == "support+ticket-42@yourco.com"));
        assert!(msg.recipients.iter().any(|r| r == "cc@example.com"));
        assert!(!msg.loop_markers.any());
    }

    #[test]
    fn uses_internal_date_when_provided() {
        let stamp = Utc::now();
        let msg = parse_rfc822_into_inbound_message(SIMPLE, Some(stamp)).unwrap();
        assert_eq!(msg.received_at.timestamp(), stamp.timestamp());
    }

    #[test]
    fn falls_back_to_date_header_when_internal_date_missing() {
        let msg = parse_rfc822_into_inbound_message(SIMPLE, None).unwrap();
        assert_eq!(msg.received_at.timestamp(), 1704110400); // 2024-01-01 12:00Z
    }

    #[test]
    fn extracts_html_alongside_text() {
        let raw = b"\
From: a@b\r\n\
Message-ID: <x@h>\r\n\
Subject: s\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"BOUND\"\r\n\
\r\n\
--BOUND\r\n\
Content-Type: text/plain\r\n\
\r\n\
plain variant\r\n\
--BOUND\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>html variant</p>\r\n\
--BOUND--\r\n";

        let msg = parse_rfc822_into_inbound_message(raw, None).unwrap();
        assert!(msg.body_text.contains("plain variant"));
        assert_eq!(
            msg.body_html.as_deref().map(str::trim),
            Some("<p>html variant</p>")
        );
    }

    #[test]
    fn extracts_attachment_with_filename() {
        let raw = b"\
From: a@b\r\n\
Message-ID: <x@h>\r\n\
Subject: s\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"BOUND\"\r\n\
\r\n\
--BOUND\r\n\
Content-Type: text/plain\r\n\
\r\n\
body\r\n\
--BOUND\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
aGVsbG8gd29ybGQ=\r\n\
--BOUND--\r\n";

        let msg = parse_rfc822_into_inbound_message(raw, None).unwrap();
        assert_eq!(msg.attachments.len(), 1);
        match &msg.attachments[0] {
            InboundAttachment::Inline {
                filename,
                mime_type,
                bytes,
            } => {
                assert_eq!(filename, "report.pdf");
                assert_eq!(mime_type, "application/pdf");
                assert_eq!(bytes, b"hello world");
            }
            _ => panic!("expected Inline attachment"),
        }
    }

    #[test]
    fn auto_reply_headers_surface_as_loop_markers() {
        let raw = b"\
From: a@b\r\n\
Message-ID: <x@h>\r\n\
Subject: Out of office\r\n\
Auto-Submitted: auto-replied\r\n\
\r\n\
I'll be back Monday\r\n";
        let msg = parse_rfc822_into_inbound_message(raw, None).unwrap();
        assert!(msg.loop_markers.any());
        assert!(msg.loop_markers.is_auto_reply);
    }

    #[test]
    fn probe_failures_are_classified_by_stage() {
        let code = |e: ChannelError| ImapProbeFailure::from_session_error(&e).code;
        assert_eq!(
            code(ChannelError::Transient("imap host: no such host".into())),
            "dns"
        );
        assert_eq!(
            code(ChannelError::Configuration(
                "imap host rejected: private address".into()
            )),
            "egress_blocked"
        );
        assert_eq!(
            code(ChannelError::Transient("tcp connect: refused".into())),
            "connect"
        );
        assert_eq!(
            code(ChannelError::Transient("tls handshake: bad cert".into())),
            "tls"
        );
        assert_eq!(
            code(ChannelError::Configuration(
                "login: No Response: AUTHENTICATE failed".into()
            )),
            "auth"
        );
        assert_eq!(
            code(ChannelError::Transient("login: connection lost".into())),
            "connect"
        );
        assert_eq!(
            code(ChannelError::Transient(
                "tcp connect: timed out after 30s".into()
            )),
            "timeout"
        );
        assert_eq!(
            code(ChannelError::Configuration(
                "plaintext IMAP is not supported".into()
            )),
            "invalid"
        );
    }
}
