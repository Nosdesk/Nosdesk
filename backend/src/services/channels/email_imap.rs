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
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use std::time::Duration;

use futures::TryStreamExt;

use crate::models::CRED_TYPE_IMAP_PASSWORD;
use crate::repository::channels as channels_repo;
use crate::services::channels::{
    ChannelAdapter, ChannelError, ExternalIdentity, InboundAttachment, InboundEvent,
    InboundMessage, LoopMarkers, OutboundContent, OutboundMessage, PullAdapter, ThreadContext,
};
use crate::services::channels::threading::format_outbound_message_id;
use crate::utils::email::{EmailService, OutboundEmail};

// ---------- Persistence shapes ----------

/// Shape of `channels.runtime_state` for the `email_imap` provider.
///
/// UIDVALIDITY pinning is what lets us use `last_seen_uid` safely across
/// restarts: IMAP UIDs are only monotonic within a given UIDVALIDITY, so
/// if that value changes (rare — mailbox restore, reprovision) we need
/// to rescan from UID 1. Without it we'd risk re-ingesting or skipping
/// messages after any backend operation that rotates UIDVALIDITY.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ImapRuntimeState {
    #[serde(default)]
    pub last_seen_uid: u32,
    #[serde(default)]
    pub uid_validity: Option<u32>,
    #[serde(default)]
    pub last_error: Option<String>,
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
    config: ImapChannelConfig,
    /// SMTP handle used by [`ChannelAdapter::send_reply`]. Not touched
    /// by the poll path.
    email: Arc<EmailService>,
    /// Pool used by the poll path to read credentials and persist
    /// runtime state. [`ChannelAdapter::send_reply`] ignores it.
    pool: crate::db::Pool,
    state: ImapRuntimeState,
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
        config: ImapChannelConfig,
        email: Arc<EmailService>,
        pool: crate::db::Pool,
        initial_state: ImapRuntimeState,
    ) -> Self {
        Self {
            id: format!("email_imap:{channel_id}"),
            channel_id,
            config,
            email,
            pool,
            state: initial_state,
            idle_supported: None,
        }
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
    Ok(EmailImapAdapter::new(channel.id, config, email, pool, state))
}

