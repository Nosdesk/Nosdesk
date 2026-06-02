//! Channel-worker supervisor.
//!
//! One long-lived Tokio task owns the [`ChannelRegistry`] map. HTTP
//! handlers never touch the map directly — they hand in `Upsert(id)` /
//! `Delete(id)` commands through an [`mpsc::Sender`], and the
//! supervisor is the sole writer.
//!
//! # Why this shape
//!
//! The alternative — `Arc<RwLock<HashMap<i32, WorkerHandle>>>` shared
//! between handlers and the runtime — invites two problems:
//!
//!   1. Locking across `.await` is a deadlock footgun, especially with
//!      tokio's `Mutex`.
//!   2. Races between a POST that creates a row and a concurrent DELETE
//!      leave the handler guessing which state to boot the worker from.
//!
//! By routing commands through mpsc and letting the supervisor re-read
//! the channel row from the DB, we serialize reconciliation and always
//! act on the authoritative state.
//!
//! # Command semantics
//!
//! - [`ChannelCmd::Upsert`]: "the row with this id might be new,
//!   changed, enabled, or disabled — reconcile". The supervisor stops
//!   the existing worker (if any), re-reads the row, and starts a new
//!   worker iff `enabled = true`. Also handles "row was deleted"
//!   gracefully — stopped worker, nothing started, no error.
//! - [`ChannelCmd::Delete`]: "the row is gone (or should be treated
//!   as gone) — stop polling". Idempotent.
//!
//! # Handler usage
//!
//! ```ignore
//! // After a successful Diesel insert/update:
//! control.upsert(channel.id).await;
//! // After a successful DELETE:
//! control.delete(channel.id).await;
//! ```
//!
//! The send is `await`'d so a wedged supervisor surfaces as backpressure
//! on the admin UI rather than silent command loss.

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::repository::channels as channels_repo;
use crate::services::channels::registry::{ChannelRegistry, RegistryDeps};

/// Bounded mailbox size. Admin edits are rare; 64 outstanding commands
/// is a comfortable ceiling that lets us fail loudly if something goes
/// wrong rather than allocating unboundedly.
const COMMAND_BUFFER: usize = 64;

/// Commands the supervisor understands. Deliberately carries only an
/// `i32` id rather than a fully-serialized config — the supervisor
/// always re-reads the authoritative row from the DB.
#[derive(Debug)]
pub enum ChannelCmd {
    /// "Reconcile this channel": stop its worker if running, then
    /// start a fresh one from the current DB state (or do nothing if
    /// the row is disabled or missing).
    Upsert(i32),
    /// Stop the worker for this channel. Idempotent.
    Delete(i32),
}

/// Handle the HTTP layer uses to drive the supervisor. Cheap to clone
/// (just an `Arc` of the mpsc sender); store once in `web::Data` at
/// startup and hand out clones per handler.
#[derive(Clone)]
pub struct ChannelControl {
    tx: mpsc::Sender<ChannelCmd>,
}

impl ChannelControl {
    pub async fn upsert(&self, channel_id: i32) {
        if let Err(e) = self.tx.send(ChannelCmd::Upsert(channel_id)).await {
            warn!(channel_id, error = %e, "channel control: supervisor is gone; upsert dropped");
        }
    }

    pub async fn delete(&self, channel_id: i32) {
        if let Err(e) = self.tx.send(ChannelCmd::Delete(channel_id)).await {
            warn!(channel_id, error = %e, "channel control: supervisor is gone; delete dropped");
        }
    }
}

/// Spawn the supervisor task, hydrate it with the current set of
/// enabled channels, and return the [`ChannelControl`] plus the
/// supervisor's [`JoinHandle`] (caller typically drops it; returned
/// so tests can `.await` clean shutdown).
///
/// The supervisor lives for the lifetime of the process. Dropping the
/// final `ChannelControl` clone (e.g. during shutdown) closes the mpsc
/// channel, the supervisor's `recv()` returns `None`, and it drains
/// all workers before exiting.
pub fn spawn(deps: RegistryDeps) -> (ChannelControl, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(COMMAND_BUFFER);
    let join = tokio::spawn(run(rx, deps));
    (ChannelControl { tx }, join)
}

/// Supervisor loop. Exposed for tests that want to drive it without
/// going through `tokio::spawn`.
async fn run(mut rx: mpsc::Receiver<ChannelCmd>, deps: RegistryDeps) {
    let mut registry = ChannelRegistry::new(deps.clone());

    // Hydrate from the DB so enabled channels at startup get a worker
    // without a synthetic Upsert storm from the caller. channels is
    // RLS-enabled; the supervisor is platform-level (manages every
    // workspace's enabled channels), so background_run with bypass
    // is correct.
    match crate::sync::session::background_run(
        &deps.pool,
        "background:channel_supervisor_hydrate",
        channels_repo::list_enabled,
    ) {
        Ok(channels) => {
            let count = channels.len();
            registry.start_many(channels);
            info!(count, "channel supervisor: hydrated from DB");
        }
        Err(e) => error!(error = %e, "channel supervisor: list_enabled failed"),
    }

    info!("channel supervisor: ready");
    while let Some(cmd) = rx.recv().await {
        handle(cmd, &mut registry, &deps).await;
    }
    info!("channel supervisor: command channel closed; shutting down");
    registry.shutdown().await;
}

