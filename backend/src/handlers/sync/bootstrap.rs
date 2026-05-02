//! `GET /api/sync/bootstrap?groups=<csv>&schema=<hash>`
//!
//! Streams an NDJSON snapshot of every aggregate row the caller's
//! granted groups can see. The response opens with a `__meta__`
//! header line, follows with one `__model__`-tagged JSON object per
//! row, and closes with `__end__`. The client streams these into the
//! object pool as they arrive — large workspaces don't block the UI
//! waiting for the whole snapshot to land.
//!
//! v1 streams three aggregates: `workflow_state` (always — workspace
//! config is small and every view needs it), `project` (subscribed
//! groups only), and `project_ticket` (associations for those
//! projects). Tickets, comments, and attachments stay lazy-loaded
//! through `useReference` so the bootstrap stays under 1MB even on
//! enterprise-scale workspaces.

use actix_web::{web, HttpResponse, Responder};
use bytes::Bytes;
use diesel::prelude::*;
use futures::stream::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;

use crate::db::Pool;
use crate::extractors::SyncContext;
use crate::models::{Project, ProjectTicket, WorkflowState};
use crate::schema::{project_tickets, projects, workflow_states};

#[derive(Debug, Deserialize)]
pub struct BootstrapQuery {
    /// Comma-separated group strings the client wants to subscribe
    /// to. The server returns the intersection with the caller's
    /// permitted set in the `__meta__.groups_granted` field.
    pub groups: String,
    /// Client's persisted schema hash. When the server's compiled
    /// hash (`NOSDESK_SCHEMA_HASH`) doesn't match, the response's
    /// `__meta__` line carries the new hash so the client wipes
    /// IndexedDB before consuming the snapshot.
    #[serde(default)]
    pub schema: Option<String>,
}

const SERVER_SCHEMA_HASH: &str = env!("NOSDESK_SCHEMA_HASH");

pub async fn bootstrap(
    pool: web::Data<Pool>,
    query: web::Query<BootstrapQuery>,
    ctx: SyncContext,
) -> impl Responder {
    let granted = intersect_groups(&query.groups, &ctx.allowed_groups);

    // Bounded mpsc channel so a slow client back-pressures the
    // streamer instead of buffering everything in memory.
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    let pool_clone = pool.clone();
    let granted_clone = granted.clone();

    // Diesel is sync; do the work on a blocking thread and ferry
    // bytes back through the channel so the Actix response future
    // can stay async-friendly.
    tokio::task::spawn_blocking(move || {
        if let Err(e) = stream_bootstrap(&pool_clone, &granted_clone, &tx) {
            error!(error = %e, "bootstrap streaming failed");
            // Best-effort: ship an `__error__` line so the client
            // can surface a useful message instead of just seeing
            // the stream close mid-snapshot.
            let _ = tx.blocking_send(Ok(line(json!({
                "__error__": "stream_failed",
                "detail": e.to_string(),
            }))));
        }
    });

    let body = ReceiverStream::new(rx).map(|r| r.map_err(actix_web::error::ErrorInternalServerError));
    HttpResponse::Ok()
        .content_type("application/x-ndjson")
        .streaming(body)
}

fn stream_bootstrap(
    pool: &web::Data<Pool>,
    granted: &[String],
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = pool.get()?;

    let last_sync_id: Option<i64> = crate::schema::sync_actions::table
        .select(diesel::dsl::max(crate::schema::sync_actions::sync_id))
        .first(&mut conn)?;
    let last_sync_id = last_sync_id.unwrap_or(0);

    // Header: schema hash, cursor, and the granted-groups echo so
    // the client knows what its subsequent delta calls should pass.
    send(tx, json!({
        "__meta__": {
            "server_schema": SERVER_SCHEMA_HASH,
            "last_sync_id": last_sync_id,
            "groups_granted": granted,
        }
    }))?;

    // Workflow states: workspace-wide config, always sent so the
    // kanban can render columns immediately even before the
    // workflow_states store loads via its own endpoint.
    let states: Vec<WorkflowState> = workflow_states::table
        .order((workflow_states::category, workflow_states::position))
        .load(&mut conn)?;
    for state in states {
        send(tx, json!({
            "__model__": "workflow_state",
            "id": state.id,
            "name": state.name,
            "category": state.category.as_str(),
            "color": state.color,
            "position": state.position,
            "is_default": state.is_default,
            "archived_at": state.archived_at,
        }))?;
    }

    // Project ids inside the granted groups. Drop the `project:`
    // prefix and parse the suffix as an i32 — the join below uses
    // the typed ids so unrelated `project:9999` strings in the
    // request don't reach SQL.
    let project_ids: Vec<i32> = granted
        .iter()
        .filter_map(|g| g.strip_prefix("project:"))
        .filter_map(|s| s.parse::<i32>().ok())
        .collect();

    if !project_ids.is_empty() {
        let projects: Vec<Project> = projects::table
            .filter(projects::id.eq_any(&project_ids))
            .load(&mut conn)?;
        for p in projects {
            send(tx, json!({
                "__model__": "project",
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "status": p.status,
                "created_at": p.created_at,
                "updated_at": p.updated_at,
                "created_by": p.created_by,
            }))?;
        }

        let assocs: Vec<ProjectTicket> = project_tickets::table
            .filter(project_tickets::project_id.eq_any(&project_ids))
            .load(&mut conn)?;
        for a in assocs {
            send(tx, json!({
                "__model__": "project_ticket",
                "project_id": a.project_id,
                "ticket_id": a.ticket_id,
                "display_order": a.display_order,
            }))?;
        }
    }

    send(tx, json!({ "__end__": { "last_sync_id": last_sync_id } }))?;
    Ok(())
}

fn send(
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
    value: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if tx.blocking_send(Ok(line(value))).is_err() {
        // Receiver dropped — client disconnected. Bail out of the
        // loop without surfacing as an error; the spawn_blocking
        // task ends, the connection releases, no rows leaked.
        return Err("client disconnected".into());
    }
    Ok(())
}

fn line(value: serde_json::Value) -> Bytes {
    let mut s = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    s.push('\n');
    Bytes::from(s)
}

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

// `intersect_groups` is duplicated between bootstrap.rs and delta.rs
// intentionally for now — the helper is small, the call site
// constraints are subtly different (bootstrap echoes the granted set
// in the response while delta short-circuits on empty), and pulling
// to a shared module would force both sites to take a Vec<String>
// allocation for what's already a tiny stack-allocated structure.
// Revisit if the helper grows past 30 lines or a third caller appears.

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn schema_hash_is_compile_time_stamped() {
        // Sanity check that build.rs ran — empty schema hash would
        // mean the bootstrap response advertises no schema, and
        // every client would treat their cached state as out of
        // sync on every cold start.
        assert!(!SERVER_SCHEMA_HASH.is_empty());
    }
}