/// Authenticated IMAP session type used by both the test-connection
/// probe and the real poll loop. Hoisted here as a type alias so the
/// signature is one place to update when lettre/tokio-native-tls rev.
type ImapSession =
    async_imap::Session<tokio_native_tls::TlsStream<tokio::net::TcpStream>>;

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

    let addr = (config.host.as_str(), config.port);
    let tcp = timed("tcp connect", TcpStream::connect(addr)).await?;

    // Defence in depth: the admin UI already labels
    // `insecure_skip_cert_verify` as dev-only, but an admin who
    // enables it for local testing and forgets to turn it off in
    // production would silently disable TLS validation — an MITM
    // vector for mailbox credentials. Require an explicit env var
    // (`NOSDESK_ALLOW_INSECURE_TLS=1`) to actually honour the flag;
    // otherwise log loudly and fall back to real validation.
    let cert_verify_disabled = config.insecure_skip_cert_verify
        && std::env::var("NOSDESK_ALLOW_INSECURE_TLS").ok().as_deref() == Some("1");
    if config.insecure_skip_cert_verify && !cert_verify_disabled {
        tracing::warn!(
            host = %config.host,
            "channel has insecure_skip_cert_verify=true but NOSDESK_ALLOW_INSECURE_TLS is not set; \
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

/// Probe an IMAP server with the given config and password. Connects,
/// authenticates, examines the configured mailbox, logs out. Returns
/// on any error — the admin-UI test-connection endpoint surfaces the
/// message verbatim so operators can debug bad creds / bad hosts
/// without digging through logs.
///
/// Only exists here (not on `EmailImapAdapter`) because test-connection
/// runs *before* the channel row is saved — there's no adapter yet.
pub async fn test_imap_connection(
    config: &ImapChannelConfig,
    password: &str,
) -> Result<(), String> {
    let mut session = open_session(config, password)
        .await
        .map_err(|e| e.to_string())?;

    // EXAMINE is read-only — it verifies the mailbox exists without
    // touching `\Seen` flags. SELECT would also work but leaves a
    // server-side session state we don't need.
    match tokio::time::timeout(IMAP_OP_TIMEOUT, session.examine(&config.mailbox)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(format!("mailbox '{}' inaccessible: {e}", config.mailbox)),
        Err(_) => {
            return Err(format!(
                "mailbox '{}' inaccessible: timed out after {}s",
                config.mailbox,
                IMAP_OP_TIMEOUT.as_secs()
            ))
        }
    }

    match tokio::time::timeout(IMAP_OP_TIMEOUT, session.logout()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("logout failed: {e}")),
        Err(_) => Err(format!(
            "logout failed: timed out after {}s",
            IMAP_OP_TIMEOUT.as_secs()
        )),
    }
}

#[async_trait]
impl ChannelAdapter for EmailImapAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn provider(&self) -> &'static str {
        "email_imap"
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
        let message_id = content
            .external_id_hint
            .clone()
            .unwrap_or_else(|| format_outbound_message_id(thread.ticket_id, 0, &self.config.reply_domain));

        let recipient = thread
            .recipient
            .known_email
            .as_deref()
            .ok_or_else(|| ChannelError::Configuration("recipient has no email".into()))?;
        let subject = thread
            .subject
            .as_deref()
            .unwrap_or("(no subject)");
        let in_reply_to = thread.external_thread_id.as_deref();

        let outbound = OutboundEmail {
            to: recipient,
            subject,
            body_text: &content.body_markdown,
            body_html: content.body_html.as_deref(),
            message_id: &message_id,
            in_reply_to,
            references: &thread.references,
            // Tech-authored reply — no auto-reply headers.
            auto_submitted: false,
        };

        self.email
            .send_ticket_reply(outbound)
            .await
            .map_err(ChannelError::Other)?;

        Ok(OutboundMessage {
            external_id: format!("<{message_id}>"),
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

/// Pool acquisition timeout for poll-path DB reads. The pool's
/// default 30s is too long for a per-poll operation — we'd rather
/// fail the poll quickly and retry than stall the worker.
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// UIDVALIDITY pinning: if the server's current UIDVALIDITY
    /// differs from what we last saw, UIDs have been rewritten — we
    /// reset `last_seen_uid` to 0 and rescan. See
    /// [`ImapRuntimeState`] module docs for background.
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
        let password = {
            let mut conn = self
                .pool
                .get_timeout(POOL_ACQUIRE_TIMEOUT)
                .map_err(ChannelError::transient("db pool"))?;
            channels_repo::get_credential(&mut conn, self.channel_id, CRED_TYPE_IMAP_PASSWORD)
                .map_err(ChannelError::transient("credential lookup"))?
                .ok_or_else(|| {
                    ChannelError::Configuration("no IMAP password stored for channel".into())
                })?
        };

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
    /// been either ingested or logged off as unparseable.
    async fn fetch_new_messages(
        &mut self,
        session: &mut ImapSession,
    ) -> Result<Vec<InboundEvent>, ChannelError> {
        let mailbox = timed("select", session.select(&self.config.mailbox)).await?;

        // UIDVALIDITY check — the one IMAP invariant we really care
        // about. Without this, a server that renumbers UIDs (rare,
        // but mailbox restores do it) would either double-ingest or
        // skip messages depending on direction of drift.
        let current_uid_validity = mailbox.uid_validity;
        let rescan_needed = match (self.state.uid_validity, current_uid_validity) {
            (Some(prev), Some(now)) if prev == now => false,
            _ => {
                debug!(
                    channel = self.id,
                    previous = ?self.state.uid_validity,
                    current = ?current_uid_validity,
                    "UIDVALIDITY changed — rescanning from UID 1"
                );
                self.state.last_seen_uid = 0;
                self.state.uid_validity = current_uid_validity;
                true
            }
        };

        // Persist the UIDVALIDITY observation right away. Even if the
        // subsequent fetches fail, we don't want to redo the rescan
        // reset on the next poll — otherwise a persistently-bad
        // mailbox would loop in "reset to UID 0 → fetch fails →
        // restart → reset again" forever.
        if rescan_needed {
            self.persist_state().await?;
        }

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

        let mut events = Vec::with_capacity(uids.len());
        let mut max_uid = self.state.last_seen_uid;

        for uid in uids {
            match fetch_single_uid(session, uid).await {
                Ok(Some(msg)) => events.push(InboundEvent::MessageReceived(msg)),
                Ok(None) => warn!(channel = self.id, uid, "fetch returned no body — skipping"),
                Err(e) => warn!(
                    channel = self.id,
                    uid,
                    error = %e,
                    "skipping unparseable message"
                ),
            }
            // Advance past every UID we attempted, regardless of
            // outcome. A poison message that can't be parsed has
            // already been logged; we don't want to replay it on
            // every poll.
            if uid > max_uid {
                max_uid = uid;
            }
        }

        if max_uid > self.state.last_seen_uid {
            self.state.last_seen_uid = max_uid;
            self.persist_state().await?;
        }

        Ok(events)
    }

    async fn persist_state(&self) -> Result<(), ChannelError> {
        let blob = serde_json::to_value(&self.state)
            .map_err(ChannelError::other("runtime_state json"))?;
        let mut conn = self
            .pool
            .get_timeout(POOL_ACQUIRE_TIMEOUT)
            .map_err(ChannelError::transient("db pool"))?;
        channels_repo::update_runtime_state(&mut conn, self.channel_id, blob)
            .map(|_| ())
            .map_err(ChannelError::transient("persist runtime_state"))
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

/// Fetch a single UID and parse it. All errors — IMAP-layer, missing
/// body, RFC 5322 parse failure — come back as `Err(String)` so the
/// caller can log per-UID and move on without tearing down the whole
/// batch. `Ok(None)` means the fetch succeeded but the server didn't
/// return a body for this UID (message deleted between SEARCH and
/// FETCH, typically).
async fn fetch_single_uid(
    session: &mut ImapSession,
    uid: u32,
) -> Result<Option<InboundMessage>, String> {
    let stream = match tokio::time::timeout(
        IMAP_OP_TIMEOUT,
        session.uid_fetch(uid.to_string(), FETCH_SPEC),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("uid_fetch: {e}")),
        Err(_) => {
            return Err(format!(
                "uid_fetch: timed out after {}s",
                IMAP_OP_TIMEOUT.as_secs()
            ))
        }
    };
    let fetches: Vec<_> = match tokio::time::timeout(IMAP_OP_TIMEOUT, stream.try_collect()).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Err(format!("fetch stream: {e}")),
        Err(_) => {
            return Err(format!(
                "fetch stream: timed out after {}s",
                IMAP_OP_TIMEOUT.as_secs()
            ))
        }
    };

    let Some(fetch) = fetches.into_iter().find(|f| f.uid == Some(uid)) else {
        return Ok(None);
    };
    let Some(body) = fetch.body() else {
        return Ok(None);
    };
    let internal_date = fetch.internal_date().map(|d| d.with_timezone(&Utc));
    parse_rfc822_into_inbound_message(body, internal_date)
        .map(Some)
        .map_err(|e| format!("parse rfc822: {e}"))
}

// ---------- Pure parser ----------

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

    let external_id = header_first(headers, "Message-ID")
        .ok_or_else(|| ChannelError::Other("message has no Message-ID header".into()))?;

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

    let received_at = internal_date
        .or_else(|| header_first(headers, "Date").and_then(parse_rfc2822_to_utc))
        .unwrap_or_else(Utc::now);

    // Keep *all* headers verbatim so debugging, audit, or future features
    // (e.g. DKIM/SPF extraction) never need a re-fetch.
    let raw_metadata = serde_json::json!({
        "headers": headers
            .iter()
            .map(|h| (h.get_key(), h.get_value()))
            .collect::<Vec<_>>(),
    });

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
    })
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

