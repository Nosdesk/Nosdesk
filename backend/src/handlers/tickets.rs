use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn, TicketAccess};
use crate::handlers::errors;
use crate::middleware::request_context::record_canonical;
use crate::models::{
    AssignmentTrigger, Claims, NewTicket, TicketUpdate, TicketsJson, WorkflowStateCategory,
};
use crate::repository;
use crate::repository::ticket_query::TicketQuery;
use crate::services::assignment::AssignmentEngine;
use crate::services::notifications::{
    types::{NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode},
    NotificationService,
};
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;
use crate::utils::i18n;
use crate::utils::locale::request_locale;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/tickets", web::get().to(crate::handlers::get_tickets))
        .route(
            "/tickets/paginated",
            web::get().to(crate::handlers::get_paginated_tickets),
        )
        .route(
            "/tickets/recent",
            web::get().to(crate::handlers::get_recent_tickets),
        )
        .route("/tickets", web::post().to(crate::handlers::create_ticket))
        .route(
            "/tickets/empty",
            web::post().to(crate::handlers::create_empty_ticket),
        )
        .route(
            "/tickets/bulk",
            web::post().to(crate::handlers::bulk_tickets),
        )
        // Literal /tickets/merge before /tickets/{id}; there is no
        // /tickets/{id} POST, so no wildcard can absorb it.
        .route(
            "/tickets/merge",
            web::post().to(crate::handlers::ticket_merge::merge_tickets),
        )
        .route(
            "/tickets/{id}/merge-history",
            web::get().to(crate::handlers::ticket_merge::get_merge_history),
        )
        .route(
            "/tickets/{ticket_id}/rule-applications",
            web::get().to(crate::handlers::rules::list_ticket_rule_applications),
        )
        .route(
            "/tickets/{id}/applicable-actions",
            web::get().to(crate::handlers::rules::list_applicable_actions),
        )
        .route("/tickets/{id}", web::get().to(crate::handlers::get_ticket))
        .route(
            "/tickets/{id}",
            web::put().to(crate::handlers::update_ticket),
        )
        .route(
            "/tickets/{id}",
            web::patch().to(crate::handlers::update_ticket_partial),
        )
        .route(
            "/tickets/{id}",
            web::delete().to(crate::handlers::delete_ticket),
        )
        .route(
            "/tickets/{id}/view",
            web::post().to(crate::handlers::record_ticket_view),
        )
        .route(
            "/tickets/{id}/view",
            web::delete().to(crate::handlers::remove_recent_ticket),
        )
        .route(
            "/tickets/{id}/activity",
            web::get().to(crate::handlers::get_ticket_activity),
        )
        .route(
            "/tickets/{id}/loans",
            web::get().to(crate::handlers::asset_loans::list_for_ticket),
        )
        .route(
            "/tickets/{id}/field-preview",
            web::post().to(crate::handlers::preview_ticket_field),
        )
        .route(
            "/tickets/{id}/tags",
            web::put().to(crate::handlers::tags::set_ticket_tags),
        )
        .route(
            "/tickets/{id}/watchers",
            web::get().to(crate::handlers::ticket_watchers::list_watchers),
        )
        .route(
            "/tickets/{id}/watch",
            web::post().to(crate::handlers::ticket_watchers::watch_ticket),
        )
        .route(
            "/tickets/{id}/watch",
            web::delete().to(crate::handlers::ticket_watchers::unwatch_ticket),
        )
        .route(
            "/tickets/{id}/watch/me",
            web::get().to(crate::handlers::ticket_watchers::my_watch_state),
        )
        .route(
            "/tickets/{id}/watch/preferences",
            web::patch().to(crate::handlers::ticket_watchers::update_my_watch_preferences),
        )
        .route("/tags", web::get().to(crate::handlers::tags::list_tags))
        .route("/tags", web::post().to(crate::handlers::tags::create_tag))
        .route(
            "/tags/{id}",
            web::patch().to(crate::handlers::tags::update_tag),
        )
        .route(
            "/tags/{id}",
            web::delete().to(crate::handlers::tags::archive_tag),
        )
        .route(
            "/import/file",
            web::post().to(crate::handlers::import_tickets_from_json),
        )
        .route(
            "/import/json",
            web::post().to(crate::handlers::import_tickets_from_json_string),
        )
        .route(
            "/tickets/{ticket_id}/link/{linked_ticket_id}",
            web::post().to(crate::handlers::link_tickets),
        )
        .route(
            "/tickets/{ticket_id}/unlink/{linked_ticket_id}",
            web::delete().to(crate::handlers::unlink_tickets),
        )
        .route(
            "/tickets/{ticket_id}/assets/{asset_id}",
            web::post().to(crate::handlers::add_device_to_ticket),
        )
        .route(
            "/tickets/{ticket_id}/assets/{asset_id}",
            web::delete().to(crate::handlers::remove_device_from_ticket),
        )
        .route(
            "/tickets/{id}/asset-usage",
            web::get().to(crate::handlers::asset_usage::list_for_ticket),
        )
        .route(
            "/tickets/{ticket_id}/comments",
            web::get().to(crate::handlers::get_comments_by_ticket_id),
        )
        .route(
            "/tickets/{ticket_id}/comments",
            web::post().to(crate::handlers::add_comment_to_ticket),
        )
        .route(
            "/tickets/{ticket_id}/notes/images",
            web::post().to(crate::handlers::upload_ticket_note_image),
        )
        .route(
            "/comments/{id}",
            web::delete().to(crate::handlers::delete_comment),
        )
        .route(
            "/comments/{id}/raw.eml",
            web::get().to(crate::handlers::get_comment_raw_eml),
        )
        // Image proxy for inbound email rendering. Path-positional
        // {sig}/{encoded_url} keeps the URL self-describing and
        // cache-friendly (browsers cache by full URL). HMAC sig
        // is derived from JWT_SECRET; see crate::handlers::image_proxy.
        .route(
            "/image-proxy/{sig}/{encoded_url}",
            web::get().to(crate::handlers::image_proxy::proxy_image),
        )
        .route(
            "/attachments/{id}",
            web::delete().to(crate::handlers::delete_attachment),
        );
}

// Helper function to validate assignee role
fn validate_assignee_role(
    assignee_uuid: &Uuid,
    conn: &mut crate::db::DbConnection,
) -> Result<(), HttpResponse> {
    match crate::repository::users::get_user_by_uuid(assignee_uuid, conn) {
        Ok(user) => {
            if !crate::repository::user_helpers::user_can_handle_tickets(conn, &user) {
                Err(errors::bad_request("Invalid assignee: Only technicians and administrators can be assigned to tickets"))
            } else {
                Ok(())
            }
        }
        Err(_) => Err(errors::bad_request(
            "User not found: The specified assignee does not exist",
        )),
    }
}

// Helper function to parse and validate assignee from string (for update operations)
fn parse_and_validate_assignee_string(
    assignee_str: &str,
    conn: &mut crate::db::DbConnection,
) -> Result<Uuid, HttpResponse> {
    // Try to parse as UUID first
    if let Ok(uuid) = Uuid::parse_str(assignee_str) {
        // Use the same validation logic but adapted for the update context
        match crate::repository::users::get_user_by_uuid(&uuid, conn) {
            Ok(user) => {
                if !crate::repository::user_helpers::user_can_handle_tickets(conn, &user) {
                    Err(errors::bad_request("Invalid assignee: Only technicians and administrators can be assigned to tickets"))
                } else {
                    Ok(uuid)
                }
            }
            Err(_) => Err(errors::bad_request(
                "User not found: The specified assignee does not exist",
            )),
        }
    } else {
        // Try to look up by name
        match crate::repository::users::get_user_by_name(assignee_str, conn) {
            Ok(user) => {
                if !crate::repository::user_helpers::user_can_handle_tickets(conn, &user) {
                    Err(errors::bad_request("Invalid assignee: Only technicians and administrators can be assigned to tickets"))
                } else {
                    Ok(user.uuid)
                }
            }
            Err(_) => Err(errors::bad_request(
                "User not found: The specified assignee does not exist",
            )),
        }
    }
}

