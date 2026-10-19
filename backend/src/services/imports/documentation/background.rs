//! What the background check and commit share: marking the job alive, and
//! database work off the async worker.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use uuid::Uuid;

use super::HEARTBEAT;
use crate::extractors::TenantConn;
use crate::repository::imports as repo;

/// Marks the job alive every [`HEARTBEAT`] while it is in `status`, until
/// dropped.
pub struct Heartbeat {
    task: tokio::task::JoinHandle<()>,
}

impl Heartbeat {
    /// Start beating for job `id` in `status`. With `progress`, each beat
    /// also records the pages written so far.
    pub fn start(
        tc: TenantConn,
        id: Uuid,
        status: &'static str,
        progress: Option<Arc<AtomicI32>>,
    ) -> Self {
        let task = tokio::spawn(async move {
            let mut every = tokio::time::interval(HEARTBEAT);
            // The first tick is immediate; the run set `updated_at` itself.
            every.tick().await;
            loop {
                every.tick().await;
                let mut tc = tc.clone();
                let done = progress.as_ref().map(|p| p.load(Ordering::Relaxed));
                let beat = tokio::task::spawn_blocking(move || {
                    tc.run(|conn| repo::heartbeat(conn, id, status, done))
                })
                .await;
                match beat {
                    Ok(Ok(0)) => break,
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        tracing::warn!(id = %id, error = ?e, "import heartbeat failed")
                    }
                    Err(e) => {
                        tracing::warn!(id = %id, error = ?e, "import heartbeat failed")
                    }
                }
            }
        });
        Self { task }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Run `f` on a connection as the request's actor, off the async worker.
pub async fn db<T, F>(tc: &TenantConn, f: F) -> Result<T, diesel::result::Error>
where
    T: Send + 'static,
    F: FnOnce(&mut crate::db::DbConnection) -> Result<T, diesel::result::Error> + Send + 'static,
{
    let mut tc = tc.clone();
    tokio::task::spawn_blocking(move || tc.run(f))
        .await
        .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?
}
