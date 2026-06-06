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
    /// Cursor — return rows with `sync_id > from`.
    pub from: i64,
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
}

#[derive(Debug, Serialize)]
pub struct DeltaResponse {
    pub actions: Vec<ActionRow>,
    pub last_sync_id: i64,
    pub has_more: bool,
}

pub async fn delta(
    mut tc: TenantConn,
    query: web::Query<DeltaQuery>,
    ctx: SyncContext,
) -> impl Responder {
    let granted = intersect_groups(&query.groups, &ctx.allowed_groups);
    if granted.is_empty() {
        // No groups in common between request and permission set;
        // return an empty delta rather than an error so clients can
        // continue polling without special-casing.
        return HttpResponse::Ok().json(DeltaResponse {
            actions: vec![],
            last_sync_id: query.from,
            has_more: false,
        });
    }

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    // Fetch limit + 1 so we can detect the "has_more" boundary
    // without an extra count query.
    let granted_pg: Vec<Option<String>> = granted.iter().map(|g| Some(g.clone())).collect();
    let from = query.from;
    let rows = tc.run(|conn| {
        sync_actions::table
            .filter(sync_actions::sync_id.gt(from))
            .filter(sync_actions::groups.overlaps_with(granted_pg))
            .order(sync_actions::sync_id.asc())
            .limit(limit + 1)
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
            ))
            .load::<ActionRow>(conn)
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

    // `last_sync_id` is taken from the fetched page BEFORE the documentation
    // visibility filter below. Dropped rows must still advance the client's
    // cursor, otherwise it would re-request them every poll forever.
    let last_sync_id = actions.last().map(|a| a.sync_id).unwrap_or(query.from);

    // Read-side documentation visibility. Documentation page/collection rows
    // are emitted to `workspace:1` (no per-row group scoping), so the group
    // overlap above lets every workspace member see them. Filter out any the
    // caller is not actually permitted to read, reusing the canonical access
    // logic. Most deltas carry no documentation rows, so this is a no-op for
    // the common case.
    use crate::models::SyncAggregate;
    let page_ids: Vec<i32> = actions
        .iter()
        .filter(|a| a.aggregate == SyncAggregate::DocumentationPage)
        .filter_map(|a| a.aggregate_id.parse::<i32>().ok())
        .collect();
    let collection_ids: Vec<i32> = actions
        .iter()
        .filter(|a| a.aggregate == SyncAggregate::DocumentationCollection)
        .filter_map(|a| a.aggregate_id.parse::<i32>().ok())
        .collect();
    if !page_ids.is_empty() || !collection_ids.is_empty() {
        let user = ctx.user.clone();
        let hidden = tc.run(move |conn| {
            let is_admin = crate::repository::user_helpers::user_is_admin(conn, &user);
            crate::repository::documentation::hidden_documentation_ids(
                conn,
                &page_ids,
                &collection_ids,
                &user.uuid,
                is_admin,
            )
        });
        match hidden {
            Ok((hidden_pages, hidden_collections)) => {
                if !hidden_pages.is_empty() || !hidden_collections.is_empty() {
                    actions.retain(|a| match a.aggregate {
                        SyncAggregate::DocumentationPage => a
                            .aggregate_id
                            .parse::<i32>()
                            .ok()
                            .is_none_or(|id| !hidden_pages.contains(&id)),
                        SyncAggregate::DocumentationCollection => a
                            .aggregate_id
                            .parse::<i32>()
                            .ok()
                            .is_none_or(|id| !hidden_collections.contains(&id)),
                        _ => true,
                    });
                }
            }
            Err(e) => {
                error!(error = %e, "documentation visibility filter failed");
                return errors::internal("Failed to load sync delta");
            }
        }
    }

    HttpResponse::Ok().json(DeltaResponse {
        actions,
        last_sync_id,
        has_more,
    })
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

    #[test]
    fn intersect_groups_empty_inputs() {
        assert!(intersect_groups("", &["workspace:1".to_string()]).is_empty());
        assert!(intersect_groups("workspace:1", &[]).is_empty());
    }
}
