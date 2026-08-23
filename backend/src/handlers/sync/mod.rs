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

/// Workspace capability flags surfaced to the client (feature chrome gates,
/// e.g. hide all SLA UI until a policy exists). Sent in the bootstrap
/// `__meta__` header and on every delta response, so a client that skips the
/// snapshot on a warm launch (delta catch-up) still converges on the current
/// flags, and a flag flipped by an admin reaches running clients within one
/// poll interval rather than at their next full bootstrap.
#[derive(Debug, serde::Serialize)]
pub struct CapabilityFlags {
    pub sla_enabled: bool,
}

pub fn capability_flags(conn: &mut crate::db::DbConnection) -> CapabilityFlags {
    use diesel::dsl::count_star;
    use diesel::prelude::*;
    // A count (rather than "any non-archived") is fine for v1: a workspace
    // either has SLA policies or it doesn't.
    let n: i64 = crate::schema::sla_policies::table
        .select(count_star())
        .first(conn)
        .unwrap_or(0);
    CapabilityFlags { sla_enabled: n > 0 }
}

/// Sync-engine routes, mounted inside the authenticated `/api` scope in main.rs.
pub fn config(cfg: &mut actix_web::web::ServiceConfig) {
    use actix_web::web;
    cfg.route("/sync/bootstrap", web::get().to(bootstrap::bootstrap))
        .route("/sync/delta", web::get().to(delta::delta))
        .route("/sync/push", web::post().to(push::push))
        .route("/sync/schema", web::get().to(schema::schema));
}

#[cfg(test)]
mod tests;
