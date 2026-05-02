//! Sync engine HTTP protocol.
//!
//! Three endpoints power the local-first client runtime:
//!
//! - `GET  /api/sync/bootstrap` — initial NDJSON snapshot of every
//!   aggregate the caller's allowed groups grant access to.
//! - `POST /api/sync/push` — applies an array of optimistic
//!   transactions and returns per-tx outcomes.
//! - `GET  /api/sync/delta` — incremental fetch from a `sync_id`
//!   cursor, used both for warm-start catch-up and SSE-disconnect
//!   recovery.
//!
//! The matching SSE topic (`sync` event type on the existing
//! `/api/events` stream) carries the same shape as `delta` but
//! pushed instead of pulled. See `services::sync_outbox` for the
//! post-commit broadcaster.

pub mod bootstrap;
pub mod delta;
pub mod push;
pub mod schema;

#[cfg(test)]
mod tests;
