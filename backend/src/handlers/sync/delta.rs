//! `GET /api/sync/delta?from=<id>&groups=<csv>&limit=<n>`
//!
//! Returns every `sync_actions` row newer than `from` whose `groups`
//! array overlaps with both the request's `groups` parameter AND the
//! caller's permitted set (server-authoritative intersection).
//!
//! Used by the client runtime in two cases:
//! 1. Warm-start catch-up: client opens a tab, rehydrates IndexedDB,
//!    then asks "what's happened since `meta.last_sync_id`?".
//! 2. SSE reconnect: client sends `Last-Event-ID`, server backfills
//!    via this endpoint instead of having to retain pushed frames.

use actix_web::{web, HttpResponse, Responder};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::extractors::{SyncContext, TenantConn};
use crate::handlers::errors;
use crate::schema::sync_actions;

#[derive(Debug, Deserialize)]
pub struct DeltaQuery {
    /// Cursor `sync_id`. With `from_xid8` it forms the commit-safe
    /// `(xid8, sync_id)` cursor; on its own (legacy clients that predate
    /// the commit-safe cursor) it falls back to `sync_id > from`.
    pub from: i64,
    /// Cursor `xid8` — see `crate::sync::feed`. When present, rows are
    /// returned in `(xid8, sync_id)` order strictly after the cursor and
    /// only once settled (below the commit horizon), so a late-committing
    /// lower-`sync_id` row is delivered next rather than skipped.
    pub from_xid8: Option<i64>,
    /// Comma-separated group strings the client wants events for.
    /// The server intersects this with the caller's permitted set.
    pub groups: String,
    /// Page size. Defaults to 1000, hard-capped at 5000 (per § 4
    /// of the architecture doc).
    pub limit: Option<i64>,
}

const DEFAULT_LIMIT: i64 = 1000;
const MAX_LIMIT: i64 = 5000;

/// One row of `sync_actions` shaped for wire delivery. Field set
/// matches the typed-event view consumers want, not the raw column
/// layout — schema_version is dropped (always == registry version),
/// occurred_at is the timestamp clients sort on.
#[derive(Debug, Serialize, Queryable)]
pub struct ActionRow {
    pub sync_id: i64,
    pub aggregate: crate::models::SyncAggregate,
    pub aggregate_id: String,
    pub op: crate::models::SyncOp,
    pub event_type: String,
    pub schema_version: i16,
    pub data: serde_json::Value,
    pub groups: Vec<Option<String>>,
    pub actor_uuid: Option<uuid::Uuid>,
    pub actor_kind: String,
    pub actor_ref: Option<String>,
    pub correlation_id: Option<uuid::Uuid>,
    pub causation_id: Option<uuid::Uuid>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// Transaction id for the commit-safe cursor; not sent per-row (the
    /// response-level `last_xid8` carries it). See `crate::sync::feed`.
    #[serde(skip)]
    pub xid8: i64,
}

#[derive(Debug, Serialize)]
pub struct DeltaResponse {
    pub actions: Vec<ActionRow>,
    /// Commit-safe cursor for the next request: `(last_xid8,
    /// last_sync_id)`. Advances over visibility-dropped rows too.
    pub last_xid8: i64,
    pub last_sync_id: i64,
    pub has_more: bool,
    /// The caller's cursor predates the oldest action we still retain, so a
    /// delta cannot reconstruct current state and the client must wipe its
    /// cache and re-bootstrap.
    ///
    /// `sync_actions` is pruned by dropping whole monthly partitions
    /// (`SYNC_ACTIONS_RETENTION_DAYS`, default 90). A client offline past that
    /// horizon has a cursor pointing into dropped partitions: the deletes it
    /// missed are gone, and a bootstrap alone cannot remove them because the
    /// bootstrap stream only upserts. Without this flag the client believes it
    /// caught up and keeps phantom rows indefinitely.
    ///
    /// Conservative by construction: `sync_id` is a sequence, so a gap below
    /// the oldest retained row may be rolled-back ids rather than pruned rows.
    /// A false positive costs one re-bootstrap; a false negative leaves the
    /// cache silently wrong, so it errs toward resyncing.
    pub resync_required: bool,
    /// Current workspace capability flags. Carried on every delta (not just
    /// the bootstrap `__meta__`) so a warm launch that catches up via delta
    /// without re-streaming the snapshot still converges on current flags.
    /// `None` when the probe failed; the client keeps its current flags
    /// rather than flickering chrome off over a transient error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<super::CapabilityFlags>,
}