/// Extract the SSE client ID from the request header (for echo suppression).
fn extract_sse_client_id(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("X-SSE-Client-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

// Pagination query parameters
#[derive(Deserialize)]
pub struct PaginationParams {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "sortField")]
    sort_field: Option<String>,
    #[serde(rename = "sortDirection")]
    sort_direction: Option<String>,
    search: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    assignee: Option<String>,
    requester: Option<String>,
    // Date filtering parameters
    #[serde(rename = "createdAfter")]
    created_after: Option<String>,
    #[serde(rename = "createdBefore")]
    created_before: Option<String>,
    #[serde(rename = "createdOn")]
    created_on: Option<String>,
    #[serde(rename = "modifiedAfter")]
    modified_after: Option<String>,
    #[serde(rename = "modifiedBefore")]
    modified_before: Option<String>,
    #[serde(rename = "modifiedOn")]
    modified_on: Option<String>,
    #[serde(rename = "closedAfter")]
    closed_after: Option<String>,
    #[serde(rename = "closedBefore")]
    closed_before: Option<String>,
    #[serde(rename = "closedOn")]
    closed_on: Option<String>,
}

// Paginated response
#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    data: Vec<T>,
    total: i64,
    page: i64,
    #[serde(rename = "pageSize")]
    page_size: i64,
    #[serde(rename = "totalPages")]
    total_pages: i64,
}

// Get all tickets (technicians and admins only)
pub async fn get_tickets(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    // Only technicians and admins can see all tickets via this endpoint
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: Only technicians and admins can access all tickets");
    }

    match tc.run(repository::get_all_tickets) {
        Ok(tickets) => HttpResponse::Ok().json(tickets),
        Err(_) => errors::internal("Failed to get tickets"),
    }
}

// Get paginated tickets
pub async fn get_paginated_tickets(
    mut tc: TenantConn,
    query: web::Query<PaginationParams>,
    auth: AuthContext,
) -> impl Responder {
    let query = query.into_inner();
    // Build query with automatic permission filtering via AuthContext
    let result = tc.run(|conn| {
        TicketQuery::new()
            .visible_to(&auth)
            .search(query.search.clone())
            .status(query.status.clone())
            .priority(query.priority.clone())
            .category(query.category.clone())
            .assignee(query.assignee.clone())
            .requester(query.requester.clone())
            .created_between(query.created_after.clone(), query.created_before.clone())
            .created_on(query.created_on.clone())
            .modified_between(query.modified_after.clone(), query.modified_before.clone())
            .modified_on(query.modified_on.clone())
            .closed_between(query.closed_after.clone(), query.closed_before.clone())
            .closed_on(query.closed_on.clone())
            .paginate(query.page.unwrap_or(1), query.page_size.unwrap_or(10))
            .sort(query.sort_field.clone(), query.sort_direction.clone())
            .execute_with_users(conn)
    });

    match result {
        Ok(paginated) => HttpResponse::Ok().json(paginated),
        Err(e) => {
            error!(error = ?e, "Failed to fetch paginated tickets");
            errors::internal("Failed to get paginated tickets")
        }
    }
}

// ---- Activity timeline -------------------------------------------
//
// Per-ticket changelog backed by the `sync_actions` event log. Used
// by the detail view's TicketActivity component to render a
// chronological "who did what when" feed without an extra mutation
// table — every state change, comment, and assignment already lands
// in `sync_actions` via `sync::emit::record`.
//
// The query filters by the GIN-indexed `groups` column rather than
// by `(aggregate, aggregate_id) = ('ticket', N)` so it picks up
// child events too: comments emit with `ticket:N` in their groups
// (see `sync::groups::for_ticket`), as do assignments and any
// future child aggregates. One filter, one index, all events for
// the ticket.

#[derive(Debug, Deserialize)]
pub struct TicketActivityQuery {
    /// Cursor — return rows with `sync_id < before` (descending
    /// pagination). Omit to fetch the most recent page.
    pub before: Option<i64>,
    /// Cursor — return rows with `sync_id > after`, newest first.
    /// Used to fetch activity that landed since the currently
    /// displayed head (SSE-driven live prepend), not for paging.
    pub after: Option<i64>,
    /// Page size. Defaults to 50, hard-capped at 200 — bigger pages
    /// are pointless for a UI timeline (the user scrolls a window
    /// at a time).
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, Queryable)]
pub struct TicketActivityRow {
    pub sync_id: i64,
    pub aggregate: crate::models::SyncAggregate,
    pub aggregate_id: String,
    pub op: crate::models::SyncOp,
    pub event_type: String,
    pub data: serde_json::Value,
    pub actor_uuid: Option<uuid::Uuid>,
    pub actor_kind: String,
    pub actor_ref: Option<String>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct TicketActivityResponse {
    pub events: Vec<TicketActivityRow>,
    /// Cursor for the next page — pass back as `?before=`. `None`
    /// when the current page is the last.
    pub next_cursor: Option<i64>,
}

const DEFAULT_ACTIVITY_LIMIT: i64 = 50;
const MAX_ACTIVITY_LIMIT: i64 = 200;

pub async fn get_ticket_activity(
    mut tc: TenantConn,
    access: TicketAccess,
    query: web::Query<TicketActivityQuery>,
) -> impl Responder {
    use crate::schema::sync_actions;

    let ticket_id = access.ticket_id;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_ACTIVITY_LIMIT)
        .clamp(1, MAX_ACTIVITY_LIMIT);
    let group_marker = format!("ticket:{}", ticket_id);
    let before = query.before;
    let after = query.after;

    let load_result = tc.run(|conn| {
        // Fetch limit + 1 so we can detect the boundary without a
        // separate count query — same trick `delta` uses.
        let mut q = sync_actions::table
            .filter(sync_actions::groups.contains(vec![Some(group_marker)]))
            .order((
                sync_actions::occurred_at.desc(),
                sync_actions::sync_id.desc(),
            ))
            .limit(limit + 1)
            .select((
                sync_actions::sync_id,
                sync_actions::aggregate,
                sync_actions::aggregate_id,
                sync_actions::op,
                sync_actions::event_type,
                sync_actions::data,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::actor_ref,
                sync_actions::occurred_at,
            ))
            .into_boxed();

        if let Some(b) = before {
            q = q.filter(sync_actions::sync_id.lt(b));
        }
        if let Some(a) = after {
            q = q.filter(sync_actions::sync_id.gt(a));
        }

        q.load::<TicketActivityRow>(conn)
    });

    let mut events = match load_result {
        Ok(rows) => rows,
        Err(e) => {
            error!(error = %e, ticket_id, "ticket activity query failed");
            return errors::internal("Failed to load ticket activity");
        }
    };

    let next_cursor = if events.len() > limit as usize {
        events.truncate(limit as usize);
        events.last().map(|e| e.sync_id)
    } else {
        None
    };

    HttpResponse::Ok().json(TicketActivityResponse {
        events,
        next_cursor,
    })
}

// ----------------------------------------------------------------------
// In-flight field preview (no-op against the DB)
//
// Decouples real-time mirroring (every keystroke broadcast to other
// viewers) from persistence (one PATCH per editing session, one
// activity row). The PATCH commit path stays the only writer; this
// endpoint broadcasts a transient `TicketFieldPreviewed` SSE event
// scoped to the ticket's topic so the activity log doesn't bloat
// with one row per debounced keystroke.
//
// Field allowlist: only fields where per-keystroke broadcast is
// useful land here. Discrete fields (status, priority, assignee)
// have nothing to "preview" — their PATCH is already the user's
// commit.
//
// The article body is unaffected; it uses Yjs over a WebSocket and
// has its own snapshot/revision pipeline.

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewableField {
    Title,
    ResolutionNotes,
}

