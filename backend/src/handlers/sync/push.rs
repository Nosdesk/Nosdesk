//! `POST /api/sync/push`
//!
//! Applies an array of optimistic transactions and returns per-tx
//! outcomes. Each transaction is independent — a malformed or
//! permission-denied row in the middle of a batch doesn't roll back
//! the others. The HTTP response is always 200; per-tx success or
//! failure lives in the body.
//!
//! v1 dispatches to the project aggregate only. Other aggregates
//! get a `unsupported_aggregate` rejection until their repository
//! helpers grow a typed-patch entrypoint that emit::record routes
//! through. Adding a new aggregate is one match arm + one
//! `apply_<aggregate>` helper.

use actix_web::{web, HttpResponse, Responder};
use diesel::prelude::*;
use diesel::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::extractors::SyncContext;
use crate::handlers::sse::{SseEvent, SseState};
use crate::handlers::sync::delta::ActionRow;
use crate::handlers::{errors, helpers};
use crate::models::{Project, ProjectUpdate, SyncAggregate, SyncOp, TicketUpdate};
use crate::schema::sync_actions;
use crate::sync::actor::{ActorContext, ActorKind};
use crate::sync::session;

const MAX_BATCH: usize = 50;

#[derive(Debug, Deserialize)]
pub struct PushTransaction {
    /// ULID or UUID — opaque to the server, used as the
    /// idempotency key on `sync_actions.client_tx_id`.
    pub tx_id: String,
    /// One of the registered `SyncAggregate` variants.
    pub aggregate: SyncAggregate,
    /// String form so the protocol stays uniform across
    /// integer-keyed and UUID-keyed aggregates.
    pub model_id: String,
    pub op: SyncOp,
    /// On `I`: full row payload. On `U`: changed fields only.
    /// On `D`: ignored (and may be empty).
    #[serde(default)]
    pub patch: Value,
    /// Reserved for future stricter conflict policies. Server logs
    /// the value but doesn't reject on staleness in v1 (LWW).
    #[serde(default)]
    pub base_sync_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PushResponse {
    pub applied: Vec<String>,
    pub rejected: Vec<RejectedTx>,
    pub last_sync_id: i64,
}

#[derive(Debug, Serialize)]
pub struct RejectedTx {
    pub tx_id: String,
    pub reason: &'static str,
    pub detail: String,
}

pub async fn push(
    pool: web::Data<Pool>,
    body: web::Json<Vec<PushTransaction>>,
    ctx: SyncContext,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    let body = body.into_inner();
    if body.len() > MAX_BATCH {
        return errors::bad_request(&format!(
            "Batch exceeds the {MAX_BATCH}-transaction limit"
        ));
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut applied: Vec<String> = Vec::with_capacity(body.len());
    let mut rejected: Vec<RejectedTx> = Vec::new();
    let mut last_sync_id: i64 = 0;
    let mut applied_sync_ids: Vec<i64> = Vec::new();

    for tx in body {
        let tx_id = tx.tx_id.clone();
        let actor = ActorContext {
            kind: ActorKind::User,
            uuid: Some(ctx.user.uuid),
            reference: None,
            correlation_id: Some(ctx.correlation_id),
            client_tx_id: Some(tx_id.clone()),
        };

        match apply_transaction(&mut conn, &tx, &actor) {
            Ok(sync_id) => {
                last_sync_id = sync_id.max(last_sync_id);
                applied_sync_ids.push(sync_id);
                applied.push(tx_id.clone());
                info!(
                    user = %ctx.user.uuid,
                    tx_id = %tx_id,
                    aggregate = %tx.aggregate.as_str(),
                    sync_id,
                    "sync push applied"
                );
            }
            Err(reject) => {
                warn!(
                    user = %ctx.user.uuid,
                    tx_id = %tx_id,
                    aggregate = %tx.aggregate.as_str(),
                    reason = %reject.0,
                    "sync push rejected"
                );
                rejected.push(RejectedTx {
                    tx_id,
                    reason: reject.0,
                    detail: reject.1,
                });
            }
        }
    }

    // Outbox bridge: re-fetch the inserted action rows post-commit and
    // broadcast them to SSE subscribers. Doing the fetch + broadcast
    // outside the per-tx loop means a slow SSE consumer never blocks
    // a write — the broadcaster is fire-and-forget through
    // `tokio::broadcast`, falling back to delta polling when a
    // consumer's buffer fills.
    if !applied_sync_ids.is_empty() {
        match load_action_rows(&mut conn, &applied_sync_ids) {
            Ok(rows) if !rows.is_empty() => {
                let payload = match serde_json::to_value(&rows) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(error = %e, "could not serialise sync_actions for SSE broadcast");
                        return HttpResponse::Ok().json(PushResponse {
                            applied,
                            rejected,
                            last_sync_id,
                        });
                    }
                };
                sse_state
                    .broadcast_event(SseEvent::SyncActions {
                        actions: payload,
                        last_sync_id,
                        timestamp: chrono::Utc::now(),
                    })
                    .await;
            }
            Ok(_) => {}
            Err(e) => {
                warn!(error = %e, "failed to load applied sync_actions for SSE outbox");
            }
        }
    }