/// Split `Name <email@host>` into (name, email). If the input is just
/// an address, name is the localpart.
fn parse_mailbox(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if let (Some(lt), Some(gt)) = (trimmed.rfind('<'), trimmed.rfind('>')) {
        if lt < gt {
            let email = trimmed[lt + 1..gt].trim().to_string();
            let name = trimmed[..lt]
                .trim()
                .trim_matches('"')
                .trim()
                .to_string();
            let display = if name.is_empty() { email.clone() } else { name };
            return (display, email);
        }
    }
    let email = trimmed.to_string();
    let name = email.split('@').next().unwrap_or(&email).to_string();
    (name, email)
}

fn extract_bodies(mail: &mailparse::ParsedMail) -> (String, Option<String>) {
    // Walk every part. First text/plain wins for body_text; first
    // text/html wins for body_html. Attachments are excluded via
    // Content-Disposition check.
    let mut text: Option<String> = None;
    let mut html: Option<String> = None;

    walk(mail, &mut |part| {
        if is_attachment(part) {
            return;
        }
        let ctype = part.ctype.mimetype.to_ascii_lowercase();
        match ctype.as_str() {
            "text/plain" if text.is_none() => {
                text = part.get_body().ok();
            }
            "text/html" if html.is_none() => {
                html = part.get_body().ok();
            }
            _ => {}
        }
    });

    (text.unwrap_or_default(), html)
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
                .split(|c: char| c == '"' || c == ';')
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

fn walk<'a, F: FnMut(&'a mailparse::ParsedMail<'a>)>(mail: &'a mailparse::ParsedMail<'a>, f: &mut F) {
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
    for name in ["To", "Cc", "Bcc", "Delivered-To", "Envelope-To", "X-Original-To"] {
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

    #[test]
    fn runtime_state_roundtrips_through_json() {
        let state = ImapRuntimeState {
            last_seen_uid: 42,
            uid_validity: Some(123),
            last_error: None,
        };
        let v = serde_json::to_value(&state).unwrap();
        let back: ImapRuntimeState = serde_json::from_value(v).unwrap();
        assert_eq!(back.last_seen_uid, 42);
        assert_eq!(back.uid_validity, Some(123));
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
            vec!["<a@h>".to_string(), "<b@h>".to_string(), "<c@h>".to_string()]
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
                "<out-1@host>".to_string(),   // In-Reply-To comes first.
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
    fn rejects_message_without_message_id() {
        let raw = b"From: a@b\r\nSubject: x\r\n\r\nbody";
        let err = parse_rfc822_into_inbound_message(raw, None).unwrap_err();
        assert!(matches!(err, ChannelError::Other(_)));
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
}
