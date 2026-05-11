//! Ticket watchers repository.
//!
//! Read + write access to `ticket_watchers`. Watch / unwatch is
//! a per-user preference; the comment-notification flow reads
//! the watcher set when fanning out so subscribers get notified
//! even when they aren't the requester or assignee.
//!
//! Watch toggles emit a `ticket.watch_added` / `.watch_removed`
//! sync_action so the detail sidebar's watcher list updates
//! across tabs / devices in real time.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewTicketWatcher, SyncAggregate, SyncOp, TicketWatcher};
use crate::schema::{ticket_watchers, tickets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Watcher uuids for a ticket, sorted for stable rendering.
/// Drives the sidebar list + the comment-notification fan-out
/// for public replies.
pub fn watcher_uuids(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Uuid>> {
    ticket_watchers::table
        .filter(ticket_watchers::ticket_id.eq(ticket_id))
        .order(ticket_watchers::created_at.asc())
        .select(ticket_watchers::user_uuid)
        .load(conn)
}

/// Watcher uuids who have opted in to internal-note notifications
/// for a given ticket. Used by the comment-notification fan-out
/// when the new comment is `is_internal = true`; watchers who
/// flipped the per-watch toggle off are dropped here. Mentions
/// fan out separately so this filter does not affect them.
pub fn watcher_uuids_for_internal_notify(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<Uuid>> {
    ticket_watchers::table
        .filter(ticket_watchers::ticket_id.eq(ticket_id))
        .filter(ticket_watchers::notify_on_internal_notes.eq(true))
        .order(ticket_watchers::created_at.asc())
        .select(ticket_watchers::user_uuid)
        .load(conn)
}

/// Bulk variant for the bootstrap streamer — one query for the
/// full ticket set rather than one-per-ticket. Returns a map
/// keyed by ticket id.
pub fn watcher_uuids_for_tickets(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, Vec<Uuid>>> {
    use std::collections::HashMap;
    let rows: Vec<(i32, Uuid)> = ticket_watchers::table
        .filter(ticket_watchers::ticket_id.eq_any(ticket_ids))
        .order(ticket_watchers::created_at.asc())
        .select((ticket_watchers::ticket_id, ticket_watchers::user_uuid))
        .load(conn)?;
    let mut out: HashMap<i32, Vec<Uuid>> = HashMap::new();
    for (tid, uuid) in rows {
        out.entry(tid).or_default().push(uuid);
    }
    Ok(out)
}

/// Fetch a single watch row so the UI can show its current
/// preferences (notify_on_internal_notes toggle, auto-added flag).
/// Returns `Ok(None)` when the user isn't watching.
pub fn get_watch(
    conn: &mut DbConnection,
    ticket_id: i32,
    user_uuid: &Uuid,
) -> QueryResult<Option<TicketWatcher>> {
    ticket_watchers::table
        .filter(ticket_watchers::ticket_id.eq(ticket_id))
        .filter(ticket_watchers::user_uuid.eq(user_uuid))
        .first(conn)
        .optional()
}

/// Update the `notify_on_internal_notes` preference on an existing
/// watch row. No-op when the user isn't watching (returns `false`);
/// the caller should add a watch first if they want to set a pref
/// pre-emptively. Emits a sync event so other tabs / devices pick
/// up the toggle live.
pub fn set_notify_on_internal_notes(
    conn: &mut DbConnection,
    ticket_id: i32,
    user_uuid: &Uuid,
    notify: bool,
) -> QueryResult<bool> {
    conn.transaction::<bool, diesel::result::Error, _>(|conn| {
        let ticket = tickets::table.find(ticket_id).first::<crate::models::Ticket>(conn)?;
        let updated = diesel::update(
            ticket_watchers::table
                .filter(ticket_watchers::ticket_id.eq(ticket_id))
                .filter(ticket_watchers::user_uuid.eq(user_uuid)),
        )
        .set(ticket_watchers::notify_on_internal_notes.eq(notify))
        .execute(conn)?;
        if updated == 0 {
            return Ok(false);
        }
        let groups = groups::for_ticket(conn, &ticket)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.watcher_pref_changed",
                data: json!({
                    "ticket_id": ticket_id,
                    "user_uuid": user_uuid,
                    "notify_on_internal_notes": notify,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(true)
    })
}

pub fn is_watching(conn: &mut DbConnection, ticket_id: i32, user_uuid: &Uuid) -> QueryResult<bool> {
    use diesel::dsl::exists;
    use diesel::select;
    select(exists(
        ticket_watchers::table
            .filter(ticket_watchers::ticket_id.eq(ticket_id))
            .filter(ticket_watchers::user_uuid.eq(user_uuid)),
    ))
    .get_result(conn)
}

/// Add a watcher. Idempotent: re-watching is a no-op (no extra
/// row, no duplicate emit). `auto_added` distinguishes the
/// implicit auto-watch path (first comment by a tech) from an
/// explicit bell toggle so a future "stop auto-watching"
/// preference can drop only the implicit ones.
pub fn add_watcher(
    conn: &mut DbConnection,
    ticket_id: i32,
    user_uuid: Uuid,
    auto_added: bool,
) -> QueryResult<bool> {
    conn.transaction::<bool, diesel::result::Error, _>(|conn| {
        // Resolve the parent ticket up front for the sync emit's
        // group computation. Surfaces a clear "no such ticket"
        // error rather than letting the FK insert fail later.
        let ticket = tickets::table.find(ticket_id).first::<crate::models::Ticket>(conn)?;
        let already = is_watching(conn, ticket_id, &user_uuid)?;
        if already {
            return Ok(false);
        }
        let row = NewTicketWatcher { ticket_id, user_uuid, auto_added };
        diesel::insert_into(ticket_watchers::table)
            .values(&row)
            .execute(conn)?;
        let groups = groups::for_ticket(conn, &ticket)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.watcher_added",
                data: json!({
                    "ticket_id": ticket_id,
                    "user_uuid": user_uuid,
                    "auto_added": auto_added,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(true)
    })
}

/// Remove a watcher. Idempotent: removing a non-existent watcher
/// is a no-op (returns false, no emit).
pub fn remove_watcher(
    conn: &mut DbConnection,
    ticket_id: i32,
    user_uuid: &Uuid,
) -> QueryResult<bool> {
    conn.transaction::<bool, diesel::result::Error, _>(|conn| {
        let ticket = tickets::table.find(ticket_id).first::<crate::models::Ticket>(conn)?;
        let removed = diesel::delete(
            ticket_watchers::table
                .filter(ticket_watchers::ticket_id.eq(ticket_id))
                .filter(ticket_watchers::user_uuid.eq(user_uuid)),
        )
        .execute(conn)?;
        if removed == 0 {
            return Ok(false);
        }
        let groups = groups::for_ticket(conn, &ticket)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.watcher_removed",
                data: json!({
                    "ticket_id": ticket_id,
                    "user_uuid": user_uuid,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(true)
    })
}

/// Full TicketWatcher rows for a ticket. Less commonly needed
/// than `watcher_uuids` (the sidebar typically only needs ids
/// → user names via the directory composable), but provided for
/// admin / audit surfaces that want to know auto-added status.
#[allow(dead_code)]
pub fn list_watchers(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<TicketWatcher>> {
    ticket_watchers::table
        .filter(ticket_watchers::ticket_id.eq(ticket_id))
        .order(ticket_watchers::created_at.asc())
        .load(conn)
}
