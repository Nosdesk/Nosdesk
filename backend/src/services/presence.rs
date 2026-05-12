//! Ticket presence registry.
//!
//! Single source of truth for "which user is on which ticket right
//! now." Two projections are derived from one piece of state:
//!
//!   * **Per-ticket viewers** (v1 consumer) — the avatar stack on
//!     the ticket detail page. Answers "who else is on ticket N?"
//!   * **Per-user activity** (designed for, not yet rendered) — a
//!     future team-activity surface that shows colleagues' current
//!     tickets. Answers "what is user X on right now?"
//!
//! Keying by `(user_uuid, ticket_id)` rather than `(doc_id,
//! session_id)` makes multi-tab dedup automatic: opening the same
//! ticket in three tabs records one entry with three session ids.
//! The user is present on the ticket as long as any of those
//! sessions remain.
//!
//! The registry is intentionally transport-agnostic. The
//! collaboration websocket calls `add_session` / `remove_session`
//! / `touch_session` based on its own heartbeat lifecycle. The
//! registry doesn't drive cleanup itself; it exposes
//! `cleanup_stale_for_ticket` for the transport to invoke on its
//! own cadence.
//!
//! ## Hooks baked in from day one
//!
//! Two extensibility points so the v1.x team-activity surface
//! doesn't need a refactor:
//!
//! * `PresenceVisibilityResolver` decides whether a user's
//!   presence should be broadcast at all. The default
//!   `AlwaysVisibleResolver` returns `Visible` for every user; a
//!   future "appear away from tickets" preference can drop in
//!   here without touching call sites.
//! * The SSE topic for `ViewersChanged` is `TopicKey::Ticket(id)`
//!   (see `handlers::sse`). Subscription to that topic is
//!   authorised via `ticket_visibility::can_view_ticket` at
//!   connect time, so the per-event filter doesn't have to run
//!   on every emission. The same pattern extends to a future
//!   per-user team-presence topic.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What the wire receives for each user currently on a ticket.
/// Deliberately minimal so the future team-presence channel can
/// reuse the same payload type.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewerInfo {
    pub user_uuid: Uuid,
    pub last_active_at: DateTime<Utc>,
}

/// Result of a registry mutation. `changed` is true when the set
/// of `viewers_on_ticket(...)` changed — i.e. a user actually
/// joined or left from a viewer's perspective. Heartbeats and
/// extra-tab additions return `changed = false` so the broadcaster
/// can skip emitting an event for them.
#[derive(Clone, Debug)]
pub struct PresenceDelta {
    pub ticket_id: i32,
    pub changed: bool,
}

/// Per-user presence preference. The team-presence channel (and
/// in v1.x, even the per-ticket broadcast) consults this so a
/// future "appear invisible to teammates" toggle can land without
/// editing every call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceVisibility {
    /// Broadcast normally.
    Visible,
    /// Suppress entirely — neither the per-ticket avatar stack
    /// nor the future team-presence channel will surface this user.
    Hidden,
}

pub trait PresenceVisibilityResolver: Send + Sync {
    fn visibility_for(&self, user_uuid: Uuid) -> PresenceVisibility;
}

/// Default resolver: everyone is visible. v1 ships this.
#[derive(Default)]
pub struct AlwaysVisibleResolver;

impl PresenceVisibilityResolver for AlwaysVisibleResolver {
    fn visibility_for(&self, _: Uuid) -> PresenceVisibility {
        PresenceVisibility::Visible
    }
}

struct UserPresenceOnTicket {
    sessions: HashSet<String>,
    last_active: Instant,
    /// Wall-clock timestamp surfaced to clients. Separate from
    /// `last_active` (monotonic) because the wire format needs a
    /// real datetime and `Instant` can't be serialised.
    last_active_at: DateTime<Utc>,
}

impl Default for UserPresenceOnTicket {
    fn default() -> Self {
        Self {
            sessions: HashSet::new(),
            last_active: Instant::now(),
            last_active_at: Utc::now(),
        }
    }
}

pub struct PresenceRegistry {
    state: RwLock<HashMap<(Uuid, i32), UserPresenceOnTicket>>,
    resolver: Arc<dyn PresenceVisibilityResolver>,
}

