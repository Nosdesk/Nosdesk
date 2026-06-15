//! Sync engine substrate.
//!
//! Wraps the `sync_actions` event log, the read-side `groups`
//! computation, and the model registry that bridges Rust and TS. The
//! protocol handlers (`/api/sync/{bootstrap,push,delta}`) live in
//! `handlers::sync` and call into this module to record events and
//! resolve group authorisation.
//!
//! Repository writes that mutate a tier-1 aggregate must call
//! [`emit::record`] in the same Diesel transaction as the SQL write,
//! once per business event. The `ALLOWLIST` constant in
//! `tests/sync_emit_lint.rs` enumerates the tables that intentionally
//! skip this — the lint test fails CI if a new repository write is
//! neither emit-wired nor in the allowlist.

pub mod actor;
pub mod emit;
pub mod feed;
pub mod groups;
pub mod partitions;
pub mod registry;
pub mod session;
pub mod system_meta;
pub mod visibility;

#[cfg(test)]
mod tests;

pub use actor::{ActorContext, ActorKind};