/// Smallest `sync_id` still present in `sync_actions`, or `None` when the
/// table is empty.
///
/// Cheap: `PRIMARY KEY (sync_id, occurred_at)` on every partition, so this is a
/// MergeAppend over per-partition index scans rather than a heap read.
fn oldest_retained_sync_id(conn: &mut crate::db::DbConnection) -> QueryResult<Option<i64>> {
    use diesel::dsl::min;
    sync_actions::table
        .select(min(sync_actions::sync_id))
        .first(conn)
}

/// Whether `from` sits below the retained horizon, meaning actions between the
/// cursor and the oldest surviving row have been pruned.
///
/// `from == 0` is exempt: that is a client with no cursor at all, which
/// bootstraps rather than deltas, and every pruned table would otherwise
/// report a resync it does not need.
fn cursor_predates_retention(from: i64, oldest: Option<i64>) -> bool {
    match oldest {
        Some(min_id) => from > 0 && from + 1 < min_id,
        // Nothing retained (fresh install, or everything pruned): there is no
        // evidence of a gap to report, and a bootstrap will seed the cursor.
        None => false,
    }
}

pub async fn delta(
    mut tc: TenantConn,
    query: web::Query<DeltaQuery>,
    ctx: SyncContext,
) -> impl Responder {
    let mut granted = intersect_groups(&query.groups, &ctx.allowed_groups);

    // Per-viewer visibility identity (ticket tier + doc tier), resolved
    // once and reused for ticket-group admission and the read-side
    // visibility filter below.
    let viewer = {
        let user = ctx.user.clone();
        match tc.run(move |conn| {
            Ok::<_, diesel::result::Error>(crate::sync::visibility::SyncViewer::resolve(
                conn, &user,
            ))
        }) {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "delta: failed to resolve viewer");
                return errors::internal("Failed to load sync delta");
            }
        }
    };

    // Admit `ticket:<id>` groups the caller can read on top of the
    // static set, so a pool-native ticket subscription's live pull
    // matches its bootstrap. Authorized per-ticket via
    // `can_view_ticket` (same gate as bootstrap + the SSE topic).
    {
        let requested = query.groups.clone();
        let ctx_copy = viewer.ctx;
        let admitted = tc.run(move |conn| {
            let mut g: Vec<String> = Vec::new();
            crate::sync::groups::admit_ticket_groups(conn, &requested, &ctx_copy, &mut g);
            Ok::<Vec<String>, diesel::result::Error>(g)
        });
        if let Ok(extra) = admitted {
            for g in extra {
                if !granted.contains(&g) {
                    granted.push(g);
                }
            }
        }
    }
    // Resolved before the early return: a client whose cursor has aged out
    // needs to know that even when it currently has no granted groups,
    // otherwise it sits on a stale cache until its permissions change.
    let resync_required = match tc.run(|conn| oldest_retained_sync_id(conn)) {
        Ok(oldest) => cursor_predates_retention(query.from, oldest),
        Err(e) => {
            // Don't fail the poll over the horizon probe. Reporting "no resync"
            // keeps the client on its existing cache, which is the same
            // position it was in before this signal existed.
            error!(error = %e, "delta: oldest-retained probe failed; assuming no resync");
            false
        }
    };

    // Cheap (count over a tiny table); every response carries current flags.
    let capabilities =
        match tc.run(|conn| Ok::<_, diesel::result::Error>(super::capability_flags(conn))) {
            Ok(c) => Some(c),
            Err(e) => {
                error!(error = %e, "delta: capability probe failed; omitting flags");
                None
            }
        };

    if granted.is_empty() {
        // No groups in common between request and permission set;
        // return an empty delta rather than an error so clients can
        // continue polling without special-casing.
        return HttpResponse::Ok().json(DeltaResponse {
            actions: vec![],
            last_xid8: query.from_xid8.unwrap_or(0),
            last_sync_id: query.from,
            has_more: false,
            resync_required,
            capabilities,
        });
    }

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    // Fetch limit + 1 so we can detect the "has_more" boundary
    // without an extra count query.
    let granted_pg: Vec<Option<String>> = granted.iter().map(|g| Some(g.clone())).collect();
    let from = query.from;
    let from_xid8 = query.from_xid8;
    let rows = tc.run(move |conn| {
        // Common shape: granted groups, only settled rows (below the
        // commit horizon). Select xid8 last to match `ActionRow`.
        let base = sync_actions::table
            .filter(sync_actions::groups.overlaps_with(granted_pg))
            .filter(crate::sync::feed::below_horizon())
            .select((
                sync_actions::sync_id,
                sync_actions::aggregate,
                sync_actions::aggregate_id,
                sync_actions::op,
                sync_actions::event_type,
                sync_actions::schema_version,
                sync_actions::data,
                sync_actions::groups,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::actor_ref,
                sync_actions::correlation_id,
                sync_actions::causation_id,
                sync_actions::occurred_at,
                sync_actions::xid8,
            ))
            .into_boxed();
        let q = match from_xid8 {
            // Commit-safe composite cursor (current clients): strictly
            // after (from_xid8, from), ordered by (xid8, sync_id).
            Some(x) => base
                .filter(
                    sync_actions::xid8
                        .gt(x)
                        .or(sync_actions::xid8.eq(x).and(sync_actions::sync_id.gt(from))),
                )
                .order((sync_actions::xid8.asc(), sync_actions::sync_id.asc())),
            // Legacy sync_id cursor (clients that predate the upgrade);
            // still horizon-gated. The response carries last_xid8 so the
            // client switches to the safe cursor once it updates.
            None => base
                .filter(sync_actions::sync_id.gt(from))
                .order(sync_actions::sync_id.asc()),
        };
        q.limit(limit + 1).load::<ActionRow>(conn)
    });

    let mut actions = match rows {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "delta query failed");
            return errors::internal("Failed to load sync delta");
        }
    };

    let has_more = actions.len() > limit as usize;
    if has_more {
        actions.truncate(limit as usize);
    }

    // `last_sync_id` is taken from the fetched page BEFORE the visibility
    // filter below. Dropped rows must still advance the client's cursor,
    // otherwise it would re-request them every poll forever.
    let last_sync_id = actions.last().map(|a| a.sync_id).unwrap_or(query.from);
    let last_xid8 = actions
        .last()
        .map(|a| a.xid8)
        .unwrap_or(query.from_xid8.unwrap_or(0));

    // Read-side visibility, via the shared sync-visibility layer:
    // documentation (every viewer) + the ticket family (restricted
    // members only). `filter_actions` returns a keep-mask and never
    // errors — a visibility-lookup failure fails closed (drops the
    // affected family) rather than 500'ing the poll.
    let (actions, keep) = match tc.run(move |conn| {
        let keep =
            crate::sync::visibility::filter_actions(conn, &viewer, &actions, action_row_to_view);
        Ok::<(Vec<ActionRow>, Vec<bool>), diesel::result::Error>((actions, keep))
    }) {
        Ok(x) => x,
        Err(e) => {
            error!(error = %e, "delta visibility filter failed");
            return errors::internal("Failed to load sync delta");
        }
    };
    let mut actions = actions;
    let mut keep_iter = keep.into_iter();
    actions.retain(|_| keep_iter.next().unwrap_or(false));

    HttpResponse::Ok().json(DeltaResponse {
        actions,
        last_xid8,
        last_sync_id,
        has_more,
        resync_required,
        capabilities,
    })
}

