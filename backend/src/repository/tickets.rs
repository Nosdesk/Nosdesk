use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::utils::storage::Storage;

/// Observer fired after `update_ticket_partial` commits a change.
/// Implementor reindexes so title / status / priority / requester
/// changes land in search regardless of which handler made them.
pub trait TicketUpdatedObserver: Send + Sync {
    fn ticket_updated(&self, ticket: &Ticket, article: Option<&ArticleContent>);
}

/// Observer fired after a ticket is deleted via
/// `delete_ticket_with_cleanup`. Implementor removes the ticket
/// from the search index.
pub trait TicketDeletedObserver: Send + Sync {
    fn ticket_deleted(&self, ticket_id: i32);
}

// ============= Helper Functions for Enum Parsing =============

/// Parse a priority string into a TicketPriority enum
fn parse_ticket_priority(priority: &str) -> TicketPriority {
    match priority {
        "low" => TicketPriority::Low,
        "medium" => TicketPriority::Medium,
        "high" => TicketPriority::High,
        _ => TicketPriority::Medium, // Default to medium if unknown
    }
}

// Get all tickets
pub fn get_all_tickets(conn: &mut DbConnection) -> QueryResult<Vec<Ticket>> {
    tickets::table.load(conn)
}

pub fn get_ticket_by_id(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Ticket> {
    tickets::table.find(ticket_id).first(conn)
}

/// Typed annotation describing where a ticket originated, attached
/// to the `ticket.created` sync_actions row so the activity feed can
/// render richer phrasing than "System created this ticket".
///
/// Every field is optional so callers can populate just what they
/// know. The activity renderer walks the populated fields in
/// priority order (channel → portal → bare) and picks the most
/// specific phrasing it can support. Adding a new origin type
/// (assignment-rule auto-create, API token, scheduled job) only
/// needs a new `source` value plus a renderer branch — no schema
/// change, no migration.
#[derive(Debug, Clone, Default)]
pub struct TicketCreationAnnotation {
    /// Origin tag the renderer switches on. Conventional values:
    ///   * `"channel:<provider>"` — inbound channel ingest
    ///     (e.g. `"channel:email_imap"`).
    ///   * `"guest_portal"` — public submission form.
    ///   * `"api"` — programmatic create via an API token.
    /// Unknown values fall through to a generic "Created" line.
    pub source: Option<String>,
    /// Sender's email (channel-derived). Surfaces in the activity
    /// entry's actor slot so the agent sees who reported the issue
    /// before opening the comment thread.
    pub from_email: Option<String>,
    /// Display name attached to the sender's address, when present.
    pub from_name: Option<String>,
    /// Subject line for email-sourced tickets. Useful in the rare
    /// case the original sender's address differs from the
    /// ticket's requester record (forwarded messages, list-bots).
    pub subject: Option<String>,
}

/// Bare create — UI handlers, the import binary, and any caller
/// without specific channel/portal context land here.
pub fn create_ticket(conn: &mut DbConnection, new_ticket: NewTicket) -> QueryResult<Ticket> {
    create_ticket_with_annotation(conn, new_ticket, TicketCreationAnnotation::default())
}

/// Create with explicit origin annotation. Channel adapters and the
/// guest portal handler use this; everything else stays on the bare
/// `create_ticket`.
pub fn create_ticket_with_annotation(
    conn: &mut DbConnection,
    new_ticket: NewTicket,
    annotation: TicketCreationAnnotation,
) -> QueryResult<Ticket> {
    conn.transaction(|conn| {
        let ticket: Ticket = diesel::insert_into(tickets::table)
            .values(&new_ticket)
            .get_result(conn)?;
        let groups = groups::for_ticket(conn, &ticket)?;
        // `created_via` is an additive nested object: legacy
        // consumers that look at the existing top-level fields keep
        // working; the activity renderer reads `created_via.source`
        // and friends when present. Always emitted (even when all
        // annotation fields are None) so the renderer can rely on a
        // stable shape rather than probing for missing keys.
        let created_via = json!({
            "source": annotation.source,
            "from_email": annotation.from_email,
            "from_name": annotation.from_name,
            "subject": annotation.subject,
        });
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket.id.to_string(),
                op: SyncOp::Insert,
                event_type: "ticket.created",
                data: json!({
                    "id": ticket.id,
                    "title": ticket.title,
                    "workflow_state_id": ticket.workflow_state_id,
                    "priority": ticket.priority.as_str(),
                    "requester_uuid": ticket.requester_uuid,
                    "assignee_uuid": ticket.assignee_uuid,
                    "category_id": ticket.category_id,
                    "submitted_via": ticket.submitted_via,
                    "origin_channel_id": ticket.origin_channel_id,
                    "created_via": created_via,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(ticket)
    })
}

/// Look up a ticket by its opaque guest-lookup token. Used by the public
/// `/api/public/tickets/{token}` status endpoint.
pub fn find_by_lookup_token(conn: &mut DbConnection, token: Uuid) -> QueryResult<Ticket> {
    tickets::table
        .filter(tickets::guest_lookup_token.eq(token))
        .first(conn)
}

// sync-pending-wire: needs sync aggregate wiring
/// Flip every `verification_state = 'pending'` ticket requested by the given
/// user over to `'verified'` and return the newly-released tickets.
///
/// Called by the accept-invitation flow: once the submitter has proven they
/// own the email they gave us, every guest ticket they've filed (potentially
/// more than one within the 7-day invitation window) is released into the
/// tech queue at the same time.
pub fn verify_pending_tickets_for_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
) -> QueryResult<Vec<Ticket>> {
    diesel::update(
        tickets::table
            .filter(tickets::requester_uuid.eq(Some(user_uuid)))
            .filter(tickets::verification_state.eq("pending")),
    )
    .set((
        tickets::verification_state.eq("verified"),
        tickets::updated_at.eq(chrono::Utc::now().naive_utc()),
    ))
    .get_results(conn)
}

pub fn update_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    ticket: NewTicket,
) -> QueryResult<Ticket> {
    conn.transaction(|conn| {
        let updated: Ticket = diesel::update(tickets::table.find(ticket_id))
            .set(&ticket)
            .get_result(conn)?;
        let groups = groups::for_ticket(conn, &updated)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: updated.id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.updated",
                data: json!({
                    "id": updated.id,
                    "title": updated.title,
                    "workflow_state_id": updated.workflow_state_id,
                    "priority": updated.priority.as_str(),
                    "requester_uuid": updated.requester_uuid,
                    "assignee_uuid": updated.assignee_uuid,
                    "category_id": updated.category_id,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(updated)
    })
}

// Add a new function for partial ticket updates
pub fn update_ticket_partial(
    conn: &mut DbConnection,
    ticket_id: i32,
    ticket_update: crate::models::TicketUpdate,
    observer: Option<&dyn TicketUpdatedObserver>,
) -> QueryResult<Ticket> {
    debug!(ticket_id, update = ?ticket_update, "Updating ticket");

    let result = conn.transaction::<Ticket, diesel::result::Error, _>(|conn| {
        let result: Ticket = diesel::update(tickets::table.find(ticket_id))
            .set(&ticket_update)
            .get_result(conn)?;
        let groups = groups::for_ticket(conn, &result)?;
        // Pick the most-specific event_type that matches what changed.
        // The full updated row goes in `data` regardless so consumers
        // don't have to query back.
        let event_type = if ticket_update.workflow_state_id.is_some() {
            "ticket.workflow_state_changed"
        } else if ticket_update.assignee_uuid.is_some() {
            "ticket.assignee_changed"
        } else if ticket_update.priority.is_some() {
            "ticket.priority_changed"
        } else if ticket_update.title.is_some() {
            "ticket.title_changed"
        } else if ticket_update.category_id.is_some() {
            "ticket.category_changed"
        } else if ticket_update.verification_state.is_some() {
            "ticket.verification_changed"
        } else if ticket_update.resolution_notes.is_some() {
            "ticket.resolution_notes_changed"
        } else {
            "ticket.updated"
        };
        // Recompute the SLA pill when a field that feeds policy
        // matching or pause state changes — workflow_state_id drives
        // the paused flag (via the state's category), priority and
        // category_id drive policy matching. Including the fresh pill
        // in the sync_action's `data` lets the frontend pool
        // shallow-merge it into the card without waiting for the next
        // bootstrap; this is what makes the "compute-on-read, no
        // refresh required" architectural claim true for collaborative
        // sessions, not just first-load. `Value::Null` is a valid pill
        // payload (no policy matches the new shape), and merging it
        // correctly clears a stale pill from the card.
        let pill_affecting = ticket_update.workflow_state_id.is_some()
            || ticket_update.priority.is_some()
            || ticket_update.category_id.is_some();
        let mut data = json!({
            "id": result.id,
            "title": result.title,
            "workflow_state_id": result.workflow_state_id,
            "priority": result.priority.as_str(),
            "requester_uuid": result.requester_uuid,
            "assignee_uuid": result.assignee_uuid,
            "category_id": result.category_id,
            "verification_state": result.verification_state,
            "due_date": result.due_date,
            "resolution_notes": result.resolution_notes,
        });
        if pill_affecting {
            if let Some(obj) = data.as_object_mut() {
                obj.insert(
                    "sla".into(),
                    crate::services::sla::compute_pill_for_ticket(conn, &result),
                );
            }
        }
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: result.id.to_string(),
                op: SyncOp::Update,
                event_type,
                data,
                groups,
                causation_id: None,
            },
        )?;
        Ok(result)
    })?;

    if let Some(observer) = observer {
        // Fetch any associated article content so the observer can
        // reindex with the body text intact, not just the metadata.
        let article =
            crate::repository::article_content::get_article_content_by_ticket_id(conn, ticket_id)
                .ok();
        observer.ticket_updated(&result, article.as_ref());
    }

    Ok(result)
}

/// Comprehensive ticket deletion that cleans up all associated data and files
/// Result of `delete_ticket_with_cleanup`: how many ticket rows
/// the delete affected (0 or 1), plus the list of storage paths
/// the caller should clean up asynchronously after the txn commits.
/// Splitting the DB and storage halves lets the handler run the
/// DB part inside `TenantConn::run` (one txn, RLS-pinned) and do
/// the storage I/O afterwards without holding the connection.
pub struct DeletedTicket {
    pub rows_affected: usize,
    pub attachment_paths: Vec<String>,
}

/// Delete a ticket and all its dependent rows in a single
/// transaction. Returns the affected row count plus the attachment
/// storage paths the caller should sweep after commit.
///
/// The function is intentionally sync and does NOT open its own
/// `conn.transaction(...)` / call `session::set_actor` — callers
/// are expected to wrap via `TenantConn::run` (or
/// `session::with_actor_context`) so the actor + workspace GUCs
/// land on the surrounding transaction and the RLS policies see
/// them. The async storage cleanup and the search observer
/// notification happen at the call site, after commit.
pub fn delete_ticket_with_cleanup(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<DeletedTicket> {
    // 0. Capture the row + sync groups BEFORE we start deleting children,
    // so the emitted ticket.deleted event has the correct group fan-out
    // (project memberships are about to be cascade-removed) and still
    // resolves on a row that exists.
    let pre_delete = tickets::table
        .find(ticket_id)
        .first::<Ticket>(conn)
        .optional()?;
    let pre_groups = match pre_delete.as_ref() {
        Some(t) => groups::for_ticket(conn, t)?,
        None => Vec::new(),
    };

    // 1. First, get all comments for this ticket to find attachments
    let comments = crate::repository::comments::get_comments_by_ticket_id(conn, ticket_id)?;

    // 2. Collect all attachment paths for file cleanup
    let mut attachment_paths = Vec::new();
    for comment in &comments {
        let attachments =
            crate::repository::comments::get_attachments_by_comment_id(conn, comment.id)?;
        for attachment in &attachments {
            // Extract the storage path from the URL
            if let Some(storage_path) = extract_storage_path_from_url(&attachment.url) {
                attachment_paths.push(storage_path);
            }
            // Delete the attachment record
            diesel::delete(crate::schema::attachments::table.find(attachment.id)).execute(conn)?;
        }
    }

    // 3. Delete all comments for this ticket
    diesel::delete(
        crate::schema::comments::table.filter(crate::schema::comments::ticket_id.eq(ticket_id)),
    )
    .execute(conn)?;

    // 4. Delete linked tickets relationships
    diesel::delete(
        crate::schema::linked_tickets::table.filter(
            crate::schema::linked_tickets::ticket_id
                .eq(ticket_id)
                .or(crate::schema::linked_tickets::linked_ticket_id.eq(ticket_id)),
        ),
    )
    .execute(conn)?;

    // 5. Delete ticket-device relationships
    diesel::delete(
        crate::schema::ticket_assets::table
            .filter(crate::schema::ticket_assets::ticket_id.eq(ticket_id)),
    )
    .execute(conn)?;

    // 6. Delete ticket-project relationships
    diesel::delete(
        crate::schema::project_tickets::table
            .filter(crate::schema::project_tickets::ticket_id.eq(ticket_id)),
    )
    .execute(conn)?;

    // 7. Delete article content
    diesel::delete(
        crate::schema::article_contents::table
            .filter(crate::schema::article_contents::ticket_id.eq(ticket_id)),
    )
    .execute(conn)?;

    // 8. Finally, delete the ticket itself
    let result = diesel::delete(tickets::table.find(ticket_id)).execute(conn)?;

    if result > 0 {
        // Emit only when the row actually existed; pre_delete is None
        // for repeated DELETE calls, in which case the result is 0
        // and we'd be emitting a phantom event.
        if pre_delete.is_some() {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::Ticket,
                    aggregate_id: ticket_id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "ticket.deleted",
                    data: json!({ "id": ticket_id }),
                    groups: pre_groups,
                    causation_id: None,
                },
            )?;
        }
    }

    Ok(DeletedTicket {
        rows_affected: result,
        attachment_paths,
    })
}

