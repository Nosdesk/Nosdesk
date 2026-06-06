//! Per-document ownership claims for multi-instance collaborative
//! editing (Phase 2, "fly-replay spike" step).
//!
//! This implements the ownership layer from
//! `docs/realtime-collab-affinity-design.md`: a short-TTL Redis claim
//! (`yjs:owner:{doc_id} -> machine_id`) deciding which backend machine
//! owns a given collaborative document, a monotonic fencing token
//! (`yjs:fence:{doc_id}`) handed out per claim, an ownership-aware
//! renewal, and a release. The fencing token is stamped on the durable
//! snapshot write so a stale owner (one whose lease expired under a GC
//! pause) cannot clobber the new owner's state.
//!
//! Ownership is only consulted when multi-instance routing is enabled
//! (`NOSDESK_COLLAB_ROUTING=fly-replay`). In the default single-instance
//! mode this module is never constructed and the routing layer is inert,
//! so existing deploys are byte-for-byte unchanged.

use redis::{AsyncCommands, RedisError};
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

/// TTL for an ownership claim. Must be comfortably longer than the
/// renewal cadence (the 30s maintenance loop) so a claim does not expire
/// while its owner is still serving the room. The full design uses a
/// shorter TTL with a dedicated sub-TTL renewal task; the spike rides
/// the existing 30s loop, so the TTL is generous.
const OWNER_TTL_SECS: u64 = 90;

/// Redis key prefix for ownership claims.
const OWNER_KEY_PREFIX: &str = "yjs:owner";

/// Redis key prefix for the per-document monotonic fencing counter.
/// Kept separate from the owner key and never expired, so the token
/// keeps increasing across ownership handoffs.
const FENCE_KEY_PREFIX: &str = "yjs:fence";

/// Redis key prefix for the machine-address registry. Maps a machine id
/// to its client-reachable WebSocket base, so a machine contacted in
/// direct-address mode can hand back the owner's address. TTL-refreshed
/// while the machine is alive; expires after death.
const MACHINE_KEY_PREFIX: &str = "yjs:machine";

/// TTL for a machine-address registry entry. Refreshed every
/// maintenance tick (30s), so this is generously longer than that.
const MACHINE_TTL_SECS: u64 = 90;

/// How a WebSocket connection is steered to the machine that owns a
/// document under multi-instance routing. Selected at startup from
/// `NOSDESK_COLLAB_ROUTING`. See
/// `docs/realtime-collab-affinity-design.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CollabRoutingMode {
    /// One backend: every doc is owned locally, the affinity layer is
    /// inert, behaviour is identical to before. The default.
    Single,
    /// fly.io: a connection landing on a non-owner is steered to the
    /// owner with a `fly-replay: instance=<id>` response before the
    /// upgrade is negotiated.
    FlyReplay,
    /// Portable / self-host: the client first calls the handshake
    /// endpoint, which returns the owner's address, and connects there
    /// directly. A WS that still lands on a non-owner is told to
    /// re-handshake.
    DirectAddress,
}

impl CollabRoutingMode {
    pub fn from_env_value(value: &str) -> Self {
        match value {
            "fly-replay" => CollabRoutingMode::FlyReplay,
            "direct-address" => CollabRoutingMode::DirectAddress,
            _ => CollabRoutingMode::Single,
        }
    }
}

/// Build the ownership manager for the configured routing mode, reading
/// machine identity (and, for direct-address, this instance's
/// client-reachable WS base) from the environment. Returns `None` for
/// single-instance mode or on any setup error, so the caller degrades to
/// single-instance routing. `Single` mode never touches Redis.
pub fn build(redis_url: &str, mode: CollabRoutingMode) -> Option<Arc<CollabOwnership>> {
    if mode == CollabRoutingMode::Single {
        return None;
    }
    let machine_id: Arc<str> = resolve_machine_id().into();
    // Direct-address publishes this instance's address so peers can route
    // clients here; fly-replay routes by machine id and needs none.
    let address: Option<Arc<str>> = if mode == CollabRoutingMode::DirectAddress {
        match std::env::var("NOSDESK_INSTANCE_ADDRESS") {
            Ok(a) if !a.is_empty() => Some(a.into()),
            _ => {
                tracing::error!("NOSDESK_COLLAB_ROUTING=direct-address requires NOSDESK_INSTANCE_ADDRESS (this instance's client-reachable wss base); falling back to single-instance");
                return None;
            }
        }
    } else {
        None
    };
    match CollabOwnership::new(redis_url, machine_id.clone(), address) {
        Ok(ownership) => {
            tracing::info!(machine_id = %machine_id, mode = ?mode, "Collab multi-instance routing enabled");
            Some(Arc::new(ownership))
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to init collab ownership; falling back to single-instance routing");
            None
        }
    }
}