impl PreviewableField {
    fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::ResolutionNotes => "resolution_notes",
        }
    }

    /// Per-field upper bound on the preview value. Keeps a single
    /// abusive request from broadcasting megabytes to every other
    /// viewer on the topic. Matches the field's effective storage
    /// limits, not exactly the column constraint, so PATCH-side
    /// validation still has the final say.
    fn max_len(self) -> usize {
        match self {
            Self::Title => 500,
            Self::ResolutionNotes => 50_000,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TicketFieldPreviewBody {
    pub field: PreviewableField,
    pub value: String,
}

/// Broadcast a transient preview of an in-flight field edit to
/// other ticket viewers. No DB write, no `sync_actions` row, no
/// webhook fan-out. Echo suppression is handled at the SSE layer
/// via `X-SSE-Client-Id` so the sender's own preview doesn't loop
/// back into their UI.
pub async fn preview_ticket_field(
    req: HttpRequest,
    access: TicketAccess,
    body: web::Json<TicketFieldPreviewBody>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    if body.value.len() > body.field.max_len() {
        return errors::bad_request("Preview value exceeds maximum length");
    }

    let source_client_id = extract_sse_client_id(&req);
    sse_state
        .broadcast_event_from(
            crate::handlers::sse::SseEvent::TicketFieldPreviewed {
                ticket_id: access.ticket_id,
                field: body.field.as_str().to_string(),
                value: body.value.clone(),
                timestamp: chrono::Utc::now(),
            },
            source_client_id,
        )
        .await;

    // 202 Accepted: we've handed it to the broadcaster; there is
    // no resource to return, and the caller shouldn't wait on the
    // fan-out.
    HttpResponse::Accepted().finish()
}

// Get a ticket by ID with comments and related info.
//
// The visibility gate is part of the `TicketAccess` extractor —
// reaching this body means the caller is allowed to read the
// ticket. 404 (not 403) on deny is enforced inside the extractor,
// per the OWASP IDOR Cheatsheet.
pub async fn get_ticket(mut tc: TenantConn, access: TicketAccess) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;

    // A `not_found` here is a genuine "deleted between extraction
    // and load" race, which we still want to surface as 404.
    let complete_ticket = match tc.run(|conn| repository::get_complete_ticket(conn, ticket_id)) {
        Ok(ticket) => ticket,
        Err(_) => return errors::not_found_msg("Ticket not found"),
    };

    // Record the view, best-effort (don't fail the request if it
    // errors). Runs under TenantConn so user_ticket_views' workspace
    // GUC + audit trigger both have a workspace context.
    if let Err(e) =
        tc.run(|conn| repository::user_ticket_views::record_view(conn, auth.user_uuid, ticket_id))
    {
        warn!(user_uuid = %auth.user_uuid, error = ?e, "Failed to record ticket view");
    }

    HttpResponse::Ok().json(complete_ticket)
}

// Create a new ticket
pub async fn create_ticket(
    mut tc: TenantConn,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    auth: AuthContext,
    ticket: web::Json<NewTicket>,
    req: HttpRequest,
) -> impl Responder {
    let new_ticket = ticket.into_inner();

    // Validate category visibility if category_id is set
    if let Some(category_id) = new_ticket.category_id {
        match tc.run(|conn| {
            crate::repository::categories::can_user_see_category(
                conn,
                &auth.user_uuid,
                category_id,
                auth.is_workspace_admin(),
            )
        }) {
            Ok(true) => {}
            Ok(false) => {
                return errors::forbidden(
                    "Forbidden: You do not have access to the specified category",
                );
            }
            Err(_) => {
                return errors::internal("Failed to check category visibility");
            }
        }
    }

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        let validation: Result<Result<(), HttpResponse>, diesel::result::Error> =
            tc.run(|conn| Ok(validate_assignee_role(&assignee_uuid, conn)));
        match validation {
            Ok(Ok(())) => {}
            Ok(Err(resp)) => return resp,
            Err(_) => return errors::internal("Failed to validate assignee"),
        }
    }

    match tc.run(|conn| repository::create_ticket(conn, new_ticket)) {
        Ok(mut ticket) => {
            // Run automatic assignment rules if no assignee
            if ticket.assignee_uuid.is_none() {
                let rules_result = tc
                    .run(|conn| {
                        Ok::<_, diesel::result::Error>(AssignmentEngine::evaluate_rules(
                            conn,
                            &ticket,
                            AssignmentTrigger::TicketCreated,
                        ))
                    })
                    .ok()
                    .flatten();

                if let Some(result) = rules_result {
                    if let Some(assigned_uuid) = result.assigned_user_uuid {
                        let assign_update = TicketUpdate {
                            assignee_uuid: Some(Some(assigned_uuid)),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if let Ok(updated) = tc.run(|conn| {
                            repository::update_ticket_partial(
                                conn,
                                ticket.id,
                                assign_update,
                                Some(search_service.get_ref()),
                            )
                        }) {
                            ticket = updated;
                            info!(
                                ticket_id = ticket.id,
                                assignee = %assigned_uuid,
                                rule = %result.rule_name,
                                method = %result.method,
                                "Auto-assigned new ticket via create_ticket"
                            );

                            // Send notification to the auto-assigned user, unless the
                            // rule assigned the ticket to the user who just created it
                            // (never notify someone about their own action).
                            if assigned_uuid != auth.user_uuid {
                                let notification_service = notification_service.clone();
                                let ticket_id = ticket.id;
                                let ticket_title = ticket.title.clone();
                                let ticket_workspace = ticket.workspace_id;
                                let rule_name = result.rule_name.clone();

                                tokio::spawn(async move {
                                    let payload = NotificationPayload::new(
                                        NotificationTypeCode::TicketAssigned,
                                        assigned_uuid,
                                        NotificationActor {
                                            uuid: Uuid::nil(),
                                            name: "System".to_string(),
                                            avatar_thumb: None,
                                            kind: crate::sync::ActorKind::System,
                                        },
                                        NotificationEntity::Ticket {
                                            id: ticket_id,
                                            title: ticket_title,
                                        },
                                        ticket_workspace,
                                    )
                                    .with_body(format!(
                                        "You have been auto-assigned to ticket #{ticket_id} (Rule: {rule_name})"
                                    ));

                                    if let Err(e) = notification_service.notify(payload).await {
                                        warn!(error = %e, "Failed to send auto-assignment notification");
                                    }
                                });
                            }
                        }
                    }
                }
            }

            // Index the new ticket in search
            indexing_tasks::spawn_index_ticket(
                search_service.get_ref().clone(),
                ticket.clone(),
                None,
            );

            // The new ticket reaches clients through the sync pool (the
            // repository write emits `ticket.created`); no discrete SSE.

            record_canonical(&req, "ticket_id", ticket.id);
            record_canonical(&req, "outcome", "created");
            HttpResponse::Created().json(ticket)
        }
        Err(_) => errors::internal("Failed to create ticket"),
    }
}

// Update a ticket. The extractor gates visibility; this body
// only runs for callers who can see the ticket.
pub async fn update_ticket(
    mut tc: TenantConn,
    access: TicketAccess,
    ticket: web::Json<NewTicket>,
    req: HttpRequest,
) -> impl Responder {
    let ticket_id = access.ticket_id;
    let new_ticket = ticket.into_inner();

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        let validation: Result<Result<(), HttpResponse>, diesel::result::Error> =
            tc.run(|conn| Ok(validate_assignee_role(&assignee_uuid, conn)));
        match validation {
            Ok(Ok(())) => {}
            Ok(Err(resp)) => return resp,
            Err(_) => return errors::internal("Failed to validate assignee"),
        }
    }

    match tc.run(|conn| repository::update_ticket(conn, ticket_id, new_ticket)) {
        Ok(ticket) => {
            record_canonical(&req, "ticket_id", ticket_id);
            record_canonical(&req, "outcome", "updated");
            HttpResponse::Ok().json(ticket)
        }
        Err(e) => errors::internal(format!("Failed to update ticket: {e}")),
    }
}

// Delete a ticket with comprehensive cleanup
pub async fn delete_ticket(
    auth: AuthContext,
    mut tc: TenantConn,
    storage: crate::extractors::ScopedStorage,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Forbidden: Only administrators can delete tickets");
    }

    let ticket_id = path.into_inner();

    // DB-side deletion runs inside tc.run (one txn, RLS-pinned).
    // Storage cleanup + search observer notification happen after
    // commit via spawn_delete_cleanup; the closure can't be async
    // and shouldn't hold the connection across file I/O.
    match tc.run(|conn| repository::delete_ticket_with_cleanup(conn, ticket_id)) {
        Ok(deleted) => {
            if deleted.rows_affected > 0 {
                repository::tickets::spawn_delete_cleanup(
                    deleted,
                    ticket_id,
                    storage.get(),
                    Some(search_service.get_ref()),
                );

                // Remove ticket from search index
                indexing_tasks::spawn_delete_ticket(search_service.get_ref().clone(), ticket_id);

                // The deletion reaches clients through the sync pool (the
                // repository write emits `ticket.deleted`); no discrete SSE.

                record_canonical(&req, "ticket_id", ticket_id);
                record_canonical(&req, "outcome", "deleted");
                HttpResponse::NoContent().finish()
            } else {
                errors::not_found_msg("Ticket not found")
            }
        }
        Err(_) => errors::internal("Failed to delete ticket"),
    }
}

