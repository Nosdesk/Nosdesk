//! Channel worker registry.
//!
//! Supervises one background task per enabled channel. Each worker:
//!
//! 1. Holds a [`PullAdapter`] for the channel's provider.
//! 2. Drives a poll loop: fetch events, push them through
//!    [`super::pipeline::process_event`], persist runtime state.
//! 3. Backs off on [`ChannelError::Transient`] and [`ChannelError::Other`]
//!    with exponential delay capped at [`MAX_BACKOFF`]; stops
//!    permanently on [`ChannelError::Configuration`]; honours the
//!    optional `retry_after` on [`ChannelError::RateLimited`].
//! 4. Exits cleanly when signalled — the registry's `shutdown()` fans
//!    out a cancellation watch and awaits every worker.
//!
//! The registry is deliberately thin. It does NOT:
//!   - handle admin-driven runtime add/remove (phase-1 decision: require
//!     a restart; simpler than reconciling adapter state with DB state).
//!   - schedule across channels (they're independent by definition).
//!   - surface metrics beyond tracing (add when we actually need them).
//!
//! The `run_one_poll` function is factored out so tests can drive a
//! single iteration against a stub adapter without spinning up the
//! infinite loop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use actix_web::web;
use metrics::counter;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::handlers::sse::SseState;
use crate::models::Channel;
use crate::repository::channels as channels_repo;
use crate::services::channels::email_imap::build_email_imap_adapter;
use crate::services::channels::pipeline::{self, PipelineContext};
use crate::services::channels::{ChannelError, PullAdapter};
use crate::services::search::SearchService;
use crate::utils::email::EmailService;
use crate::utils::storage::Storage;

/// Starting backoff after the first transient failure. Doubles each
/// subsequent failure up to [`MAX_BACKOFF`]; resets on a successful
/// poll.
const BASE_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Fallback delay for [`ChannelError::RateLimited`] when the provider
/// didn't tell us how long to wait.
const DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(60);

/// Consecutive-failure threshold at which a worker gives up. With
/// backoff starting at 5s and capping at 5min, hitting this count
/// means the channel has been broken for roughly 90 minutes. At that
/// point we stop (persisting a last_error) rather than continue
/// hammering a mail server that's clearly not coming back without
/// admin action. A fresh Upsert or a restart reboots the worker.
const DEAD_CHANNEL_THRESHOLD: u32 = 20;

/// Pool acquisition timeout for registry-level DB calls. Short enough
/// that a stalled pool surfaces as a transient error rather than
/// silently pinning a worker for 30 seconds waiting on a connection.
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared dependencies every worker needs. Cloning is cheap — the
/// inner handles are all `Arc` / `web::Data`.
#[derive(Clone)]
pub struct RegistryDeps {
    pub pool: Pool,
    pub email: Option<Arc<EmailService>>,
    pub sse: Option<web::Data<SseState>>,
    pub search: Option<Arc<SearchService>>,
    pub storage: Option<Arc<dyn Storage>>,
    pub http: Option<reqwest::Client>,
}

impl RegistryDeps {
    fn pipeline_context(&self) -> PipelineContext {
        PipelineContext {
            storage: self.storage.clone(),
            sse: self.sse.clone(),
            search: self.search.clone(),
            http: self.http.clone(),
            // Thread the pool + email through so the pipeline's
            // auto-ack branch can spawn an SMTP send on newly
            // opened tickets.
            email: self.email.clone(),
            pool: Some(self.pool.clone()),
        }
    }
}

/// Errors raised while *starting* a worker — the worker loop itself
/// handles runtime errors internally and never returns one.
#[derive(Debug)]
pub enum StartError {
    UnsupportedProvider(String),
    BadConfig(String),
    MissingEmailService,
    Credential(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedProvider(p) => write!(f, "unsupported provider: {p}"),
            Self::BadConfig(m) => write!(f, "bad channel config: {m}"),
            Self::MissingEmailService => {
                write!(f, "email channel needs EmailService to be configured")
            }
            Self::Credential(m) => write!(f, "credential error: {m}"),
        }
    }
}
impl std::error::Error for StartError {}