/// Resolve this process's machine identity once at startup.
///
/// Prefers `FLY_MACHINE_ID` (set per Machine on fly), then an
/// operator-provided `NOSDESK_MACHINE_ID`, falling back to a random
/// UUID. The fallback is fine for single-instance and for any
/// environment where the operator does not pin an id; each restart is a
/// fresh identity, which only matters under multi-instance routing.
pub fn resolve_machine_id() -> String {
    std::env::var("FLY_MACHINE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("NOSDESK_MACHINE_ID")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| Uuid::now_v7().to_string())
}

/// Outcome of resolving (and, if free, claiming) ownership of a doc.
#[derive(Debug, Clone)]
pub struct OwnerResolution {
    /// The machine id that currently owns the document.
    pub owner: String,
    /// True when `owner` is this process.
    pub is_local: bool,
    /// The fencing token for our claim, present only when `is_local`.
    /// Stamped on durable snapshot writes so a stale owner is rejected.
    pub fence: Option<i64>,
}

/// Redis-backed ownership claim manager.
pub struct CollabOwnership {
    client: redis::Client,
    machine_id: Arc<str>,
    /// This machine's client-reachable WebSocket base (e.g.
    /// `wss://host:port`), published to the machine-address registry for
    /// the direct-address routing mode. `None` in fly-replay mode, where
    /// routing uses the machine id in a `fly-replay` header instead.
    address: Option<Arc<str>>,
}

impl CollabOwnership {
    pub fn new(
        redis_url: &str,
        machine_id: Arc<str>,
        address: Option<Arc<str>>,
    ) -> Result<Self, RedisError> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self {
            client,
            machine_id,
            address,
        })
    }

    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }

    /// This machine's client-reachable WebSocket base, if configured
    /// (direct-address mode).
    pub fn address(&self) -> Option<&str> {
        self.address.as_deref()
    }

    fn owner_key(doc_id: &str) -> String {
        format!("{OWNER_KEY_PREFIX}:{doc_id}")
    }

    fn fence_key(doc_id: &str) -> String {
        format!("{FENCE_KEY_PREFIX}:{doc_id}")
    }

    fn machine_key(machine_id: &str) -> String {
        format!("{MACHINE_KEY_PREFIX}:{machine_id}")
    }

    /// Publish this machine's address to the registry (direct-address
    /// mode). No-op when no address is configured (fly-replay mode).
    /// Called at startup and refreshed each maintenance tick.
    pub async fn register_self(&self) {
        let Some(address) = &self.address else {
            return;
        };
        let key = Self::machine_key(&self.machine_id);
        if let Ok(mut conn) = self.client.get_multiplexed_async_connection().await {
            let _: Result<(), _> = conn.set_ex(&key, address.as_ref(), MACHINE_TTL_SECS).await;
        }
    }

    /// Look up the client-reachable address of `machine_id` from the
    /// registry. Returns `None` if unknown (machine dead or never
    /// registered an address).
    pub async fn owner_address(&self, machine_id: &str) -> Option<String> {
        let key = Self::machine_key(machine_id);
        let mut conn = self.client.get_multiplexed_async_connection().await.ok()?;
        conn.get::<_, Option<String>>(&key).await.ok().flatten()
    }

    /// Resolve the owner of `doc_id`, claiming it for this machine if it
    /// is currently unowned.
    ///
    /// Atomic via a Lua script over the owner key and the fence counter:
    /// - unowned: `SET NX EX` claims it, `INCR` mints a fresh fence, and
    ///   we return `(self, fence)`.
    /// - already ours: return `(self, current fence)`.
    /// - owned elsewhere: return `(other, -1)` (no fence; we are not the
    ///   writer).
    ///
    /// So the return value is always "who owns it now", `is_local` says
    /// whether that is us, and `fence` carries our claim's token when it
    /// is.
    ///
    /// On any Redis error this falls back to a *local* resolution with
    /// no fence. That keeps the node serving rather than failing the
    /// connection; multi-instance correctness is forfeited only while
    /// Redis is unreachable, which the scaling plan accepts. A `None`
    /// fence means the snapshot write is unconditional (today's
    /// behaviour), which is correct for the single-owner degraded case.
    pub async fn resolve_or_claim(&self, doc_id: &str) -> OwnerResolution {
        let script = redis::Script::new(
            r#"
            local ok = redis.call('SET', KEYS[1], ARGV[1], 'NX', 'EX', ARGV[2])
            if ok then
                return {ARGV[1], redis.call('INCR', KEYS[2])}
            end
            local owner = redis.call('GET', KEYS[1])
            if owner == ARGV[1] then
                local f = redis.call('GET', KEYS[2])
                if f then
                    return {owner, tonumber(f)}
                else
                    return {owner, redis.call('INCR', KEYS[2])}
                end
            end
            return {owner, -1}
            "#,
        );

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                let res: Result<(String, i64), _> = script
                    .key(Self::owner_key(doc_id))
                    .key(Self::fence_key(doc_id))
                    .arg(self.machine_id.as_ref())
                    .arg(OWNER_TTL_SECS)
                    .invoke_async(&mut conn)
                    .await;
                match res {
                    Ok((owner, fence)) => {
                        let is_local = owner == self.machine_id.as_ref();
                        debug!(doc_id = %doc_id, owner = %owner, is_local, fence, "Resolved doc owner");
                        OwnerResolution {
                            owner,
                            is_local,
                            fence: is_local.then_some(fence),
                        }
                    }
                    Err(e) => {
                        warn!(doc_id = %doc_id, error = ?e, "Ownership claim script failed; serving locally");
                        self.local_fallback()
                    }
                }
            }
            Err(e) => {
                warn!(doc_id = %doc_id, error = ?e, "Redis connection failed for ownership; serving locally");
                self.local_fallback()
            }
        }
    }

    /// Extend this machine's claim on `doc_id`. Ownership-aware: it only
    /// extends a claim this machine actually holds, so it can never
    /// prolong another machine's lease. Returns `true` if the claim is
    /// still held (extended), `false` if it has been lost to another
    /// machine (the caller must then stop serving the doc). On a Redis
    /// error it returns `true` (assume still held) so a transient blip
    /// does not trigger spurious evictions.
    pub async fn renew(&self, doc_id: &str) -> bool {
        let script = redis::Script::new(
            r#"
            if redis.call('GET', KEYS[1]) == ARGV[1] then
                return redis.call('EXPIRE', KEYS[1], ARGV[2])
            else
                return 0
            end
            "#,
        );

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                let res: Result<i64, _> = script
                    .key(Self::owner_key(doc_id))
                    .arg(self.machine_id.as_ref())
                    .arg(OWNER_TTL_SECS)
                    .invoke_async(&mut conn)
                    .await;
                match res {
                    Ok(1) => true,
                    Ok(_) => {
                        warn!(doc_id = %doc_id, "Ownership claim lost on renew");
                        false
                    }
                    Err(e) => {
                        warn!(doc_id = %doc_id, error = ?e, "Renew failed; assuming still owned");
                        true
                    }
                }
            }
            Err(_) => true,
        }
    }

    /// Release this machine's claim on `doc_id` (compare-and-delete, so
    /// it only ever drops a claim this machine holds). Called on idle
    /// eviction so the next connection re-elects an owner cleanly. The
    /// fence counter is intentionally left in place so tokens stay
    /// monotonic across the next claim.
    pub async fn release(&self, doc_id: &str) {
        let script = redis::Script::new(
            r#"
            if redis.call('GET', KEYS[1]) == ARGV[1] then
                return redis.call('DEL', KEYS[1])
            else
                return 0
            end
            "#,
        );

        if let Ok(mut conn) = self.client.get_multiplexed_async_connection().await {
            let _: Result<i64, _> = script
                .key(Self::owner_key(doc_id))
                .arg(self.machine_id.as_ref())
                .invoke_async(&mut conn)
                .await;
        }
    }

    fn local_fallback(&self) -> OwnerResolution {
        OwnerResolution {
            owner: self.machine_id.to_string(),
            is_local: true,
            fence: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_key_format() {
        assert_eq!(
            CollabOwnership::owner_key("ws-abc_ticket-1"),
            "yjs:owner:ws-abc_ticket-1"
        );
    }

    #[test]
    fn fence_and_machine_key_format() {
        assert_eq!(
            CollabOwnership::fence_key("ws-abc_ticket-1"),
            "yjs:fence:ws-abc_ticket-1"
        );
        assert_eq!(CollabOwnership::machine_key("m-123"), "yjs:machine:m-123");
    }

    #[test]
    fn routing_mode_parsing() {
        use super::CollabRoutingMode;
        assert_eq!(
            CollabRoutingMode::from_env_value("fly-replay"),
            CollabRoutingMode::FlyReplay
        );
        assert_eq!(
            CollabRoutingMode::from_env_value("direct-address"),
            CollabRoutingMode::DirectAddress
        );
        assert_eq!(
            CollabRoutingMode::from_env_value("single"),
            CollabRoutingMode::Single
        );
        // Unknown values default to the safe single-instance mode.
        assert_eq!(
            CollabRoutingMode::from_env_value("garbage"),
            CollabRoutingMode::Single
        );
    }

    #[test]
    fn machine_id_falls_back_to_uuid() {
        // No FLY_MACHINE_ID / NOSDESK_MACHINE_ID in the test env: should
        // produce a parseable UUID rather than an empty string.
        let id = resolve_machine_id();
        assert!(!id.is_empty());
    }
}
