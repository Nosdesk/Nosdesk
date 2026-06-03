//! Minimal periodic-task scheduler.
//!
//! Purpose-built for "run this async closure every N seconds/minutes"
//! — the MS Graph delta sync, expired-session GC, orphaned-upload GC
//! shape of problem. Deliberately NOT a general-purpose cron engine:
//!
//! * No cron expressions (every job in this codebase just wants a
//!   `Duration` interval — if we ever need "every Tuesday at 3am"
//!   we can add a cron crate then, not preemptively).
//! * No persistence (missing a tick on restart is fine; next tick
//!   catches up idempotent work).
//! * No distributed leader election (single Docker container).
//!
//! # Design rationale
//!
//! Researched alternatives (`tokio-cron-scheduler`, `apalis`,
//! `clokwerk`) bring surface area we don't need and — in the most
//! popular crate's case — documented footguns around broadcast channel
//! saturation. The canonical Rust answer for this shape of problem is
//! the `loop { select! { shutdown / interval } }` pattern from
//! Palmieri's *Zero to Production in Rust*, echoed by Meilisearch and
//! Vector's internal schedulers.
//!
//! # Key properties
//!
//! * `.await` inline (not `spawn`) inside the tick arm → if a job
//!   runs longer than `every`, the next tick is **naturally delayed**
//!   rather than overlapping. Overlap prevention is structural.
//! * [`MissedTickBehavior::Skip`] → after a GC pause or clock jump
//!   we don't burst-fire missed ticks. Default is `Burst`; that's a
//!   well-documented footgun.
//! * Startup jitter per task → N jobs scheduled at minute boundaries
//!   don't all hammer the DB at `:00:00`.
//! * [`CancellationToken`] + `select!` shutdown → idiomatic Tokio,
//!   same primitive the channels supervisor uses.
//! * Errors log-and-continue → periodic maintenance should shrug off
//!   a transient failure; next tick retries. Retry-on-error state
//!   machines belong in a job queue (apalis), not here.
//!
//! # Observability
//!
//! [`PeriodicStatus`] is updated atomically on each run. The
//! in-memory map can be exposed through an admin endpoint to show
//! "last ran X / N ok / M failed" per job.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::Rng;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, info_span, Instrument};

/// One job's most recent execution snapshot. Cheap to `clone()`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PeriodicStatus {
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_duration_ms: Option<u128>,
    pub last_outcome: Option<String>, // "ok" | error message
    pub total_runs: u64,
    pub total_failures: u64,
    pub in_progress: bool,
}

/// Shared, clone-cheap status registry. Exposed from `main.rs` so an
/// admin endpoint can serialize it for a "jobs health" view.
pub type StatusRegistry = Arc<RwLock<HashMap<&'static str, PeriodicStatus>>>;

/// Build an empty registry. Callers register with
/// [`register_job`] via [`spawn_periodic`] automatically.
pub fn status_registry() -> StatusRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Spawn a periodic task and register its status row.
///
/// The closure is called at `every`-spaced intervals until `shutdown`
/// fires. Returns the `JoinHandle` so a graceful-shutdown hook can
/// `.await` it (main.rs typically drops the handle since tokio
/// tears tasks down with the runtime).
///
/// The closure is `FnMut` — if you need to carry per-run state across
/// ticks (e.g. a delta token, last-seen id) capture it in the outer
/// closure and mutate it inside.
pub fn spawn_periodic<F, Fut>(
    name: &'static str,
    every: Duration,
    shutdown: CancellationToken,
    statuses: StatusRegistry,
    mut job: F,
) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    // Initialize this job's slot so a status endpoint shows it as
    // registered even before its first run.
    statuses
        .write()
        .expect("scheduler status registry poisoned")
        .insert(name, PeriodicStatus::default());

    // Startup jitter — 0..(every/10) — so jobs registered in the same
    // main.rs block don't stampede the DB at identical boundaries.
    let jitter_max_ms = (every.as_millis() / 10).clamp(1, u64::MAX as u128) as u64;
    let jitter = Duration::from_millis(rand::thread_rng().gen_range(0..=jitter_max_ms));
    let mut ticker = interval_at(Instant::now() + jitter, every);
    // Default MissedTickBehavior::Burst fires missed ticks back-to-back
    // after a pause — usually a surprise. Skip is right for cleanup
    // work (missing a tick is fine; next tick catches up).
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    tokio::spawn(async move {
        info!(
            task = name,
            every_secs = every.as_secs(),
            "scheduler: job registered"
        );
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    info!(task = name, "scheduler: shutdown signal, stopping");
                    break;
                }
                _ = ticker.tick() => {
                    run_once(name, &mut job, &statuses).await;
                }
            }
        }
    })
}

