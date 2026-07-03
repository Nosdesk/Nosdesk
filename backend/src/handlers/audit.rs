//! Unified audit read API (Item C/W5).
//!
//! `GET /api/admin/audit` — one paginated, filterable feed over all
//! three audit substrates. `GET /api/admin/audit/export` — the same
//! filtered set as a JSON download.
//!
//! Both are gated by [`rbac::require_audit_read`] (admin / audit
//! reviewer role AND the `audit:read` scope). Each request opens its
//! transaction with `SET LOCAL nosdesk.in_audit_read = 'true'` so the
//! audit trigger short-circuits (D5: no recursive audit rows), and
//! emits exactly one tier-1 meta event recording who read/exported
//! what filter and how many rows came back.

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{SyncAggregate, SyncOp};
use crate::repository::audit as repo;
use crate::sync::{emit, groups};
use crate::utils::rbac;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use base64::Engine;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;
use uuid::Uuid;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/audit-log",
        web::get().to(crate::handlers::audit_log::list),
    )
    // Item C/W5: unified audit feed over all three
    // substrates (sync_actions + security_events +
    // audit_log), gated by the audit:read scope and the
    // admin / audit-reviewer roles.
    .route("/admin/audit", web::get().to(crate::handlers::audit::list))
    .route(
        "/admin/audit/export",
        web::get().to(crate::handlers::audit::export),
    );
}

/// Cap on a single export so a wide filter can't OOM the process.
const EXPORT_MAX_ROWS: usize = 5000;
const EXPORT_PAGE: i64 = 200;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    pub actor_uuid: Option<Uuid>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    /// Event-type prefix, e.g. `auth.` or `ticket.`.
    pub event_prefix: Option<String>,
    /// 1 (app), 2 (auth), or 3 (row diffs).
    pub tier: Option<i16>,
    pub severity: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub entries: Vec<repo::AuditEntry>,
    pub next_cursor: Option<String>,
}

fn build_filter(q: &ListQuery) -> repo::AuditFilter {
    repo::AuditFilter {
        actor_uuid: q.actor_uuid,
        // Default to the last 7 days: the feed spans three append-only
        // tables and an unbounded scan is the wrong default.
        since: q
            .since
            .or_else(|| Some(Utc::now() - chrono::Duration::days(7))),
        until: q.until,
        event_prefix: q.event_prefix.clone(),
        tier: q.tier,
        severity: q.severity.clone(),
    }
}

/// One JSON object summarising the filter for the self-audit event.
fn filter_summary(f: &repo::AuditFilter) -> serde_json::Value {
    json!({
        "actor_uuid": f.actor_uuid,
        "since": f.since,
        "until": f.until,
        "event_prefix": f.event_prefix,
        "tier": f.tier,
        "severity": f.severity,
    })
}

/// Set the recursive-read guard GUC for this transaction (D5).
fn arm_read_guard(conn: &mut crate::db::DbConnection) -> QueryResult<()> {
    diesel::sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;
    Ok(())
}

/// `GET /api/admin/audit`
pub async fn list(
    req: HttpRequest,
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_audit_read(&req) {
        return resp;
    }

    let cursor = match query.cursor.as_deref().map(decode_cursor) {
        Some(Ok(c)) => Some(c),
        Some(Err(_)) => {
            return errors::bad_request(
                "Invalid cursor; pass the next_cursor from the previous response verbatim",
            )
        }
        None => None,
    };

    let filter = build_filter(&query);
    let limit = query.limit.unwrap_or(50);

    let result = tc.run(|conn| {
        arm_read_guard(conn)?;
        let page = repo::list(conn, &filter, cursor, limit)?;
        emit_audit_event(conn, "data.audit.read", &filter, page.entries.len())?;
        Ok(page)
    });

    let page = match result {
        Ok(p) => p,
        Err(e) => {
            warn!(error = ?e, "unified audit list failed");
            return errors::internal("Failed to read the audit log");
        }
    };

    HttpResponse::Ok().json(ListResponse {
        entries: page.entries,
        next_cursor: page.next_cursor.map(encode_cursor),
    })
}

/// `GET /api/admin/audit/export` — full filtered set as a JSON
/// attachment (capped at EXPORT_MAX_ROWS). Emits `data.audit.exported`.
pub async fn export(
    req: HttpRequest,
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_audit_read(&req) {
        return resp;
    }

    let filter = build_filter(&query);

    let result = tc.run(|conn| {
        arm_read_guard(conn)?;
        let mut all: Vec<repo::AuditEntry> = Vec::new();
        let mut cursor = None;
        loop {
            let page = repo::list(conn, &filter, cursor, EXPORT_PAGE)?;
            all.extend(page.entries);
            match page.next_cursor {
                Some(c) if all.len() < EXPORT_MAX_ROWS => cursor = Some(c),
                _ => break,
            }
        }
        all.truncate(EXPORT_MAX_ROWS);
        emit_audit_event(conn, "data.audit.exported", &filter, all.len())?;
        Ok(all)
    });

    let entries = match result {
        Ok(e) => e,
        Err(e) => {
            warn!(error = ?e, "unified audit export failed");
            return errors::internal("Failed to export the audit log");
        }
    };

    let filename = format!("audit-export-{}.json", Utc::now().format("%Y%m%d-%H%M%S"));
    let body = json!({
        "exported_at": Utc::now(),
        "filter": filter_summary(&filter),
        "count": entries.len(),
        "truncated": entries.len() >= EXPORT_MAX_ROWS,
        "entries": entries,
    });

    HttpResponse::Ok()
        .insert_header((
            actix_web::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ))
        .json(body)
}

/// Emit the one tier-1 meta event for this read/export. Runs inside the
/// same transaction (and same `in_audit_read` guard) as the query, but
/// writes to `sync_actions`, which the audit trigger never touches, so
/// there is no recursion.
fn emit_audit_event(
    conn: &mut crate::db::DbConnection,
    event_type: &'static str,
    filter: &repo::AuditFilter,
    rows_returned: usize,
) -> QueryResult<()> {
    emit::record(
        conn,
        emit::SyncEmit {
            aggregate: SyncAggregate::Data,
            aggregate_id: "audit".to_string(),
            op: SyncOp::Insert,
            event_type,
            data: json!({
                "filter": filter_summary(filter),
                "rows_returned": rows_returned,
            }),
            groups: groups::workspace(),
            causation_id: None,
        },
    )?;
    Ok(())
}

fn encode_cursor(c: repo::Cursor) -> String {
    let json = serde_json::to_vec(&c).expect("Cursor serialises to JSON");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

fn decode_cursor(s: &str) -> Result<repo::Cursor, ()> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}
