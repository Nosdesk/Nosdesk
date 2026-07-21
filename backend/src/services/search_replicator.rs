//! Search-index replicator.
//!
//! Keeps each machine's local Tantivy index current with writes made on
//! *other* machines, by projecting the `sync_actions` change stream into
//! the index. Without this, on N>1 Fly machines an entity indexed on
//! machine A (via the write-time observer) is invisible to a search on
//! machine B, because the index is per-machine local disk.
//!
//! Same plumbing as [`crate::services::sync_outbox`]: a dedicated
//! `tokio_postgres` LISTEN connection on `sync_actions_new`, drain-since-
//! watermark on each notification, reconnect with backoff. The difference
//! is the sink — instead of broadcasting rows to SSE, each row is applied
//! to the index via [`SearchService::apply_sync_action`] (delete-by-id, or
//! reload-from-Postgres + upsert-by-id).
//!
//! Relationship to the write-time observer: this is *additive*. The
//! observer still indexes immediately on the machine that handled the
//! write (and, crucially, is the only thing that sees collaborative
//! article-body saves, which are deliberately off the sync stream). The
//! replicator re-applies the structured change everywhere; because
//! `apply_sync_action` is delete/upsert-by-id, the writer machine doing
//! both is idempotent.
//!
//! Gated behind `NOSDESK_SEARCH_REPLICATION` (see `main.rs`): a single
//! machine doesn't need it, so self-hosted and the single-machine deploy
//! run observer-only with zero extra load.

use std::sync::Arc;
use std::time::Duration;

use diesel::prelude::*;
use futures::FutureExt;
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::models::{SyncAggregate, SyncOp};
use crate::schema::sync_actions;
use crate::services::search::SearchService;

/// Page size for the drain query — same cap as the sync outbox.
const DRAIN_PAGE_SIZE: i64 = 1000;

const RECONNECT_BACKOFF_MS: u64 = 500;
const RECONNECT_BACKOFF_MAX_MS: u64 = 5_000;

/// Just the fields the index projection needs from a `sync_actions` row.
#[derive(Debug, Queryable)]
struct IndexRow {
    sync_id: i64,
    aggregate: SyncAggregate,
    aggregate_id: String,
    op: SyncOp,
}

/// Spawn the replicator. Returns immediately; the task runs for the
/// process lifetime, reconnecting on its own.
pub fn spawn(
    database_url: String,
    pool: Pool,
    search: Arc<SearchService>,
    shutdown: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            _ = shutdown.cancelled() => info!("search replicator: shutting down"),
            _ = run(database_url, pool, search) => {}
        }
    })
}

async fn run(database_url: String, pool: Pool, search: Arc<SearchService>) {
    // Start from the current max: everything already in the index came
    // from the boot rebuild (or this machine's observer). We only tail
    // changes from here forward; the boot rebuild owns the baseline.
    let mut watermark = match initial_watermark(&pool) {
        Ok(w) => w,
        Err(e) => {
            error!(error = %e, "search replicator: failed to read initial watermark; defaulting to 0");
            0
        }
    };
    info!(initial_watermark = watermark, "search replicator starting");

    let mut backoff_ms = RECONNECT_BACKOFF_MS;
    loop {
        match listen_loop(&database_url, &pool, &search, &mut watermark).await {
            Ok(()) => {
                warn!("search replicator exited cleanly; reconnecting");
                backoff_ms = RECONNECT_BACKOFF_MS;
            }
            Err(e) => {
                warn!(error = %e, backoff_ms, "search replicator disconnected; reconnecting");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(RECONNECT_BACKOFF_MAX_MS);
            }
        }
    }
}

fn initial_watermark(pool: &Pool) -> Result<i64, anyhow::Error> {
    // sync_actions is RLS-enabled; the replicator reads across every
    // workspace (the index spans tenants), so it bypasses via background_run.
    // cross-tenant: watermark over the cross-tenant sync_actions feed.
    let max_id: Option<i64> = crate::sync::session::background_run(
        pool,
        "background:search_replicator_watermark",
        |conn| {
            sync_actions::table
                .select(diesel::dsl::max(sync_actions::sync_id))
                .first(conn)
        },
    )?;
    Ok(max_id.unwrap_or(0))
}