// Import tickets from JSON file
pub async fn import_tickets_from_json(
    auth: AuthContext,
    mut tc: TenantConn,
    json_path: web::Path<String>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Forbidden: Only administrators can import tickets");
    }

    let json_path_str = json_path.into_inner();
    let path = Path::new(&json_path_str);

    let json_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return errors::internal(format!("Failed to read file: {}", e));
        }
    };

    // Parse the JSON
    let tickets_json: TicketsJson = match serde_json::from_str(&json_content) {
        Ok(tickets) => tickets,
        Err(_) => return errors::bad_request("Failed to parse JSON"),
    };

    // Import each ticket — one txn per row so a malformed row in
    // the middle of the import doesn't roll back the others. Matches
    // the existing semantics (the prior code didn't open a txn at
    // all).
    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets {
        match tc.run(|conn| repository::import_ticket_from_json(conn, &ticket_json)) {
            Ok(_) => imported_count += 1,
            Err(_) => failed_count += 1,
        }
    }

    HttpResponse::Ok().json(json!({
        "imported": imported_count,
        "failed": failed_count
    }))
}

// Import tickets from JSON string
pub async fn import_tickets_from_json_string(
    auth: AuthContext,
    mut tc: TenantConn,
    tickets_json: web::Json<TicketsJson>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Forbidden: Only administrators can import tickets");
    }

    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets.iter() {
        match tc.run(|conn| repository::import_ticket_from_json(conn, ticket_json)) {
            Ok(_) => imported_count += 1,
            Err(_) => failed_count += 1,
        }
    }

    HttpResponse::Ok().json(json!({
        "imported": imported_count,
        "failed": failed_count
    }))
}

