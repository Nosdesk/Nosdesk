//! HTTP handlers for ticket merge.
//!
//! Thin wrappers over `repository::ticket_merge`: resolve the actor +
//! workspace from the request, gate on the workspace Agent role, call
//! the repository, and map `MergeError` onto the structured
//! `{ error, code }` envelope. The repository owns the transaction and
//! the lifecycle order.

use std::sync::Arc;

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::db::Pool;
use crate::handlers::errors;
use crate::handlers::sse::{SseEvent, SseState};
use crate::middleware::request_context::RequestContext;
use crate::models::WorkspaceRole;
use crate::repository::ticket_merge::{self, ExpectedState, MergeError, MergeInput};
use crate::services::search::{indexing_tasks, SearchService};
use crate::utils::rbac::require_workspace_role;

/// `POST /api/tickets/merge` request body. Mirrors
/// `docs/ticket-merge-plan.md` section 7.1.
#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub destination_ticket_id: i32,
    pub source_ticket_ids: Vec<i32>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub notify_customer: bool,
    #[serde(default)]
    pub expected_state: Vec<ExpectedStateDto>,
}

#[derive(Debug, Deserialize)]
pub struct ExpectedStateDto {
    pub ticket_id: i32,
    pub workflow_state_id: i32,
}

/// Resolve the workspace-pinned actor the auth middleware attached.
fn actor_from(req: &HttpRequest) -> Option<crate::sync::actor::ActorContext> {
    req.extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.clone())
}

/// `POST /api/tickets/merge`. Agent role or higher.
pub async fn merge_tickets(
    req: HttpRequest,
    body: web::Json<MergeRequest>,
    pool: web::Data<Pool>,
    sse_state: web::Data<SseState>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }
    let actor = match actor_from(&req) {
        Some(a) => a,
        None => return errors::unauthorized("Authentication required"),
    };
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let body = body.into_inner();
    let input = MergeInput {
        destination_ticket_id: body.destination_ticket_id,
        source_ticket_ids: body.source_ticket_ids,
        reason: body.reason,
        notify_customer: body.notify_customer,
        expected_state: body
            .expected_state
            .into_iter()
            .map(|e| ExpectedState {
                ticket_id: e.ticket_id,
                workflow_state_id: e.workflow_state_id,
            })
            .collect(),
    };

    let outcome = match ticket_merge::execute_merge(&mut conn, input, &actor) {
        Ok(o) => o,
        Err(e) => return map_merge_error(e),
    };

    // Post-commit, best-effort. None of this rolls back the merge.
    // Reindex the now-merged sources so their search snippet reflects
    // the terminal state. The destination's ticket fields are unchanged,
    // so it doesn't need reindexing (and reindexing it with no article
    // body would drop its article from the index).
    for source in &outcome.merged_sources {
        indexing_tasks::spawn_index_ticket(
            search_service.get_ref().clone(),
            source.clone(),
            None,
        );
    }

    // Broadcast so open viewers react without a reload: the destination
    // refetches, each source shows the merged-into banner.
    let actor_uuid = actor.uuid.map(|u| u.to_string()).unwrap_or_default();
    let now = chrono::Utc::now();
    let source_ids: Vec<i32> = outcome.merged_sources.iter().map(|t| t.id).collect();
    sse_state
        .broadcast_event(SseEvent::TicketMerged {
            target_ticket_id: outcome.destination.id,
            source_ticket_ids: source_ids,
            actor_uuid: actor_uuid.clone(),
            merge_event_id: outcome.merge_event_id,
            timestamp: now,
        })
        .await;
    for source in &outcome.merged_sources {
        sse_state
            .broadcast_event(SseEvent::TicketUpdated {
                ticket_id: source.id,
                field: "workflow_state_id".to_string(),
                value: serde_json::json!(source.workflow_state_id),
                updated_by: actor_uuid.clone(),
                timestamp: now,
            })
            .await;
    }

    HttpResponse::Ok().json(serde_json::json!({
        "merge_event_id": outcome.merge_event_id,
        "destination_ticket": outcome.destination,
        "merged_sources": outcome.merged_sources,
        "comments_moved": outcome.comments_moved,
        "channel_messages_rerouted": outcome.channel_messages_rerouted,
        "watchers_added_to_destination": outcome.watchers_added_to_destination,
        "merge_marker_comment_id": outcome.merge_marker_comment_id,
    }))
}

/// `GET /api/tickets/{id}/merge-history`. Agent role or higher.
pub async fn get_merge_history(
    req: HttpRequest,
    path: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }
    let actor = match actor_from(&req) {
        Some(a) => a,
        None => return errors::unauthorized("Authentication required"),
    };
    let ticket_id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // Read under the actor context so RLS scopes the query to the
    // caller's workspace.
    let result = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        ticket_merge::merge_history_for_ticket(c, ticket_id)
    });
    match result {
        Ok(history) => HttpResponse::Ok().json(history),
        Err(e) => errors::db_error(&e),
    }
}

/// Map a `MergeError` to the structured `{ error, code }` response.
fn map_merge_error(err: MergeError) -> HttpResponse {
    let message = err.to_string();
    match err {
        MergeError::Db(e) => errors::db_error(&e),
        MergeError::NotFound(_) => errors::not_found_msg(message),
        MergeError::StateConflict(_) => {
            errors::conflict_with_code(message, "MERGE_STATE_CONFLICT")
        }
        MergeError::MissingWorkspace | MergeError::MergedStateMissing => errors::internal(message),
        // Every remaining variant is a pre-flight validation failure.
        _ => errors::bad_request_with_code(message, "MERGE_VALIDATION"),
    }
}