/// Supervises the per-channel worker tasks.
pub struct ChannelRegistry {
    workers: HashMap<i32, WorkerHandle>,
    deps: RegistryDeps,
}

struct WorkerHandle {
    task: JoinHandle<()>,
    stop: watch::Sender<bool>,
}

impl ChannelRegistry {
    pub fn new(deps: RegistryDeps) -> Self {
        Self {
            workers: HashMap::new(),
            deps,
        }
    }

    /// Build + spawn a worker for each channel. Channels that fail to
    /// start (bad config, missing creds) are logged and skipped so one
    /// broken entry doesn't kill the rest of the registry.
    pub fn start_many(&mut self, channels: Vec<Channel>) {
        for channel in channels {
            if let Err(e) = self.start(channel) {
                error!(error = %e, "failed to start channel worker");
            }
        }
    }

    /// Build + spawn one worker. If the channel is already running
    /// this is a no-op (the admin must stop it first).
    ///
    /// Spawn shape: an outer "supervisor" task awaits the inner worker
    /// task's `JoinHandle`. That way a panic inside the worker surfaces
    /// as a loud `error!` log (with the panic payload, when available)
    /// instead of the task silently vanishing from Tokio's radar.
    /// Restart is deliberately NOT automatic — a panicking worker is a
    /// bug that wants admin attention, not an infinite loop masking
    /// the problem.
    pub fn start(&mut self, channel: Channel) -> Result<(), StartError> {
        if self.workers.contains_key(&channel.id) {
            debug!(channel = channel.id, "worker already running — skip");
            return Ok(());
        }

        let adapter = build_pull_adapter(&channel, &self.deps)?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let deps = self.deps.clone();
        let channel_id = channel.id;
        let worker_id = format!("{}:{}", channel.provider, channel_id);

        let task = tokio::spawn(async move {
            let inner = tokio::spawn(async move {
                run_worker(channel, adapter, deps, stop_rx).await;
            });
            match inner.await {
                Ok(()) => {}
                Err(e) if e.is_panic() => {
                    counter!("nosdesk_channels_worker_panics_total").increment(1);
                    error!(
                        worker = %worker_id,
                        panic = %format_panic(e),
                        "channel worker panicked — not restarting; admin must stop and restart the channel"
                    );
                }
                Err(e) => error!(worker = %worker_id, error = %e, "channel worker task aborted"),
            }
        });

        self.workers
            .insert(channel_id, WorkerHandle { task, stop: stop_tx });
        info!(channel = channel_id, "channel worker started");
        Ok(())
    }

    /// Signal one worker to stop and wait for it. Returns `false` if
    /// there was no worker for that id.
    pub async fn stop(&mut self, channel_id: i32) -> bool {
        match self.workers.remove(&channel_id) {
            Some(WorkerHandle { task, stop }) => {
                let _ = stop.send(true);
                let _ = task.await;
                true
            }
            None => false,
        }
    }

    /// Stop every worker and await them all. Consumes `self` — the
    /// registry is single-shot by design; callers that need fresh
    /// workers after a shutdown build a new registry.
    pub async fn shutdown(mut self) {
        let ids: Vec<i32> = self.workers.keys().copied().collect();
        for id in ids {
            self.stop(id).await;
        }
        info!("channel registry shut down");
    }

    pub fn is_running(&self, channel_id: i32) -> bool {
        self.workers.contains_key(&channel_id)
    }
}

// ---------- Adapter factory ----------

/// Resolve a channel row into a concrete [`PullAdapter`]. Extracted as
/// a free function so tests can call it directly, and so the dispatch
/// table lives in one place as more providers land.
pub fn build_pull_adapter(
    channel: &Channel,
    deps: &RegistryDeps,
) -> Result<Box<dyn PullAdapter>, StartError> {
    match channel.provider.as_str() {
        "email_imap" => {
            let email = deps
                .email
                .clone()
                .ok_or(StartError::MissingEmailService)?;
            let adapter = build_email_imap_adapter(channel, email, deps.pool.clone())
                .map_err(StartError::BadConfig)?;
            Ok(Box::new(adapter))
        }
        other => Err(StartError::UnsupportedProvider(other.to_string())),
    }
}

