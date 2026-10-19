//! What the background check and commit share: marking the job alive,
//! the commit's progress, and database work off the async worker.

use std::future::Future;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;
use uuid::Uuid;

use super::{HEARTBEAT, PROGRESS_BATCH};
use crate::extractors::TenantConn;
use crate::repository::imports as repo;

/// The pages a commit has written so far, shared by its transaction and
/// its heartbeat. Every [`PROGRESS_BATCH`] pages the heartbeat records the
/// count at once instead of at its next beat, so Done moves as the pages
/// are written.
#[derive(Default)]
pub struct Progress {
    written: AtomicI32,
    batch: Notify,
}

impl Progress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// One more page written. Safe from any thread.
    pub fn page_written(&self) {
        let written = self.written.fetch_add(1, Ordering::Relaxed) + 1;
        if written % PROGRESS_BATCH == 0 {
            self.batch.notify_one();
        }
    }

    pub fn written(&self) -> i32 {
        self.written.load(Ordering::Relaxed)
    }

    /// Resolves once a batch completes (at once if one completed since the
    /// last wait).
    async fn next_batch(&self) {
        self.batch.notified().await
    }
}

/// Marks the job alive every [`HEARTBEAT`] while it is in `status`, until
/// dropped.
pub struct Heartbeat {
    task: tokio::task::JoinHandle<()>,
}

impl Heartbeat {
    /// Start beating for job `id` in `status`. With `progress`, each beat
    /// also records the pages written so far, and a finished batch beats at
    /// once.
    pub fn start(
        tc: TenantConn,
        id: Uuid,
        status: &'static str,
        progress: Option<Arc<Progress>>,
    ) -> Self {
        Self::start_with(progress, move |done| {
            let mut tc = tc.clone();
            async move {
                let beat = tokio::task::spawn_blocking(move || {
                    tc.run(|conn| repo::heartbeat(conn, id, status, done))
                })
                .await;
                match beat {
                    Ok(Ok(0)) => false,
                    Ok(Ok(_)) => true,
                    Ok(Err(e)) => {
                        tracing::warn!(id = %id, error = ?e, "import heartbeat failed");
                        true
                    }
                    Err(e) => {
                        tracing::warn!(id = %id, error = ?e, "import heartbeat failed");
                        true
                    }
                }
            }
        })
    }

    /// Beat with `beat`, which is given the pages written (with `progress`)
    /// and answers whether to go on.
    fn start_with<F, Fut>(progress: Option<Arc<Progress>>, mut beat: F) -> Self
    where
        F: FnMut(Option<i32>) -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send,
    {
        let task = tokio::spawn(async move {
            let mut every = tokio::time::interval(HEARTBEAT);
            // The first tick is immediate; the run set `updated_at` itself.
            every.tick().await;
            loop {
                match &progress {
                    Some(p) => {
                        tokio::select! {
                            _ = every.tick() => {}
                            _ = p.next_batch() => {}
                        }
                    }
                    None => {
                        every.tick().await;
                    }
                }
                let done = progress.as_ref().map(|p| p.written());
                if !beat(done).await {
                    break;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn each_batch_of_pages_records_progress_at_once() {
        let progress = Progress::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _beat = Heartbeat::start_with(Some(progress.clone()), move |done| {
            let _ = tx.send(done);
            async { true }
        });

        // Short of a batch, nothing is recorded before the 30 s beat.
        for _ in 0..PROGRESS_BATCH - 1 {
            progress.page_written();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(200), rx.recv())
                .await
                .is_err(),
            "no beat before a batch completes"
        );

        progress.page_written();
        let recorded = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the batch beats at once");
        assert_eq!(recorded, Some(Some(PROGRESS_BATCH)));

        // A batch finished while nobody waited still beats.
        for _ in 0..PROGRESS_BATCH {
            progress.page_written();
        }
        let recorded = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the second batch beats");
        assert_eq!(recorded, Some(Some(PROGRESS_BATCH * 2)));
    }

    #[tokio::test]
    async fn a_beat_that_finds_the_job_moved_on_stops() {
        let progress = Progress::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _beat = Heartbeat::start_with(Some(progress.clone()), move |done| {
            let _ = tx.send(done);
            async { false }
        });
        for _ in 0..PROGRESS_BATCH {
            progress.page_written();
        }
        assert!(tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .is_ok());
        // The loop ended and dropped the sender with it.
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("closed"),
            None
        );
    }
}
