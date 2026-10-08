//! Releasing a held guest ticket's events.
//!
//! While a guest ticket waits for its submitter to confirm their email, its
//! events reach only the ticket's own group (`groups::for_ticket`), so the
//! consumers that fan out by workspace (webhooks, notifications, the activity
//! feed) skip them. [`release`] is the other half: once the ticket is
//! confirmed it records those events again for the ticket's full audience.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{SyncAggregate, SyncOp, Ticket};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Record `ticket`'s held events again for its full audience, oldest first,
/// as if they were new: same aggregate, event type and data, with
/// `causation_id` naming the held row. The held `ticket.created` is left out,
/// because confirming records a fresh one. Call it once the ticket is no
/// longer pending, after that `ticket.created`. Returns how many were sent.
pub fn release(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<usize> {
    use crate::schema::sync_actions;

    type Held = (
        Uuid,
        SyncAggregate,
        String,
        SyncOp,
        String,
        serde_json::Value,
        Vec<Option<String>>,
    );
    let held: Vec<Held> = sync_actions::table
        .filter(sync_actions::groups.contains(vec![Some(format!("ticket:{}", ticket.id))]))
        // Nothing for the ticket predates it; this also prunes partitions.
        .filter(
            sync_actions::occurred_at.ge(DateTime::<Utc>::from_naive_utc_and_offset(
                ticket.created_at,
                Utc,
            )),
        )
        .filter(sync_actions::event_type.ne("ticket.created"))
        .order(sync_actions::sync_id.asc())
        .select((
            sync_actions::event_uuid,
            sync_actions::aggregate,
            sync_actions::aggregate_id,
            sync_actions::op,
            sync_actions::event_type,
            sync_actions::data,
            sync_actions::groups,
        ))
        .load(conn)?;

    let audience = groups::for_ticket(conn, ticket)?;
    let mut sent = 0;
    for (event_uuid, aggregate, aggregate_id, op, event_type, data, row_groups) in held {
        if groups::has_workspace_audience(&row_groups) {
            continue;
        }
        emit::record(
            conn,
            SyncEmit {
                aggregate,
                aggregate_id,
                op,
                event_type: &event_type,
                data,
                groups: audience.clone(),
                causation_id: Some(event_uuid),
            },
        )?;
        sent += 1;
    }
    Ok(sent)
}