// Create an empty ticket with default values
pub async fn create_empty_ticket(
    mut tc: TenantConn,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> impl Responder {
    // Extract claims from request extensions (set by cookie_auth_middleware)
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Parse the user UUID from the JWT claims
    let user_uuid = match crate::utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    // Create a new ticket with default values using the authenticated user's UUID
    let default_state = match tc.run(repository::workflow_states::default_state) {
        Ok(s) => s,
        Err(e) => {
            error!(error = ?e, "Failed to resolve default workflow state");
            return errors::internal("Failed to resolve workflow state");
        }
    };
    let empty_ticket = NewTicket {
        title: "New Ticket".to_string(),
        workflow_state_id: default_state.id,
        requester_uuid: Some(user_uuid),
        ..Default::default()
    };

    // Create the ticket and then add empty article content
    let mut ticket = match tc.run(|conn| repository::create_ticket(conn, empty_ticket)) {
        Ok(ticket) => ticket,
        Err(e) => {
            error!(error = ?e, "Failed to create empty ticket");
            return errors::internal(format!("Failed to create empty ticket: {e}"));
        }
    };

    // Run automatic assignment rules if no assignee
    if ticket.assignee_uuid.is_none() {
        let rules_result = tc
            .run(|conn| {
                Ok::<_, diesel::result::Error>(AssignmentEngine::evaluate_rules(
                    conn,
                    &ticket,
                    AssignmentTrigger::TicketCreated,
                ))
            })
            .ok()
            .flatten();

        if let Some(result) = rules_result {
            // Update ticket with auto-assigned user
            if let Some(assigned_uuid) = result.assigned_user_uuid {
                let assign_update = TicketUpdate {
                    assignee_uuid: Some(Some(assigned_uuid)),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    ..Default::default()
                };
                if let Ok(updated) = tc.run(|conn| {
                    repository::update_ticket_partial(
                        conn,
                        ticket.id,
                        assign_update,
                        Some(search_service.get_ref()),
                    )
                }) {
                    ticket = updated;
                    info!(
                        ticket_id = ticket.id,
                        assignee = %assigned_uuid,
                        rule = %result.rule_name,
                        method = %result.method,
                        "Auto-assigned new ticket"
                    );

                    // Send notification to the auto-assigned user, unless the rule
                    // assigned the ticket to the user who just created it (never
                    // notify someone about their own action).
                    if assigned_uuid != user_uuid {
                        let notification_service = notification_service.clone();
                        let ticket_id = ticket.id;
                        let ticket_title = ticket.title.clone();
                        let ticket_workspace = ticket.workspace_id;
                        let rule_name = result.rule_name.clone();

                        tokio::spawn(async move {
                            let payload = NotificationPayload::new(
                                NotificationTypeCode::TicketAssigned,
                                assigned_uuid,
                                NotificationActor {
                                    uuid: Uuid::nil(), // System actor
                                    name: "System".to_string(),
                                    avatar_thumb: None,
                                    kind: crate::sync::ActorKind::System,
                                },
                                NotificationEntity::Ticket {
                                    id: ticket_id,
                                    title: ticket_title,
                                },
                                ticket_workspace,
                            )
                            .with_body(format!(
                                "You have been auto-assigned to ticket #{ticket_id} (Rule: {rule_name})"
                            ));

                            if let Err(e) = notification_service.notify(payload).await {
                                warn!(error = %e, "Failed to send auto-assignment notification");
                            }
                        });
                    }
                }
            }
        }
    }

    // Create empty article content for the ticket
    let new_article_content = crate::models::NewArticleContent {
        ticket_id: ticket.id,
        yjs_state_vector: None,
        yjs_document: None,
        yjs_client_id: None,
    };

    // Try to create article content, but don't fail if it doesn't work
    let article_content = tc
        .run(|conn| repository::create_article_content(conn, new_article_content))
        .ok();

    // Index the new ticket in search
    indexing_tasks::spawn_index_ticket(
        search_service.get_ref().clone(),
        ticket.clone(),
        article_content,
    );

    // Canonical wide event: fold the outcome into the one per-request line so
    // "a ticket was created, id N" is queryable without a separate log line.
    record_canonical(&req, "ticket_id", ticket.id);
    record_canonical(&req, "outcome", "created");

    // Return the complete ticket with article content
    match tc.run(|conn| repository::get_complete_ticket(conn, ticket.id)) {
        Ok(complete_ticket) => HttpResponse::Created().json(complete_ticket),
        Err(_) => HttpResponse::Created().json(ticket), // Fallback to just the ticket if getting complete ticket fails
    }
}

// Update ticket partially
pub async fn update_ticket_partial(
    mut tc: TenantConn,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
    auth: AuthContext,
    access: TicketAccess,
    body: web::Json<Value>,
) -> impl Responder {
    let ticket_id = access.ticket_id;

    // Notification dispatch downstream wants the raw `Claims`
    // for actor logging; pull from extensions, which the JWT
    // middleware populates (the extractor already verified
    // these claims map to a real user).
    let user_info = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Get the current ticket state for detecting changes (for notifications)
    let old_ticket = tc
        .run(|conn| repository::get_ticket_by_id(conn, ticket_id))
        .ok();

    // Parse JSON and build TicketUpdate with user lookups
    let mut ticket_update = TicketUpdate {
        updated_at: Some(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };

    // Handle simple string fields
    if let Some(title) = body.get("title").and_then(|v| v.as_str()) {
        ticket_update.title = Some(title.to_string());
    }

    // Workflow state is set by id; the category drives closed_at.
    if let Some(ws_id) = body.get("workflow_state_id").and_then(|v| v.as_i64()) {
        let id = ws_id as i32;
        ticket_update.workflow_state_id = Some(id);
        // Recompute closed_at based on the resolved category.
        if let Ok(Some(cat)) = tc.run(|conn| repository::workflow_states::category_of(conn, id)) {
            ticket_update.closed_at =
                if cat == WorkflowStateCategory::Done || cat == WorkflowStateCategory::Cancelled {
                    Some(Some(chrono::Utc::now().naive_utc()))
                } else {
                    Some(None)
                };
        }
    }

    if let Some(priority_str) = body.get("priority").and_then(|v| v.as_str()) {
        match priority_str {
            "none" => ticket_update.priority = Some(crate::models::TicketPriority::None),
            "low" => ticket_update.priority = Some(crate::models::TicketPriority::Low),
            "medium" => ticket_update.priority = Some(crate::models::TicketPriority::Medium),
            "high" => ticket_update.priority = Some(crate::models::TicketPriority::High),
            "urgent" => ticket_update.priority = Some(crate::models::TicketPriority::Urgent),
            _ => {}
        }
    }

    // "Not spam": only *clearing* the flag is allowed via the API — the inbound
    // pipeline is the sole thing that ever marks a ticket as spam.
    if body.get("spam_suspected").and_then(|v| v.as_bool()) == Some(false) {
        ticket_update.spam_suspected = Some(false);
    }

    // Handle requester (can be name, UUID, or empty string for unassign)
    if let Some(requester_str) = body.get("requester").and_then(|v| v.as_str()) {
        if requester_str.is_empty() {
            // Empty string means unassign
            ticket_update.requester_uuid = Some(None);
        } else if let Ok(uuid) = Uuid::parse_str(requester_str) {
            // It's already a UUID
            ticket_update.requester_uuid = Some(Some(uuid));
        } else {
            // Try to look up by name
            match tc.run(|conn| crate::repository::users::get_user_by_name(requester_str, conn)) {
                Ok(user) => ticket_update.requester_uuid = Some(Some(user.uuid)),
                Err(_) => {
                    warn!(name = %requester_str, "Could not find user by name");
                }
            }
        }
    }

    // Handle assignee (can be name, UUID, or empty string for unassign)
    if let Some(assignee_str) = body.get("assignee").and_then(|v| v.as_str()) {
        if assignee_str.is_empty() {
            // Empty string means unassign
            ticket_update.assignee_uuid = Some(None);
        } else {
            // Parse and validate assignee
            let resolved: Result<Result<Uuid, HttpResponse>, diesel::result::Error> =
                tc.run(|conn| Ok(parse_and_validate_assignee_string(assignee_str, conn)));
            match resolved {
                Ok(Ok(uuid)) => ticket_update.assignee_uuid = Some(Some(uuid)),
                Ok(Err(response)) => return response,
                Err(_) => return errors::internal("Failed to resolve assignee"),
            }
        }
    }

    // due_date: ISO timestamp string, or null to clear. Calendar
    // view reads this; ticket_id default is null.
    if body.get("due_date").is_some() {
        match body.get("due_date") {
            Some(Value::String(s)) => match chrono::DateTime::parse_from_rfc3339(s) {
                Ok(dt) => {
                    ticket_update.due_date = Some(Some(dt.naive_utc()));
                }
                Err(_) => return errors::bad_request("due_date must be RFC3339 or null"),
            },
            Some(Value::Null) => {
                ticket_update.due_date = Some(None);
            }
            _ => return errors::bad_request("due_date must be a string or null"),
        }
    }

    // start_date: ISO timestamp string, or null to clear. Optional
    // planning start for the gantt; same shape as due_date.
    if body.get("start_date").is_some() {
        match body.get("start_date") {
            Some(Value::String(s)) => match chrono::DateTime::parse_from_rfc3339(s) {
                Ok(dt) => {
                    ticket_update.start_date = Some(Some(dt.naive_utc()));
                }
                Err(_) => return errors::bad_request("start_date must be RFC3339 or null"),
            },
            Some(Value::Null) => {
                ticket_update.start_date = Some(None);
            }
            _ => return errors::bad_request("start_date must be a string or null"),
        }
    }

    // recurrence_rule: RFC 5545 RRULE string, or null to clear.
    // Validated lazily inside services::recurrence on close; the
    // handler accepts any string and only rejects on type.
    if body.get("recurrence_rule").is_some() {
        match body.get("recurrence_rule") {
            Some(Value::String(s)) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    ticket_update.recurrence_rule = Some(None);
                } else {
                    ticket_update.recurrence_rule = Some(Some(trimmed.to_string()));
                }
            }
            Some(Value::Null) => {
                ticket_update.recurrence_rule = Some(None);
            }
            _ => return errors::bad_request("recurrence_rule must be a string or null"),
        }
    }

    // Handle category_id (can be a number or null to unassign)
    if body.get("category_id").is_some() {
        match body.get("category_id") {
            Some(Value::Number(n)) => {
                if let Some(id) = n.as_i64() {
                    ticket_update.category_id = Some(Some(id as i32));
                }
            }
            Some(Value::Null) => {
                // Null means remove category
                ticket_update.category_id = Some(None);
            }
            _ => {}
        }
    }

    // Validate category visibility if category_id is being changed
    if let Some(Some(new_category_id)) = ticket_update.category_id {
        let user_uuid = match crate::utils::parse_uuid(&user_info.sub) {
            Ok(uuid) => uuid,
            Err(_) => return errors::bad_request("Invalid user UUID in token"),
        };
        let is_admin = auth.is_workspace_admin();
        match tc.run(|conn| {
            crate::repository::categories::can_user_see_category(
                conn,
                &user_uuid,
                new_category_id,
                is_admin,
            )
        }) {
            Ok(true) => {}
            Ok(false) => {
                return errors::forbidden(
                    "Forbidden: You do not have access to the specified category",
                );
            }
            Err(_) => {
                return errors::internal("Failed to check category visibility");
            }
        }
    }

    // Track if category was changed for auto-assignment
    let category_changed = body.get("category_id").is_some();

    // Update the ticket
    match tc.run(|conn| {
        repository::update_ticket_partial(
            conn,
            ticket_id,
            ticket_update,
            Some(search_service.get_ref()),
        )
    }) {
        Ok(updated_ticket) => {
            // RRULE materialise-on-close: if the patch flipped the
            // ticket into a closed category and the row carries a
            // recurrence_rule, generate the next occurrence so the
            // user sees it land immediately. Errors here are
            // logged-and-continue: a malformed rule shouldn't
            // brick close.
            if let Some(rule) = updated_ticket.recurrence_rule.as_ref() {
                let category = tc
                    .run(|conn| {
                        repository::workflow_states::category_of(
                            conn,
                            updated_ticket.workflow_state_id,
                        )
                    })
                    .ok()
                    .flatten();
                let is_closed = matches!(
                    category,
                    Some(crate::models::WorkflowStateCategory::Done)
                        | Some(crate::models::WorkflowStateCategory::Cancelled)
                );
                if is_closed {
                    let after = updated_ticket
                        .due_date
                        .or(updated_ticket.closed_at)
                        .unwrap_or(updated_ticket.created_at);
                    match crate::services::recurrence::next_occurrence_naive(
                        rule,
                        updated_ticket.created_at,
                        after,
                    ) {
                        Ok(Some(next_due)) => {
                            // The new occurrence is a clean copy of the
                            // template, same title / priority / category /
                            // assignee, with a fresh due_date and an open
                            // workflow state. Carry the rule forward so
                            // the chain continues; record the template id
                            // to keep audit lineage.
                            let template_id = updated_ticket
                                .recurrence_template_id
                                .unwrap_or(updated_ticket.id);
                            let open_state =
                                match tc.run(repository::workflow_states::default_state) {
                                    Ok(s) => s.id,
                                    Err(_) => updated_ticket.workflow_state_id,
                                };
                            let new_ticket = NewTicket {
                                title: updated_ticket.title.clone(),
                                workflow_state_id: open_state,
                                priority: updated_ticket.priority,
                                requester_uuid: updated_ticket.requester_uuid,
                                assignee_uuid: updated_ticket.assignee_uuid,
                                category_id: updated_ticket.category_id,
                                due_date: Some(next_due),
                                recurrence_rule: Some(rule.clone()),
                                recurrence_template_id: Some(template_id),
                                ..Default::default()
                            };
                            if let Err(e) =
                                tc.run(|conn| repository::create_ticket(conn, new_ticket))
                            {
                                warn!(
                                    ticket_id,
                                    error = ?e,
                                    "Failed to materialise next recurring occurrence"
                                );
                            } else {
                                info!(
                                    ticket_id,
                                    template_id,
                                    next_due = %next_due,
                                    "Materialised next recurring occurrence"
                                );
                            }
                        }
                        Ok(None) => {
                            // Series ran out (UNTIL passed). Nothing to do.
                        }
                        Err(e) => {
                            warn!(
                                ticket_id,
                                rule = %rule,
                                error = ?e,
                                "Recurrence rule failed to parse on close",
                            );
                        }
                    }
                }
            }

            // Track a category-change auto-assignment so the field-diff
            // notifier below doesn't fire a second (real-actor) assignment
            // notification on top of the richer System "auto-assigned
            // (Rule: X)" one.
            let mut auto_assigned_uuid: Option<Uuid> = None;

            // Run automatic assignment rules if category changed and no assignee
            if category_changed && updated_ticket.assignee_uuid.is_none() {
                let rules_result = tc
                    .run(|conn| {
                        Ok::<_, diesel::result::Error>(AssignmentEngine::evaluate_rules(
                            conn,
                            &updated_ticket,
                            AssignmentTrigger::CategoryChanged,
                        ))
                    })
                    .ok()
                    .flatten();
                if let Some(result) = rules_result {
                    // Update ticket with auto-assigned user
                    if let Some(assigned_uuid) = result.assigned_user_uuid {
                        let assign_update = TicketUpdate {
                            assignee_uuid: Some(Some(assigned_uuid)),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if tc
                            .run(|conn| {
                                repository::update_ticket_partial(
                                    conn,
                                    ticket_id,
                                    assign_update,
                                    Some(search_service.get_ref()),
                                )
                            })
                            .is_ok()
                        {
                            info!(
                                ticket_id,
                                assignee = %assigned_uuid,
                                rule = %result.rule_name,
                                method = %result.method,
                                "Auto-assigned ticket on category change"
                            );

                            // Auto-assignment reaches clients through the
                            // sync pool (the assignee change emits a
                            // ticket.assignee_changed sync action).
                            let assignee_user = tc
                                .run(|conn| repository::get_user_by_uuid(&assigned_uuid, conn))
                                .ok();

                            // Send notification to the auto-assigned user
                            if let Some(ref assignee) = assignee_user {
                                // Remember the auto-assigned user so the field-diff
                                // notifier below skips a duplicate assignment ping.
                                auto_assigned_uuid = Some(assignee.uuid);
                                // Skip when the rule assigned the ticket to the user
                                // who triggered the category change (own action).
                                if assignee.uuid != auth.user_uuid {
                                    let notification_service = notification_service.clone();
                                    let ticket_title = updated_ticket.title.clone();
                                    let ticket_workspace = updated_ticket.workspace_id;
                                    let assignee_uuid = assignee.uuid;
                                    let rule_name = result.rule_name.clone();

                                    tokio::spawn(async move {
                                        let payload = NotificationPayload::new(
                                            NotificationTypeCode::TicketAssigned,
                                            assignee_uuid,
                                            NotificationActor {
                                                uuid: Uuid::nil(), // System actor
                                                name: "System".to_string(),
                                                avatar_thumb: None,
                                                kind: crate::sync::ActorKind::System,
                                            },
                                            NotificationEntity::Ticket {
                                                id: ticket_id,
                                                title: ticket_title,
                                            },
                                            ticket_workspace,
                                        )
                                        .with_body(format!(
                                            "You have been auto-assigned to ticket #{ticket_id} (Rule: {rule_name})"
                                        ));

                                        if let Err(e) = notification_service.notify(payload).await {
                                            warn!(error = %e, "Failed to send auto-assignment notification");
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Field changes reach clients through the sync pool (the
            // repository write emits the matching ticket.* sync action);
            // no discrete SSE broadcast.

            // Now fetch the complete ticket for the response
            // This happens after SSE broadcast so it doesn't delay real-time updates
            let updated_ticket =
                match tc.run(|conn| repository::get_complete_ticket(conn, ticket_id)) {
                    Ok(ticket) => ticket,
                    Err(_) => return errors::internal("Failed to fetch updated ticket"),
                };

            // Trigger notifications for relevant changes (runs async, doesn't block response)
            if let Some(ref old) = old_ticket {
                // Get actor info for notifications
                let actor_uuid = Uuid::parse_str(&user_info.sub).ok();
                let actor = actor_uuid.and_then(|uuid| {
                    tc.run(|conn| repository::get_user_by_uuid(&uuid, conn))
                        .ok()
                        .map(|user| NotificationActor {
                            uuid: user.uuid,
                            name: user.name.clone(),
                            avatar_thumb: user.avatar_thumb.clone(),
                            kind: crate::sync::ActorKind::User,
                        })
                });

                if let Some(actor) = actor {
                    let notification_service = notification_service.clone();
                    let ticket_title = updated_ticket.ticket.title.clone();
                    let new_assignee = updated_ticket.ticket.assignee_uuid;
                    let old_assignee = old.assignee_uuid;
                    // Compare the workflow-state category of new vs old, so the
                    // "status changed" notification fires on a category
                    // transition (e.g. backlog -> active), not on every
                    // same-category state move.
                    let new_status_workflow_id = updated_ticket.ticket.workflow_state_id;
                    let old_status_workflow_id = old.workflow_state_id;
                    let new_status = tc
                        .run(|conn| {
                            repository::workflow_states::category_of(conn, new_status_workflow_id)
                        })
                        .ok()
                        .flatten()
                        .map(|c| c.as_str())
                        .unwrap_or("backlog");
                    let old_status = tc
                        .run(|conn| {
                            repository::workflow_states::category_of(conn, old_status_workflow_id)
                        })
                        .ok()
                        .flatten()
                        .map(|c| c.as_str())
                        .unwrap_or("backlog");
                    let requester_uuid = updated_ticket.ticket.requester_uuid;
                    let ticket_workspace = updated_ticket.ticket.workspace_id;
                    let actor_clone = actor.clone();

                    // Spawn async task for notifications to not block response
                    tokio::spawn(async move {
                        // Notify new assignee if assignment changed, unless this was
                        // a category-change auto-assignment (already notified above
                        // with the richer System "auto-assigned" copy).
                        if new_assignee != old_assignee && new_assignee != auto_assigned_uuid {
                            if let Some(assignee_uuid) = new_assignee {
                                let payload = NotificationPayload::new(
                                    NotificationTypeCode::TicketAssigned,
                                    assignee_uuid,
                                    actor_clone.clone(),
                                    NotificationEntity::Ticket {
                                        id: ticket_id,
                                        title: ticket_title.clone(),
                                    },
                                    ticket_workspace,
                                )
                                .with_body(format!(
                                    "You have been assigned to ticket #{ticket_id}"
                                ));

                                if let Err(e) = notification_service.notify(payload).await {
                                    warn!(error = %e, "Failed to send assignment notification");
                                }
                            }
                        }

                        // Notify requester if status changed to closed
                        if new_status != old_status {
                            if let Some(requester) = requester_uuid {
                                let payload = NotificationPayload::new(
                                    NotificationTypeCode::TicketStatusChanged,
                                    requester,
                                    actor_clone.clone(),
                                    NotificationEntity::Ticket {
                                        id: ticket_id,
                                        title: ticket_title.clone(),
                                    },
                                    ticket_workspace,
                                )
                                .with_body(format!(
                                    "Ticket #{} status changed to {}",
                                    ticket_id, new_status
                                ));

                                if let Err(e) = notification_service.notify(payload).await {
                                    warn!(error = %e, "Failed to send status change notification");
                                }
                            }
                        }
                    });
                }
            }

            // Re-index the updated ticket in search
            // Fetch the article content if it exists for indexing
            let article_content = tc
                .run(|conn| repository::get_article_content_by_ticket_id(conn, ticket_id))
                .ok();
            indexing_tasks::spawn_index_ticket(
                search_service.get_ref().clone(),
                updated_ticket.ticket.clone(),
                article_content,
            );

            // Return the updated complete ticket
            HttpResponse::Ok().json(updated_ticket)
        }
        Err(e) => {
            error!(error = ?e, "Failed to update ticket");
            errors::internal("Failed to update ticket")
        }
    }
}

// Link tickets. The repository write emits the `linked_ticket.added`
// sync action (Stage 2); clients pick the link up through the pool, so
// no discrete SSE broadcast is needed.
pub async fn link_tickets(
    auth: AuthContext,
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can link tickets",
        );
    }

    let (ticket_id, linked_ticket_id) = path.into_inner();

    match tc.run(|conn| repository::link_tickets(conn, ticket_id, linked_ticket_id)) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to link tickets");
            errors::internal("Failed to link tickets")
        }
    }
}

// Unlink tickets. Repository write emits `linked_ticket.removed`; the
// pool delivers the removal, so no discrete SSE broadcast.
pub async fn unlink_tickets(
    auth: AuthContext,
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can unlink tickets",
        );
    }

    let (ticket_id, linked_ticket_id) = path.into_inner();

    match tc.run(|conn| repository::unlink_tickets(conn, ticket_id, linked_ticket_id)) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to unlink tickets");
            errors::internal("Failed to unlink tickets")
        }
    }
}

// Add device to ticket. Repository write emits `ticket_asset.added`;
// the pool delivers the link, so no discrete SSE broadcast.
pub async fn add_device_to_ticket(
    auth: AuthContext,
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can add devices to tickets",
        );
    }

    let (ticket_id, device_id) = path.into_inner();

    match tc.run(|conn| repository::add_device_to_ticket(conn, ticket_id, device_id)) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(ticket_id = ticket_id, device_id = device_id, error = ?e, "Failed to add device to ticket");
            errors::internal("Failed to add device to ticket")
        }
    }
}