/// Spawn the post-commit cleanup that ticket deletion needs: file
/// removal in storage and a search-index observer notification.
/// Fire-and-forget; failures are logged but don't propagate.
pub fn spawn_delete_cleanup(
    deleted: DeletedTicket,
    ticket_id: i32,
    storage: Arc<dyn Storage>,
    observer: Option<&dyn TicketDeletedObserver>,
) {
    if deleted.rows_affected > 0 {
        if let Some(observer) = observer {
            observer.ticket_deleted(ticket_id);
        }
    }

    tokio::spawn(async move {
        for path in deleted.attachment_paths {
            if let Err(e) = storage.delete_file(&path).await {
                warn!(path, error = ?e, "Failed to delete file during ticket cleanup");
            }
        }
    });
}

/// Extract storage path from attachment URL
/// Converts /uploads/tickets/123/filename.ext to tickets/123/filename.ext
fn extract_storage_path_from_url(url: &str) -> Option<String> {
    url.strip_prefix("/uploads/")
        .filter(|r| r.starts_with("tickets/") || r.starts_with("temp/"))
        .map(String::from)
}

// Composite operations for tickets
pub fn get_complete_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> Result<CompleteTicket, Error> {
    // Get the main ticket first
    let ticket = get_ticket_by_id(conn, ticket_id)?;
    debug!(id = ticket.id, title = %ticket.title, "Found ticket");

    // Look up complete user data for requester and assignee
    let requester_user = ticket
        .requester_uuid
        .as_ref()
        .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
        .map(crate::models::UserInfoWithAvatar::from);

    let assignee_user = ticket
        .assignee_uuid
        .as_ref()
        .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
        .map(UserInfoWithAvatar::from);

    // Get devices associated with this ticket through the junction table
    let devices = get_devices_for_ticket(conn, ticket_id).unwrap_or_default();

    // Delegate to the enriched assembler so the comments embedded in
    // a `CompleteTicket` carry the same `from_address`, attachments
    // and user payload as the standalone `/tickets/:id/comments`
    // endpoint. Two parallel implementations meant the channel-sourced
    // sender's email was missing here.
    let comments_with_attachments =
        crate::repository::comments::get_comments_with_attachments_by_ticket_id(conn, ticket_id)?;

    // Get article content (now handled by Yjs collaborative editing)
    let article_content: Option<String> = None;

    // Get linked tickets
    let linked_tickets =
        crate::repository::linked_tickets::get_linked_tickets(conn, ticket_id).unwrap_or_default();
    debug!(
        ticket_id,
        count = linked_tickets.len(),
        "Found linked tickets"
    );

    // Get projects for this ticket
    let projects =
        crate::repository::projects::get_projects_for_ticket(conn, ticket_id).unwrap_or_default();
    debug!(
        ticket_id,
        count = projects.len(),
        "Found projects for ticket"
    );

    // Cycle membership for the sidebar pill. Embed the cycle row
    // (name + state + ids) so the frontend renders the chip
    // without a separate fetch — the cycles store is per-project
    // keyed and the detail view doesn't know the cycle's project
    // up-front. Best-effort: a query failure here shouldn't fail
    // the whole detail load.
    let cycle = crate::repository::cycles::cycle_id_for_ticket(conn, ticket_id)
        .ok()
        .flatten()
        .and_then(|cid| {
            use crate::schema::cycles;
            cycles::table
                .find(cid)
                .select((
                    cycles::id,
                    cycles::uuid,
                    cycles::project_id,
                    cycles::name,
                    cycles::state,
                ))
                .first::<(i32, Uuid, i32, String, String)>(conn)
                .ok()
                .map(
                    |(id, uuid, project_id, name, state)| crate::models::TicketCycleSummary {
                        id,
                        uuid,
                        project_id,
                        name,
                        state,
                    },
                )
        });

    // SLA pill — mirrors the bootstrap stream's per-ticket
    // computation so the detail sidebar shows the same Breached /
    // At Risk / On Track / Paused state the list view does. The
    // SLA context (policies + calendars + holidays) is small, so
    // loading the full set for one ticket is cheap; the alternative
    // (a per-ticket policy lookup) would still need the calendar
    // and holiday rows.
    let sla = crate::repository::sla::load_for_pill_computation(conn)
        .ok()
        .and_then(|ctx| {
            let policy = crate::services::sla::pick_policy(&ctx.policies, &ticket)?;
            let cal_id = policy.working_calendar_id?;
            let calendar = ctx.calendars_by_id.get(&cal_id)?;
            let holidays = ctx
                .holidays_by_calendar
                .get(&cal_id)
                .cloned()
                .unwrap_or_default();
            // Resolve the ticket's workflow state category for the
            // pause-state computation. Backlog default matches the
            // bootstrap fallback so a missing state row degrades
            // gracefully rather than panicking.
            let category = crate::schema::workflow_states::table
                .find(ticket.workflow_state_id)
                .select(crate::schema::workflow_states::category)
                .first::<crate::models::WorkflowStateCategory>(conn)
                .unwrap_or(crate::models::WorkflowStateCategory::Backlog);
            crate::services::sla::compute_pill(
                &ticket,
                category,
                policy,
                calendar,
                &holidays,
                chrono::Utc::now(),
            )
        })
        .and_then(|pill| serde_json::to_value(pill).ok())
        .unwrap_or(serde_json::Value::Null);

    let tag_ids = crate::repository::tags::tag_ids_for_ticket(conn, ticket_id).unwrap_or_default();
    let watcher_uuids =
        crate::repository::ticket_watchers::watcher_uuids(conn, ticket_id).unwrap_or_default();

    Ok(CompleteTicket {
        ticket,
        requester_user,
        assignee_user,
        devices,
        comments: comments_with_attachments,
        article_content,
        linked_tickets,
        projects,
        cycle,
        sla,
        tag_ids,
        watcher_uuids,
    })
}

