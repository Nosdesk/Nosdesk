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
use crate::middleware::request_context::RequestContext;
use crate::models::WorkspaceRole;
use crate::repository::ticket_merge::{self, ExpectedState, MergeError, MergeInput};
use crate::services::search::{indexing_tasks, SearchService};
use crate::utils::rbac::require_workspace_role;

/// `POST /api/tickets/merge` request body.
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
    /// Agent-edited merge-marker comment body from the dialog.
    #[serde(default)]
    pub marker_body: Option<String>,
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
    let notify_customer = body.notify_customer;
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
        marker_body: body.marker_body,
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
        indexing_tasks::spawn_index_ticket(search_service.get_ref().clone(), source.clone(), None);
    }

    // The merge reaches open viewers entirely through the sync pool:
    // each source ticket's `ticket.merged_into` emit flips its
    // merged-into banner + read-only composer (and carries the new
    // workflow state), and the merge-marker comment lands on the
    // destination's timeline. No discrete SSE broadcast.

    // Step 15: customer notification, opt-in and best-effort. Runs in
    // its own transaction so a send-queue hiccup never unwinds the
    // committed merge.
    if notify_customer {
        if let Err(e) = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
            ticket_merge::enqueue_merge_notifications(
                c,
                &outcome.destination,
                &outcome.merged_sources,
            )
        }) {
            tracing::warn!(error = %e, "merge customer notification enqueue failed");
        }
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
        MergeError::StateConflict(_) => errors::conflict_with_code(message, "MERGE_STATE_CONFLICT"),
        MergeError::MissingWorkspace | MergeError::MergedStateMissing => errors::internal(message),
        // Every remaining variant is a pre-flight validation failure.
        _ => errors::bad_request_with_code(message, "MERGE_VALIDATION"),
    }
}