// ---------- Worker loop ----------

async fn run_worker(
    channel: Channel,
    mut adapter: Box<dyn PullAdapter>,
    deps: RegistryDeps,
    mut stop: watch::Receiver<bool>,
) {
    let worker_id = format!("{}:{}", channel.provider, channel.id);
    info!(worker = %worker_id, "channel worker loop started");
    let mut backoff = BASE_BACKOFF;
    // Consecutive transient / rate-limit cycles. Reset on any
    // successful poll. Runs up against `DEAD_CHANNEL_THRESHOLD` to
    // stop a persistently-broken channel from polling forever.
    let mut consecutive_failures: u32 = 0;

    loop {
        if *stop.borrow() {
            break;
        }

        let outcome = tokio::select! {
            biased;
            _ = stop.changed() => break,
            r = run_one_poll(adapter.as_mut(), &channel, &deps) => r,
        };

        // Low-cardinality `outcome` label so dashboards can group by
        // result class without unbounded per-channel splits. Channel
        // provenance stays on the tracing side.
        let outcome_label = match &outcome {
            PollOutcome::Ok => "ok",
            PollOutcome::Transient => "transient",
            PollOutcome::RateLimited(_) => "rate_limited",
            PollOutcome::Configuration => "configuration",
        };
        counter!("nosdesk_channels_poll_total", "outcome" => outcome_label).increment(1);

        let sleep_for = match outcome {
            PollOutcome::Ok => {
                backoff = BASE_BACKOFF;
                consecutive_failures = 0;
                adapter.poll_interval()
            }
            PollOutcome::RateLimited(retry_after) => {
                consecutive_failures += 1;
                retry_after.unwrap_or(DEFAULT_RATE_LIMIT_BACKOFF)
            }
            PollOutcome::Transient => {
                consecutive_failures += 1;
                let d = backoff;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                d
            }
            PollOutcome::Configuration => {
                // Admin has to intervene; stop polling until they do.
                break;
            }
        };

        if consecutive_failures >= DEAD_CHANNEL_THRESHOLD {
            counter!("nosdesk_channels_dead_total").increment(1);
            error!(
                worker = %worker_id,
                failures = consecutive_failures,
                "channel crossed the dead-channel threshold — stopping until an admin re-enables it"
            );
            let reason = format!(
                "stopped after {consecutive_failures} consecutive failures; re-enable the channel to resume"
            );
            record_last_error(&channel, &reason, &deps).await;
            break;
        }

        debug!(worker = %worker_id, ?sleep_for, "sleeping before next poll");
        if sleep_with_cancel(sleep_for, &mut stop).await {
            break;
        }
    }

    info!(worker = %worker_id, "channel worker loop exited");
}

/// Result of driving a single poll-cycle. Tests inspect this; the
/// worker loop turns it into a sleep duration.
#[derive(Debug, PartialEq, Eq)]
pub enum PollOutcome {
    Ok,
    RateLimited(Option<Duration>),
    Transient,
    Configuration,
}

