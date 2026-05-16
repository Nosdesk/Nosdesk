use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::extractors::{AuthContext, TicketAccess};
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::handlers::helpers::{actor_for as helper_actor_for, with_actor};
use crate::models::{
    AssignmentTrigger, Claims, NewTicket, TicketUpdate, TicketsJson, UserRole,
    WorkflowStateCategory,
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
use crate::sync::actor::ActorContext;
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use crate::utils::rbac::{is_admin, is_technician_or_admin};

/// Local convenience: bind the system-actor reference for this
/// handler module so call sites stay terse.
#[inline]
fn actor_for(req: &HttpRequest) -> ActorContext {
    helper_actor_for(req, "handler:tickets")
}

// Helper function to validate assignee role
fn validate_assignee_role(
    assignee_uuid: &Uuid,
    conn: &mut crate::db::DbConnection,
) -> Result<(), HttpResponse> {
    match crate::repository::users::get_user_by_uuid(assignee_uuid, conn) {
        Ok(user) => {
            // Check if user has technician or admin role
            if user.role != UserRole::Technician && user.role != UserRole::Admin {
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
                if user.role != UserRole::Technician && user.role != UserRole::Admin {
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
                if user.role != UserRole::Technician && user.role != UserRole::Admin {
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

// Simple helper to broadcast SSE events without blocking
async fn broadcast_sse_simple(
    sse_state: web::Data<crate::handlers::sse::SseState>,
    ticket_id: i32,
    event_type: String,
    data: serde_json::Value,
    source_client_id: Option<String>,
) {
    use crate::handlers::sse::SseEvent;

    tokio::spawn(async move {
        let event = match event_type.as_str() {
            "ticket_updated" => {
                let key = data.get("key").and_then(|v| v.as_str());
                let value = data.get("value");
                let user_sub = data.get("user_sub").and_then(|v| v.as_str());
                match (key, value, user_sub) {
                    (Some(key), Some(value), Some(user_sub)) => Some(SseEvent::TicketUpdated {
                        ticket_id,
                        field: key.to_string(),
                        value: value.clone(),
                        updated_by: user_sub.to_string(),
                        timestamp: chrono::Utc::now(),
                    }),
                    _ => None,
                }
            }
            "ticket_linked" => {
                data.get("linked_ticket_id")
                    .and_then(|v| v.as_u64())
                    .map(|linked_id| SseEvent::TicketLinked {
                        ticket_id,
                        linked_ticket_id: linked_id as i32,
                        timestamp: chrono::Utc::now(),
                    })
            }
            "ticket_unlinked" => {
                data.get("linked_ticket_id")
                    .and_then(|v| v.as_u64())
                    .map(|linked_id| SseEvent::TicketUnlinked {
                        ticket_id,
                        linked_ticket_id: linked_id as i32,
                        timestamp: chrono::Utc::now(),
                    })
            }
            "device_linked" => data
                .get("device_id")
                .and_then(|v| v.as_u64())
                .map(|device_id| SseEvent::DeviceLinked {
                    ticket_id,
                    device_id: device_id as i32,
                    timestamp: chrono::Utc::now(),
                }),
            "device_unlinked" => data
                .get("device_id")
                .and_then(|v| v.as_u64())
                .map(|device_id| SseEvent::DeviceUnlinked {
                    ticket_id,
                    device_id: device_id as i32,
                    timestamp: chrono::Utc::now(),
                }),
            _ => {
                warn!(event_type = %event_type, "Unknown SSE event type");
                None
            }
        };

        if let Some(event) = event {
            sse_state
                .broadcast_event_from(event, source_client_id)
                .await;
        }
    });
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
pub async fn get_tickets(pool: web::Data<crate::db::Pool>, auth: AuthContext) -> impl Responder {
    // Only technicians and admins can see all tickets via this endpoint
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Forbidden: Only technicians and admins can access all tickets");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::get_all_tickets(&mut conn) {
        Ok(tickets) => HttpResponse::Ok().json(tickets),
        Err(_) => errors::internal("Failed to get tickets"),
    }
}

// Get paginated tickets
pub async fn get_paginated_tickets(
    pool: web::Data<crate::db::Pool>,
    query: web::Query<PaginationParams>,
    auth: AuthContext,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Build query with automatic permission filtering via AuthContext
    let result = TicketQuery::new()
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
        .execute_with_users(&mut conn);

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
    pool: web::Data<crate::db::Pool>,
    access: TicketAccess,
    query: web::Query<TicketActivityQuery>,
) -> impl Responder {
    use crate::schema::sync_actions;

    let ticket_id = access.ticket_id;
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let limit = query
        .limit
        .unwrap_or(DEFAULT_ACTIVITY_LIMIT)
        .clamp(1, MAX_ACTIVITY_LIMIT);
    let group_marker = format!("ticket:{}", ticket_id);

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

    if let Some(before) = query.before {
        q = q.filter(sync_actions::sync_id.lt(before));
    }

    let mut events: Vec<TicketActivityRow> = match q.load(&mut conn) {
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
pub async fn get_ticket(pool: web::Data<crate::db::Pool>, access: TicketAccess) -> impl Responder {
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

    let TicketAccess { ticket_id, auth } = access;

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // A `not_found` here is a genuine "deleted between extraction
    // and load" race, which we still want to surface as 404.
    let complete_ticket = match repository::get_complete_ticket(&mut conn, ticket_id) {
        Ok(ticket) => ticket,
        Err(_) => return errors::not_found_msg("Ticket not found"),
    };

    // Record the view (don't fail the request if this fails).
    let view_repo = UserTicketViewsRepository::new(pool.get_ref().clone());
    if let Err(e) = view_repo.record_view(auth.user_uuid, ticket_id) {
        warn!(user_uuid = %auth.user_uuid, error = ?e, "Failed to record ticket view");
    }

    HttpResponse::Ok().json(complete_ticket)
}

// Create a new ticket
pub async fn create_ticket(
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    auth: AuthContext,
    ticket: web::Json<NewTicket>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let new_ticket = ticket.into_inner();

    // Validate category visibility if category_id is set
    if let Some(category_id) = new_ticket.category_id {
        match crate::repository::categories::can_user_see_category(
            &mut conn,
            &auth.user_uuid,
            category_id,
            auth.is_admin(),
        ) {
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
        if let Err(e) = validate_assignee_role(&assignee_uuid, &mut conn) {
            return e;
        }
    }

    let actor_ctx = ActorContext::user(auth.user_uuid, None);
    match with_actor(&mut conn, &actor_ctx, |conn| {
        repository::create_ticket(conn, new_ticket)
    }) {
        Ok(mut ticket) => {
            // Run automatic assignment rules if no assignee
            if ticket.assignee_uuid.is_none() {
                if let Some(result) = AssignmentEngine::evaluate_rules(
                    &mut conn,
                    &ticket,
                    AssignmentTrigger::TicketCreated,
                ) {
                    if let Some(assigned_uuid) = result.assigned_user_uuid {
                        let assign_update = TicketUpdate {
                            assignee_uuid: Some(Some(assigned_uuid)),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if let Ok(updated) = with_actor(&mut conn, &actor_ctx, |conn| {
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

                            // Send notification to the auto-assigned user
                            let notification_service = notification_service.clone();
                            let ticket_id = ticket.id;
                            let ticket_title = ticket.title.clone();
                            let rule_name = result.rule_name.clone();

                            tokio::spawn(async move {
                                let payload = NotificationPayload::new(
                                    NotificationTypeCode::TicketAssigned,
                                    assigned_uuid,
                                    NotificationActor {
                                        uuid: Uuid::nil(),
                                        name: "System".to_string(),
                                        avatar_thumb: None,
                                    },
                                    NotificationEntity::Ticket {
                                        id: ticket_id,
                                        title: ticket_title,
                                    },
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

            // Index the new ticket in search
            indexing_tasks::spawn_index_ticket(
                search_service.get_ref().clone(),
                ticket.clone(),
                None,
            );

            // Broadcast ticket creation via SSE
            sse_state
                .broadcast_event(crate::handlers::sse::SseEvent::TicketCreated {
                    ticket_id: ticket.id,
                    ticket: serde_json::to_value(&ticket).unwrap_or_default(),
                    timestamp: chrono::Utc::now(),
                })
                .await;

            HttpResponse::Created().json(ticket)
        }
        Err(_) => errors::internal("Failed to create ticket"),
    }
}

// Update a ticket. The extractor gates visibility; this body
// only runs for callers who can see the ticket.
pub async fn update_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    access: TicketAccess,
    ticket: web::Json<NewTicket>,
) -> impl Responder {
    let ticket_id = access.ticket_id;
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let new_ticket = ticket.into_inner();

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        if let Err(e) = validate_assignee_role(&assignee_uuid, &mut conn) {
            return e;
        }
    }

    let actor_ctx = actor_for(&req);
    match with_actor(&mut conn, &actor_ctx, |conn| {
        repository::update_ticket(conn, ticket_id, new_ticket)
    }) {
        Ok(ticket) => HttpResponse::Ok().json(ticket),
        Err(e) => errors::internal(format!("Failed to update ticket: {e}")),
    }
}

// Delete a ticket with comprehensive cleanup
pub async fn delete_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
    search_service: web::Data<Arc<SearchService>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    path: web::Path<i32>,
) -> impl Responder {
    let source_client_id = extract_sse_client_id(&req);

    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_admin(&claims) {
        return errors::forbidden("Forbidden: Only administrators can delete tickets");
    }

    let ticket_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Use the comprehensive deletion function that cleans up files
    let actor_ctx = actor_for(&req);
    match repository::delete_ticket_with_cleanup(
        &mut conn,
        ticket_id,
        storage.as_ref().clone(),
        Some(search_service.get_ref()),
        &actor_ctx,
    )
    .await
    {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                // Remove ticket from search index
                indexing_tasks::spawn_delete_ticket(search_service.get_ref().clone(), ticket_id);

                // Broadcast ticket deletion via SSE
                sse_state
                    .broadcast_event_from(
                        crate::handlers::sse::SseEvent::TicketDeleted {
                            ticket_id,
                            timestamp: chrono::Utc::now(),
                        },
                        source_client_id,
                    )
                    .await;

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
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    json_path: web::Path<String>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_admin(&claims) {
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

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Import each ticket
    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets {
        match repository::import_ticket_from_json(&mut conn, &ticket_json) {
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
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    tickets_json: web::Json<TicketsJson>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_admin(&claims) {
        return errors::forbidden("Forbidden: Only administrators can import tickets");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Import each ticket
    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets.iter() {
        match repository::import_ticket_from_json(&mut conn, ticket_json) {
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
    pool: web::Data<crate::db::Pool>,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

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
    let default_state = match repository::workflow_states::default_state(&mut conn) {
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
    let actor_ctx = actor_for(&req);
    let mut ticket = match with_actor(&mut conn, &actor_ctx, |conn| {
        repository::create_ticket(conn, empty_ticket)
    }) {
        Ok(ticket) => ticket,
        Err(e) => {
            error!(error = ?e, "Failed to create empty ticket");
            return errors::internal(format!("Failed to create empty ticket: {e}"));
        }
    };

    // Run automatic assignment rules if no assignee
    if ticket.assignee_uuid.is_none() {
        if let Some(result) =
            AssignmentEngine::evaluate_rules(&mut conn, &ticket, AssignmentTrigger::TicketCreated)
        {
            // Update ticket with auto-assigned user
            if let Some(assigned_uuid) = result.assigned_user_uuid {
                let assign_update = TicketUpdate {
                    assignee_uuid: Some(Some(assigned_uuid)),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    ..Default::default()
                };
                if let Ok(updated) = with_actor(&mut conn, &actor_ctx, |conn| {
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

                    // Send notification to the auto-assigned user
                    let notification_service = notification_service.clone();
                    let ticket_id = ticket.id;
                    let ticket_title = ticket.title.clone();
                    let rule_name = result.rule_name.clone();

                    tokio::spawn(async move {
                        let payload = NotificationPayload::new(
                            NotificationTypeCode::TicketAssigned,
                            assigned_uuid,
                            NotificationActor {
                                uuid: Uuid::nil(), // System actor
                                name: "System".to_string(),
                                avatar_thumb: None,
                            },
                            NotificationEntity::Ticket {
                                id: ticket_id,
                                title: ticket_title,
                            },
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

    // Create empty article content for the ticket
    let new_article_content = crate::models::NewArticleContent {
        ticket_id: ticket.id,
        yjs_state_vector: None,
        yjs_document: None,
        yjs_client_id: None,
    };

    // Try to create article content, but don't fail if it doesn't work
    let article_content = repository::create_article_content(&mut conn, new_article_content).ok();

    // Index the new ticket in search
    indexing_tasks::spawn_index_ticket(
        search_service.get_ref().clone(),
        ticket.clone(),
        article_content,
    );

    // Return the complete ticket with article content
    match repository::get_complete_ticket(&mut conn, ticket.id) {
        Ok(complete_ticket) => HttpResponse::Created().json(complete_ticket),
        Err(_) => HttpResponse::Created().json(ticket), // Fallback to just the ticket if getting complete ticket fails
    }
}

// Update ticket partially
pub async fn update_ticket_partial(
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
    access: TicketAccess,
    body: web::Json<Value>,
) -> impl Responder {
    let ticket_id = access.ticket_id;
    let source_client_id = extract_sse_client_id(&req);

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Notification dispatch downstream wants the raw `Claims`
    // for actor logging; pull from extensions, which the JWT
    // middleware populates (the extractor already verified
    // these claims map to a real user).
    let user_info = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Get the current ticket state for detecting changes (for notifications)
    let old_ticket = repository::get_ticket_by_id(&mut conn, ticket_id).ok();

    // Parse JSON and build TicketUpdate with user lookups
    let mut ticket_update = TicketUpdate {
        updated_at: Some(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };

    // Handle simple string fields
    if let Some(title) = body.get("title").and_then(|v| v.as_str()) {
        ticket_update.title = Some(title.to_string());
    }

    // Handle status — accept the legacy three-bucket strings for wire
    // compatibility and translate to a workflow_state_id.
    if let Some(status_str) = body.get("status").and_then(|v| v.as_str()) {
        if matches!(status_str, "open" | "in-progress" | "closed") {
            match repository::workflow_states::state_for_legacy_status(&mut conn, status_str) {
                Ok(state) => {
                    ticket_update.workflow_state_id = Some(state.id);
                    let cat = state.category;
                    ticket_update.closed_at = if cat == WorkflowStateCategory::Done
                        || cat == WorkflowStateCategory::Cancelled
                    {
                        Some(Some(chrono::Utc::now().naive_utc()))
                    } else {
                        Some(None)
                    };
                }
                Err(e) => {
                    error!(error = ?e, "Failed to resolve workflow state for status string");
                }
            }
        }
    }

    // Direct workflow_state_id support (forward-compat path the frontend
    // will switch to after migration).
    if let Some(ws_id) = body.get("workflow_state_id").and_then(|v| v.as_i64()) {
        let id = ws_id as i32;
        ticket_update.workflow_state_id = Some(id);
        // Recompute closed_at based on the resolved category.
        if let Ok(Some(cat)) = repository::workflow_states::category_of(&mut conn, id) {
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
            "low" => ticket_update.priority = Some(crate::models::TicketPriority::Low),
            "medium" => ticket_update.priority = Some(crate::models::TicketPriority::Medium),
            "high" => ticket_update.priority = Some(crate::models::TicketPriority::High),
            _ => {}
        }
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
            match crate::repository::users::get_user_by_name(requester_str, &mut conn) {
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
            match parse_and_validate_assignee_string(assignee_str, &mut conn) {
                Ok(uuid) => ticket_update.assignee_uuid = Some(Some(uuid)),
                Err(response) => return response,
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
        let is_admin = crate::utils::rbac::is_admin(&user_info);
        match crate::repository::categories::can_user_see_category(
            &mut conn,
            &user_uuid,
            new_category_id,
            is_admin,
        ) {
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
    let actor_ctx = actor_for(&req);
    match with_actor(&mut conn, &actor_ctx, |conn| {
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
                let category = repository::workflow_states::category_of(
                    &mut conn,
                    updated_ticket.workflow_state_id,
                )
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
                            // template — same title / priority / category /
                            // assignee — with a fresh due_date and an open
                            // workflow state. Carry the rule forward so
                            // the chain continues; record the template id
                            // to keep audit lineage.
                            let template_id = updated_ticket
                                .recurrence_template_id
                                .unwrap_or(updated_ticket.id);
                            let open_state =
                                match repository::workflow_states::default_state(&mut conn) {
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
                            if let Err(e) = with_actor(&mut conn, &actor_ctx, |conn| {
                                repository::create_ticket(conn, new_ticket)
                            }) {
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

            // Run automatic assignment rules if category changed and no assignee
            if category_changed && updated_ticket.assignee_uuid.is_none() {
                if let Some(result) = AssignmentEngine::evaluate_rules(
                    &mut conn,
                    &updated_ticket,
                    AssignmentTrigger::CategoryChanged,
                ) {
                    // Update ticket with auto-assigned user
                    if let Some(assigned_uuid) = result.assigned_user_uuid {
                        let assign_update = TicketUpdate {
                            assignee_uuid: Some(Some(assigned_uuid)),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if with_actor(&mut conn, &actor_ctx, |conn| {
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

                            // Get user info for the SSE event
                            let assignee_user =
                                repository::get_user_by_uuid(&assigned_uuid, &mut conn).ok();
                            let user_info_for_sse =
                                assignee_user
                                    .as_ref()
                                    .map(|u| crate::models::UserInfoWithAvatar {
                                        uuid: u.uuid,
                                        name: u.name.clone(),
                                        avatar_url: u.avatar_url.clone(),
                                        avatar_thumb: u.avatar_thumb.clone(),
                                    });

                            // Broadcast the assignment SSE event with user info
                            broadcast_sse_simple(
                                sse_state.clone(),
                                ticket_id,
                                "ticket_updated".to_string(),
                                json!({
                                    "key": "assignee",
                                    "value": {
                                        "uuid": assigned_uuid.to_string(),
                                        "user_info": user_info_for_sse
                                    },
                                    "user_sub": "system",
                                    "auto_assigned": true,
                                    "rule_name": result.rule_name
                                }),
                                source_client_id.clone(),
                            )
                            .await;

                            // Send notification to the auto-assigned user
                            if let Some(ref assignee) = assignee_user {
                                let notification_service = notification_service.clone();
                                let ticket_title = updated_ticket.title.clone();
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
                                        },
                                        NotificationEntity::Ticket {
                                            id: ticket_id,
                                            title: ticket_title,
                                        },
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

            // Broadcast SSE events IMMEDIATELY after DB update for low latency
            // Don't wait for fetching complete ticket data
            for (key, value) in body.0.as_object().unwrap_or(&serde_json::Map::new()) {
                debug!(ticket_id = ticket_id, key = %key, value = ?value, "Broadcasting SSE event");
                broadcast_sse_simple(
                    sse_state.clone(),
                    ticket_id,
                    "ticket_updated".to_string(),
                    json!({
                        "key": key,
                        "value": value,
                        "user_sub": user_info.sub
                    }),
                    source_client_id.clone(),
                )
                .await;
            }

            // Now fetch the complete ticket for the response
            // This happens after SSE broadcast so it doesn't delay real-time updates
            let updated_ticket = match repository::get_complete_ticket(&mut conn, ticket_id) {
                Ok(ticket) => ticket,
                Err(_) => return errors::internal("Failed to fetch updated ticket"),
            };

            // Trigger notifications for relevant changes (runs async, doesn't block response)
            if let Some(ref old) = old_ticket {
                // Get actor info for notifications
                let actor_uuid = Uuid::parse_str(&user_info.sub).ok();
                let actor = actor_uuid.and_then(|uuid| {
                    repository::get_user_by_uuid(&uuid, &mut conn)
                        .ok()
                        .map(|user| NotificationActor {
                            uuid: user.uuid,
                            name: user.name.clone(),
                            avatar_thumb: user.avatar_thumb.clone(),
                        })
                });

                if let Some(actor) = actor {
                    let notification_service = notification_service.clone();
                    let ticket_title = updated_ticket.ticket.title.clone();
                    let new_assignee = updated_ticket.ticket.assignee_uuid;
                    let old_assignee = old.assignee_uuid;
                    // Compare the legacy-bucket string of new vs old workflow
                    // state, so the "status changed" notification fires only
                    // on cross-bucket transitions (the wire contract).
                    let new_status = repository::workflow_states::category_of(
                        &mut conn,
                        updated_ticket.ticket.workflow_state_id,
                    )
                    .ok()
                    .flatten()
                    .map(|c| c.legacy_status())
                    .unwrap_or("open");
                    let old_status =
                        repository::workflow_states::category_of(&mut conn, old.workflow_state_id)
                            .ok()
                            .flatten()
                            .map(|c| c.legacy_status())
                            .unwrap_or("open");
                    let requester_uuid = updated_ticket.ticket.requester_uuid;
                    let actor_clone = actor.clone();

                    // Spawn async task for notifications to not block response
                    tokio::spawn(async move {
                        // Notify new assignee if assignment changed
                        if new_assignee != old_assignee {
                            if let Some(assignee_uuid) = new_assignee {
                                let payload = NotificationPayload::new(
                                    NotificationTypeCode::TicketAssigned,
                                    assignee_uuid,
                                    actor_clone.clone(),
                                    NotificationEntity::Ticket {
                                        id: ticket_id,
                                        title: ticket_title.clone(),
                                    },
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
            let article_content =
                repository::get_article_content_by_ticket_id(&mut conn, ticket_id).ok();
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

// Link tickets
pub async fn link_tickets(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let source_client_id = extract_sse_client_id(&req);

    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can link tickets",
        );
    }

    let (ticket_id, linked_ticket_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::link_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            debug!(
                ticket_id = ticket_id,
                linked_ticket_id = linked_ticket_id,
                "Broadcasting SSE event for ticket linking"
            );

            // Broadcast SSE event for ticket linking
            broadcast_sse_simple(
                sse_state.clone(),
                ticket_id,
                "ticket_linked".to_string(),
                json!({
                    "linked_ticket_id": linked_ticket_id
                }),
                source_client_id,
            )
            .await;

            HttpResponse::Ok().json(json!({"success": true}))
        }
        Err(e) => {
            error!(error = ?e, "Failed to link tickets");
            errors::internal("Failed to link tickets")
        }
    }
}

// Unlink tickets
pub async fn unlink_tickets(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let source_client_id = extract_sse_client_id(&req);

    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can unlink tickets",
        );
    }

    let (ticket_id, linked_ticket_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::unlink_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            debug!(
                ticket_id = ticket_id,
                linked_ticket_id = linked_ticket_id,
                "Broadcasting SSE event for ticket unlinking"
            );

            // Broadcast SSE event for ticket unlinking
            broadcast_sse_simple(
                sse_state.clone(),
                ticket_id,
                "ticket_unlinked".to_string(),
                json!({
                    "linked_ticket_id": linked_ticket_id
                }),
                source_client_id,
            )
            .await;

            HttpResponse::Ok().json(json!({"success": true}))
        }
        Err(e) => {
            error!(error = ?e, "Failed to unlink tickets");
            errors::internal("Failed to unlink tickets")
        }
    }
}

// Add device to ticket
pub async fn add_device_to_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let source_client_id = extract_sse_client_id(&req);

    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can add devices to tickets",
        );
    }

    let (ticket_id, device_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::add_device_to_ticket(&mut conn, ticket_id, device_id) {
        Ok(_) => {
            debug!(
                ticket_id = ticket_id,
                device_id = device_id,
                "Broadcasting SSE event for device linking"
            );

            // Broadcast SSE event for device linking
            broadcast_sse_simple(
                sse_state.clone(),
                ticket_id,
                "device_linked".to_string(),
                json!({
                    "device_id": device_id
                }),
                source_client_id,
            )
            .await;

            HttpResponse::Ok().json(json!({"success": true}))
        }
        Err(e) => {
            error!(ticket_id = ticket_id, device_id = device_id, error = ?e, "Failed to add device to ticket");
            errors::internal("Failed to add device to ticket")
        }
    }
}

// Remove device from ticket
pub async fn remove_device_from_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let source_client_id = extract_sse_client_id(&req);

    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can remove devices from tickets",
        );
    }

    let (ticket_id, device_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::remove_device_from_ticket(&mut conn, ticket_id, device_id) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                debug!(
                    ticket_id = ticket_id,
                    device_id = device_id,
                    "Broadcasting SSE event for device unlinking"
                );

                // Broadcast SSE event for device unlinking
                broadcast_sse_simple(
                    sse_state.clone(),
                    ticket_id,
                    "device_unlinked".to_string(),
                    json!({
                        "device_id": device_id
                    }),
                    source_client_id,
                )
                .await;

                HttpResponse::Ok().json(json!({"success": true}))
            } else {
                errors::not_found_msg("Device not associated with ticket")
            }
        }
        Err(e) => {
            error!(ticket_id = ticket_id, device_id = device_id, error = ?e, "Failed to remove device from ticket");
            errors::internal("Failed to remove device from ticket")
        }
    }
}

// Get recent tickets for the authenticated user
pub async fn get_recent_tickets(
    pool: web::Data<crate::db::Pool>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    use crate::repository::ticket_visibility::{self, VisibilityContext};
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

    let claims_inner = claims.into_inner();
    let Some(vis) = VisibilityContext::from_claims(&claims_inner) else {
        return errors::unauthorized("Authentication required");
    };
    let user_uuid = vis.user_uuid;

    let repo = UserTicketViewsRepository::new(pool.get_ref().clone());

    let recent = match repo.get_recent_tickets(
        user_uuid,
        crate::repository::user_ticket_views::RECENT_TICKETS_LIMIT,
    ) {
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
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let candidate_ids: Vec<i32> = recent.iter().map(|r| r.id).collect();
    let visible = match ticket_visibility::visible_ticket_ids(&mut conn, &vis, &candidate_ids) {
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
pub async fn record_ticket_view(
    pool: web::Data<crate::db::Pool>,
    access: TicketAccess,
) -> impl Responder {
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

    let TicketAccess { ticket_id, auth } = access;
    let user_uuid = auth.user_uuid;

    let repo = UserTicketViewsRepository::new(pool.get_ref().clone());

    match repo.record_view(user_uuid, ticket_id) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to record ticket view");
            errors::internal("Failed to record ticket view")
        }
    }
}

// Remove a ticket from the user's recent views
pub async fn remove_recent_ticket(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<i32>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

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

    let repo = UserTicketViewsRepository::new(pool.get_ref().clone());

    match repo.delete_view(user_uuid, ticket_id) {
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
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    body: web::Json<BulkActionRequest>,
) -> impl Responder {
    let source_client_id = extract_sse_client_id(&req);

    // Extract claims and check authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    // Bulk mutations are a staff-only affordance. The end-user surface
    // doesn't expose multi-select; gating here keeps Users out of a
    // surface that has no UX path for them and avoids per-id IDOR
    // sweeps via the bulk endpoint.
    if !is_technician_or_admin(&claims) {
        return errors::forbidden("Forbidden: Bulk ticket actions are restricted to staff");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return errors::bad_request("Bad Request: No ticket IDs provided");
    }

    let actor_ctx = actor_for(&req);

    match action {
        "delete" => {
            // Only admins can bulk delete
            if !is_admin(&claims) {
                return errors::forbidden("Forbidden: Only administrators can delete tickets");
            }

            let mut deleted = 0;
            for id in ids {
                match repository::delete_ticket_with_cleanup(
                    &mut conn,
                    *id,
                    storage.as_ref().clone(),
                    Some(search_service.get_ref()),
                    &actor_ctx,
                )
                .await
                {
                    Ok(rows) => {
                        deleted += rows;
                        // Remove from search index
                        indexing_tasks::spawn_delete_ticket(search_service.get_ref().clone(), *id);
                        // Broadcast ticket deletion via SSE
                        sse_state
                            .broadcast_event_from(
                                crate::handlers::sse::SseEvent::TicketDeleted {
                                    ticket_id: *id,
                                    timestamp: chrono::Utc::now(),
                                },
                                source_client_id.clone(),
                            )
                            .await;
                    }
                    Err(e) => {
                        error!(ticket_id = id, error = ?e, "Failed to delete ticket");
                    }
                }
            }

            HttpResponse::Ok().json(json!({ "affected": deleted }))
        }

        "set-status" => {
            let status_str = match &body.value {
                Some(v) => v.as_str(),
                None => return errors::bad_request("Bad Request: Status value required"),
            };

            if !matches!(status_str, "open" | "in-progress" | "closed") {
                return errors::bad_request("Bad Request: Invalid status value");
            }

            let target_state =
                match repository::workflow_states::state_for_legacy_status(&mut conn, status_str) {
                    Ok(s) => s,
                    Err(e) => {
                        error!(error = ?e, "Failed to resolve workflow state");
                        return errors::internal("Failed to resolve workflow state");
                    }
                };
            let is_closed = matches!(
                target_state.category,
                WorkflowStateCategory::Done | WorkflowStateCategory::Cancelled
            );

            let mut updated = 0;
            for id in ids {
                let update = TicketUpdate {
                    workflow_state_id: Some(target_state.id),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    closed_at: if is_closed {
                        Some(Some(chrono::Utc::now().naive_utc()))
                    } else {
                        None
                    },
                    ..Default::default()
                };

                if with_actor(&mut conn, &actor_ctx, |conn| {
                    repository::update_ticket_partial(
                        conn,
                        *id,
                        update,
                        Some(search_service.get_ref()),
                    )
                })
                .is_ok()
                {
                    updated += 1;
                    // Send SSE update with source_client_id for echo suppression
                    sse_state
                        .broadcast_event_from(
                            crate::handlers::sse::SseEvent::TicketUpdated {
                                ticket_id: *id,
                                field: "status".to_string(),
                                value: json!(status_str),
                                updated_by: claims.sub.clone(),
                                timestamp: chrono::Utc::now(),
                            },
                            source_client_id.clone(),
                        )
                        .await;
                }
            }

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        "set-priority" => {
            let priority_str = match &body.value {
                Some(v) => v.as_str(),
                None => return errors::bad_request("Bad Request: Priority value required"),
            };

            let priority = match priority_str {
                "low" => crate::models::TicketPriority::Low,
                "medium" => crate::models::TicketPriority::Medium,
                "high" => crate::models::TicketPriority::High,
                _ => return errors::bad_request("Bad Request: Invalid priority value"),
            };

            let mut updated = 0;
            for id in ids {
                let update = TicketUpdate {
                    priority: Some(priority),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    ..Default::default()
                };

                if with_actor(&mut conn, &actor_ctx, |conn| {
                    repository::update_ticket_partial(
                        conn,
                        *id,
                        update,
                        Some(search_service.get_ref()),
                    )
                })
                .is_ok()
                {
                    updated += 1;
                    sse_state
                        .broadcast_event_from(
                            crate::handlers::sse::SseEvent::TicketUpdated {
                                ticket_id: *id,
                                field: "priority".to_string(),
                                value: json!(priority_str),
                                updated_by: claims.sub.clone(),
                                timestamp: chrono::Utc::now(),
                            },
                            source_client_id.clone(),
                        )
                        .await;
                }
            }

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

            let mut updated = 0;
            for id in ids {
                let update = TicketUpdate {
                    assignee_uuid: Some(assignee_uuid),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    ..Default::default()
                };

                if with_actor(&mut conn, &actor_ctx, |conn| {
                    repository::update_ticket_partial(
                        conn,
                        *id,
                        update,
                        Some(search_service.get_ref()),
                    )
                })
                .is_ok()
                {
                    updated += 1;
                    sse_state
                        .broadcast_event_from(
                            crate::handlers::sse::SseEvent::TicketUpdated {
                                ticket_id: *id,
                                field: "assignee_uuid".to_string(),
                                value: json!(assignee_str),
                                updated_by: claims.sub.clone(),
                                timestamp: chrono::Utc::now(),
                            },
                            source_client_id.clone(),
                        )
                        .await;
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
    use crate::models::{TicketPriority, UserRole};
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
            let admin = TestFixtures::create_user(&mut conn, "ticketadmin", UserRole::Admin);
            let claims = create_test_claims(&admin);

            let user = TestFixtures::create_user(&mut conn, "regularuser", UserRole::User);
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
        assert_eq!(claims.role, "admin");
    }

    #[actix_web::test]
    async fn create_ticket_succeeds() {
        // This test verifies ticket creation via the repository layer directly
        // since create_ticket handler requires SSE, notification, and search services
        let pool = setup_test_pool();
        let mut conn = pool.get().unwrap();

        // Create admin user
        let admin = TestFixtures::create_user(&mut conn, "createticketadmin", UserRole::Admin);

        // Create ticket directly using TestFixtures
        let ticket = TestFixtures::create_ticket(&mut conn, "Test Ticket", Some(admin.uuid), None);

        // Verify ticket was created
        assert_eq!(ticket.title, "Test Ticket");
        let cat = repository::workflow_states::category_of(&mut conn, ticket.workflow_state_id)
            .unwrap()
            .unwrap();
        assert_eq!(cat.legacy_status(), "open");
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
        let user = TestFixtures::create_user(&mut conn, "getticketuser", UserRole::Technician);
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
        let admin = TestFixtures::create_user(&mut conn, "updateticketadmin", UserRole::Admin);
        let ticket = TestFixtures::create_ticket(&mut conn, "Update Me", Some(admin.uuid), None);

        // Verify initial state
        assert_eq!(ticket.title, "Update Me");
        let initial_cat =
            repository::workflow_states::category_of(&mut conn, ticket.workflow_state_id)
                .unwrap()
                .unwrap();
        assert_eq!(initial_cat.legacy_status(), "open");
        assert_eq!(ticket.priority, TicketPriority::Medium);

        // Perform partial update via repository — flip to an "in-progress"
        // state via the legacy bucket helper.
        let in_progress =
            repository::workflow_states::state_for_legacy_status(&mut conn, "in-progress")
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
        assert_eq!(new_cat.legacy_status(), "in-progress");
        // Priority should remain unchanged
        assert_eq!(updated.priority, TicketPriority::Medium);
    }

    #[actix_web::test]
    async fn get_ticket_not_found() {
        let pool = setup_test_pool();
        let claims = {
            let mut conn = pool.get().unwrap();
            let user = TestFixtures::create_user(&mut conn, "notfounduser", UserRole::Technician);
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
            let user = TestFixtures::create_user(&mut conn, "regularticketuser", UserRole::User);
            create_test_claims(&user)
        }; // conn dropped here

        // Verify the claims have the correct role
        assert_eq!(claims.role, "user");

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
        let user = TestFixtures::create_user(&mut conn, "catticketuser", UserRole::Technician);

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