    HttpResponse::Ok().json(PushResponse {
        applied,
        rejected,
        last_sync_id,
    })
}

fn load_action_rows(
    conn: &mut DbConnection,
    ids: &[i64],
) -> diesel::QueryResult<Vec<ActionRow>> {
    sync_actions::table
        .filter(sync_actions::sync_id.eq_any(ids))
        .order(sync_actions::sync_id.asc())
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
}

struct TxReject(&'static str, String);

/// Test-only entrypoint mirroring `apply_transaction` but returning
/// a public tuple so callers in `#[cfg(test)]` modules can assert
/// against it without `TxReject` needing to leak out of this file.
#[cfg(test)]
pub(super) fn apply_transaction_for_test(
    conn: &mut DbConnection,
    tx: &PushTransaction,
    actor: &ActorContext,
) -> Result<i64, (&'static str, String)> {
    apply_transaction(conn, tx, actor).map_err(|TxReject(r, d)| (r, d))
}

fn apply_transaction(
    conn: &mut DbConnection,
    tx: &PushTransaction,
    actor: &ActorContext,
) -> Result<i64, TxReject> {
    // Idempotency short-circuit: if this `tx_id` already has a row
    // in sync_actions, we treat it as applied (this is the legitimate
    // path for a client retry crossing a successful response). Real
    // dedup is enforced by the unique partial index on
    // `sync_actions.client_tx_id` — this fast-path just avoids the
    // round-trip when the duplicate is obvious.
    if let Some(existing_sync_id) = lookup_existing(conn, &tx.tx_id) {
        return Ok(existing_sync_id);
    }

    match tx.aggregate {
        SyncAggregate::Project => apply_project(conn, tx, actor),
        SyncAggregate::Ticket => apply_ticket(conn, tx, actor),
        SyncAggregate::ProjectTicket
        | SyncAggregate::WorkflowState
        | SyncAggregate::Comment
        | SyncAggregate::Attachment
        | SyncAggregate::Assignment
        | SyncAggregate::GroupMembership
        | SyncAggregate::Plugin
        | SyncAggregate::Cycle
        | SyncAggregate::CycleTicket => Err(TxReject(
            "unsupported_aggregate",
            format!(
                "push for aggregate `{}` is not yet wired",
                tx.aggregate.as_str()
            ),
        )),
    }
}

fn apply_ticket(
    conn: &mut DbConnection,
    tx: &PushTransaction,
    actor: &ActorContext,
) -> Result<i64, TxReject> {
    let ticket_id: i32 = tx
        .model_id
        .parse()
        .map_err(|_| TxReject("invalid_model_id", format!("expected i32, got {}", tx.model_id)))?;

    match tx.op {
        SyncOp::Update => {
            // Decode the patch through TicketUpdate so unknown
            // fields fail at the boundary instead of getting
            // silently dropped by Diesel.
            let patch = decode_ticket_patch(&tx.patch)?;
            run_with_actor(conn, actor, |conn| {
                crate::repository::tickets::update_ticket_partial(conn, ticket_id, patch, None)?;
                latest_sync_id(conn)
            })
            .map_err(reject_diesel)
        }
        SyncOp::Insert => Err(TxReject(
            "use_rest_endpoint",
            "ticket creation goes through POST /api/tickets, not /api/sync/push (until the bootstrap-time-of-creation flow lands in Phase 5)".into(),
        )),
        SyncOp::Delete => Err(TxReject(
            "unsupported_op",
            "ticket deletion goes through DELETE /api/tickets/{id} (heavy cleanup pipeline that doesn't fit in a sync push)".into(),
        )),
        SyncOp::Archive => Err(TxReject(
            "unsupported_op",
            "tickets don't support soft-archive yet".into(),
        )),
    }
}

fn decode_ticket_patch(value: &Value) -> Result<TicketUpdate, TxReject> {
    serde_json::from_value(value.clone()).map_err(|e| {
        TxReject(
            "invalid_patch",
            format!("ticket patch failed to deserialise: {e}"),
        )
    })
}

fn lookup_existing(conn: &mut DbConnection, tx_id: &str) -> Option<i64> {
    use crate::schema::sync_actions::dsl;
    dsl::sync_actions
        .filter(dsl::client_tx_id.eq(tx_id))
        .select(dsl::sync_id)
        .first::<i64>(conn)
        .ok()
}

fn apply_project(
    conn: &mut DbConnection,
    tx: &PushTransaction,
    actor: &ActorContext,
) -> Result<i64, TxReject> {
    let project_id: i32 = tx
        .model_id
        .parse()
        .map_err(|_| TxReject("invalid_model_id", format!("expected i32, got {}", tx.model_id)))?;

    match tx.op {
        SyncOp::Update => {
            let patch = decode_project_patch(&tx.patch)?;
            run_with_actor(conn, actor, |conn| {
                let _: Project = crate::repository::projects::update_project(conn, project_id, patch)?;
                latest_sync_id(conn)
            })
            .map_err(reject_diesel)
        }
        SyncOp::Delete => run_with_actor(conn, actor, |conn| {
            let _ = crate::repository::projects::delete_project(conn, project_id)?;
            latest_sync_id(conn)
        })
        .map_err(reject_diesel),
        SyncOp::Insert => Err(TxReject(
            "use_rest_endpoint",
            "project creation goes through POST /api/projects, not /api/sync/push (until the bootstrap-time-of-creation flow lands)".into(),
        )),
        SyncOp::Archive => Err(TxReject(
            "unsupported_op",
            "projects don't support soft-archive yet".into(),
        )),
    }
}

fn decode_project_patch(value: &Value) -> Result<ProjectUpdate, TxReject> {
    serde_json::from_value(value.clone()).map_err(|e| {
        TxReject(
            "invalid_patch",
            format!("project patch failed to deserialise: {e}"),
        )
    })
}

fn run_with_actor<T>(
    conn: &mut DbConnection,
    actor: &ActorContext,
    f: impl FnOnce(&mut DbConnection) -> diesel::QueryResult<T>,
) -> diesel::QueryResult<T> {
    conn.transaction(|conn| {
        session::set_actor(conn, actor)?;
        f(conn)
    })
}

/// Latest sync_id after a successful write. Reads from the same
/// transaction so the value is the row this push just inserted.
fn latest_sync_id(conn: &mut DbConnection) -> diesel::QueryResult<i64> {
    use crate::schema::sync_actions::dsl;
    dsl::sync_actions
        .select(diesel::dsl::max(dsl::sync_id))
        .first::<Option<i64>>(conn)
        .map(|v| v.unwrap_or(0))
}

fn reject_diesel(err: diesel::result::Error) -> TxReject {
    use diesel::result::Error;
    match err {
        Error::NotFound => TxReject("not_found", "model_id does not exist".into()),
        other => TxReject("internal", other.to_string()),
    }
}

// Suppress "unused import" if Uuid isn't otherwise referenced in this
// module after the compiler optimises everything down. Keep the
// import live because future op handlers (Insert paths) will need it
// for assignee_uuid / requester_uuid coercions.
#[allow(dead_code)]
fn _uuid_keepalive() -> Option<Uuid> {
    None
}
