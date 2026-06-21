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

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use diesel::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::extractors::SyncContext;
use crate::handlers::{errors, helpers};
use crate::middleware::RequestContext;
use crate::models::{Project, ProjectUpdate, SyncAggregate, SyncOp, TicketUpdate};
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
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<Vec<PushTransaction>>,
    ctx: SyncContext,
) -> impl Responder {
    let body = body.into_inner();
    if body.len() > MAX_BATCH {
        return errors::bad_request(format!("Batch exceeds the {MAX_BATCH}-transaction limit"));
    }

    // Pull the workspace pin from RequestContext (populated by the
    // auth middleware after WorkspaceContextMiddleware resolves the
    // tenant). Required post-RLS: sync_actions inserts now go
    // through the workspace-isolation policy, so every push tx
    // needs `app.workspace_id` set on the same tx the emit happens
    // in. None is only ever the case for pre-workspace-middleware
    // requests (which the auth chain rejects upstream anyway).
    let workspace_id = req
        .extensions()
        .get::<RequestContext>()
        .and_then(|rc| rc.actor.workspace_id);

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Pin the request's workspace so the per-transaction idempotency
    // short-circuit (`lookup_existing` reads the RLS-isolated sync_actions)
    // is visible; the per-write actor context below re-pins it transactionally.
    // Without this the fast-path never fires and a client retry collides with
    // the unique index instead of being recognised as already applied.
    if let Some(ws) = workspace_id {
        helpers::pin_workspace(&mut conn, ws);
    }

    let mut applied: Vec<String> = Vec::with_capacity(body.len());
    let mut rejected: Vec<RejectedTx> = Vec::new();
    let mut last_sync_id: i64 = 0;

    for tx in body {
        let tx_id = tx.tx_id.clone();
        let actor = ActorContext {
            kind: ActorKind::User,
            uuid: Some(ctx.user.uuid),
            reference: None,
            correlation_id: Some(ctx.correlation_id),
            client_tx_id: Some(tx_id.clone()),
            // Phase 3c.2 (wave 2): pinned from the RequestContext
            // populated by the auth middleware, so emit::record
            // writes sync_actions with `app.workspace_id` set and
            // satisfies the workspace-isolation RLS policy.
            workspace_id,
        };

        match apply_transaction(&mut conn, &tx, &actor) {
            Ok(sync_id) => {
                last_sync_id = sync_id.max(last_sync_id);
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

    // Broadcast happens elsewhere: every committed sync_actions row
    // fires the `sync_actions_notify_trigger` Postgres trigger
    // post-commit, which the `services::sync_outbox` listener
    // picks up and broadcasts as `SseEvent::SyncActions`. The
    // listener path covers HTTP push, channel-pipeline ingest,
    // background jobs, and any future write site uniformly — no
    // per-handler SSE plumbing required.

    HttpResponse::Ok().json(PushResponse {
        applied,
        rejected,
        last_sync_id,
    })
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
        | SyncAggregate::CycleTicket
        | SyncAggregate::User
        | SyncAggregate::Asset
        | SyncAggregate::AssetMedia
        | SyncAggregate::AssetLifecycleEvent
        | SyncAggregate::Webhook
        | SyncAggregate::Channel
        | SyncAggregate::KnowledgeGap
        | SyncAggregate::DocumentationPage
        | SyncAggregate::DocumentationCollection
        | SyncAggregate::Data
        | SyncAggregate::Notification
        | SyncAggregate::TicketAsset
        | SyncAggregate::LinkedTicket
        | SyncAggregate::AssetUsage
        | SyncAggregate::AssetAudit
        | SyncAggregate::AssetLoan => Err(TxReject(
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
    let ticket_id: i32 = tx.model_id.parse().map_err(|_| {
        TxReject(
            "invalid_model_id",
            format!("expected i32, got {}", tx.model_id),
        )
    })?;

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
    let project_id: i32 = tx.model_id.parse().map_err(|_| {
        TxReject(
            "invalid_model_id",
            format!("expected i32, got {}", tx.model_id),
        )
    })?;

    match tx.op {
        SyncOp::Update => {
            let patch = decode_project_patch(&tx.patch)?;
            run_with_actor(conn, actor, |conn| {
                let _: Project =
                    crate::repository::projects::update_project(conn, project_id, patch, None)?;
                latest_sync_id(conn)
            })
            .map_err(reject_diesel)
        }
        SyncOp::Delete => run_with_actor(conn, actor, |conn| {
            let _ = crate::repository::projects::delete_project(conn, project_id, None)?;
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