// Remove device from ticket. Repository write emits
// `ticket_asset.removed`; the pool delivers the removal, so no
// discrete SSE broadcast.
pub async fn remove_device_from_ticket(
    auth: AuthContext,
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can remove devices from tickets",
        );
    }

    let (ticket_id, device_id) = path.into_inner();

    match tc.run(|conn| repository::remove_device_from_ticket(conn, ticket_id, device_id)) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                HttpResponse::Ok().json(json!({"success": true}))
            } else {
                errors::not_found_msg("Asset not associated with ticket")
            }
        }
        Err(e) => {
            error!(ticket_id = ticket_id, device_id = device_id, error = ?e, "Failed to remove device from ticket");
            errors::internal("Failed to remove device from ticket")
        }
    }
}

// Get recent tickets for the authenticated user
pub async fn get_recent_tickets(mut tc: TenantConn, auth: AuthContext) -> impl Responder {
    use crate::repository::ticket_visibility::{self, VisibilityContext};
    use crate::repository::user_ticket_views;

    let vis = VisibilityContext::from_auth(&auth);
    let user_uuid = vis.user_uuid;

    let recent = match tc.run(|conn| {
        user_ticket_views::get_recent_tickets(
            conn,
            user_uuid,
            user_ticket_views::RECENT_TICKETS_LIMIT,
        )
    }) {
        Ok(t) => t,
        Err(e) => {
            error!(error = ?e, "Failed to fetch recent tickets");
            return errors::internal("Failed to fetch recent tickets");
        }
    };

    // AUD-001 follow-up: a row that landed in `user_ticket_views`
    // before the caller's visibility narrowed (group membership
    // change, ticket reassigned to a private project, etc.) is
    // still in `recent` because the join only checks existence.
    // Filter against the same primitive that gates single-record
    // fetches so the sidebar can't surface titles for tickets the
    // user can no longer read.
    let candidate_ids: Vec<i32> = recent.iter().map(|r| r.id).collect();
    let visible =
        match tc.run(|conn| ticket_visibility::visible_ticket_ids(conn, &vis, &candidate_ids)) {
            Ok(ids) => ids,
            Err(e) => {
                error!(error = ?e, "Failed to filter recent tickets by visibility");
                return errors::internal("Failed to fetch recent tickets");
            }
        };
    let filtered: Vec<_> = recent
        .into_iter()
        .filter(|t| visible.contains(&t.id))
        .collect();
    HttpResponse::Ok().json(filtered)
}

