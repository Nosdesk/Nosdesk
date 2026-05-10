//! Sync-action outbox listener.
//!
//! Single background task that broadcasts every committed
//! `sync_actions` row to SSE subscribers. The flow:
//!
//!   1. The DB trigger `sync_actions_notify_trigger` (see migration
//!      `2026-05-11-100000_sync_actions_notify_trigger`) fires
//!      `pg_notify('sync_actions_new', '')` after every insert. The
//!      `NOTIFY` is buffered by Postgres until the writer's
//!      transaction commits, so listeners only ever see post-commit
//!      events.
//!
//!   2. This service holds a dedicated async Postgres connection
//!      (`tokio_postgres`, *outside* the r2d2 pool — Diesel's libpq
//!      client is synchronous and doesn't expose notifications
//!      cleanly) and `LISTEN`s for the `sync_actions_new` channel.
//!
//!   3. On each notification, drain `sync_actions WHERE sync_id >
//!      watermark` via the existing Diesel pool, broadcast the rows
//!      as `SseEvent::SyncActions`, advance the watermark.
//!
//! Why drain-since-watermark instead of "fetch the row whose id is
//! in the notification payload": the trigger sends an empty payload
//! so multi-row commits dedupe to a single wakeup; the watermark
//! query then catches every row that committed since last drain.
//! Robust to dropped notifications, debounced bursts, and concurrent
//! writers (a NOTIFY handler that only knew about its own row would
//! miss any rows committed concurrently from other writers).
//!
//! Recovery: on disconnect, the loop reconnects with backoff and
//! immediately drains since the watermark. Anything that committed
//! during the gap is broadcast on reconnect. For longer gaps —
//! backend restart, network partition that exceeded our retention —
//! the frontend's existing `/api/sync/delta?from=N` catch-up
//! endpoint covers the gap on client reconnection. The watermark
//! is in-memory by design: persisting it would only matter if SSE
//! were the *only* delivery path, which it isn't.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use diesel::prelude::*;
use futures::FutureExt;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::handlers::sse::{SseEvent, SseState};
use crate::schema::sync_actions;

/// Page size for the drain query. Multi-row commits dedupe to a
/// single notification, so a single drain might cover many writes.
/// 1000 is the same cap the `/api/sync/delta` endpoint uses; the
/// listener loops if it hits the cap so larger bursts get drained
/// over multiple iterations rather than a single oversized payload.
const DRAIN_PAGE_SIZE: i64 = 1000;

/// Reconnect backoff. Capped — we want quick recovery from a
/// Postgres restart, not silent data loss from runaway retries.
const RECONNECT_BACKOFF_MS: u64 = 500;
const RECONNECT_BACKOFF_MAX_MS: u64 = 5_000;

/// Wire shape for `sync_actions` rows. Mirrors
/// `handlers::sync::delta::ActionRow` exactly — frontend already
/// has typed handling for this shape via the existing `SyncActions`
/// SSE event. Defined here too rather than crossing the
/// `handlers::sync` module boundary, since the listener belongs in
/// `services::` and the row shape is small + stable.
#[derive(Debug, Serialize, Queryable)]
struct ActionRow {
    sync_id: i64,
    aggregate: crate::models::SyncAggregate,
    aggregate_id: String,
    op: crate::models::SyncOp,
    event_type: String,
    schema_version: i16,
    data: serde_json::Value,
    groups: Vec<Option<String>>,
    actor_uuid: Option<uuid::Uuid>,
    actor_kind: String,
    actor_ref: Option<String>,
    correlation_id: Option<uuid::Uuid>,
    causation_id: Option<uuid::Uuid>,
    occurred_at: chrono::DateTime<chrono::Utc>,
}

/// Spawn the outbox listener. Returns immediately; the task runs
/// for the lifetime of the process. Errors inside the task are
/// logged and the listener reconnects — there's no recoverable
/// "the task failed" state for the caller to act on.
pub fn spawn(database_url: String, pool: Pool, sse: Arc<SseState>) {
    tokio::spawn(async move {
        run(database_url, pool, sse).await;
    });
}

