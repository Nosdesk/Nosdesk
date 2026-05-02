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
//! once per business event. The [`audit_only_allowlist`] module
//! enumerates the tables that intentionally skip this — anything not
//! in the allowlist is checked by an integration test in
//! `tests/sync_emit_lint.rs` so a missed write is caught at CI.

pub mod actor;
pub mod emit;
pub mod groups;
pub mod registry;
pub mod session;
pub mod system_meta;

#[cfg(test)]
mod tests;

pub use actor::{ActorContext, ActorKind};