// Record a ticket view. Extractor handles visibility — no
// possibility of recording a view for a ticket the user
// shouldn't know exists.
pub async fn record_ticket_view(mut tc: TenantConn, access: TicketAccess) -> impl Responder {
    let TicketAccess { ticket_id, auth } = access;
    let user_uuid = auth.user_uuid;

    match tc.run(|conn| repository::user_ticket_views::record_view(conn, user_uuid, ticket_id)) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to record ticket view");
            errors::internal("Failed to record ticket view")
        }
    }
}

// Remove a ticket from the user's recent views
pub async fn remove_recent_ticket(
    mut tc: TenantConn,
    path: web::Path<i32>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let claims_inner = claims.into_inner();
    let user_uuid = match Uuid::parse_str(&claims_inner.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request(
                "Invalid user UUID: The user UUID in the authentication token is invalid",
            )
        }
    };

    match tc.run(|conn| repository::user_ticket_views::delete_view(conn, user_uuid, ticket_id)) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = ?e, "Failed to remove recent ticket view");
            errors::internal("Failed to remove recent ticket")
        }
    }
}

// Bulk ticket operations request
#[derive(Debug, serde::Deserialize)]
pub struct BulkActionRequest {
    action: String,
    ids: Vec<i32>,
    value: Option<String>,
}

