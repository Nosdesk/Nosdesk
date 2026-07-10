//! Concurrent long-lived connection cap, shared by the SSE and collaboration
//! WebSocket surfaces.
//!
//! Both surfaces spawn a task + hold a socket + per-session buffers for the life
//! of a connection, and neither is covered by the `/api` rate limiter (which
//! deliberately excludes long-lived endpoints). Without a cap, one authenticated
//! principal can open unbounded streams. This registry bounds the count per
//! `(user, workspace)` and process-wide; acquisition returns a [`ConnGuard`]
//! whose `Drop` releases the slot, so every disconnect path (normal close,
//! error, timeout, panic) decrements exactly once with no bookkeeping in the
//! handlers.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

/// `(user_uuid, workspace_id)`. Connections are already workspace-bound, so this
/// is the natural unit; on single-workspace self-hosted it collapses to
/// per-user.
pub type ConnKey = (Uuid, i32);

pub struct ConnectionRegistry {
    per_key: DashMap<ConnKey, usize>,
    global: AtomicUsize,
    max_per_key: usize,
    max_global: usize,
}

impl ConnectionRegistry {
    /// Build from env: `NOSDESK_MAX_CONN_PER_USER` (per user+workspace, default
    /// 50 — generous over a heavy multi-tab/multi-doc user, small enough to cap
    /// a runaway reconnect loop) and `NOSDESK_MAX_CONN_GLOBAL` (process memory
    /// backstop, default 10_000).
    pub fn from_env() -> Self {
        let max_per_key = std::env::var("NOSDESK_MAX_CONN_PER_USER")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);
        let max_global = std::env::var("NOSDESK_MAX_CONN_GLOBAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10_000);
        Self::with_limits(max_per_key, max_global)
    }

    pub fn with_limits(max_per_key: usize, max_global: usize) -> Self {
        Self {
            per_key: DashMap::new(),
            global: AtomicUsize::new(0),
            max_per_key,
            max_global,
        }
    }

    /// Reserve a connection slot for `key`, or `None` when at the per-key or
    /// global cap. On success hold the returned guard for the connection's life;
    /// dropping it frees the slot.
    pub fn try_acquire(self: &Arc<Self>, key: ConnKey) -> Option<ConnGuard> {
        // Global ceiling first (CAS loop), so a per-key rejection below can roll
        // it back cleanly.
        let mut cur = self.global.load(Ordering::Relaxed);
        loop {
            if cur >= self.max_global {
                return None;
            }
            match self.global.compare_exchange_weak(
                cur,
                cur + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }

        // Per-key under the shard entry lock.
        {
            let mut entry = self.per_key.entry(key).or_insert(0);
            if *entry >= self.max_per_key {
                drop(entry);
                self.global.fetch_sub(1, Ordering::AcqRel);
                return None;
            }
            *entry += 1;
        }

        Some(ConnGuard {
            registry: self.clone(),
            key,
        })
    }

    fn release(&self, key: &ConnKey) {
        let mut hit_zero = false;
        if let Some(mut entry) = self.per_key.get_mut(key) {
            *entry = entry.saturating_sub(1);
            hit_zero = *entry == 0;
        }
        // Keep the map bounded: drop the entry at zero, re-checking under the
        // shard lock so a concurrent acquire that just bumped it isn't lost.
        if hit_zero {
            self.per_key.remove_if(key, |_, v| *v == 0);
        }
        self.global.fetch_sub(1, Ordering::AcqRel);
    }

    #[cfg(test)]
    fn live(&self) -> usize {
        self.global.load(Ordering::Relaxed)
    }
}

/// Process-wide registry, limits read from env once. Both connection surfaces
/// acquire against this (no per-request wiring, matching `cors_allowlist`).
static REGISTRY: once_cell::sync::Lazy<Arc<ConnectionRegistry>> =
    once_cell::sync::Lazy::new(|| Arc::new(ConnectionRegistry::from_env()));

/// The shared connection registry.
pub fn global() -> &'static Arc<ConnectionRegistry> {
    &REGISTRY
}

/// Releases its connection slot on `Drop` — the reliable decrement across every
/// disconnect path. Held by `SseStream` and moved into the collab session task.
pub struct ConnGuard {
    registry: Arc<ConnectionRegistry>,
    key: ConnKey,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.registry.release(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u128) -> ConnKey {
        (Uuid::from_u128(n), 1)
    }

    #[test]
    fn per_key_cap_and_release() {
        let reg = Arc::new(ConnectionRegistry::with_limits(2, 100));
        let k = key(1);
        let g1 = reg.try_acquire(k).expect("1");
        let _g2 = reg.try_acquire(k).expect("2");
        assert!(reg.try_acquire(k).is_none(), "3rd over per-key cap");
        drop(g1);
        let _g3 = reg.try_acquire(k).expect("freed slot reusable");
        assert_eq!(reg.live(), 2);
    }

    #[test]
    fn entry_removed_at_zero() {
        let reg = Arc::new(ConnectionRegistry::with_limits(4, 100));
        let g = reg.try_acquire(key(7)).expect("acquire");
        assert_eq!(reg.per_key.len(), 1);
        drop(g);
        assert_eq!(reg.per_key.len(), 0, "zeroed key is dropped from the map");
        assert_eq!(reg.live(), 0);
    }

    #[test]
    fn global_ceiling_and_rollback() {
        let reg = Arc::new(ConnectionRegistry::with_limits(10, 2));
        let _a = reg.try_acquire(key(1)).expect("a");
        let _b = reg.try_acquire(key(2)).expect("b");
        assert!(reg.try_acquire(key(3)).is_none(), "global ceiling");

        // A per-key rejection must not consume a global slot.
        let reg2 = Arc::new(ConnectionRegistry::with_limits(1, 5));
        let _x = reg2.try_acquire(key(9)).expect("x");
        assert!(reg2.try_acquire(key(9)).is_none(), "per-key reject");
        assert_eq!(
            reg2.live(),
            1,
            "rejected per-key did not leak a global slot"
        );
    }
}
