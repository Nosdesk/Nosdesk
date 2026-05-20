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
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::sync::session;
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
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: result.id.to_string(),
                op: SyncOp::Update,
                event_type,
                data: json!({
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
                }),
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
pub async fn delete_ticket_with_cleanup(
    conn: &mut DbConnection,
    ticket_id: i32,
    storage: Arc<dyn Storage>,
    observer: Option<&dyn TicketDeletedObserver>,
    actor: &ActorContext,
) -> Result<usize, Error> {
    // Start a transaction to ensure all operations succeed or fail together
    conn.transaction(|conn| {
        // Set the session-local actor GUC so the delete event below
        // (and any cascade triggers) carry the right attribution.
        session::set_actor(conn, actor)?;
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
                diesel::delete(crate::schema::attachments::table.find(attachment.id))
                    .execute(conn)?;
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
            crate::schema::ticket_devices::table
                .filter(crate::schema::ticket_devices::ticket_id.eq(ticket_id)),
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
            if let Some(_t) = pre_delete.as_ref() {
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

        // Return the attachment paths for file cleanup (outside transaction)
        Ok((result, attachment_paths))
    })
    .map(|(result, attachment_paths)| {
        // Clean up files after successful database transaction
        // This is done outside the transaction to avoid blocking the database
        tokio::spawn(async move {
            for path in attachment_paths {
                if let Err(e) = storage.delete_file(&path).await {
                    warn!(path, error = ?e, "Failed to delete file during ticket cleanup");
                }
            }
        });
        // Notify the search observer once the row is gone. Skipped on
        // result == 0 (ticket didn't exist) so we don't drop a
        // phantom delete into the index.
        if result > 0 {
            if let Some(observer) = observer {
                observer.ticket_deleted(ticket_id);
            }
        }
        result
    })
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
            Some(crate::services::sla::compute_pill(
                &ticket,
                category,
                policy,
                calendar,
                &holidays,
                chrono::Utc::now(),
            ))
        })
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

    // Create device if present (without ticket association)
    if let Some(device_json) = &ticket_json.device {
        let new_device = NewDevice {
            name: device_json.name.clone(),
            hostname: Some(device_json.hostname.clone()),
            device_type: None,
            serial_number: Some(device_json.serial_number.clone()),
            manufacturer: None, // Will be populated during Microsoft Entra sync
            model: Some(device_json.model.clone()),
            warranty_status: Some(device_json.warranty_status.clone()),
            location: None,
            notes: None,
            primary_user_uuid: None, // Will be populated during Microsoft Entra sync
            microsoft_device_id: None,
            intune_device_id: None,
            entra_device_id: None,
            compliance_state: None,
            last_sync_time: None,
            operating_system: None,
            os_version: None,
            is_managed: None,
            enrollment_date: None,
            warranty_start_date: None,
            warranty_end_date: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: serde_json::json!({}),
            quantity: None,
            unit: None,
        };

        crate::repository::devices::create_device(conn, new_device)?;
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

// Ticket-Device relationship functions
// sync-pending-wire: needs sync aggregate wiring
pub fn add_device_to_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    device_id: i32,
) -> QueryResult<TicketDevice> {
    let new_ticket_device = NewTicketDevice {
        ticket_id,
        asset_id: device_id,
    };

    diesel::insert_into(ticket_devices::table)
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
        ticket_devices::table
            .filter(ticket_devices::ticket_id.eq(ticket_id))
            .filter(ticket_devices::asset_id.eq(device_id)),
    )
    .execute(conn)
}

pub fn get_devices_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Device>> {
    ticket_devices::table
        .inner_join(devices::table)
        .filter(ticket_devices::ticket_id.eq(ticket_id))
        .select(devices::all_columns)
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
    let counts: Vec<(i32, i64)> = ticket_devices::table
        .filter(ticket_devices::ticket_id.eq_any(ticket_ids))
        .group_by(ticket_devices::ticket_id)
        .select((ticket_devices::ticket_id, sql::<BigInt>("COUNT(*)")))
        .load(conn)?;

    // Pick the lowest-id device per ticket as the "first" — stable
    // across reads, no NULLs to break sort. The kanban only renders
    // the count + name; if a richer "primary device" model arrives
    // (criticality-weighted, manual pin) we swap the picker here.
    let firsts: Vec<(i32, i32, String, Option<String>)> = ticket_devices::table
        .inner_join(devices::table)
        .filter(ticket_devices::ticket_id.eq_any(ticket_ids))
        .order((
            ticket_devices::ticket_id.asc(),
            ticket_devices::asset_id.asc(),
        ))
        .select((
            ticket_devices::ticket_id,
            devices::id,
            devices::name,
            devices::operating_system,
        ))
        .distinct_on(ticket_devices::ticket_id)
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
}
