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
    /// Some request type needs approval (show approval chrome).
    pub approvals_enabled: bool,
    /// Requests waiting for approval are held out of the working queues.
    pub approvals_held: bool,
    /// The viewer sees the workspace's assets: staff always, a requester
    /// unless the workspace shows requesters none. A client that sees this
    /// change re-reads its snapshot, which drops or brings back the assets.
    pub assets_visible: bool,
}

pub fn capability_flags(
    conn: &mut crate::db::DbConnection,
    viewer: &crate::sync::visibility::SyncViewer,
) -> CapabilityFlags {
    use diesel::dsl::count_star;
    use diesel::prelude::*;
    // A count (rather than "any non-archived") is fine for v1: a workspace
    // either has SLA policies or it doesn't.
    let n: i64 = crate::schema::sla_policies::table
        .select(count_star())
        .first(conn)
        .unwrap_or(0);
    let approvals_enabled: bool = diesel::select(diesel::dsl::exists(
        crate::schema::ticket_categories::table
            .filter(crate::schema::ticket_categories::approval_required.eq(true)),
    ))
    .get_result(conn)
    .unwrap_or(false);
    let approvals_held = approvals_enabled
        && crate::schema::site_settings::table
            .select(crate::schema::site_settings::approval_waiting_display)
            .first::<String>(conn)
            .map(|d| d == "held")
            .unwrap_or(false);
    // A failed read shows a requester no assets.
    let assets_visible = viewer.sees_all()
        || crate::repository::assets::requester_asset_visibility(conn)
            .is_ok_and(|v| v == crate::models::RequesterAssetVisibility::All);
    CapabilityFlags {
        sla_enabled: n > 0,
        approvals_enabled,
        approvals_held,
        assets_visible,
    }
}

/// `SYNC_SNAPSHOT_MAX_AGE_SECS` when unset: a day.
const DEFAULT_SNAPSHOT_MAX_AGE_SECS: u64 = 86_400;

/// How old a client's snapshot of a group may grow, in seconds, before the
/// client takes a fresh snapshot instead of catching up by delta. A fresh
/// snapshot drops whatever the client holds that the group no longer has, so
/// this bounds how long a missed removal can linger. `0` turns it off.
///
/// Set by `SYNC_SNAPSHOT_MAX_AGE_SECS` (default a day) and sent in the
/// bootstrap `__meta__` and on every delta, so the server owns the number.
/// The client times the age on its own clock from when it took the snapshot,
/// so clock skew between the two doesn't matter.
pub fn snapshot_max_age_secs() -> u64 {
    static SECS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *SECS.get_or_init(|| {
        parse_snapshot_max_age(std::env::var("SYNC_SNAPSHOT_MAX_AGE_SECS").ok().as_deref())
    })
}

/// `SYNC_SNAPSHOT_MAX_AGE_SECS` as seconds. Unset or blank is the default; a
/// value that isn't a whole number of seconds is ignored with a warning,
/// rather than read as `0`, which would turn the fresh snapshots off.
fn parse_snapshot_max_age(raw: Option<&str>) -> u64 {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return DEFAULT_SNAPSHOT_MAX_AGE_SECS;
    };
    raw.parse::<u64>().unwrap_or_else(|_| {
        tracing::warn!(
            "sync: SYNC_SNAPSHOT_MAX_AGE_SECS isn't a whole number of seconds; using {DEFAULT_SNAPSHOT_MAX_AGE_SECS}"
        );
        DEFAULT_SNAPSHOT_MAX_AGE_SECS
    })
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