/// One iteration of the loop, factored out so the status-tracking
/// bookkeeping is in one place and tests can drive a single run
/// without involving tokio::time.
async fn run_once<F, Fut>(name: &'static str, job: &mut F, statuses: &StatusRegistry)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    set_in_progress(statuses, name, true);
    // Fresh correlation id per invocation, so a grep across log lines
    // produced by one tick joins back together. Matches the per-HTTP-
    // request correlation id assigned by tracing-actix-web; the two
    // share a Uuid::now_v7 source so they're trivially comparable.
    let correlation_id = uuid::Uuid::now_v7();
    let span = info_span!("periodic", task = name, correlation_id = %correlation_id);
    let started = Instant::now();
    let result = job().instrument(span).await;
    let elapsed = started.elapsed();

    match &result {
        Ok(()) => debug!(task = name, elapsed_ms = %elapsed.as_millis(), "scheduler: ok"),
        Err(e) => {
            // Display (`%e`), not Debug (`?e`). anyhow's Debug walks
            // the full source chain AND prints the captured backtrace,
            // which lit up the scheduler logs with 60-frame stack
            // traces for what was usually a one-line operational
            // failure ("sync completed with errors", "db pool
            // exhausted"). Display gives the message + source chain
            // without the backtrace; if an operator wants the full
            // chain they can re-run with RUST_LOG=debug or inspect
            // the status registry's last_error field.
            error!(task = name, error = %e, elapsed_ms = %elapsed.as_millis(), "scheduler: failed; retrying next tick")
        }
    }
    record(statuses, name, elapsed, result.as_ref().err());
}

fn set_in_progress(statuses: &StatusRegistry, name: &'static str, value: bool) {
    if let Ok(mut map) = statuses.write() {
        map.entry(name).or_default().in_progress = value;
    }
}

fn record(
    statuses: &StatusRegistry,
    name: &'static str,
    elapsed: Duration,
    error: Option<&anyhow::Error>,
) {
    let Ok(mut map) = statuses.write() else {
        return;
    };
    let entry = map.entry(name).or_default();
    entry.last_run_at = Some(Utc::now());
    entry.last_duration_ms = Some(elapsed.as_millis());
    entry.total_runs += 1;
    entry.in_progress = false;
    if let Some(e) = error {
        entry.total_failures += 1;
        entry.last_outcome = Some(e.to_string());
    } else {
        entry.last_outcome = Some("ok".into());
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    //! Scheduler tests drive `run_once` directly (no real time) so we
    //! can verify status bookkeeping without sleeping in the test
    //! thread. The cancellation / interval plumbing in `spawn_periodic`
    //! is thin enough that its correctness follows from the Tokio
    //! primitives it composes.
    use super::*;

    #[tokio::test]
    async fn run_once_records_success() {
        let statuses = status_registry();
        let mut job = || async { Ok::<(), anyhow::Error>(()) };
        run_once("test_ok", &mut job, &statuses).await;

        let map = statuses.read().unwrap();
        let s = map.get("test_ok").expect("status registered");
        assert_eq!(s.total_runs, 1);
        assert_eq!(s.total_failures, 0);
        assert_eq!(s.last_outcome.as_deref(), Some("ok"));
        assert!(!s.in_progress);
        assert!(s.last_run_at.is_some());
    }

    #[tokio::test]
    async fn run_once_records_failure_message() {
        let statuses = status_registry();
        let mut job = || async { Err::<(), _>(anyhow::anyhow!("boom")) };
        run_once("test_err", &mut job, &statuses).await;

        let map = statuses.read().unwrap();
        let s = map.get("test_err").unwrap();
        assert_eq!(s.total_runs, 1);
        assert_eq!(s.total_failures, 1);
        assert_eq!(s.last_outcome.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn repeated_runs_accumulate_counters() {
        let statuses = status_registry();
        let mut good = || async { Ok::<(), anyhow::Error>(()) };
        let mut bad = || async { Err::<(), _>(anyhow::anyhow!("bad")) };

        run_once("mix", &mut good, &statuses).await;
        run_once("mix", &mut bad, &statuses).await;
        run_once("mix", &mut good, &statuses).await;

        let map = statuses.read().unwrap();
        let s = map.get("mix").unwrap();
        assert_eq!(s.total_runs, 3);
        assert_eq!(s.total_failures, 1);
        // Most recent outcome is the "ok" from the third run.
        assert_eq!(s.last_outcome.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn spawn_periodic_stops_on_cancel() {
        let statuses = status_registry();
        let shutdown = CancellationToken::new();
        let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let c = counter.clone();
        let handle = spawn_periodic(
            "counter",
            // Tight interval — we're validating cancellation,
            // not timing fidelity.
            Duration::from_millis(10),
            shutdown.clone(),
            statuses.clone(),
            move || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                }
            },
        );

        // Let it tick a handful of times.
        tokio::time::sleep(Duration::from_millis(80)).await;
        shutdown.cancel();
        // Wait for the task to observe the cancellation.
        handle.await.unwrap();

        let final_count = counter.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            final_count >= 1,
            "expected at least one run, got {final_count}"
        );
        // Registry should carry the final state too.
        let map = statuses.read().unwrap();
        assert!(map.contains_key("counter"));
    }
}