async fn run(database_url: String, pool: Pool, sse: Arc<SseState>) {
    // Initial watermark = current MAX(sync_id). Anything written
    // before this moment is "history" — clients fetch it via
    // bootstrap or the delta endpoint, not via our live broadcast.
    let mut watermark = match initial_watermark(&pool) {
        Ok(w) => w,
        Err(e) => {
            error!(error = %e, "outbox listener: failed to read initial watermark; defaulting to 0");
            0
        }
    };
    info!(initial_watermark = watermark, "sync outbox listener starting");

    let mut backoff_ms = RECONNECT_BACKOFF_MS;
    loop {
        match listen_loop(&database_url, &pool, &sse, &mut watermark).await {
            Ok(()) => {
                // listen_loop only returns Ok if the notification
                // channel closed normally — treat as needing to
                // reconnect.
                warn!("sync outbox listener exited cleanly; reconnecting");
                backoff_ms = RECONNECT_BACKOFF_MS;
            }
            Err(e) => {
                warn!(
                    error = %e,
                    backoff_ms,
                    "sync outbox listener disconnected; reconnecting"
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(RECONNECT_BACKOFF_MAX_MS);
            }
        }
    }
}

fn initial_watermark(pool: &Pool) -> Result<i64, anyhow::Error> {
    let mut conn = pool.get()?;
    let max_id: Option<i64> = sync_actions::table
        .select(diesel::dsl::max(sync_actions::sync_id))
        .first(&mut conn)?;
    Ok(max_id.unwrap_or(0))
}

async fn listen_loop(
    database_url: &str,
    pool: &Pool,
    sse: &SseState,
    watermark: &mut i64,
) -> Result<(), anyhow::Error> {
    // tokio-postgres needs a separate connection from r2d2's pool.
    // Diesel's libpq client doesn't expose async LISTEN cleanly.
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

    // Drive the protocol on a dedicated task. `poll_message` yields
    // both protocol messages and async notifications; we forward
    // notifications via channel and discard everything else.
    // `Box::pin` moves the connection into the closure with a
    // stable pin — `poll_message` needs `Pin<&mut Self>`.
    let (notif_tx, mut notif_rx) = mpsc::channel::<()>(64);
    let driver = tokio::spawn(async move {
        use futures::StreamExt;
        let mut connection = Box::pin(connection);
        let mut stream =
            futures::stream::poll_fn(move |cx| connection.as_mut().poll_message(cx));
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(AsyncMessage::Notification(_)) => {
                    // Empty payload — the listener doesn't care
                    // which sync_id triggered it, only that
                    // *something* committed. The drain query finds
                    // everything since the watermark.
                    if notif_tx.send(()).await.is_err() {
                        break;
                    }
                }
                Ok(AsyncMessage::Notice(n)) => {
                    debug!(message = %n.message(), "postgres notice");
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "postgres connection error");
                    break;
                }
            }
        }
    });

    client.batch_execute("LISTEN sync_actions_new").await?;
    info!("sync outbox listening on `sync_actions_new`");

    // Catch-up on connect: anything written between our last drain
    // and this LISTEN was missed. Drain it now before tailing.
    drain_since(pool, sse, watermark).await?;

    while notif_rx.recv().await.is_some() {
        if let Err(e) = drain_since(pool, sse, watermark).await {
            warn!(error = %e, "sync outbox drain failed; will retry on next notification");
        }
    }

    // Notification channel closed → driver task ended → connection
    // is dead. Bail so the outer `run` loop reconnects.
    let _ = driver.now_or_never();
    Err(anyhow::anyhow!("notification stream closed"))
}

async fn drain_since(
    pool: &Pool,
    sse: &SseState,
    watermark: &mut i64,
) -> Result<(), anyhow::Error> {
    // Loop in case a single notification covered more than the page
    // cap (large multi-row commit, or many concurrent writers).
    loop {
        let snapshot_watermark = *watermark;
        let pool = pool.clone();
        let rows = tokio::task::spawn_blocking(move || -> Result<Vec<ActionRow>, anyhow::Error> {
            let mut conn = pool.get()?;
            let rows = sync_actions::table
                .filter(sync_actions::sync_id.gt(snapshot_watermark))
                .order(sync_actions::sync_id.asc())
                .limit(DRAIN_PAGE_SIZE)
                .select((
                    sync_actions::sync_id,
                    sync_actions::aggregate,
                    sync_actions::aggregate_id,
                    sync_actions::op,
                    sync_actions::event_type,
                    sync_actions::schema_version,
                    sync_actions::data,
                    sync_actions::groups,
                    sync_actions::actor_uuid,
                    sync_actions::actor_kind,
                    sync_actions::actor_ref,
                    sync_actions::correlation_id,
                    sync_actions::causation_id,
                    sync_actions::occurred_at,
                ))
                .load::<ActionRow>(&mut conn)?;
            Ok(rows)
        })
        .await??;

        if rows.is_empty() {
            return Ok(());
        }

        let last_sync_id = rows.last().unwrap().sync_id;
        let payload = serde_json::to_value(&rows)?;
        let count = rows.len();

        sse.broadcast_event(SseEvent::SyncActions {
            actions: payload,
            last_sync_id,
            timestamp: Utc::now(),
        })
        .await;

        debug!(
            from = snapshot_watermark,
            to = last_sync_id,
            count,
            "broadcast sync_actions batch"
        );
        *watermark = last_sync_id;

        // If we hit the page cap, there might be more — loop again
        // immediately to drain the rest before going back to wait.
        if (count as i64) < DRAIN_PAGE_SIZE {
            return Ok(());
        }
    }
}