// Import from JSON
pub fn import_ticket_from_json(
    conn: &mut DbConnection,
    ticket_json: &TicketJson,
) -> Result<Ticket, Error> {
    // Map the legacy status string ("open" / "in-progress" / "closed") to a
    // concrete workflow_state row for the new schema. Unknown strings fall
    // back to the workspace default state via state_for_legacy_status.
    let workflow_state =
        crate::repository::workflow_states::state_for_legacy_status(conn, &ticket_json.status)?;
    let priority = parse_ticket_priority(&ticket_json.priority);

    // Create the ticket
    let new_ticket = NewTicket {
        title: ticket_json.title.clone(),
        workflow_state_id: workflow_state.id,
        priority,
        requester_uuid: Some(
            Uuid::parse_str(&ticket_json.requester).unwrap_or_else(|_| Uuid::now_v7()),
        ),
        assignee_uuid: if ticket_json.assignee.is_empty() {
            None
        } else {
            Uuid::parse_str(&ticket_json.assignee).ok()
        },
        ..Default::default()
    };

    let ticket = create_ticket(conn, new_ticket)?;

    // Create device if present (without ticket association).
    // hostname / warranty_status moved into the `attributes`
    // JSONB blob in Pass B, so the ticket-import shape that
    // still carries them as top-level keys gets translated
    // here before insert.
    if let Some(device_json) = &ticket_json.device {
        let mut attrs = serde_json::Map::new();
        if !device_json.hostname.is_empty() {
            attrs.insert(
                "hostname".to_string(),
                serde_json::Value::String(device_json.hostname.clone()),
            );
        }
        if !device_json.warranty_status.is_empty() {
            attrs.insert(
                "warranty_status".to_string(),
                serde_json::Value::String(device_json.warranty_status.clone()),
            );
        }
        let new_device = NewAsset {
            name: device_json.name.clone(),
            serial_number: Some(device_json.serial_number.clone()),
            manufacturer: None,
            model: Some(device_json.model.clone()),
            location: None,
            notes: None,
            primary_user_uuid: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: serde_json::Value::Object(attrs),
            quantity: None,
            unit: None,
            external_sync_source: None,
            low_stock_threshold: None,
        };

        crate::repository::assets::create_device(conn, new_device)?;
    }

    // Create comments and attachments if present
    if let Some(comments_json) = &ticket_json.comments {
        // Default system user UUID for imported comments
        let default_user_uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
            .unwrap_or_else(|_| Uuid::now_v7());

        for comment_json in comments_json {
            let new_comment = NewComment {
                content: comment_json.content.clone(),
                ticket_id: ticket.id,
                user_uuid: default_user_uuid,
                ..Default::default()
            };

            let comment = crate::repository::comments::create_comment(conn, new_comment, None)?;

            // Create attachments for this comment
            for attachment_json in &comment_json.attachments {
                let new_attachment = NewAttachment {
                    url: attachment_json.url.clone(),
                    name: attachment_json.name.clone(),
                    file_size: None,
                    mime_type: None,
                    checksum: None,
                    comment_id: Some(comment.id),
                    uploaded_by: None,
                    transcription: None,
                };

                crate::repository::comments::create_attachment(conn, new_attachment)?;
            }
        }
    }

    // Create article content if present
    if ticket_json.article_content.is_some() {
        let new_article_content = NewArticleContent {
            ticket_id: ticket.id,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
        };

        crate::repository::article_content::create_article_content(conn, new_article_content)?;
    }

    Ok(ticket)
}