/// Lower an `ActionRow` into the visibility layer's `ActionView`.
fn action_row_to_view(row: &ActionRow) -> crate::sync::visibility::ActionView {
    crate::sync::visibility::ActionView {
        aggregate: Some(row.aggregate),
        is_delete: matches!(row.op, crate::models::SyncOp::Delete),
        aggregate_id: row.aggregate_id.parse().ok(),
        ticket_id: row
            .data
            .get("ticket_id")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
        is_internal: row.data.get("is_internal").and_then(|v| v.as_bool()),
        comment_id: row
            .data
            .get("comment_id")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
    }
}

/// Intersect the comma-separated client-requested groups with the
/// server-side permitted set. Returns the granted subset preserving
/// the input order, deduped.
fn intersect_groups(requested_csv: &str, allowed: &[String]) -> Vec<String> {
    use std::collections::HashSet;
    let allowed_set: HashSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in requested_csv.split(',') {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !allowed_set.contains(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_groups_keeps_order_drops_unknown() {
        let allowed = vec![
            "workspace:1".to_string(),
            "project:7".to_string(),
            "project:9".to_string(),
        ];
        let got = intersect_groups("project:7,workspace:1,project:99,project:9", &allowed);
        assert_eq!(got, vec!["project:7", "workspace:1", "project:9"]);
    }

    #[test]
    fn intersect_groups_dedupes_and_trims() {
        let allowed = vec!["workspace:1".to_string()];
        let got = intersect_groups(" workspace:1 , workspace:1, ", &allowed);
        assert_eq!(got, vec!["workspace:1"]);
    }

    /// The horizon predicate, exhaustively. This is the whole decision: get it
    /// wrong toward false and clients keep phantom rows silently; wrong toward
    /// true and they re-bootstrap needlessly.
    #[test]
    fn cursor_predates_retention_boundaries() {
        // Empty table: nothing to compare against, never signal.
        assert!(!cursor_predates_retention(0, None));
        assert!(!cursor_predates_retention(9_999, None));

        // `from == 0` is a cursorless client; it bootstraps, so never signal
        // even when the table has been pruned well past 0.
        assert!(!cursor_predates_retention(0, Some(5_000)));

        // Contiguous: the cursor sits exactly one below the oldest retained
        // row, so the next row it wants is the one we still have. No gap.
        assert!(!cursor_predates_retention(4_999, Some(5_000)));

        // Cursor at or beyond the oldest retained row: caught up, no gap.
        assert!(!cursor_predates_retention(5_000, Some(5_000)));
        assert!(!cursor_predates_retention(9_999, Some(5_000)));

        // Cursor two or more below: ids between it and the oldest survivor are
        // gone. Conservative — they may have been rolled-back sequence values
        // rather than pruned rows, and a needless re-bootstrap is the safe
        // side of that ambiguity.
        assert!(cursor_predates_retention(4_998, Some(5_000)));
        assert!(cursor_predates_retention(1, Some(5_000)));
    }

    /// The probe runs against the real partitioned table, so a schema change
    /// that breaks the MIN query surfaces here rather than as every client
    /// silently never resyncing.
    #[test]
    fn oldest_retained_sync_id_reads_the_partitioned_table() {
        let mut conn = crate::test_helpers::setup_test_connection();
        let oldest = oldest_retained_sync_id(&mut conn).expect("probe query runs");
        // Value depends on test-db contents; the contract is that it answers
        // rather than errors, and that a present value is a positive sequence
        // id which the predicate can compare against.
        if let Some(v) = oldest {
            assert!(v > 0, "sync_id is a positive sequence, got {v}");
            assert!(!cursor_predates_retention(v, Some(v)));
        }
    }

    #[test]
    fn intersect_groups_empty_inputs() {
        assert!(intersect_groups("", &["workspace:1".to_string()]).is_empty());
        assert!(intersect_groups("workspace:1", &[]).is_empty());
    }
}
