//! Tags repository.
//!
//! Workspace-scoped namespace of free-form labels, plus the
//! ticket↔tag join. Tag CRUD (create / rename / archive) is an
//! admin operation; assignment to tickets is a per-staff
//! operation. Both emit through the same path because they share
//! the `tags` and `ticket_tags` tables.
//!
//! Tag rows themselves are NOT a sync aggregate today — workspace
//! config changes infrequently and the picker re-fetches on
//! demand. Ticket→tag *assignments* DO surface on the ticket's
//! sync_actions stream as `ticket.tags_changed` so the list view
//! / detail view re-render when a tag is attached or removed.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewTag, NewTicketTag, SyncAggregate, SyncOp, Tag, TagUpdate, Ticket};
use crate::schema::{tags, ticket_tags, tickets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

// ---- Tag CRUD --------------------------------------------------

pub fn list_tags(conn: &mut DbConnection, include_archived: bool) -> QueryResult<Vec<Tag>> {
    let mut q = tags::table.into_boxed();
    if !include_archived {
        q = q.filter(tags::archived_at.is_null());
    }
    q.order(tags::name.asc()).load(conn)
}

pub fn get_tag(conn: &mut DbConnection, id: i32) -> QueryResult<Tag> {
    tags::table.find(id).first(conn)
}

// sync-audit-only: Tag CRUD — tags are NOT a sync aggregate (workspace config changes infrequently, picker re-fetches on demand). Ticket↔ tag assignment IS sync-wired via the `ticket.tags_changed` event in `tags::set_tags_for_ticket`
pub fn create_tag(conn: &mut DbConnection, new_tag: NewTag) -> QueryResult<Tag> {
    diesel::insert_into(tags::table)
        .values(&new_tag)
        .get_result(conn)
}

// sync-audit-only: Tag CRUD — tags are NOT a sync aggregate (workspace config changes infrequently, picker re-fetches on demand). Ticket↔ tag assignment IS sync-wired via the `ticket.tags_changed` event in `tags::set_tags_for_ticket`
pub fn update_tag(conn: &mut DbConnection, id: i32, update: TagUpdate) -> QueryResult<Tag> {
    diesel::update(tags::table.find(id))
        .set(&update)
        .get_result(conn)
}

// sync-audit-only: Tag CRUD — tags are NOT a sync aggregate (workspace config changes infrequently, picker re-fetches on demand). Ticket↔ tag assignment IS sync-wired via the `ticket.tags_changed` event in `tags::set_tags_for_ticket`
/// Soft-archive a tag (sets `archived_at`). The row stays so any
/// historical ticket→tag references keep their join target;
/// archived tags drop out of the picker.
pub fn archive_tag(conn: &mut DbConnection, id: i32) -> QueryResult<Tag> {
    diesel::update(tags::table.find(id))
        .set(tags::archived_at.eq(diesel::dsl::now))
        .get_result(conn)
}

// ---- Ticket↔tag join -------------------------------------------

/// Tag ids attached to a ticket, sorted by tag id for stable
/// rendering. Returns `Vec<i32>` rather than full Tag rows
/// because the bootstrap streamer + detail handler both want
/// just the ids; the workspace tag picker provides the
/// id → row mapping.
pub fn tag_ids_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<i32>> {
    ticket_tags::table
        .filter(ticket_tags::ticket_id.eq(ticket_id))
        .order(ticket_tags::tag_id.asc())
        .select(ticket_tags::tag_id)
        .load(conn)
}

/// Bulk variant for the bootstrap streamer — one query for the
/// full ticket set rather than one-per-ticket. Returns a map
/// keyed by ticket id.
pub fn tag_ids_for_tickets(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, Vec<i32>>> {
    use std::collections::HashMap;
    let rows: Vec<(i32, i32)> = ticket_tags::table
        .filter(ticket_tags::ticket_id.eq_any(ticket_ids))
        .order(ticket_tags::tag_id.asc())
        .select((ticket_tags::ticket_id, ticket_tags::tag_id))
        .load(conn)?;
    let mut out: HashMap<i32, Vec<i32>> = HashMap::new();
    for (tid, tag_id) in rows {
        out.entry(tid).or_default().push(tag_id);
    }
    Ok(out)
}

/// Replace the tag set for a ticket atomically. Computes the
/// diff against the current set and emits one
/// `ticket.tags_changed` sync_action with the resulting list so
/// the list / detail views refresh. Repeated assignments are
/// idempotent — sending the same set as is currently attached
/// is a no-op (no emit).
pub fn set_tags_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    desired_tag_ids: &[i32],
    actor_uuid: Option<Uuid>,
) -> QueryResult<Vec<i32>> {
    use std::collections::HashSet;

    conn.transaction::<Vec<i32>, diesel::result::Error, _>(|conn| {
        // Resolve the parent ticket up front so we can compute
        // the sync groups and surface a clear "no such ticket"
        // error rather than letting the FK insert fail later.
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;

        let current: HashSet<i32> = ticket_tags::table
            .filter(ticket_tags::ticket_id.eq(ticket_id))
            .select(ticket_tags::tag_id)
            .load::<i32>(conn)?
            .into_iter()
            .collect();
        let desired: HashSet<i32> = desired_tag_ids.iter().copied().collect();

        let to_add: Vec<i32> = desired.difference(&current).copied().collect();
        let to_remove: Vec<i32> = current.difference(&desired).copied().collect();

        if to_add.is_empty() && to_remove.is_empty() {
            // No-op — return the current set without an emit.
            let mut sorted: Vec<i32> = current.into_iter().collect();
            sorted.sort_unstable();
            return Ok(sorted);
        }

        if !to_remove.is_empty() {
            diesel::delete(
                ticket_tags::table
                    .filter(ticket_tags::ticket_id.eq(ticket_id))
                    .filter(ticket_tags::tag_id.eq_any(&to_remove)),
            )
            .execute(conn)?;
        }

        if !to_add.is_empty() {
            let new_rows: Vec<NewTicketTag> = to_add
                .iter()
                .map(|&tag_id| NewTicketTag {
                    ticket_id,
                    tag_id,
                    created_by: actor_uuid,
                })
                .collect();
            diesel::insert_into(ticket_tags::table)
                .values(&new_rows)
                .execute(conn)?;
        }

        // Emit one event with the full tag list so consumers
        // don't have to assemble it from add / remove deltas.
        // Same shape ticket.updated uses (full row data) so the
        // frontend's pool-update path stays uniform.
        let groups = groups::for_ticket(conn, &ticket)?;
        let mut sorted: Vec<i32> = desired.into_iter().collect();
        sorted.sort_unstable();
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.tags_changed",
                data: json!({
                    "id": ticket_id,
                    "tag_ids": sorted,
                    "added": to_add,
                    "removed": to_remove,
                }),
                groups,
                causation_id: None,
            },
        )?;

        Ok(sorted)
    })
}