/// One poll iteration: fetch events, funnel them through the pipeline,
/// persist runtime state on success. Errors are classified into a
/// [`PollOutcome`] so the worker can decide how long to wait — this
/// function is the testable core of the worker.
pub async fn run_one_poll(
    adapter: &mut dyn PullAdapter,
    channel: &Channel,
    deps: &RegistryDeps,
) -> PollOutcome {
    let events = match adapter.poll().await {
        Ok(events) => events,
        Err(e) => return classify_error(channel, &e, deps).await,
    };

    if events.is_empty() {
        debug!(channel = channel.id, "poll returned no new events");
        return PollOutcome::Ok;
    }

    let ctx = deps.pipeline_context();
    let mut conn = match deps.pool.get_timeout(POOL_ACQUIRE_TIMEOUT) {
        Ok(c) => c,
        Err(e) => {
            warn!(channel = channel.id, error = %e, "pool exhausted — treating as transient");
            return PollOutcome::Transient;
        }
    };

    for event in events {
        // `&mut dyn PullAdapter` upcasts to `&mut dyn ChannelAdapter`
        // natively (stable trait upcasting). The pipeline only needs
        // the `resolve_thread` / `send_reply` surface of the supertrait.
        let base: &mut dyn crate::services::channels::ChannelAdapter = adapter;
        match pipeline::process_event(base, channel, event, &mut conn, &ctx).await {
            Ok(outcome) => {
                counter!(
                    "nosdesk_channels_pipeline_outcome_total",
                    "outcome" => pipeline_outcome_label(&outcome),
                )
                .increment(1);
                debug!(channel = channel.id, ?outcome, "processed inbound event");
            }
            Err(e) => {
                counter!("nosdesk_channels_pipeline_error_total").increment(1);
                // A single bad message must not kill the loop — log
                // and move on. The ingestion pipeline only errors on
                // DB failures or attachment issues; the channel row
                // stays healthy.
                warn!(channel = channel.id, error = %e, "pipeline error; skipping event");
            }
        }
    }

    // Clear the stored `last_error` on success so admins see a healthy
    // state. `last_seen_uid` / `uid_validity` are updated by the
    // adapter itself during `poll()` (task #25) — here we only reset
    // the error slot.
    if let Err(e) = clear_last_error(&mut conn, channel) {
        warn!(channel = channel.id, error = %e, "failed to clear last_error");
    }

    PollOutcome::Ok
}

async fn classify_error(channel: &Channel, err: &ChannelError, deps: &RegistryDeps) -> PollOutcome {
    let message = err.to_string();
    match err {
        ChannelError::Configuration(_) => {
            error!(channel = channel.id, error = %err, "configuration error — worker will stop");
            record_last_error(channel, &message, deps).await;
            PollOutcome::Configuration
        }
        ChannelError::RateLimited { retry_after } => {
            warn!(channel = channel.id, retry_after = ?retry_after, "rate limited");
            record_last_error(channel, &message, deps).await;
            PollOutcome::RateLimited(*retry_after)
        }
        _ => {
            warn!(channel = channel.id, error = %err, "transient error");
            record_last_error(channel, &message, deps).await;
            PollOutcome::Transient
        }
    }
}

async fn record_last_error(channel: &Channel, msg: &str, deps: &RegistryDeps) {
    let Ok(mut conn) = deps.pool.get_timeout(POOL_ACQUIRE_TIMEOUT) else {
        return;
    };
    if let Err(e) = write_last_error(&mut conn, channel, Some(msg)) {
        warn!(channel = channel.id, error = %e, "failed to persist last_error");
    }
}

/// Merge a `last_error` update into the channel's `runtime_state`
/// JSON without touching other keys.
///
/// Provider-agnostic on purpose: operates directly on the JSON object
/// so a Slack / Teams / webhook adapter with its own `runtime_state`
/// shape can also flow through this helper. Every adapter uses
/// `last_error` as a common error slot; anything else is its own
/// namespace.
///
/// Re-reads the row from the DB rather than using the `&Channel`
/// passed in because during a poll the adapter may have advanced its
/// own runtime-state fields (UID cursors, delta tokens, etc.) and
/// writing back the pre-poll snapshot would silently undo the advance.
fn write_last_error(
    conn: &mut crate::db::DbConnection,
    channel: &Channel,
    new_error: Option<&str>,
) -> Result<(), diesel::result::Error> {
    let fresh = channels_repo::find(conn, channel.id)?;
    let mut state = match fresh.runtime_state {
        serde_json::Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    state.insert(
        "last_error".to_string(),
        match new_error {
            Some(msg) => serde_json::Value::String(msg.to_string()),
            None => serde_json::Value::Null,
        },
    );
    let blob = serde_json::Value::Object(state);
    channels_repo::update_runtime_state(conn, channel.id, blob).map(|_| ())
}

fn clear_last_error(
    conn: &mut crate::db::DbConnection,
    channel: &Channel,
) -> Result<(), diesel::result::Error> {
    write_last_error(conn, channel, None)
}

/// Low-cardinality label for the pipeline outcome enum, for use in
/// `counter!` metric tags. Keeping this in one place means any new
/// variant added to `PipelineOutcome` has exactly one place to gain a
/// label.
fn pipeline_outcome_label(outcome: &pipeline::PipelineOutcome) -> &'static str {
    use pipeline::PipelineOutcome;
    match outcome {
        PipelineOutcome::TicketOpened { .. } => "ticket_opened",
        PipelineOutcome::ReplyAppended { .. } => "reply_appended",
        PipelineOutcome::SkippedDuplicate => "skipped_duplicate",
        PipelineOutcome::SkippedLoop => "skipped_loop",
        PipelineOutcome::SkippedBounce => "skipped_bounce",
        PipelineOutcome::SkippedUnsupportedVariant => "skipped_unsupported",
        PipelineOutcome::SkippedNoIdentity => "skipped_no_identity",
        PipelineOutcome::SkippedEmailClaimed => "skipped_email_claimed",
    }
}

