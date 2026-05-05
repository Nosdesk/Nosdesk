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
use crate::models::{Project, ProjectTicket, Ticket, WorkflowState};
use crate::schema::{project_tickets, projects, tickets, workflow_states};

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

    // Workspace capability flags. These are simple booleans the
    // frontend uses to gate optional UI surfaces (filter chips,
    // default visible columns, summary segments). Adding a flag
    // here is the right place when the client should treat a
    // feature as "exists for this workspace" vs "available
    // everywhere" — eg. SLA chrome should hide entirely until
    // an admin sets up at least one policy. Counts (rather than
    // "any non-archived") are fine for v1: a workspace either
    // has policies or it doesn't.
    let sla_enabled: bool = {
        use diesel::dsl::count_star;
        let n: i64 = crate::schema::sla_policies::table
            .select(count_star())
            .first(&mut conn)
            .unwrap_or(0);
        n > 0
    };

    // Header: schema hash, cursor, granted groups, and capability
    // flags. Clients read this once at the start of every
    // bootstrap and cache the values for the session.
    send(tx, json!({
        "__meta__": {
            "server_schema": SERVER_SCHEMA_HASH,
            "last_sync_id": last_sync_id,
            "groups_granted": granted,
            "sla_enabled": sla_enabled,
        }
    }))?;

    // Workflow states: workspace-wide config, always sent so the
    // kanban can render columns immediately even before the
    // workflow_states store loads via its own endpoint. Captured
    // into a HashMap so the ticket loader below can denormalise
    // each ticket's workflow_state inline without a per-row query.
    let states: Vec<WorkflowState> = workflow_states::table
        .order((workflow_states::category, workflow_states::position))
        .load(&mut conn)?;
    let mut states_by_id: std::collections::HashMap<i32, WorkflowState> =
        std::collections::HashMap::with_capacity(states.len());
    for state in &states {
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
    for state in states {
        states_by_id.insert(state.id, state);
    }

    // Two project-loading paths:
    //
    // 1. Workspace-wide (`workspace:1` in granted set): load every
    //    project the user has visibility into. Single-workspace
    //    deployment means this is "all projects" in practice; the
    //    permission check happens upstream in
    //    `sync::groups::allowed_for_user`.
    //
    // 2. Per-project (`project:<id>` strings in granted set):
    //    incremental subscribe-on-route-entry. Drop the prefix,
    //    parse the suffix as i32, fetch the matching projects.
    //
    // Both paths land in the same set; HashSet dedupes if a request
    // ever asks for both `workspace:1` and `project:7` together.
    use std::collections::HashSet;
    let want_all = granted.iter().any(|g| g == "workspace:1");

    let project_ids: Vec<i32> = if want_all {
        projects::table.select(projects::id).load::<i32>(&mut conn)?
    } else {
        let mut ids: HashSet<i32> = HashSet::new();
        for g in granted {
            if let Some(suffix) = g.strip_prefix("project:") {
                if let Ok(id) = suffix.parse::<i32>() {
                    ids.insert(id);
                }
            }
        }
        ids.into_iter().collect()
    };

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

    // Tickets: two paths, mirroring the project loader above.
    //
    // 1. Workspace-wide (`workspace:1` granted): every ticket in the
    //    workspace. The TicketsListViewV2 reads from this — My Queue
    //    and Triage filter on assignee / workflow_state.category
    //    respectively, both of which need the full ticket set.
    //
    // 2. Per-project: tickets associated with the granted project ids
    //    via project_tickets. The kanban view reads from this.
    //
    // The two paths produce the same denormalised ticket shape; we
    // load whichever set the granted groups call for, deduping
    // implicitly through eq_any-on-id.
    let ticket_query = if want_all {
        tickets::table.into_boxed()
    } else if !project_ids.is_empty() {
        // Use the project_tickets join to scope tickets to granted
        // projects. Loading via project_id eq_any is a single index
        // scan; the per-row workflow_state lookup stays O(1) below.
        let scoped_ids: Vec<i32> = project_tickets::table
            .filter(project_tickets::project_id.eq_any(&project_ids))
            .select(project_tickets::ticket_id)
            .load(&mut conn)?;
        tickets::table.filter(tickets::id.eq_any(scoped_ids)).into_boxed()
    } else {
        // No projects in the granted set and no workspace grant —
        // skip the ticket loader entirely.
        return finish(tx, last_sync_id);
    };

    let ticket_rows: Vec<Ticket> = ticket_query.load(&mut conn)?;

    // Per-ticket pill data computed in one batch each so the
    // bootstrap stays O(n) rather than N round-trips. Empty maps
    // for the tickets without signals / devices; consumers default
    // those to 'none' / null.
    let ticket_ids: Vec<i32> = ticket_rows.iter().map(|t| t.id).collect();
    let kb_gap_counts = crate::repository::knowledge_gaps::open_signal_counts_for_tickets(
        &mut conn, &ticket_ids,
    )?;
    let device_summaries =
        crate::repository::tickets::devices_summary_for_tickets(&mut conn, &ticket_ids)?;
    let cycle_membership =
        crate::repository::cycles::cycle_ids_for_tickets(&mut conn, &ticket_ids)?;
    // Load every SLA policy + working calendar once; the
    // pill-computation loop below resolves each ticket against
    // them in memory.
    let sla_ctx = crate::repository::sla::load_for_pill_computation(&mut conn)?;
    let now = chrono::Utc::now();

    for t in ticket_rows {
        let ws = states_by_id.get(&t.workflow_state_id);
        let workflow_state_payload = ws.map(|s| json!({
            "id": s.id,
            "name": s.name,
            "category": s.category.as_str(),
            "color": s.color,
        }));
        let kb_gap_signal = match kb_gap_counts.get(&t.id).copied().unwrap_or(0) {
            0 => "none",
            1..=2 => "weak",
            _ => "strong",
        };
        let affected_devices = device_summaries.get(&t.id).map(|(count, id, name, os)| {
            json!({
                "count": count,
                "first": { "id": id, "name": name, "os": os },
            })
        });
        // SLA pill: pick the most-specific applicable policy, then
        // resolve the working calendar + holidays it points at.
        // Tickets without a matching policy or calendar render
        // without a pill; consumers tolerate the null shape.
        let sla = crate::services::sla::pick_policy(&sla_ctx.policies, &t)
            .and_then(|policy| {
                let cal_id = policy.working_calendar_id?;
                let calendar = sla_ctx.calendars_by_id.get(&cal_id)?;
                let holidays = sla_ctx
                    .holidays_by_calendar
                    .get(&cal_id)
                    .cloned()
                    .unwrap_or_default();
                let category = ws.map(|s| s.category)
                    .unwrap_or(crate::models::WorkflowStateCategory::Backlog);
                Some(crate::services::sla::compute_pill(
                    &t, category, policy, calendar, &holidays, now,
                ))
            })
            .unwrap_or(serde_json::Value::Null);
        send(tx, json!({
            "__model__": "ticket",
            "id": t.id,
            "title": t.title,
            "workflow_state": workflow_state_payload,
            "workflow_state_id": t.workflow_state_id,
            "priority": match t.priority {
                crate::models::TicketPriority::Low => "low",
                crate::models::TicketPriority::Medium => "medium",
                crate::models::TicketPriority::High => "high",
            },
            "requester_uuid": t.requester_uuid,
            "assignee_uuid": t.assignee_uuid,
            "category_id": t.category_id,
            "triage_state": t.triage_state,
            "due_date": t.due_date,
            "kb_gap_signal": kb_gap_signal,
            "affected_devices": affected_devices,
            "cycle_id": cycle_membership.get(&t.id),
            "sla": sla,
            "recurrence_rule": t.recurrence_rule,
            "recurrence_template_id": t.recurrence_template_id,
            "created_at": t.created_at,
            "updated_at": t.updated_at,
            "last_activity_at": t.updated_at,
        }))?;
    }

    finish(tx, last_sync_id)
}

fn finish(
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
    last_sync_id: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