async fn listen_loop(
    database_url: &str,
    pool: &Pool,
    search: &Arc<SearchService>,
    watermark: &mut i64,
) -> Result<(), anyhow::Error> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

    let (notif_tx, mut notif_rx) = mpsc::channel::<()>(64);
    let driver = tokio::spawn(async move {
        use futures::StreamExt;
        let mut connection = Box::pin(connection);
        let mut stream = futures::stream::poll_fn(move |cx| connection.as_mut().poll_message(cx));
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(AsyncMessage::Notification(_)) => {
                    if notif_tx.send(()).await.is_err() {
                        break;
                    }
                }
                Ok(AsyncMessage::Notice(n)) => debug!(message = %n.message(), "postgres notice"),
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "postgres connection error");
                    break;
                }
            }
        }
    });

    client.batch_execute("LISTEN sync_actions_new").await?;
    info!("search replicator listening on `sync_actions_new`");

    // Catch-up on connect: drain anything committed during the gap.
    drain_since(pool, search, watermark).await?;

    while notif_rx.recv().await.is_some() {
        if let Err(e) = drain_since(pool, search, watermark).await {
            warn!(error = %e, "search replicator drain failed; will retry on next notification");
        }
    }

    let _ = driver.now_or_never();
    Err(anyhow::anyhow!("notification stream closed"))
}

async fn drain_since(
    pool: &Pool,
    search: &Arc<SearchService>,
    watermark: &mut i64,
) -> Result<(), anyhow::Error> {
    loop {
        let snapshot_watermark = *watermark;
        let pool = pool.clone();
        let search = search.clone();

        // Diesel (blocking) load + Tantivy writes happen on the blocking
        // pool; `apply_sync_action` reloads from Postgres and takes the
        // index writer lock, neither of which is async-friendly.
        let (last_sync_id, count) =
            tokio::task::spawn_blocking(move || -> Result<(Option<i64>, usize), anyhow::Error> {
                // cross-tenant: pages the cross-tenant sync_actions feed.
                let rows: Vec<IndexRow> = crate::sync::session::background_run(
                    &pool,
                    "background:search_replicator_drain",
                    |conn| {
                        sync_actions::table
                            .filter(sync_actions::sync_id.gt(snapshot_watermark))
                            .order(sync_actions::sync_id.asc())
                            .limit(DRAIN_PAGE_SIZE)
                            .select((
                                sync_actions::sync_id,
                                sync_actions::aggregate,
                                sync_actions::aggregate_id,
                                sync_actions::op,
                            ))
                            .load::<IndexRow>(conn)
                    },
                )?;

                if rows.is_empty() {
                    return Ok((None, 0));
                }

                let last = rows.last().unwrap().sync_id;
                let count = rows.len();
                for row in &rows {
                    // A single bad row (parse error, transient load failure)
                    // must not stall the whole stream; log and carry on.
                    if let Err(e) =
                        search.apply_sync_action(row.aggregate, row.op, &row.aggregate_id)
                    {
                        warn!(
                            sync_id = row.sync_id,
                            aggregate = ?row.aggregate,
                            aggregate_id = %row.aggregate_id,
                            error = %e,
                            "search replicator: failed to apply sync action"
                        );
                    }
                }
                // One commit per drained batch rather than per row.
                if let Err(e) = search.commit() {
                    return Err(anyhow::anyhow!("search index commit failed: {e}"));
                }
                Ok((Some(last), count))
            })
            .await??;

        match last_sync_id {
            None => return Ok(()),
            Some(id) => {
                *watermark = id;
                debug!(to = id, count, "search replicator applied batch");
                if (count as i64) < DRAIN_PAGE_SIZE {
                    return Ok(());
                }
            }
        }
    }
}