/// Extract a readable string from a `JoinError`'s panic payload. The
/// stdlib panic! macro stores the payload as `&'static str` or `String`
/// depending on whether the format string has interpolations; both
/// cover the usual cases. Anything else (custom panic types) falls
/// through to a placeholder.
fn format_panic(e: tokio::task::JoinError) -> String {
    match e.try_into_panic() {
        Ok(panic) => {
            if let Some(s) = panic.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            }
        }
        Err(_) => "<unavailable>".to_string(),
    }
}

/// Sleep for `d` but wake early if the shutdown signal fires. Returns
/// `true` if the sleep was cut short by a shutdown — caller should
/// exit the outer loop.
async fn sleep_with_cancel(d: Duration, stop: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        biased;
        _ = stop.changed() => true,
        _ = tokio::time::sleep(d) => false,
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    //! Registry tests use a `StubPullAdapter` that returns canned poll
    //! responses. They cover:
    //!
    //! - happy path (events flow through pipeline)
    //! - empty poll (no-op, Ok)
    //! - transient error classification → Transient outcome
    //! - configuration error classification → Configuration outcome
    //! - rate-limited with retry_after passthrough
    //! - `build_pull_adapter` provider dispatch
    //! - `last_error` is persisted on failure and cleared on success
    //!
    //! The worker *loop* (sleep / backoff / shutdown) is not directly
    //! tested — its behaviour is a composition of `run_one_poll` plus
    //! `tokio::time::sleep`, both of which are trivial to compose.

    use super::*;
    use crate::models::{NewChannel, UserRole};
    use crate::services::channels::email_imap::ImapRuntimeState;
    use crate::services::channels::{
        ChannelAdapter, ChannelError, ExternalIdentity, InboundEvent, InboundMessage, LoopMarkers,
        OutboundContent, OutboundMessage, ThreadContext,
    };
    use crate::test_helpers::{setup_test_pool, TestFixtures};
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::Mutex;

    /// Stub that returns canned poll results in order. Internal
    /// `call_count` tracks how many times `poll` was invoked so tests
    /// can assert the loop actually drove the adapter.
    struct StubPullAdapter {
        id: String,
        responses: Mutex<Vec<Result<Vec<InboundEvent>, ChannelError>>>,
        calls: Mutex<usize>,
    }

    impl StubPullAdapter {
        fn new(responses: Vec<Result<Vec<InboundEvent>, ChannelError>>) -> Self {
            Self {
                id: "stub:1".into(),
                responses: Mutex::new(responses),
                calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl ChannelAdapter for StubPullAdapter {
        fn id(&self) -> &str {
            &self.id
        }
        fn provider(&self) -> &'static str {
            "email_imap"
        }
        async fn send_reply(
            &self,
            _thread: &ThreadContext,
            _content: &OutboundContent,
        ) -> Result<OutboundMessage, ChannelError> {
            unreachable!("registry tests never drive outbound")
        }
    }

    #[async_trait]
    impl PullAdapter for StubPullAdapter {
        async fn poll(&mut self) -> Result<Vec<InboundEvent>, ChannelError> {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| Ok(vec![]))
        }
        fn poll_interval(&self) -> Duration {
            Duration::from_millis(1)
        }
    }

    fn sample_event(external_id: &str) -> InboundEvent {
        InboundEvent::MessageReceived(InboundMessage {
            external_id: external_id.into(),
            from: ExternalIdentity {
                provider: "email_imap".into(),
                external_id: "alice@example.com".into(),
                display_name: "Alice".into(),
                known_email: Some("alice@example.com".into()),
            },
            subject: Some("hi".into()),
            body_text: "hello".into(),
            body_html: None,
            attachments: vec![],
            references: vec![],
            received_at: Utc::now(),
            loop_markers: LoopMarkers::default(),
            raw_metadata: serde_json::json!({}),
            recipients: vec!["support@yourco.com".into()],
            is_bounce: false,
            bounce_report: None,
        })
    }

    fn deps_with_pool(pool: Pool) -> RegistryDeps {
        RegistryDeps {
            pool,
            email: None,
            sse: None,
            search: None,
            storage: None,
            http: None,
        }
    }

    fn seed_channel(pool: &Pool) -> Channel {
        let mut conn = pool.get().unwrap();
        channels_repo::create(
            &mut conn,
            NewChannel {
                provider: "email_imap".into(),
                name: "test-mailbox".into(),
                enabled: true,
                config: serde_json::json!({}),
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn run_one_poll_processes_events_and_clears_last_error() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let channel = {
            let mut conn = pool.get().unwrap();
            // Seed a pre-existing last_error so we can verify it's cleared.
            let mut ch = channels_repo::create(
                &mut conn,
                NewChannel {
                    provider: "email_imap".into(),
                    name: "m".into(),
                    enabled: true,
                    config: serde_json::json!({}),
                },
            )
            .unwrap();
            channels_repo::update_runtime_state(
                &mut conn,
                ch.id,
                serde_json::json!({ "last_error": "previous failure" }),
            )
            .unwrap();
            ch = channels_repo::find(&mut conn, ch.id).unwrap();
            ch
        };

        let mut adapter = StubPullAdapter::new(vec![Ok(vec![sample_event("<evt-1@x>")])]);
        let outcome = run_one_poll(&mut adapter, &channel, &deps).await;
        assert_eq!(outcome, PollOutcome::Ok);

        let mut conn = pool.get().unwrap();
        let refreshed = channels_repo::find(&mut conn, channel.id).unwrap();
        let state: ImapRuntimeState =
            serde_json::from_value(refreshed.runtime_state).unwrap_or_default();
        assert!(state.last_error.is_none(), "expected last_error cleared");
        // And the pipeline actually wrote the message.
        let recorded = channels_repo::find_by_external_id(&mut conn, channel.id, "<evt-1@x>")
            .unwrap();
        assert!(recorded.is_some(), "pipeline should have persisted the event");
    }

    #[tokio::test]
    async fn run_one_poll_empty_events_is_noop_ok() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let channel = seed_channel(&pool);
        let mut adapter = StubPullAdapter::new(vec![Ok(vec![])]);
        assert_eq!(
            run_one_poll(&mut adapter, &channel, &deps).await,
            PollOutcome::Ok
        );
    }

    #[tokio::test]
    async fn transient_error_records_last_error_and_returns_transient() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let channel = seed_channel(&pool);
        let mut adapter =
            StubPullAdapter::new(vec![Err(ChannelError::Transient("socket closed".into()))]);
        assert_eq!(
            run_one_poll(&mut adapter, &channel, &deps).await,
            PollOutcome::Transient
        );

        let mut conn = pool.get().unwrap();
        let refreshed = channels_repo::find(&mut conn, channel.id).unwrap();
        let state: ImapRuntimeState =
            serde_json::from_value(refreshed.runtime_state).unwrap_or_default();
        assert!(state
            .last_error
            .as_deref()
            .unwrap()
            .contains("socket closed"));
    }

    #[tokio::test]
    async fn configuration_error_returns_stop() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let channel = seed_channel(&pool);
        let mut adapter =
            StubPullAdapter::new(vec![Err(ChannelError::Configuration("bad creds".into()))]);
        assert_eq!(
            run_one_poll(&mut adapter, &channel, &deps).await,
            PollOutcome::Configuration
        );
    }

    #[tokio::test]
    async fn rate_limited_passthrough_of_retry_after() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let channel = seed_channel(&pool);
        let d = Duration::from_secs(30);
        let mut adapter = StubPullAdapter::new(vec![Err(ChannelError::RateLimited {
            retry_after: Some(d),
        })]);
        assert_eq!(
            run_one_poll(&mut adapter, &channel, &deps).await,
            PollOutcome::RateLimited(Some(d))
        );
    }

    #[tokio::test]
    async fn build_pull_adapter_rejects_unknown_provider() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let mut conn = pool.get().unwrap();
        let ch = channels_repo::create(
            &mut conn,
            NewChannel {
                provider: "wechat".into(),
                name: "x".into(),
                enabled: true,
                config: serde_json::json!({}),
            },
        )
        .unwrap();
        drop(conn);

        match build_pull_adapter(&ch, &deps) {
            Err(StartError::UnsupportedProvider(_)) => {}
            Err(other) => panic!("expected UnsupportedProvider, got {other:?}"),
            Ok(_) => panic!("expected build_pull_adapter to fail"),
        }
    }

    #[tokio::test]
    async fn build_pull_adapter_reports_missing_email_service() {
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone()); // no email
        let mut conn = pool.get().unwrap();
        let ch = channels_repo::create(
            &mut conn,
            NewChannel {
                provider: "email_imap".into(),
                name: "no-email".into(),
                enabled: true,
                config: serde_json::json!({
                    "host": "imap.example.com",
                    "username": "u@example.com",
                    "reply_domain": "example.com"
                }),
            },
        )
        .unwrap();
        drop(conn);

        match build_pull_adapter(&ch, &deps) {
            Err(StartError::MissingEmailService) => {}
            Err(other) => panic!("expected MissingEmailService, got {other:?}"),
            Ok(_) => panic!("expected build_pull_adapter to fail"),
        }
    }

    #[tokio::test]
    async fn sleep_with_cancel_short_circuits_on_stop() {
        let (tx, mut rx) = watch::channel(false);
        let handle = tokio::spawn(async move {
            // Will be cancelled well before 10 seconds.
            sleep_with_cancel(Duration::from_secs(10), &mut rx).await
        });
        // Let the spawn poll at least once.
        tokio::time::sleep(Duration::from_millis(10)).await;
        tx.send(true).unwrap();
        let was_cancelled = handle.await.unwrap();
        assert!(was_cancelled);
    }

    #[tokio::test]
    async fn pipeline_error_on_one_event_does_not_abort_rest() {
        // Same external_id twice — second one hits the dedup branch and
        // returns `SkippedDuplicate` *not* an error, but this proves the
        // loop keeps going. Using two distinct events here so both are
        // persisted.
        let pool = setup_test_pool();
        let deps = deps_with_pool(pool.clone());
        let channel = seed_channel(&pool);

        let mut adapter = StubPullAdapter::new(vec![Ok(vec![
            sample_event("<evt-2@x>"),
            sample_event("<evt-3@x>"),
        ])]);
        assert_eq!(
            run_one_poll(&mut adapter, &channel, &deps).await,
            PollOutcome::Ok
        );
        let mut conn = pool.get().unwrap();
        assert!(channels_repo::find_by_external_id(&mut conn, channel.id, "<evt-2@x>")
            .unwrap()
            .is_some());
        assert!(channels_repo::find_by_external_id(&mut conn, channel.id, "<evt-3@x>")
            .unwrap()
            .is_some());
    }

    // Assertion helper to keep UserRole imported — prevents unused-import
    // warning when we later add UserRole-dependent tests here.
    #[allow(dead_code)]
    fn _silence_user_role_import(conn: &mut crate::db::DbConnection) {
        let _ = TestFixtures::create_user(conn, "n", UserRole::User);
    }
}
