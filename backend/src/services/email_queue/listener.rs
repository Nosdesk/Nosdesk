//! Listener that wakes the worker on `pg_notify('outbound_emails_new')`.
//!
//! Single background task. Mirrors `services::sync_outbox` exactly —
//! same dual-connection pattern (a dedicated `tokio_postgres` LISTEN
//! connection alongside the r2d2/Diesel pool, since libpq doesn't
//! expose async LISTEN), same reconnect-with-backoff loop. Differences:
//!
//! - No watermark: the worker's `claim_batch` is self-walking via the
//!   `outbound_emails_due_idx` partial index. There's no "we saw row N
//!   already" state to maintain.
//! - No SSE broadcast: the listener invokes the worker directly, which
//!   talks to SMTP. SSE notifications about queue state changes (for
//!   the admin UI) are emitted by the worker, not here.
//! - 30-second safety-net tick: in case we miss a notification (rare —
//!   reconnect window or the LISTEN connection dropped just as a
//!   producer wrote a row), the periodic tick guarantees we drain at
//!   least every 30s. Equivalent to the worst-case latency for an
//!   email send.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::services::email_queue::circuit::CircuitBreakerRegistry;
use crate::services::email_queue::worker;
use crate::services::outbound_email::OutboundEmailResolver;

const RECONNECT_BACKOFF_MS: u64 = 500;
const RECONNECT_BACKOFF_MAX_MS: u64 = 5_000;
const SAFETY_NET_TICK: Duration = Duration::from_secs(30);

/// Spawn the listener. Returns immediately; the task lives for the
/// process. Errors inside the task are logged and the listener
/// reconnects — there's no recoverable "task failed" state for the
/// caller to act on.
pub fn spawn(database_url: String, pool: Pool, resolver: Arc<OutboundEmailResolver>) {
    let registry = Arc::new(CircuitBreakerRegistry::new());
    tokio::spawn(async move {
        run(database_url, pool, resolver, registry).await;
    });
}

async fn run(
    database_url: String,
    pool: Pool,
    resolver: Arc<OutboundEmailResolver>,
    registry: Arc<CircuitBreakerRegistry>,
) {
    info!("email queue listener starting");
    let mut backoff_ms = RECONNECT_BACKOFF_MS;
    loop {
        match listen_loop(&database_url, &pool, &resolver, &registry).await {
            Ok(()) => {
                warn!("email queue listener exited cleanly; reconnecting");
                backoff_ms = RECONNECT_BACKOFF_MS;
            }
            Err(e) => {
                warn!(
                    error = %e,
                    backoff_ms,
                    "email queue listener disconnected; reconnecting"
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(RECONNECT_BACKOFF_MAX_MS);
            }
        }
    }
}

async fn listen_loop(
    database_url: &str,
    pool: &Pool,
    resolver: &Arc<OutboundEmailResolver>,
    registry: &Arc<CircuitBreakerRegistry>,
) -> Result<(), anyhow::Error> {
    // Same dedicated tokio_postgres connection pattern as sync_outbox.
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

    let (notif_tx, mut notif_rx) = mpsc::channel::<()>(64);
    let driver = tokio::spawn(async move {
        use futures::StreamExt;
        let mut connection = Box::pin(connection);
        let mut stream = futures::stream::poll_fn(move |cx| connection.as_mut().poll_message(cx));
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(AsyncMessage::Notification(_)) => {
                    // Empty payload — multi-row commits dedupe to a
                    // single wakeup; the worker's claim covers everything
                    // due via the partial index.
                    if notif_tx.send(()).await.is_err() {
                        break;
                    }
                }
                Ok(AsyncMessage::Notice(n)) => {
                    debug!(message = %n.message(), "postgres notice (email queue)");
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "postgres connection error (email queue)");
                    break;
                }
            }
        }
    });

    client.batch_execute("LISTEN outbound_emails_new").await?;
    info!("email queue listening on `outbound_emails_new`");

    // Catch-up on connect: anything enqueued between our last drain
    // and this LISTEN was missed. Drain now before tailing.
    drain(pool, resolver, registry).await;

    let mut safety_tick = tokio::time::interval(SAFETY_NET_TICK);
    safety_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    safety_tick.tick().await; // first tick fires immediately; absorb it

    loop {
        tokio::select! {
            recv = notif_rx.recv() => {
                if recv.is_none() {
                    break;
                }
                drain(pool, resolver, registry).await;
            }
            _ = safety_tick.tick() => {
                // Belt-and-braces: in case a notification was missed
                // (reconnect window, channel buffer overflow). Cheap
                // when the queue is empty — claim_batch returns []
                // immediately.
                drain(pool, resolver, registry).await;
            }
        }
    }

    let _ = driver.now_or_never();
    Err(anyhow::anyhow!("notification stream closed"))
}

async fn drain(
    pool: &Pool,
    resolver: &Arc<OutboundEmailResolver>,
    registry: &Arc<CircuitBreakerRegistry>,
) {
    // Loop until the worker reports an empty claim — drains a burst
    // (multi-row commit) in one notification rather than waiting for
    // the next tick.
    loop {
        let stats =
            match worker::run_one_drain(pool.clone(), resolver.clone(), registry.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    error!(error = %e, "email queue: drain failed");
                    break;
                }
            };
        if stats.claimed == 0 {
            break;
        }
        // If the instance relay's breaker tripped mid-batch, stop looping until
        // next tick rather than re-claiming rows we'll only release.
        if stats.circuit_skipped > 0 {
            break;
        }
    }
}