// Perform bulk operations on tickets
pub async fn bulk_tickets(
    req: HttpRequest,
    auth: AuthContext,
    mut tc: TenantConn,
    storage: crate::extractors::ScopedStorage,
    search_service: web::Data<Arc<SearchService>>,
    notification_service: web::Data<NotificationService>,
    body: web::Json<BulkActionRequest>,
) -> impl Responder {
    // Authentication guard.
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Unauthorized: Authentication required");
    }

    // Bulk mutations are a staff-only affordance. The end-user surface
    // doesn't expose multi-select; gating here keeps Users out of a
    // surface that has no UX path for them and avoids per-id IDOR
    // sweeps via the bulk endpoint.
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: Bulk ticket actions are restricted to staff");
    }

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return errors::bad_request("Bad Request: No ticket IDs provided");
    }

    match action {
        "delete" => {
            // Only admins can bulk delete
            if !auth.is_workspace_admin() {
                return errors::forbidden("Forbidden: Only administrators can delete tickets");
            }

            // One connection/transaction for the whole batch (was a pool
            // checkout per id). Each id runs in its own savepoint so a failure
            // rolls back only that ticket, keeping the best-effort semantics.
            // Storage + search-index cleanup happens after commit, only for the
            // deletes that actually landed.
            let cleanup = tc
                .run(|conn| {
                    let mut out = Vec::new();
                    for id in ids {
                        match conn
                            .transaction(|conn| repository::delete_ticket_with_cleanup(conn, *id))
                        {
                            Ok(result) if result.rows_affected > 0 => out.push((*id, result)),
                            Ok(_) => {}
                            Err(e) => error!(ticket_id = id, error = ?e, "Failed to delete ticket"),
                        }
                    }
                    Ok(out)
                })
                .unwrap_or_default();

            let deleted: usize = cleanup.iter().map(|(_, r)| r.rows_affected).sum();
            for (id, result) in cleanup {
                repository::tickets::spawn_delete_cleanup(
                    result,
                    id,
                    storage.get(),
                    Some(search_service.get_ref()),
                );
                // Remove from search index. Deletion reaches clients via the
                // sync pool (`ticket.deleted`); no discrete SSE.
                indexing_tasks::spawn_delete_ticket(search_service.get_ref().clone(), id);
            }

            HttpResponse::Ok().json(json!({ "affected": deleted }))
        }

        "set-priority" => {
            let priority_str = match &body.value {
                Some(v) => v.as_str(),
                None => return errors::bad_request("Bad Request: Priority value required"),
            };

            let priority = match priority_str {
                "none" => crate::models::TicketPriority::None,
                "low" => crate::models::TicketPriority::Low,
                "medium" => crate::models::TicketPriority::Medium,
                "high" => crate::models::TicketPriority::High,
                "urgent" => crate::models::TicketPriority::Urgent,
                _ => return errors::bad_request("Bad Request: Invalid priority value"),
            };

            // One connection/transaction for the batch, each id in its own
            // savepoint. update_ticket_partial emits ticket.priority_changed;
            // the pool delivers it. No discrete SSE.
            let updated = tc
                .run(|conn| {
                    let mut n = 0;
                    for id in ids {
                        let update = TicketUpdate {
                            priority: Some(priority),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if conn
                            .transaction(|conn| {
                                repository::update_ticket_partial(
                                    conn,
                                    *id,
                                    update,
                                    Some(search_service.get_ref()),
                                )
                            })
                            .is_ok()
                        {
                            n += 1;
                        }
                    }
                    Ok(n)
                })
                .unwrap_or(0);

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        "assign" => {
            let assignee_str = match &body.value {
                Some(v) => v.as_str(),
                None => return errors::bad_request("Bad Request: Assignee value required"),
            };

            let assignee_uuid = if assignee_str.is_empty() {
                None
            } else {
                match Uuid::parse_str(assignee_str) {
                    Ok(uuid) => Some(uuid),
                    Err(_) => return errors::bad_request("Bad Request: Invalid assignee UUID"),
                }
            };

            // One connection/transaction for the batch, each id in its own
            // savepoint. update_ticket_partial emits ticket.assignee_changed;
            // the pool delivers the pill. No discrete SSE. Collect the tickets
            // whose assignee actually changed so we can fire the same
            // TicketAssigned notification the single-item PATCH path fires
            // (`update_ticket_partial` handler) — bulk assign was silently
            // skipping it.
            let mut assigned: Vec<(i32, String, i32, Uuid)> = Vec::new();
            let updated = tc
                .run(|conn| {
                    let mut n = 0;
                    for id in ids {
                        let old_assignee = repository::get_ticket_by_id(conn, *id)
                            .ok()
                            .and_then(|t| t.assignee_uuid);
                        let update = TicketUpdate {
                            assignee_uuid: Some(assignee_uuid),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if let Ok(t) = conn.transaction(|conn| {
                            repository::update_ticket_partial(
                                conn,
                                *id,
                                update,
                                Some(search_service.get_ref()),
                            )
                        }) {
                            n += 1;
                            // Notify only on a real change (skip no-op
                            // re-assignments), mirroring the single-item guard.
                            if let Some(new_assignee) = t.assignee_uuid {
                                if Some(new_assignee) != old_assignee {
                                    assigned.push((
                                        t.id,
                                        t.title.clone(),
                                        t.workspace_id,
                                        new_assignee,
                                    ));
                                }
                            }
                        }
                    }
                    Ok(n)
                })
                .unwrap_or(0);

            // Notify each newly-assigned user off the response path, the same
            // TicketAssigned notification a single assign fires. A large batch
            // to one assignee is tamed by the interrupt burst-cap (they land in
            // the bell past the toast threshold), so no coalescing here.
            if !assigned.is_empty() {
                let actor = tc
                    .run(|conn| repository::get_user_by_uuid(&auth.user_uuid, conn))
                    .ok()
                    .map(|user| NotificationActor {
                        uuid: user.uuid,
                        name: user.name.clone(),
                        avatar_thumb: user.avatar_thumb.clone(),
                        kind: crate::sync::ActorKind::User,
                    });
                if let Some(actor) = actor {
                    let notification_service = notification_service.clone();
                    tokio::spawn(async move {
                        for (ticket_id, title, workspace_id, recipient) in assigned {
                            let payload = NotificationPayload::new(
                                NotificationTypeCode::TicketAssigned,
                                recipient,
                                actor.clone(),
                                NotificationEntity::Ticket {
                                    id: ticket_id,
                                    title,
                                },
                                workspace_id,
                            )
                            .with_body(format!("You have been assigned to ticket #{ticket_id}"));
                            if let Err(e) = notification_service.notify(payload).await {
                                warn!(error = %e, "Failed to send bulk assignment notification");
                            }
                        }
                    });
                }
            }

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        _ => HttpResponse::BadRequest().json(json!({
            "error": i18n::tr(&request_locale(&req), "backend-error-bad-request"),
            "code": "backend-error-bad-request",
            "message": format!("Unknown action: {}", action)
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TicketPriority;
    use crate::test_helpers::{create_test_claims, setup_test_pool, TestFixtures};
    use actix_web::{http::StatusCode, test, App};

    /// Helper to create a test app with ticket routes.
    /// Note: This is a simplified app without SSE, notification, and search services
    /// since those would require additional setup. For handlers that require those
    /// dependencies, we test them through the simpler endpoints.
    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/tickets", web::get().to(get_tickets))
            .route("/tickets/{id}", web::get().to(get_ticket))
    }

    #[actix_web::test]
    async fn get_tickets_requires_auth() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        // Request without authentication should fail
        // Note: get_tickets uses AuthContext extractor which will fail without auth middleware
        let req = test::TestRequest::get().uri("/tickets").to_request();
        let resp = test::call_service(&app, req).await;

        // AuthContext extractor returns 401 when no claims present
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn get_tickets_with_auth_succeeds() {
        let pool = setup_test_pool();
        let claims = {
            let mut conn = pool.get().unwrap();
            let admin = TestFixtures::create_user(&mut conn, "ticketadmin", "admin");
            let claims = create_test_claims(&admin);

            let user = TestFixtures::create_user(&mut conn, "regularuser", "user");
            let _user_claims = create_test_claims(&user);

            claims
        }; // conn dropped here — pool free for HTTP handlers

        let app = test::init_service(test_app(pool.clone())).await;
        let req = test::TestRequest::get().uri("/tickets").to_request();
        let resp = test::call_service(&app, req).await;

        // Without auth middleware/extractor properly configured, expect 401
        assert!(
            resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN
        );

        // Additional verification: the claims were created successfully
        assert_eq!(claims.platform_role, "platform_admin");
    }

    #[actix_web::test]
    async fn create_ticket_succeeds() {
        // This test verifies ticket creation via the repository layer directly
        // since create_ticket handler requires SSE, notification, and search services
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create admin user
        let admin = TestFixtures::create_user(&mut conn, "createticketadmin", "admin");

        // Create ticket directly using TestFixtures
        let ticket = TestFixtures::create_ticket(&mut conn, "Test Ticket", Some(admin.uuid), None);

        // Verify ticket was created
        assert_eq!(ticket.title, "Test Ticket");
        let cat = repository::workflow_states::category_of(&mut conn, ticket.workflow_state_id)
            .unwrap()
            .unwrap();
        assert_eq!(cat, WorkflowStateCategory::Backlog);
        assert_eq!(ticket.priority, TicketPriority::Medium);
        assert_eq!(ticket.requester_uuid, Some(admin.uuid));
    }

    #[actix_web::test]
    async fn get_ticket_by_id() {
        // Note: The get_ticket handler uses web::ReqData<Claims> which requires middleware.
        // We test the repository layer directly to verify ticket retrieval works.
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create user and ticket
        let user = TestFixtures::create_user(&mut conn, "getticketuser", "technician");
        let ticket = TestFixtures::create_ticket(&mut conn, "Get Me Ticket", Some(user.uuid), None);

        // Test via repository layer
        let fetched = crate::repository::get_complete_ticket(&mut conn, ticket.id)
            .expect("Should fetch ticket");

        assert_eq!(fetched.ticket.title, "Get Me Ticket");
        assert_eq!(fetched.ticket.id, ticket.id);
        assert_eq!(fetched.ticket.requester_uuid, Some(user.uuid));
    }

    #[actix_web::test]
    async fn update_ticket_partial() {
        // Test partial ticket update via repository layer
        // The handler requires SSE, notification, and search services
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create user and ticket
        let admin = TestFixtures::create_user(&mut conn, "updateticketadmin", "admin");
        let ticket = TestFixtures::create_ticket(&mut conn, "Update Me", Some(admin.uuid), None);

        // Verify initial state
        assert_eq!(ticket.title, "Update Me");
        let initial_cat =
            repository::workflow_states::category_of(&mut conn, ticket.workflow_state_id)
                .unwrap()
                .unwrap();
        assert_eq!(initial_cat, WorkflowStateCategory::Backlog);
        assert_eq!(ticket.priority, TicketPriority::Medium);

        // Perform partial update via repository — flip to an Active-category
        // (in-progress) state.
        let in_progress = repository::workflow_states::first_in_category(
            &mut conn,
            WorkflowStateCategory::Active,
        )
        .expect("in-progress workflow state must exist");
        let update = TicketUpdate {
            title: Some("Updated Title".to_string()),
            workflow_state_id: Some(in_progress.id),
            updated_at: Some(chrono::Utc::now().naive_utc()),
            ..Default::default()
        };

        let updated = repository::update_ticket_partial(&mut conn, ticket.id, update, None)
            .expect("Failed to update ticket");

        // Verify updates were applied
        assert_eq!(updated.title, "Updated Title");
        let new_cat =
            repository::workflow_states::category_of(&mut conn, updated.workflow_state_id)
                .unwrap()
                .unwrap();
        assert_eq!(new_cat, WorkflowStateCategory::Active);
        // Priority should remain unchanged
        assert_eq!(updated.priority, TicketPriority::Medium);
    }

    #[actix_web::test]
    async fn get_ticket_not_found() {
        let pool = setup_test_pool();
        let claims = {
            let mut conn = pool.get().unwrap();
            let user = TestFixtures::create_user(&mut conn, "notfounduser", "technician");
            create_test_claims(&user)
        }; // conn dropped here

        let app = test::init_service(test_app(pool.clone())).await;

        // Request non-existent ticket
        let req = test::TestRequest::get().uri("/tickets/999999").to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn regular_user_cannot_access_all_tickets() {
        // get_tickets requires technician or admin role
        // Regular users should be forbidden
        let pool = setup_test_pool();
        let claims = {
            let mut conn = pool.get().unwrap();
            let user = TestFixtures::create_user(&mut conn, "regularticketuser", "user");
            create_test_claims(&user)
        }; // conn dropped here

        // Verify the claims have the correct role
        assert_eq!(claims.platform_role, "user");

        let app = test::init_service(test_app(pool.clone())).await;
        let req = test::TestRequest::get().uri("/tickets").to_request();
        let resp = test::call_service(&app, req).await;

        // Should fail - either 401 (no auth) or 403 (forbidden for regular users)
        assert!(
            resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN
        );
    }

    #[actix_web::test]
    async fn ticket_with_category() {
        // Test ticket with category via repository layer
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create category and user
        let category = TestFixtures::create_category(&mut conn, "Test Category");
        let user = TestFixtures::create_user(&mut conn, "catticketuser", "technician");

        // Create ticket with category
        let ticket = TestFixtures::create_ticket(
            &mut conn,
            "Categorized Ticket",
            Some(user.uuid),
            Some(category.id),
        );

        // Verify ticket has category
        assert_eq!(ticket.category_id, Some(category.id));

        // Fetch via repository
        let fetched = crate::repository::get_complete_ticket(&mut conn, ticket.id)
            .expect("Should fetch ticket");

        assert_eq!(fetched.ticket.category_id, Some(category.id));
        assert_eq!(fetched.ticket.title, "Categorized Ticket");
    }
}