impl PresenceRegistry {
    pub fn new(resolver: Arc<dyn PresenceVisibilityResolver>) -> Self {
        Self {
            state: RwLock::new(HashMap::new()),
            resolver,
        }
    }

    /// Default registry with the always-visible resolver. Used by
    /// the runtime; tests can construct one with a stub resolver.
    pub fn with_default_resolver() -> Self {
        Self::new(Arc::new(AlwaysVisibleResolver))
    }

    /// Record a new session for `user` on `ticket`. Returns
    /// `changed = true` only when this is the user's first
    /// session on this ticket (a real "user joined").
    pub fn add_session(&self, user: Uuid, ticket: i32, session: String) -> PresenceDelta {
        let mut state = self.state.write().expect("presence state poisoned");
        let entry = state.entry((user, ticket)).or_default();
        let first_session = entry.sessions.is_empty();
        entry.sessions.insert(session);
        let now = Instant::now();
        entry.last_active = now;
        entry.last_active_at = Utc::now();
        PresenceDelta {
            ticket_id: ticket,
            changed: first_session,
        }
    }

    /// Heartbeat: bump the user's last-active timestamp. Never
    /// `changed`; this exists so the visible set's ordering can
    /// reflect recency without firing a wire event.
    pub fn touch_session(&self, user: Uuid, ticket: i32) {
        let mut state = self.state.write().expect("presence state poisoned");
        if let Some(entry) = state.get_mut(&(user, ticket)) {
            entry.last_active = Instant::now();
            entry.last_active_at = Utc::now();
        }
    }

    /// Remove a session. Returns `changed = true` only when this
    /// drops the user's last session on the ticket (a real "user
    /// left"). Multi-tab close → only the final tab triggers a
    /// broadcast.
    pub fn remove_session(&self, user: Uuid, ticket: i32, session: &str) -> PresenceDelta {
        let mut state = self.state.write().expect("presence state poisoned");
        let Some(entry) = state.get_mut(&(user, ticket)) else {
            return PresenceDelta {
                ticket_id: ticket,
                changed: false,
            };
        };
        entry.sessions.remove(session);
        let now_empty = entry.sessions.is_empty();
        if now_empty {
            state.remove(&(user, ticket));
        }
        PresenceDelta {
            ticket_id: ticket,
            changed: now_empty,
        }
    }

    /// Drop entries whose last activity is older than `threshold`.
    /// Returns one delta per ticket whose viewer set changed so the
    /// caller can emit `ViewersChanged` for those tickets only.
    pub fn cleanup_stale(&self, threshold: Duration) -> Vec<PresenceDelta> {
        let now = Instant::now();
        let mut state = self.state.write().expect("presence state poisoned");
        let stale_keys: Vec<(Uuid, i32)> = state
            .iter()
            .filter(|(_, v)| now.duration_since(v.last_active) > threshold)
            .map(|(k, _)| *k)
            .collect();
        let mut affected_tickets = HashSet::new();
        for key in stale_keys {
            state.remove(&key);
            affected_tickets.insert(key.1);
        }
        affected_tickets
            .into_iter()
            .map(|ticket_id| PresenceDelta {
                ticket_id,
                changed: true,
            })
            .collect()
    }

    /// Per-ticket projection. Applies the visibility resolver so
    /// users marked Hidden are dropped from the output.
    pub fn viewers_on_ticket(&self, ticket_id: i32) -> Vec<ViewerInfo> {
        let state = self.state.read().expect("presence state poisoned");
        let mut out: Vec<ViewerInfo> = state
            .iter()
            .filter(|((_, t), _)| *t == ticket_id)
            .filter(|((u, _), _)| {
                matches!(
                    self.resolver.visibility_for(*u),
                    PresenceVisibility::Visible
                )
            })
            .map(|((u, _), v)| ViewerInfo {
                user_uuid: *u,
                last_active_at: v.last_active_at,
            })
            .collect();
        out.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
        out
    }

    /// Per-user projection. Returns the tickets the given user is
    /// currently present on. Not consumed in v1; lives here so the
    /// future team-presence channel doesn't reach into private
    /// state.
    #[allow(dead_code)]
    pub fn tickets_for_user(&self, user_uuid: Uuid) -> Vec<i32> {
        let state = self.state.read().expect("presence state poisoned");
        state
            .iter()
            .filter(|((u, _), _)| *u == user_uuid)
            .map(|((_, t), _)| *t)
            .collect()
    }
}