/// One-step command handler. Split out so tests can drive it without
/// owning an mpsc receiver.
async fn handle(cmd: ChannelCmd, registry: &mut ChannelRegistry, deps: &RegistryDeps) {
    match cmd {
        ChannelCmd::Upsert(id) => reconcile(id, registry, deps).await,
        ChannelCmd::Delete(id) => {
            let stopped = registry.stop(id).await;
            debug!(channel_id = id, stopped, "channel supervisor: deleted");
        }
    }
}

/// Apply an Upsert: stop any running worker, re-read the row, start a
/// new worker iff the row exists and is enabled. Errors are logged but
/// never panic the supervisor — one bad row must not take down healthy
/// workers for other channels.
async fn reconcile(id: i32, registry: &mut ChannelRegistry, deps: &RegistryDeps) {
    registry.stop(id).await;

    // channels is RLS-enabled; reconcile is cross-tenant
    // (supervisor manages every workspace's channels).
    let channel = match crate::sync::session::background_run(
        &deps.pool,
        "background:channel_supervisor_reconcile",
        |conn| channels_repo::find(conn, id),
    ) {
        Ok(c) => c,
        Err(crate::sync::session::BackgroundRunError::Db(diesel::result::Error::NotFound)) => {
            debug!(
                channel_id = id,
                "channel supervisor: row deleted; no worker to start"
            );
            return;
        }
        Err(e) => {
            warn!(channel_id = id, error = %e, "channel supervisor: load failed");
            return;
        }
    };

    if !channel.enabled {
        debug!(
            channel_id = id,
            "channel supervisor: row disabled; leaving stopped"
        );
        return;
    }

    if let Err(e) = registry.start(channel) {
        error!(channel_id = id, error = %e, "channel supervisor: start failed");
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    //! Supervisor tests drive `handle` directly against a live
    //! `ChannelRegistry` so the concurrency surface (mpsc, tokio::spawn)
    //! doesn't leak into every assertion. `start` / `stop` are already
    //! covered in `registry::tests`; here we only verify the
    //! command-dispatch + DB-reread logic.

    use super::*;
    use crate::models::{ChannelUpdate, NewChannel};
    use crate::repository::channels as channels_repo;
    use crate::test_helpers::setup_test_pool;

    fn bare_deps(pool: crate::db::Pool) -> RegistryDeps {
        RegistryDeps {
            pool,
            email: None,
            sse: None,
            search: None,
            storage: None,
            http: None,
        }
    }

    /// Build a channel row with a config that would pass validation
    /// except we leave `email` out of deps so `build_pull_adapter`
    /// returns `MissingEmailService`. That's fine — the supervisor
    /// logs and moves on; we only want to verify the reconcile path
    /// reaches the start call.
    fn insert_enabled(pool: &crate::db::Pool, provider: &str) -> i32 {
        let mut conn = pool.get().unwrap();
        channels_repo::create(
            &mut conn,
            NewChannel {
                provider: provider.into(),
                name: "test".into(),
                enabled: true,
                config: serde_json::json!({
                    "host": "imap.example.com",
                    "username": "u",
                    "reply_domain": "example.com",
                }),
            },
        )
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn delete_stops_missing_worker_without_error() {
        let pool = setup_test_pool();
        let deps = bare_deps(pool.clone());
        let mut registry = ChannelRegistry::new(deps.clone());
        // No worker for id 999; Delete is a no-op, not a panic.
        handle(ChannelCmd::Delete(999), &mut registry, &deps).await;
    }

    #[tokio::test]
    async fn upsert_of_disabled_channel_is_a_noop() {
        let pool = setup_test_pool();
        let deps = bare_deps(pool.clone());
        let mut registry = ChannelRegistry::new(deps.clone());
        let id = insert_enabled(&pool, "email_imap");

        // Flip to disabled.
        {
            let mut conn = pool.get().unwrap();
            channels_repo::update(
                &mut conn,
                id,
                ChannelUpdate {
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        }

        handle(ChannelCmd::Upsert(id), &mut registry, &deps).await;
        assert!(
            !registry.is_running(id),
            "disabled channel should not have a running worker"
        );
    }

    #[tokio::test]
    async fn upsert_of_missing_row_is_a_noop() {
        let pool = setup_test_pool();
        let deps = bare_deps(pool.clone());
        let mut registry = ChannelRegistry::new(deps.clone());

        // id that doesn't exist in the DB.
        handle(ChannelCmd::Upsert(404404), &mut registry, &deps).await;
        assert!(!registry.is_running(404404));
    }

    #[tokio::test]
    async fn upsert_of_unsupported_provider_logs_and_moves_on() {
        let pool = setup_test_pool();
        let deps = bare_deps(pool.clone());
        let mut registry = ChannelRegistry::new(deps.clone());
        let id = insert_enabled(&pool, "quantum-mail"); // unknown provider

        // Reconcile: build_pull_adapter returns UnsupportedProvider;
        // the supervisor must absorb it without panicking.
        handle(ChannelCmd::Upsert(id), &mut registry, &deps).await;
        assert!(!registry.is_running(id));
    }
}