// Ticket-Asset relationship functions
// sync-pending-wire: needs sync aggregate wiring
pub fn add_device_to_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    device_id: i32,
) -> QueryResult<TicketAsset> {
    let new_ticket_device = NewTicketAsset {
        ticket_id,
        asset_id: device_id,
    };

    diesel::insert_into(ticket_assets::table)
        .values(&new_ticket_device)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring
pub fn remove_device_from_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    device_id: i32,
) -> QueryResult<usize> {
    diesel::delete(
        ticket_assets::table
            .filter(ticket_assets::ticket_id.eq(ticket_id))
            .filter(ticket_assets::asset_id.eq(device_id)),
    )
    .execute(conn)
}

pub fn get_devices_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Asset>> {
    ticket_assets::table
        .inner_join(assets::table)
        .filter(ticket_assets::ticket_id.eq(ticket_id))
        .select(assets::all_columns)
        .load(conn)
}

/// Per-ticket affected-devices summary for the kanban / list / calendar
/// CardData payload. One round trip per bootstrap rather than N. Returns
/// `(count, first_device_id, first_device_name, first_device_os)` so
/// the consumer can build the spec'd `affected_devices` shape without
/// joining devices a second time. Tickets without any device link are
/// omitted; the consumer defaults those to `null`.
pub fn devices_summary_for_tickets(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, (i64, i32, String, Option<String>)>> {
    use diesel::dsl::sql;
    use diesel::sql_types::BigInt;

    if ticket_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    // Per-ticket counts.
    let counts: Vec<(i32, i64)> = ticket_assets::table
        .filter(ticket_assets::ticket_id.eq_any(ticket_ids))
        .group_by(ticket_assets::ticket_id)
        .select((ticket_assets::ticket_id, sql::<BigInt>("COUNT(*)")))
        .load(conn)?;

    // Pick the lowest-id device per ticket as the "first" — stable
    // across reads, no NULLs to break sort. The kanban only renders
    // the count + name; if a richer "primary device" model arrives
    // (criticality-weighted, manual pin) we swap the picker here.
    //
    // OS used to live as its own column; Pass B moved it into the
    // attributes JSONB so the select extracts it via JSON path.
    let firsts: Vec<(i32, i32, String, Option<String>)> = ticket_assets::table
        .inner_join(assets::table)
        .filter(ticket_assets::ticket_id.eq_any(ticket_ids))
        .order((
            ticket_assets::ticket_id.asc(),
            ticket_assets::asset_id.asc(),
        ))
        .select((
            ticket_assets::ticket_id,
            assets::id,
            assets::name,
            sql::<diesel::sql_types::Nullable<diesel::sql_types::Text>>(
                "attributes->>'operating_system'",
            ),
        ))
        .distinct_on(ticket_assets::ticket_id)
        .load(conn)?;

    let mut by_count: std::collections::HashMap<i32, i64> = counts.into_iter().collect();
    let mut out = std::collections::HashMap::new();
    for (ticket_id, device_id, name, os) in firsts {
        let count = by_count.remove(&ticket_id).unwrap_or(0);
        out.insert(ticket_id, (count, device_id, name, os));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_priority_known_values() {
        assert_eq!(parse_ticket_priority("low"), TicketPriority::Low);
        assert_eq!(parse_ticket_priority("medium"), TicketPriority::Medium);
        assert_eq!(parse_ticket_priority("high"), TicketPriority::High);
    }

    #[test]
    fn parse_priority_unknown_defaults_to_medium() {
        assert_eq!(parse_ticket_priority("critical"), TicketPriority::Medium);
        assert_eq!(parse_ticket_priority(""), TicketPriority::Medium);
    }

    #[test]
    fn extract_storage_path_tickets() {
        assert_eq!(
            extract_storage_path_from_url("/uploads/tickets/abc.pdf"),
            Some("tickets/abc.pdf".into())
        );
    }

    #[test]
    fn extract_storage_path_temp() {
        assert_eq!(
            extract_storage_path_from_url("/uploads/temp/xyz.png"),
            Some("temp/xyz.png".into())
        );
    }

    #[test]
    fn extract_storage_path_unknown_returns_none() {
        assert_eq!(extract_storage_path_from_url("/other/path.pdf"), None);
        assert_eq!(
            extract_storage_path_from_url("https://example.com/file"),
            None
        );
    }

    // ---- Guest-submission helpers ----

    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    /// Insert a ticket with an explicit verification state. The shared
    /// fixture helper always passes `None`, which is the wrong shape for
    /// testing the pending-release path.
    fn insert_ticket_with_state(
        conn: &mut DbConnection,
        requester: Uuid,
        state: Option<&str>,
    ) -> Ticket {
        let default_state = crate::repository::workflow_states::default_state(conn)
            .expect("default workflow state must exist");
        let new_ticket = NewTicket {
            title: "T".into(),
            workflow_state_id: default_state.id,
            requester_uuid: Some(requester),
            submitted_via: Some("guest".into()),
            guest_lookup_token: Some(Uuid::new_v4()),
            verification_state: state.map(|s| s.to_string()),
            ..Default::default()
        };
        diesel::insert_into(tickets::table)
            .values(&new_ticket)
            .get_result(conn)
            .expect("insert ticket")
    }

    #[test]
    fn verify_pending_flips_only_requesters_pending_tickets() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "Alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "Bob", UserRole::User);

        // Alice has two pending + one already-verified + one nullable
        // (authenticated) ticket. Bob has one pending. Only Alice's two
        // pending tickets should flip.
        let alice_pending_1 = insert_ticket_with_state(&mut conn, alice.uuid, Some("pending"));
        let alice_pending_2 = insert_ticket_with_state(&mut conn, alice.uuid, Some("pending"));
        let alice_verified = insert_ticket_with_state(&mut conn, alice.uuid, Some("verified"));
        let alice_null = insert_ticket_with_state(&mut conn, alice.uuid, None);
        let bob_pending = insert_ticket_with_state(&mut conn, bob.uuid, Some("pending"));

        let released =
            verify_pending_tickets_for_user(&mut conn, alice.uuid).expect("verify returns");

        let released_ids: Vec<i32> = released.iter().map(|t| t.id).collect();
        assert_eq!(released_ids.len(), 2);
        assert!(released_ids.contains(&alice_pending_1.id));
        assert!(released_ids.contains(&alice_pending_2.id));

        // Already-verified, null, and Bob's pending should be untouched.
        let mut fetch = |id| get_ticket_by_id(&mut conn, id).unwrap();
        assert_eq!(
            fetch(alice_verified.id).verification_state.as_deref(),
            Some("verified")
        );
        assert_eq!(fetch(alice_null.id).verification_state, None);
        assert_eq!(
            fetch(bob_pending.id).verification_state.as_deref(),
            Some("pending")
        );
    }

    #[test]
    fn verify_pending_noop_when_user_has_no_pending() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "NoPending", UserRole::User);
        let released = verify_pending_tickets_for_user(&mut conn, user.uuid).unwrap();
        assert!(released.is_empty());
    }

    #[test]
    fn find_by_lookup_token_returns_match() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Lookup", UserRole::User);
        let ticket = insert_ticket_with_state(&mut conn, user.uuid, Some("verified"));
        let token = ticket.guest_lookup_token.expect("fixture sets a token");

        let found = find_by_lookup_token(&mut conn, token).expect("found");
        assert_eq!(found.id, ticket.id);
    }

    #[test]
    fn find_by_lookup_token_not_found_for_random_uuid() {
        let mut conn = setup_test_connection();
        let result = find_by_lookup_token(&mut conn, Uuid::new_v4());
        assert!(matches!(result, Err(diesel::result::Error::NotFound)));
    }

    // ---- Phase 3a: workspace isolation matrix ----
    //
    // Verifies the `tickets_workspace_isolation` RLS policy against
    // the GUC-driven session state. Every Phase 3c policy will reuse
    // the same template, but the matrix runs only here for now; once
    // 3c lands and 3f adds the per-table sweep, the shared isolation
    // harness can subsume this.

    use crate::sync::actor::ActorContext;
    use crate::sync::session::{with_actor_bypass_context, with_actor_context};

    /// Seed a second workspace + one ticket per workspace. Returns
    /// (ws1_ticket_id, ws2_ticket_id). Setup uses `bypass_context` so
    /// the seed crosses workspaces deliberately.
    fn seed_two_workspaces(conn: &mut DbConnection) -> (i32, i32) {
        let admin = ActorContext::system("rls.test");

        with_actor_bypass_context(conn, &admin, |c| {
            // Workspace 2 (workspace 1 already exists from the
            // bootstrap migration).
            diesel::sql_query(
                "INSERT INTO workspaces (id, slug, name) \
                 VALUES (2, 'other', 'Other Workspace')",
            )
            .execute(c)?;

            // Reset the workspaces sequence so subsequent inserts
            // don't collide with the explicit id=2. Postgres sequences
            // don't auto-advance on explicit inserts.
            diesel::sql_query("SELECT setval('workspaces_id_seq', 2, true)").execute(c)?;

            Ok::<(), diesel::result::Error>(())
        })
        .expect("seed workspace 2");

        let state = crate::repository::workflow_states::default_state(conn)
            .expect("default workflow state seeded");

        // Bootstrap workspace 1 ticket via the column default.
        let t1: Ticket = with_actor_context(conn, &admin.clone().with_workspace(1), |c| {
            let new = NewTicket {
                title: "ws1 ticket".into(),
                workflow_state_id: state.id,
                ..Default::default()
            };
            diesel::insert_into(tickets::table)
                .values(&new)
                .get_result::<Ticket>(c)
        })
        .expect("insert ws1 ticket");

        // Workspace 2 ticket needs an explicit workspace_id; raw SQL
        // because NewTicket doesn't carry workspace_id yet (the
        // column-default-from-GUC swap lands in 3c).
        let t2_id: i32 = with_actor_bypass_context(conn, &admin, |c| {
            #[derive(diesel::QueryableByName)]
            struct IdRow {
                #[diesel(sql_type = diesel::sql_types::Integer)]
                id: i32,
            }
            let row: IdRow = diesel::sql_query(
                "INSERT INTO tickets (title, workflow_state_id, priority, workspace_id) \
                 VALUES ('ws2 ticket', $1, 'medium', 2) RETURNING id",
            )
            .bind::<diesel::sql_types::Integer, _>(state.id)
            .get_result(c)?;
            Ok::<i32, diesel::result::Error>(row.id)
        })
        .expect("insert ws2 ticket");

        (t1.id, t2_id)
    }

    fn count_visible_tickets(conn: &mut DbConnection) -> i64 {
        tickets::table
            .count()
            .get_result::<i64>(conn)
            .expect("count tickets")
    }

    #[test]
    fn rls_workspace_1_sees_only_workspace_1_tickets() {
        let mut conn = setup_test_connection();
        let (t1, t2) = seed_two_workspaces(&mut conn);
        let actor = ActorContext::system("rls.test").with_workspace(1);

        let visible: Vec<i32> = with_actor_context(&mut conn, &actor, |c| {
            tickets::table.select(tickets::id).load::<i32>(c)
        })
        .expect("query tickets");

        assert!(visible.contains(&t1), "workspace 1 should see ws1 ticket");
        assert!(
            !visible.contains(&t2),
            "workspace 1 must NOT see ws2 ticket"
        );
    }

    #[test]
    fn rls_workspace_2_sees_only_workspace_2_tickets() {
        let mut conn = setup_test_connection();
        let (t1, t2) = seed_two_workspaces(&mut conn);
        let actor = ActorContext::system("rls.test").with_workspace(2);

        let visible: Vec<i32> = with_actor_context(&mut conn, &actor, |c| {
            tickets::table.select(tickets::id).load::<i32>(c)
        })
        .expect("query tickets");

        assert!(visible.contains(&t2), "workspace 2 should see ws2 ticket");
        assert!(
            !visible.contains(&t1),
            "workspace 2 must NOT see ws1 ticket"
        );
    }

    #[test]
    fn rls_unset_workspace_returns_no_rows() {
        let mut conn = setup_test_connection();
        let _ = seed_two_workspaces(&mut conn);
        // Explicitly clear the ambient workspace GUC: simulates the
        // production failure mode where neither the request middleware
        // nor a background-job operator pinned a workspace. Strict
        // policy returns zero rows rather than silently leaking.
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false)")
            .execute(&mut conn)
            .expect("clear workspace GUC");
        let actor = ActorContext::system("rls.test");

        let count = with_actor_context(&mut conn, &actor, |c| {
            Ok::<i64, diesel::result::Error>(count_visible_tickets(c))
        })
        .expect("query tickets");

        assert_eq!(count, 0, "unset workspace must surface zero rows");
    }

    #[test]
    fn rls_bypass_context_sees_all_workspaces() {
        let mut conn = setup_test_connection();
        let (t1, t2) = seed_two_workspaces(&mut conn);
        let actor = ActorContext::system("rls.test");

        let visible: Vec<i32> = with_actor_bypass_context(&mut conn, &actor, |c| {
            tickets::table.select(tickets::id).load::<i32>(c)
        })
        .expect("query tickets");

        assert!(visible.contains(&t1));
        assert!(visible.contains(&t2));
    }

    #[test]
    fn rls_insert_with_mismatched_workspace_is_rejected() {
        let mut conn = setup_test_connection();
        let _ = seed_two_workspaces(&mut conn);
        let state = crate::repository::workflow_states::default_state(&mut conn)
            .expect("default workflow state seeded");
        let actor = ActorContext::system("rls.test").with_workspace(1);

        // While pinned to workspace 1, attempt to insert a row that
        // claims workspace 2. RLS WITH CHECK rejects as a policy
        // violation surfaced through Diesel as a generic db error.
        let result = with_actor_context(&mut conn, &actor, |c| {
            diesel::sql_query(
                "INSERT INTO tickets (title, workflow_state_id, priority, workspace_id) \
                 VALUES ('forbidden', $1, 'medium', 2)",
            )
            .bind::<diesel::sql_types::Integer, _>(state.id)
            .execute(c)
        });

        assert!(
            result.is_err(),
            "cross-workspace insert must fail the WITH CHECK"
        );
    }

    #[test]
    fn rls_update_cannot_reach_other_workspace_row() {
        let mut conn = setup_test_connection();
        let (_, t2) = seed_two_workspaces(&mut conn);
        let actor = ActorContext::system("rls.test").with_workspace(1);

        // A workspace 1 actor running UPDATE against a workspace 2
        // row sees the row filtered out by the USING clause, so the
        // UPDATE affects zero rows (silent no-op, by design of RLS).
        let affected = with_actor_context(&mut conn, &actor, |c| {
            diesel::update(tickets::table.filter(tickets::id.eq(t2)))
                .set(tickets::title.eq("tampered"))
                .execute(c)
        })
        .expect("update returns");

        assert_eq!(
            affected, 0,
            "workspace 1 actor must not be able to update workspace 2 rows"
        );

        // Verify the row wasn't touched (read via bypass).
        let admin = ActorContext::system("rls.test");
        let title: String = with_actor_bypass_context(&mut conn, &admin, |c| {
            tickets::table
                .filter(tickets::id.eq(t2))
                .select(tickets::title)
                .first::<String>(c)
        })
        .expect("read ws2 ticket via bypass");
        assert_eq!(title, "ws2 ticket");
    }

    // ---- Phase 3h.7: substrate-hardening regression tests ----
    //
    // Four cases the external review identified as missing from
    // the original 6-case matrix:
    //   1. stale-GUC-after-error — txn rollback must clear GUCs
    //      so a subsequent operation on the same conn doesn't
    //      inherit them.
    //   2. forgot-the-wrapper — a raw conn under nosdesk_app
    //      with no GUC set must see zero rows on every tenant
    //      table. Proves the strict-policy fail-closed
    //      guarantee in regression form.
    //   3. subquery-bypass attempt — `INSERT ... SELECT FROM ...`
    //      that tries to copy across workspaces must fail the
    //      WITH CHECK.
    //   4. bypass-context leak resistance — after a
    //      with_actor_bypass_context txn, the next
    //      with_actor_context call must NOT still see bypass /
    //      admin role. Proves the baseline-role-reset in
    //      set_actor works under savepoint nesting.

    #[test]
    fn rls_workspace_guc_clears_on_txn_rollback() {
        // The substrate uses txn-scoped set_config (the SET LOCAL
        // form). When the txn rolls back, the GUC must revert to
        // its pre-txn value. Without this guarantee, a panic
        // inside tc.run could leak the workspace context into the
        // next pool checkout.
        let mut conn = setup_test_connection();
        let actor = ActorContext::system("rls.test").with_workspace(2);

        // Run a closure that errors, forcing the savepoint to
        // roll back. with_actor_context inside this would have
        // set app.workspace_id = '2'; we want to confirm the
        // rollback reverts it.
        let _ = with_actor_context(&mut conn, &actor, |_c| {
            Err::<(), diesel::result::Error>(diesel::result::Error::RollbackTransaction)
        });

        // After rollback, the workspace GUC should be back at
        // the ambient '1' that setup_test_connection seeded.
        #[derive(diesel::QueryableByName)]
        struct GucReadback {
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
            current_setting: Option<String>,
        }
        let row: GucReadback = diesel::sql_query(
            "SELECT current_setting('app.workspace_id', true) AS current_setting",
        )
        .get_result(&mut conn)
        .expect("read GUC");
        assert_eq!(
            row.current_setting.as_deref(),
            Some("1"),
            "workspace GUC must revert to ambient on txn rollback"
        );
    }

    #[test]
    fn rls_forgot_the_wrapper_returns_zero_rows() {
        // The contract: a code path that touches a tenant table
        // without going through TenantConn / with_actor_context
        // gets the strict policy's fail-closed empty result. This
        // is what guarantees that "forgot to wrap" surfaces as a
        // zero-row bug in staging rather than a silent cross-
        // tenant leak.
        let mut conn = setup_test_connection();
        let _ = seed_two_workspaces(&mut conn);

        // After seed_two_workspaces (which uses bypass internally
        // and leaves the role elevated to nosdesk_admin at the
        // outer-txn level), restore the production role baseline
        // and clear the ambient GUC. This is "fresh production
        // pool checkout": nosdesk_app + no workspace pinned.
        diesel::sql_query("SET LOCAL ROLE nosdesk_app")
            .execute(&mut conn)
            .expect("reset role");
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false)")
            .execute(&mut conn)
            .expect("clear GUC");

        let count: i64 = tickets::table.count().get_result(&mut conn).expect("count");
        assert_eq!(
            count, 0,
            "no workspace GUC must surface zero rows; \
             if you weakened the policy to default-permit, this assert catches it"
        );
    }

    #[test]
    fn rls_insert_select_cross_workspace_is_rejected() {
        // Subquery bypass attempt: pinned to workspace 1, try
        // to INSERT a copy of a workspace 2 row by reading it
        // via SELECT inside the INSERT. The SELECT side returns
        // zero rows under the strict policy (USING clause), so
        // the INSERT inserts nothing — fail-closed. Even if the
        // SELECT side somehow leaked the workspace 2 row, the
        // WITH CHECK would reject the workspace_id = 2 insert
        // while pinned to 1. This is the belt-and-braces test.
        let mut conn = setup_test_connection();
        let (_, t2) = seed_two_workspaces(&mut conn);
        let actor = ActorContext::system("rls.test").with_workspace(1);

        let result = with_actor_context(&mut conn, &actor, |c| {
            // Try to copy the ws2 ticket as a new ws2 ticket
            // while pinned to ws1.
            diesel::sql_query(format!(
                "INSERT INTO tickets (title, workflow_state_id, priority, workspace_id) \
                 SELECT title, workflow_state_id, priority, 2 FROM tickets WHERE id = {}",
                t2
            ))
            .execute(c)
        });

        // The SELECT side filters to zero rows under workspace
        // 1's policy, so INSERT 0. That's fail-closed (no leak).
        // The result is Ok(0), not an error.
        let affected = result.expect("insert returns");
        assert_eq!(
            affected, 0,
            "subquery against another workspace's row must surface as zero-row insert"
        );

        // Also verify the policy rejects the explicit cross-
        // workspace INSERT (the same INSERT with VALUES rather
        // than SELECT): this case is the original 6-case
        // matrix's INSERT-rejection test in slightly different
        // form.
        let state =
            crate::repository::workflow_states::default_state(&mut conn).expect("default state");
        let direct_attempt = with_actor_context(&mut conn, &actor, |c| {
            diesel::sql_query(format!(
                "INSERT INTO tickets (title, workflow_state_id, priority, workspace_id) \
                 VALUES ('hop', {}, 'medium', 2)",
                state.id
            ))
            .execute(c)
        });
        assert!(
            direct_attempt.is_err(),
            "explicit cross-workspace INSERT must fail WITH CHECK"
        );
    }

    #[test]
    fn rls_bypass_does_not_leak_into_subsequent_actor_context() {
        // The bypass primitive elevates to nosdesk_admin via
        // SET LOCAL ROLE. Savepoint commit propagates SET LOCAL
        // to the outer txn (Postgres semantics), so without the
        // baseline-role reset in set_actor, a sequence of
        // (with_actor_bypass_context, with_actor_context) would
        // leave the second call still elevated, silently
        // bypassing RLS. Regression test for set_actor's
        // `SET LOCAL ROLE nosdesk_app` baseline.
        let mut conn = setup_test_connection();
        let (t1, t2) = seed_two_workspaces(&mut conn);

        // First, do a bypass — sees all rows.
        let admin = ActorContext::system("rls.test");
        let all_visible: Vec<i32> = with_actor_bypass_context(&mut conn, &admin, |c| {
            tickets::table.select(tickets::id).load::<i32>(c)
        })
        .expect("bypass query");
        assert!(all_visible.contains(&t1));
        assert!(all_visible.contains(&t2));

        // Now do a non-bypass call pinned to workspace 1. If
        // the bypass role leaked, this would see both tickets;
        // it must see only ws1.
        let actor = ActorContext::system("rls.test").with_workspace(1);
        let scoped_visible: Vec<i32> = with_actor_context(&mut conn, &actor, |c| {
            tickets::table.select(tickets::id).load::<i32>(c)
        })
        .expect("scoped query");
        assert!(scoped_visible.contains(&t1));
        assert!(
            !scoped_visible.contains(&t2),
            "bypass role must not leak into a subsequent scoped call; \
             set_actor's baseline reset is the defense"
        );
    }

    // ---- Phase 3i.2: UNIQUE-index workspace-composite guard ----
    //
    // Bytebase RLS footgun #8 (cross-tenant existence leak via
    // duplicate-key) is the threat. Phase 3h.8 caught named UNIQUE
    // constraints; this guard catches the rest by scanning
    // pg_indexes for UNIQUE indexes on tenant tables (tables with a
    // workspace_id column) where workspace_id is not part of the
    // indexed column list. Any hit is either a real leak, a Phase
    // 3i workspace-lifecycle blocker, or an intent-questionable
    // index that needs an explicit allowlist entry below.
    //
    // ALLOWLIST policy: add an entry only with a product decision
    // attached. The default is "make it composite." The allowlist
    // is small on purpose — each entry is a known cross-workspace
    // semantic that must be documented inline.
    #[test]
    fn no_unique_indexes_on_tenant_tables_omit_workspace_id() {
        let mut conn = setup_test_connection();

        // Pkeys + uuid-keyed unique indexes are intrinsically unique
        // by ID / by UUID-generation — no cross-tenant leak surface.
        // Composite-with-FK-to-tenant indexes are scoped through the
        // FK chain (e.g. channel_id, plugin_id, page_id all FK into
        // tenant tables). The intent-questionable allowlist below is
        // for indexes where the global UNIQUE shape is a deliberate
        // product decision documented elsewhere.
        let allowlist: &[&str] = &[
            // Real-world device serial numbers are globally unique by
            // manufacturer. Two tenants importing the same physical
            // device is a defensible global UNIQUE — flagged for
            // product decision but not a leak per se.
            "idx_asset_serial_unique",
            // Azure object IDs are GUIDs. Cross-tenant overlap doesn't
            // happen in practice; not a leak.
            "idx_groups_external_id",
            // P2 follow-up (per 3h.8 migration header): cross-workspace
            // notification prefs are a correctness question, not a
            // duplicate-key existence leak.
            "notification_preferences_user_uuid_notification_type_id_cha_key",
            // FK-chain-scoped composites: the indexed columns include
            // an FK to a tenant table, so cross-tenant collisions
            // can't happen through them.
            "cycles_one_active_per_project",
            "cycle_tickets_one_per_ticket",
            "documentation_page_embeddings_pkey",
            "documentation_page_tickets_pkey",
            "documentation_collection_pages_pkey",
            "documentation_collection_pages_page_id_key",
            "asset_audits_pkey",
            "channel_credentials_pkey",
            "channel_credentials_channel_id_credential_type_key",
            "channel_messages_pkey",
            "channel_messages_channel_id_external_id_direction_key",
            "category_group_visibility_pkey",
            "article_content_revisions_pkey",
            "article_content_revisions_article_content_id_revision_numbe_key",
            "assignment_rule_state_pkey",
            "device_groups_pkey",
            "documentation_revisions_pkey",
            "documentation_revisions_page_id_revision_number_key",
            "group_includes_pkey",
            "knowledge_gap_signals_pkey",
            "knowledge_gap_signals_gap_id_source_kind_source_ref_key",
            "linked_tickets_pkey",
            "outbound_emails_comment_id_key",
            "outbound_emails_message_id_key",
            "plugin_collection_schemas_plugin_id_collection_name_key",
            "plugin_data_plugin_id_data_type_key_key",
            "project_tickets_pkey",
            "ticket_devices_pkey",
            "ticket_tags_pkey",
            "ticket_watchers_pkey",
            "tickets_guest_lookup_token_key",
            "user_groups_pkey",
            "user_ticket_views_user_uuid_ticket_id_key",
            "working_calendar_holidays_calendar_id_date_key",
            // FK-chain-scoped via page_id (serial PK on
            // documentation_pages, globally unique). A user
            // starring/subscribing-to the same page from two
            // workspaces is impossible because page_id itself is
            // globally unique.
            "documentation_starred_pages_user_uuid_page_id_key",
            "documentation_subscriptions_user_uuid_page_id_key",
            // sync_actions.client_tx_id_idx is ON ONLY parent,
            // doesn't propagate to partitions. INSERTs route to
            // partitions, so the parent-only index doesn't
            // actively enforce. The intended idempotency check is
            // already cross-workspace by the client-supplied
            // tx_id semantic (client_tx_id is the caller's
            // dedup key; cross-workspace collisions are
            // structurally possible but operationally rare —
            // clients use UUIDs). Documented as Phase 3i
            // follow-up to make the index workspace-aware.
            "sync_actions_client_tx_id_idx",
        ];

        #[derive(diesel::QueryableByName, Debug)]
        struct LeakyIndex {
            #[diesel(sql_type = diesel::sql_types::Text)]
            tablename: String,
            #[diesel(sql_type = diesel::sql_types::Text)]
            indexname: String,
            #[diesel(sql_type = diesel::sql_types::Text)]
            indexdef: String,
        }

        // Find UNIQUE indexes on tenant tables (tables with
        // workspace_id) whose definition doesn't mention
        // workspace_id. Pkey + uuid-keyed indexes are
        // intrinsically unique; filter them out below.
        let candidates: Vec<LeakyIndex> = diesel::sql_query(
            "SELECT tablename, indexname, indexdef \
             FROM pg_indexes \
             WHERE schemaname = 'public' \
               AND tablename IN ( \
                 SELECT table_name FROM information_schema.columns \
                 WHERE column_name = 'workspace_id' AND table_schema = 'public' \
               ) \
               AND indexdef LIKE '%UNIQUE%' \
               AND indexdef NOT LIKE '%workspace_id%' \
               AND tablename NOT LIKE 'audit_log_%' \
               AND tablename NOT LIKE 'sync_actions_%' \
             ORDER BY tablename, indexname",
        )
        .load(&mut conn)
        .expect("query pg_indexes");

        let leaks: Vec<_> = candidates
            .into_iter()
            .filter(|row| {
                // Intrinsically unique by ID / UUID — no leak.
                !row.indexname.ends_with("_pkey")
                    && !row.indexname.ends_with("_uuid_key")
                    && !row.indexname.ends_with("_token_hash_key")
                    && !allowlist.contains(&row.indexname.as_str())
            })
            .collect();

        if !leaks.is_empty() {
            let mut msg = String::from(
                "UNIQUE indexes on tenant tables missing workspace_id (Bytebase footgun #8):\n",
            );
            for leak in &leaks {
                msg.push_str(&format!(
                    "  - {}.{}: {}\n",
                    leak.tablename, leak.indexname, leak.indexdef
                ));
            }
            msg.push_str(
                "\nFix: flip to composite (workspace_id, ...) in a migration, OR \
                 if globally-unique by intent, add to the allowlist in this test \
                 with a one-line product-decision comment.",
            );
            panic!("{}", msg);
        }
    }

    // ---- Phase 3i.7: substrate hardening regression tests ----

    #[test]
    fn nosdesk_app_grant_of_nosdesk_admin_has_inherit_option_false() {
        // The 3h.6 migration set the explicit INHERIT FALSE on the
        // GRANT nosdesk_admin TO nosdesk_app because Postgres 16+
        // tracks the inherit option on the membership row, not
        // just the role attribute. Without INHERIT FALSE on the
        // grant, nosdesk_app silently inherits BYPASSRLS via
        // membership and every RLS policy becomes optional. This
        // test reads the grant row directly so any future
        // migration that re-grants without INHERIT FALSE fails CI
        // before it ships.
        let mut conn = setup_test_connection();

        #[derive(diesel::QueryableByName)]
        struct InheritRow {
            #[diesel(sql_type = diesel::sql_types::Bool)]
            inherit_option: bool,
        }
        let row: InheritRow = diesel::sql_query(
            "SELECT m.inherit_option \
             FROM pg_auth_members m \
             JOIN pg_roles r ON r.oid = m.roleid \
             JOIN pg_roles g ON g.oid = m.member \
             WHERE r.rolname = 'nosdesk_admin' AND g.rolname = 'nosdesk_app'",
        )
        .get_result(&mut conn)
        .expect("nosdesk_app must be a member of nosdesk_admin");

        assert!(
            !row.inherit_option,
            "GRANT nosdesk_admin TO nosdesk_app must carry INHERIT FALSE; \
             without it, nosdesk_app picks up BYPASSRLS via membership and \
             every RLS policy becomes opt-in"
        );
    }

    #[test]
    fn audit_log_trigger_stamps_actor_uuid_from_guc() {
        // The substrate's audit_log_trigger reads the actor UUID
        // from app.actor_uuid (set by with_actor_context /
        // with_actor_bypass_context) and writes it into the
        // audit_log row. This is the only mechanism that ties an
        // INSERT/UPDATE/DELETE back to a specific user; if the
        // trigger ever stops reading the GUC, the audit trail
        // silently goes anonymous. Insert a ticket via
        // with_actor_context with a known user UUID, then read
        // audit_log via bypass and confirm the actor_uuid column
        // matches.
        let mut conn = setup_test_connection();
        let user_uuid = uuid::Uuid::now_v7();
        let state = crate::repository::workflow_states::default_state(&mut conn)
            .expect("default workflow state");

        let actor = ActorContext::user(user_uuid, None).with_workspace(1);
        let ticket = with_actor_context(&mut conn, &actor, |c| {
            let new = NewTicket {
                title: "audit trigger probe".into(),
                workflow_state_id: state.id,
                ..Default::default()
            };
            diesel::insert_into(tickets::table)
                .values(&new)
                .get_result::<Ticket>(c)
        })
        .expect("insert ticket under user actor");

        #[derive(diesel::QueryableByName)]
        struct AuditRow {
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
            actor_uuid: Option<uuid::Uuid>,
        }
        let admin = ActorContext::system("rls.test");
        let row: AuditRow = with_actor_bypass_context(&mut conn, &admin, |c| {
            diesel::sql_query(
                "SELECT actor_uuid FROM audit_log \
                 WHERE table_name = 'tickets' AND op = 'I' AND pk_text = $1 \
                 ORDER BY occurred_at DESC LIMIT 1",
            )
            .bind::<diesel::sql_types::Text, _>(ticket.id.to_string())
            .get_result(c)
        })
        .expect("read audit_log row");

        assert_eq!(
            row.actor_uuid,
            Some(user_uuid),
            "audit_log trigger must stamp actor_uuid from the app.actor_uuid GUC"
        );
    }
}
