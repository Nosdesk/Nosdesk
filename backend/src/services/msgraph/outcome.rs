//! Structured outcomes for a delta-sync run.
//!
//! These replace the previous `SyncProgress { errors: Vec<String> }`
//! / `SyncResult { total_errors: usize, message: String }` shape with
//! a typed equivalent: per-item failures carry an `MsGraphSyncError`
//! (not a string) plus identity + attempt count, and the aggregate
//! `SyncOutcome` is the source of truth that the admin-UI response
//! struct (`handlers::msgraph_integration::SyncResult`) is projected
//! from at the boundary.
//!
//! Two layers, deliberately:
//!
//!   * **`EntityOutcome`** — the result of syncing one entity
//!     (users / devices / groups) within a run. Carries its own
//!     processed + failure list so the executor can stitch them
//!     together without flattening.
//!   * **`SyncOutcome`** — the result of the whole run. Wraps a
//!     `Vec<EntityOutcome>` plus timing and cancellation state.
//!
//! Neither layer talks to anyhow. Job-level Err is reserved for
//! "the sync couldn't even start" (token unavailable, DB pool
//! exhausted, machinery bug); a sync that ran to completion always
//! returns `Ok(SyncOutcome)`, with item failures recorded inside.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};

use super::error::{Classification, MsGraphSyncError};

/// Entity kinds the delta sync knows how to pull. Bounded enum so
/// it's safe to emit on the `entity` tracing field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Users,
    Devices,
    Groups,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Users => "users",
            Self::Devices => "devices",
            Self::Groups => "groups",
        }
    }

    /// Parse the wire identifier used by the admin UI's "select
    /// which entities to sync" request. Returns None on unknown
    /// values so the handler boundary can reject them with a 400.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "users" => Some(Self::Users),
            "devices" => Some(Self::Devices),
            "groups" => Some(Self::Groups),
            _ => None,
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single item that the sync tried and failed to process.
///
/// `external_id` is the MS Graph stable identifier (a GUID-shaped
/// string). It's tenant data but not user-typed; it's allowlisted
/// on the tracing layer alongside other tenant-stable identifiers
/// (user_uuid, ticket_id, etc.).
///
/// `attempt` records how many tries the retry executor made before
/// giving up — so an admin reviewing a run can tell "this failed
/// once on a 429" from "this failed five times and we still
/// couldn't get it through".
#[derive(Debug)]
pub struct ItemFailure {
    pub entity: EntityKind,
    pub external_id: String,
    pub error: MsGraphSyncError,
    pub attempt: u32,
}

impl ItemFailure {
    pub fn classification(&self) -> Classification {
        self.error.classify()
    }

    pub fn kind_str(&self) -> &'static str {
        self.error.kind_str()
    }
}

/// Result of syncing one entity within a run. Always returned by
/// the per-entity sync functions, even when nothing went wrong
/// (failures is empty in that case).
#[derive(Debug)]
pub struct EntityOutcome {
    pub entity: EntityKind,
    /// Items that landed in the DB successfully (new or updated).
    pub processed: usize,
    /// Item-level failures. Always typed; never strings.
    pub failures: Vec<ItemFailure>,
    /// Did the cancellation token fire mid-entity?
    pub cancelled: bool,
}

impl EntityOutcome {
    pub fn empty(entity: EntityKind) -> Self {
        Self {
            entity,
            processed: 0,
            failures: Vec::new(),
            cancelled: false,
        }
    }

    pub fn failed(&self) -> usize {
        self.failures.len()
    }

    pub fn ok(&self) -> bool {
        self.failures.is_empty() && !self.cancelled
    }
}

/// Result of a whole sync run. Built by the executor by walking the
/// requested entity list and accumulating the per-entity outcomes;
/// never returned directly from a handler — the
/// `handlers::msgraph_integration::SyncResult` wire shape is
/// projected from this at the API boundary so the admin UI's
/// response stays stable.
#[derive(Debug)]
pub struct SyncOutcome {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub elapsed: Duration,
    pub cancelled: bool,
    pub entities: Vec<EntityOutcome>,
}

impl SyncOutcome {
    /// Total items written to the local DB across all entities.
    pub fn processed(&self) -> usize {
        self.entities.iter().map(|e| e.processed).sum()
    }

    /// Total per-item failures across all entities.
    pub fn failed(&self) -> usize {
        self.entities.iter().map(EntityOutcome::failed).sum()
    }

    pub fn ok(&self) -> bool {
        !self.cancelled && self.entities.iter().all(EntityOutcome::ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_kind_round_trip() {
        assert_eq!(EntityKind::parse("users"), Some(EntityKind::Users));
        assert_eq!(EntityKind::parse("devices"), Some(EntityKind::Devices));
        assert_eq!(EntityKind::parse("groups"), Some(EntityKind::Groups));
        assert_eq!(EntityKind::parse("bogus"), None);
        assert_eq!(EntityKind::Users.as_str(), "users");
    }

    #[test]
    fn entity_outcome_ok_only_with_no_failures_and_not_cancelled() {
        let mut o = EntityOutcome::empty(EntityKind::Users);
        assert!(o.ok());
        o.cancelled = true;
        assert!(!o.ok());
        o.cancelled = false;
        o.failures.push(ItemFailure {
            entity: EntityKind::Users,
            external_id: "abc".into(),
            error: MsGraphSyncError::Cancelled,
            attempt: 1,
        });
        assert!(!o.ok());
    }
}