impl Default for PresenceRegistry {
    fn default() -> Self {
        Self::with_default_resolver()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(byte: u8) -> Uuid {
        Uuid::from_bytes([byte; 16])
    }

    #[test]
    fn first_session_is_a_change() {
        let r = PresenceRegistry::default();
        let d = r.add_session(uuid(1), 42, "s1".into());
        assert!(d.changed);
        assert_eq!(d.ticket_id, 42);
    }

    #[test]
    fn second_tab_from_same_user_is_not_a_change() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 42, "s1".into());
        let d = r.add_session(uuid(1), 42, "s2".into());
        assert!(
            !d.changed,
            "second session for same user must not emit a delta"
        );
        assert_eq!(r.viewers_on_ticket(42).len(), 1);
    }

    #[test]
    fn removing_one_of_two_tabs_is_not_a_change() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 42, "s1".into());
        r.add_session(uuid(1), 42, "s2".into());
        let d = r.remove_session(uuid(1), 42, "s1");
        assert!(
            !d.changed,
            "removing one of two sessions must not emit a delta"
        );
        assert_eq!(r.viewers_on_ticket(42).len(), 1);
    }

    #[test]
    fn removing_last_session_is_a_change() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 42, "s1".into());
        let d = r.remove_session(uuid(1), 42, "s1");
        assert!(d.changed);
        assert!(r.viewers_on_ticket(42).is_empty());
    }

    #[test]
    fn multiple_users_on_one_ticket() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 42, "alice-1".into());
        r.add_session(uuid(2), 42, "bob-1".into());
        r.add_session(uuid(2), 42, "bob-2".into());
        let viewers = r.viewers_on_ticket(42);
        assert_eq!(viewers.len(), 2, "two users despite three sessions");
        let ids: HashSet<_> = viewers.iter().map(|v| v.user_uuid).collect();
        assert!(ids.contains(&uuid(1)));
        assert!(ids.contains(&uuid(2)));
    }

    #[test]
    fn tickets_for_user_returns_user_keyed_projection() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 10, "s1".into());
        r.add_session(uuid(1), 20, "s2".into());
        r.add_session(uuid(2), 10, "s3".into());
        let mut tickets = r.tickets_for_user(uuid(1));
        tickets.sort();
        assert_eq!(tickets, vec![10, 20]);
    }

    #[test]
    fn hidden_user_is_dropped_from_per_ticket_projection() {
        struct HideUser1;
        impl PresenceVisibilityResolver for HideUser1 {
            fn visibility_for(&self, u: Uuid) -> PresenceVisibility {
                if u == uuid(1) {
                    PresenceVisibility::Hidden
                } else {
                    PresenceVisibility::Visible
                }
            }
        }
        let r = PresenceRegistry::new(Arc::new(HideUser1));
        r.add_session(uuid(1), 42, "s1".into());
        r.add_session(uuid(2), 42, "s2".into());
        let viewers = r.viewers_on_ticket(42);
        assert_eq!(viewers.len(), 1, "user(1) is Hidden, only user(2) visible");
        assert_eq!(viewers[0].user_uuid, uuid(2));
    }

    #[test]
    fn cleanup_stale_drops_expired_entries() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 42, "s1".into());
        std::thread::sleep(Duration::from_millis(50));
        r.add_session(uuid(2), 42, "s2".into());
        let deltas = r.cleanup_stale(Duration::from_millis(25));
        assert_eq!(deltas.len(), 1, "one ticket affected by cleanup");
        assert_eq!(deltas[0].ticket_id, 42);
        let viewers = r.viewers_on_ticket(42);
        assert_eq!(viewers.len(), 1, "fresh user remains");
        assert_eq!(viewers[0].user_uuid, uuid(2));
    }

    #[test]
    fn viewers_sorted_by_recency() {
        let r = PresenceRegistry::default();
        r.add_session(uuid(1), 42, "s1".into());
        std::thread::sleep(Duration::from_millis(15));
        r.add_session(uuid(2), 42, "s2".into());
        let viewers = r.viewers_on_ticket(42);
        assert_eq!(viewers[0].user_uuid, uuid(2), "most-recent first");
        assert_eq!(viewers[1].user_uuid, uuid(1));
    }
}
