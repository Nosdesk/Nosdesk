//! Workspace reindexes another process asks for.
//!
//! The CLI imports and resets workspaces beside the running server, and the
//! server holds the search index (one writer per index directory), so the
//! CLI can't reindex. It asks with `pg_notify` on
//! [`REQUEST_CHANNEL`] (`repository::search_reindex::request`), payload the
//! workspace id, and each server listening here rebuilds that workspace's
//! documents (`indexing_tasks::spawn_reindex_workspace`). A request sent while
//! no server listens is lost; `POST /api/search/rebuild?workspace_id=N` does
//! the same by hand.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio_postgres::AsyncMessage;
use tracing::{info, warn};

use crate::repository::search_reindex::REQUEST_CHANNEL;

use super::{indexing_tasks, SearchService};

const RECONNECT_BACKOFF_MS: u64 = 500;
const RECONNECT_BACKOFF_MAX_MS: u64 = 5_000;

/// Listen for reindex requests for the life of the process.
pub fn spawn(
    database_url: String,
    search: Arc<SearchService>,
    shutdown: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            _ = shutdown.cancelled() => info!("search reindex requests: shutting down"),
            _ = run(database_url, search) => {}
        }
    })
}

async fn run(database_url: String, search: Arc<SearchService>) {
    let mut backoff_ms = RECONNECT_BACKOFF_MS;
    loop {
        match listen(&database_url, &search).await {
            Ok(()) => backoff_ms = RECONNECT_BACKOFF_MS,
            Err(e) => {
                warn!(error = %e, "search reindex requests: disconnected; reconnecting");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(RECONNECT_BACKOFF_MAX_MS);
            }
        }
    }
}

async fn listen(database_url: &str, search: &Arc<SearchService>) -> anyhow::Result<()> {
    let (client, mut messages) = crate::db::listen::connect(database_url).await?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<i32>();
    let driver = tokio::spawn(async move {
        while let Some(message) = messages.next().await {
            match message {
                Ok(AsyncMessage::Notification(n)) => match n.payload().trim().parse::<i32>() {
                    Ok(workspace_id) => {
                        if tx.send(workspace_id).is_err() {
                            break;
                        }
                    }
                    Err(_) => warn!("search reindex requests: a request without a workspace id"),
                },
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "search reindex requests: connection error");
                    break;
                }
            }
        }
    });
    client
        .batch_execute(&format!("LISTEN {REQUEST_CHANNEL}"))
        .await?;
    info!("search reindex requests: listening");
    while let Some(workspace_id) = rx.recv().await {
        info!(workspace_id, "search reindex requested");
        indexing_tasks::spawn_reindex_workspace(Arc::clone(search), workspace_id);
    }
    driver.abort();
    Ok(())
}
