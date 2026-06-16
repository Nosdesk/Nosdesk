//! Cycles + cycle_tickets CRUD.
//!
//! Cycles are project-scoped time-boxed buckets a ticket can join.
//! Lifecycle: planned → active → completed. The `cycles_active_unique`
//! partial index gates "exactly one active cycle per project," so
//! the helpers here promote/demote without an explicit lock — a
//! constraint violation surfaces as the failure mode rather than a
//! race window.
//!
//! Completion freezes a snapshot of the cycle's stats so post-
//! completion ticket edits do not retroactively rewrite the
//! burndown. The `cycles_completed_snapshot` CHECK constraint
//! mirrors that invariant in the DB.

use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;
use diesel::Connection;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    Cycle, CycleTicket, CycleUpdate, NewCycle, NewCycleTicket, SyncAggregate, SyncOp,
    WorkflowStateCategory,
};
use crate::schema::{cycle_tickets, cycles, tickets, workflow_states};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

fn cycle_payload(cycle: &Cycle) -> serde_json::Value {
    json!({
        "id": cycle.id,
        "uuid": cycle.uuid,
        "project_id": cycle.project_id,
        "name": cycle.name,
        "start_at": cycle.start_at,
        "end_at": cycle.end_at,
        "state": cycle.state,
        "completed_at": cycle.completed_at,
        "archived_at": cycle.archived_at,
    })
}

pub fn list_for_project(conn: &mut DbConnection, project_id: i32) -> QueryResult<Vec<Cycle>> {
    cycles::table
        .filter(cycles::archived_at.is_null())
        .filter(cycles::project_id.eq(project_id))
        .order((cycles::state.asc(), cycles::start_at.asc().nulls_last()))
        .load(conn)
}

/// Workspace-wide cycle list. Optional state filter so the
/// workspace overview can default to "active + planned" without
/// pulling completed cycles into the response. The architecture
/// spec keeps cycles project-scoped at the data layer; this is
/// a read-only convenience endpoint, not a denormalisation.
pub fn list_for_workspace(
    conn: &mut DbConnection,
    states: Option<&[&str]>,
) -> QueryResult<Vec<Cycle>> {
    let mut query = cycles::table
        .filter(cycles::archived_at.is_null())
        .into_boxed();
    if let Some(states) = states {
        query = query
            .filter(cycles::state.eq_any(states.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
    }
    query
        .order((cycles::state.asc(), cycles::start_at.asc().nulls_last()))
        .load(conn)
}

pub fn find_by_uuid(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Option<Cycle>> {
    cycles::table
        .filter(cycles::uuid.eq(uuid))
        .first(conn)
        .optional()
}

pub fn find_by_id(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Cycle>> {
    cycles::table.find(id).first(conn).optional()
}

pub fn create(conn: &mut DbConnection, new: NewCycle) -> QueryResult<Cycle> {
    conn.transaction(|conn| {
        let cycle: Cycle = diesel::insert_into(cycles::table)
            .values(&new)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Cycle,
                aggregate_id: cycle.id.to_string(),
                op: SyncOp::Insert,
                event_type: "cycle.created",
                data: cycle_payload(&cycle),
                groups: groups::for_cycle(cycle.id, cycle.project_id),
                causation_id: None,
            },
        )?;
        Ok(cycle)
    })
}

pub fn update(conn: &mut DbConnection, uuid: Uuid, patch: CycleUpdate) -> QueryResult<Cycle> {
    conn.transaction(|conn| {
        let cycle: Cycle = diesel::update(cycles::table.filter(cycles::uuid.eq(uuid)))
            .set(&patch)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Cycle,
                aggregate_id: cycle.id.to_string(),
                op: SyncOp::Update,
                event_type: "cycle.updated",
                data: cycle_payload(&cycle),
                groups: groups::for_cycle(cycle.id, cycle.project_id),
                causation_id: None,
            },
        )?;
        Ok(cycle)
    })
}

/// Soft-archive. Active cycles can be archived; the partial unique
/// index ignores archived rows so a planned cycle can be promoted
/// to active in the same project immediately.
pub fn archive(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Cycle> {
    conn.transaction(|conn| {
        let cycle: Cycle = diesel::update(cycles::table.filter(cycles::uuid.eq(uuid)))
            .set((
                cycles::archived_at.eq(Some(Utc::now())),
                cycles::state.eq("planned"),
            ))
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Cycle,
                aggregate_id: cycle.id.to_string(),
                op: SyncOp::Archive,
                event_type: "cycle.archived",
                data: cycle_payload(&cycle),
                groups: groups::for_cycle(cycle.id, cycle.project_id),
                causation_id: None,
            },
        )?;
        Ok(cycle)
    })
}

/// Mark a cycle complete and freeze its snapshot. The snapshot
/// records the cycle's terminal state so the burndown widget can
/// render historic cycles without re-querying live tickets.
pub fn complete(
    conn: &mut DbConnection,
    uuid: Uuid,
    snapshot: serde_json::Value,
) -> QueryResult<Cycle> {
    let now = Utc::now();
    conn.transaction(|conn| {
        let cycle: Cycle = diesel::update(cycles::table.filter(cycles::uuid.eq(uuid)))
            .set((
                cycles::state.eq("completed"),
                cycles::completion_snapshot.eq(Some(snapshot.clone())),
                cycles::completed_at.eq(Some(now)),
            ))
            .get_result(conn)?;
        // The completion snapshot rides on this event so consumers
        // (burndown projections, retros) can persist it without a
        // follow-up read.
        let mut payload = cycle_payload(&cycle);
        payload["completion_snapshot"] = snapshot;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Cycle,
                aggregate_id: cycle.id.to_string(),
                op: SyncOp::Update,
                event_type: "cycle.completed",
                data: payload,
                groups: groups::for_cycle(cycle.id, cycle.project_id),
                causation_id: None,
            },
        )?;
        Ok(cycle)
    })
}

// ---- cycle_tickets ----

// sync-pending-wire: cycle membership change; needs a ticket.cycle_changed event
pub fn add_ticket(
    conn: &mut DbConnection,
    cycle_id: i32,
    ticket_id: i32,
    actor: Option<Uuid>,
) -> QueryResult<CycleTicket> {
    conn.transaction(|conn| {
        // Replace any existing membership for the ticket. The
        // partial unique index `cycle_tickets_one_per_ticket`
        // would otherwise reject the insert; doing it explicitly
        // keeps the move semantic ("ticket changed cycle") clear.
        // Emit a removal event for the previous cycle so consumers
        // observing that cycle's group see the ticket leave.
        let previous: Option<i32> = cycle_tickets::table
            .filter(cycle_tickets::ticket_id.eq(ticket_id))
            .select(cycle_tickets::cycle_id)
            .first(conn)
            .optional()?;
        if let Some(prev_cycle_id) = previous {
            diesel::delete(cycle_tickets::table.filter(cycle_tickets::ticket_id.eq(ticket_id)))
                .execute(conn)?;
            emit_cycle_ticket_event(
                conn,
                prev_cycle_id,
                ticket_id,
                SyncOp::Delete,
                "cycle_ticket.removed",
                None,
            )?;
        }
        let row: CycleTicket = diesel::insert_into(cycle_tickets::table)
            .values(&NewCycleTicket {
                cycle_id,
                ticket_id,
                added_by: actor,
            })
            .get_result(conn)?;
        emit_cycle_ticket_event(
            conn,
            cycle_id,
            ticket_id,
            SyncOp::Insert,
            "cycle_ticket.added",
            actor,
        )?;
        Ok(row)
    })
}

// sync-pending-wire: cycle membership change; needs a ticket.cycle_changed event
pub fn remove_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let previous: Option<i32> = cycle_tickets::table
            .filter(cycle_tickets::ticket_id.eq(ticket_id))
            .select(cycle_tickets::cycle_id)
            .first(conn)
            .optional()?;
        let n = diesel::delete(cycle_tickets::table.filter(cycle_tickets::ticket_id.eq(ticket_id)))
            .execute(conn)?;
        if let Some(cycle_id) = previous {
            emit_cycle_ticket_event(
                conn,
                cycle_id,
                ticket_id,
                SyncOp::Delete,
                "cycle_ticket.removed",
                None,
            )?;
        }
        Ok(n)
    })
}

/// Cycle-ticket events propagate through the cycle's project group
/// (so calendar / cycles surfaces refresh) and the ticket's own
/// group (so a ticket-detail subscriber learns its cycle changed).
fn emit_cycle_ticket_event(
    conn: &mut DbConnection,
    cycle_id: i32,
    ticket_id: i32,
    op: SyncOp,
    event_type: &'static str,
    actor: Option<Uuid>,
) -> QueryResult<()> {
    let project_id: Option<i32> = cycles::table
        .find(cycle_id)
        .select(cycles::project_id)
        .first(conn)
        .optional()?;
    let mut groups = vec![
        "workspace:1".to_string(),
        format!("cycle:{}", cycle_id),
        format!("ticket:{}", ticket_id),
    ];
    if let Some(pid) = project_id {
        groups.push(format!("project:{}", pid));
    }
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::CycleTicket,
            aggregate_id: format!("{}:{}", cycle_id, ticket_id),
            op,
            event_type,
            data: json!({
                "cycle_id": cycle_id,
                "ticket_id": ticket_id,
                "added_by": actor,
            }),
            groups,
            causation_id: None,
        },
    )?;
    Ok(())
}

pub fn ticket_ids_for_cycle(conn: &mut DbConnection, cycle_id: i32) -> QueryResult<Vec<i32>> {
    cycle_tickets::table
        .filter(cycle_tickets::cycle_id.eq(cycle_id))
        .select(cycle_tickets::ticket_id)
        .load(conn)
}

pub fn cycle_id_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Option<i32>> {
    cycle_tickets::table
        .filter(cycle_tickets::ticket_id.eq(ticket_id))
        .select(cycle_tickets::cycle_id)
        .first(conn)
        .optional()
}

/// Per-ticket cycle membership for the bootstrap payload. Returns
/// only tickets that belong to a cycle so the consumer defaults
/// the rest to null. The Triage saved view's `cycle_id is_empty`
/// predicate reads this denormalised field so the spec'd filter
/// (`triage_state = 'untriaged' AND cycle = NULL`) evaluates
/// client-side without a join.
pub fn cycle_ids_for_tickets(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, i32>> {
    if ticket_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(i32, i32)> = cycle_tickets::table
        .filter(cycle_tickets::ticket_id.eq_any(ticket_ids))
        .select((cycle_tickets::ticket_id, cycle_tickets::cycle_id))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// A cycle's member tickets with the fields the snapshot and carryover
/// builders read. (The burnup builds its own membership view from the
/// event log, so it doesn't go through here.)
struct CycleMember {
    ticket_id: i32,
    category: WorkflowStateCategory,
    added_at: DateTime<Utc>,
}

fn cycle_members(conn: &mut DbConnection, cycle_id: i32) -> QueryResult<Vec<CycleMember>> {
    let rows: Vec<(i32, WorkflowStateCategory, DateTime<Utc>)> = cycle_tickets::table
        .inner_join(tickets::table.on(tickets::id.eq(cycle_tickets::ticket_id)))
        .inner_join(workflow_states::table.on(workflow_states::id.eq(tickets::workflow_state_id)))
        .filter(cycle_tickets::cycle_id.eq(cycle_id))
        .select((
            cycle_tickets::ticket_id,
            workflow_states::category,
            cycle_tickets::added_at,
        ))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(ticket_id, category, added_at)| CycleMember {
            ticket_id,
            category,
            added_at,
        })
        .collect())
}

/// Build the completion snapshot that gets frozen on cycle.complete.
/// Counts the cycle's tickets and breaks them down by workflow
/// state category. Burndown reads this snapshot for completed
/// cycles so post-completion edits don't move the line.
pub fn build_completion_snapshot(
    conn: &mut DbConnection,
    cycle: &Cycle,
) -> QueryResult<serde_json::Value> {
    let members = cycle_members(conn, cycle.id)?;

    let total = members.len();
    let mut by_category: std::collections::BTreeMap<String, i32> = Default::default();
    let mut completed = 0i32;
    for m in &members {
        let key = m.category.as_str().to_string();
        *by_category.entry(key.clone()).or_insert(0) += 1;
        if matches!(m.category, WorkflowStateCategory::Done) {
            completed += 1;
        }
    }
    // Scope added after the cycle started (mid-cycle creep), when the
    // cycle has a start date to measure against.
    let scope_added = cycle
        .start_at
        .map(|start| members.iter().filter(|m| m.added_at > start).count())
        .unwrap_or(0);
    Ok(json!({
        "frozen_at": Utc::now().to_rfc3339(),
        "tickets": total,
        "completed": completed,
        "by_category": by_category,
        "scope_added": scope_added,
    }))
}

/// On completion, move every still-incomplete ticket (non-terminal
/// workflow state) out of the cycle: into the next non-archived
/// planned/active cycle in the project if one exists, else unlink it
/// (back to the backlog). Returns the carried-over count. Must run in
/// the same transaction as `complete`, AFTER the snapshot is built so
/// the snapshot still reflects the cycle's full membership.
// sync-pending-wire: cycle membership change; needs a ticket.cycle_changed event (emits via emit_cycle_ticket_event)
pub fn carry_over_incomplete(conn: &mut DbConnection, cycle: &Cycle) -> QueryResult<i64> {
    let incomplete: Vec<i32> = cycle_members(conn, cycle.id)?
        .into_iter()
        .filter(|m| {
            !matches!(
                m.category,
                WorkflowStateCategory::Done
                    | WorkflowStateCategory::Cancelled
                    | WorkflowStateCategory::Merged
            )
        })
        .map(|m| m.ticket_id)
        .collect();

    if incomplete.is_empty() {
        return Ok(0);
    }

    // Next planned/active cycle in the project (NULL start_at sorts
    // last under ASC, which is the intent: dated cycles win).
    let target: Option<i32> = cycles::table
        .filter(cycles::project_id.eq(cycle.project_id))
        .filter(cycles::archived_at.is_null())
        .filter(cycles::id.ne(cycle.id))
        .filter(cycles::state.eq_any(["planned", "active"]))
        .order((cycles::start_at.asc().nulls_last(), cycles::id.asc()))
        .select(cycles::id)
        .first(conn)
        .optional()?;

    for ticket_id in &incomplete {
        diesel::delete(
            cycle_tickets::table
                .filter(cycle_tickets::cycle_id.eq(cycle.id))
                .filter(cycle_tickets::ticket_id.eq(*ticket_id)),
        )
        .execute(conn)?;
        emit_cycle_ticket_event(
            conn,
            cycle.id,
            *ticket_id,
            SyncOp::Delete,
            "cycle_ticket.removed",
            None,
        )?;
        if let Some(target_id) = target {
            diesel::insert_into(cycle_tickets::table)
                .values(&NewCycleTicket {
                    cycle_id: target_id,
                    ticket_id: *ticket_id,
                    added_by: None,
                })
                .execute(conn)?;
            emit_cycle_ticket_event(
                conn,
                target_id,
                *ticket_id,
                SyncOp::Insert,
                "cycle_ticket.added",
                None,
            )?;
        }
    }

    Ok(incomplete.len() as i64)
}

/// A span during which a ticket belonged to the cycle. `end == None`
/// means it is still a member.
struct Interval {
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

/// Reconstruct each ticket's cycle-membership intervals by replaying the
/// `cycle_ticket.added` / `cycle_ticket.removed` events, seeded by the
/// current members. The seed covers tickets whose `added` event predates
/// the event log (or was pruned): they're treated as joined at their
/// recorded `added_at` and still present. The replay adds back tickets
/// that were removed mid-cycle, which the current-members snapshot alone
/// can't see. With no events, this degrades exactly to the snapshot
/// (current members from their `added_at`, no removals).
fn membership_intervals(
    conn: &mut DbConnection,
    cycle: &Cycle,
    fallback_start: DateTime<Utc>,
) -> QueryResult<std::collections::HashMap<i32, Vec<Interval>>> {
    use crate::schema::sync_actions;
    use std::collections::HashMap;

    // aggregate_id is "{cycle_id}:{ticket_id}"; the trailing colon keeps
    // the prefix from matching e.g. cycle 120 when filtering cycle 12.
    let events: Vec<(String, String, DateTime<Utc>)> = sync_actions::table
        .filter(sync_actions::aggregate.eq(SyncAggregate::CycleTicket))
        .filter(sync_actions::aggregate_id.like(format!("{}:%", cycle.id)))
        .filter(
            sync_actions::event_type
                .eq("cycle_ticket.added")
                .or(sync_actions::event_type.eq("cycle_ticket.removed")),
        )
        .order((sync_actions::occurred_at.asc(), sync_actions::sync_id.asc()))
        .select((
            sync_actions::aggregate_id,
            sync_actions::event_type,
            sync_actions::occurred_at,
        ))
        .load(conn)?;

    let mut intervals: HashMap<i32, Vec<Interval>> = HashMap::new();
    let mut open: HashMap<i32, DateTime<Utc>> = HashMap::new();
    for (agg_id, event_type, ts) in events {
        let ticket_id = match agg_id.split(':').nth(1).and_then(|s| s.parse::<i32>().ok()) {
            Some(t) => t,
            None => continue,
        };
        match event_type.as_str() {
            "cycle_ticket.added" => {
                open.entry(ticket_id).or_insert(ts);
            }
            "cycle_ticket.removed" => {
                // A removal with no matching open span means the join
                // predates the log; treat it as present since the cycle
                // start so its pre-removal scope isn't lost.
                let start = open.remove(&ticket_id).unwrap_or(fallback_start);
                intervals.entry(ticket_id).or_default().push(Interval {
                    start,
                    end: Some(ts),
                });
            }
            _ => {}
        }
    }
    for (ticket_id, start) in open {
        intervals
            .entry(ticket_id)
            .or_default()
            .push(Interval { start, end: None });
    }

    // Seed current members the replay didn't leave open.
    let current: Vec<(i32, DateTime<Utc>)> = cycle_tickets::table
        .filter(cycle_tickets::cycle_id.eq(cycle.id))
        .select((cycle_tickets::ticket_id, cycle_tickets::added_at))
        .load(conn)?;
    for (ticket_id, added_at) in current {
        let has_open = intervals
            .get(&ticket_id)
            .is_some_and(|v| v.iter().any(|iv| iv.end.is_none()));
        if !has_open {
            intervals.entry(ticket_id).or_default().push(Interval {
                start: added_at,
                end: None,
            });
        }
    }

    Ok(intervals)
}

/// Reconstruct a count-based burnup series for a cycle. Scope is the
/// number of tickets that were members on each day (replayed across
/// mid-cycle removals, see membership_intervals); completed is the
/// members whose ticket had closed by that day. Returns JSON:
/// { start, end, final_scope, start_scope, points: [{ day, scope, completed }] }.
/// Empty points when the cycle has no start_at (can't place a timeline).
///
/// Burnup (not burndown) because cycles allow mid-cycle scope changes:
/// a separate scope line keeps "behind" distinct from "added work."
pub fn build_burnup(conn: &mut DbConnection, cycle: &Cycle) -> QueryResult<serde_json::Value> {
    let start_at = match cycle.start_at {
        Some(s) => s,
        None => {
            return Ok(json!({
                "start": null,
                "end": null,
                "final_scope": 0,
                "points": [],
            }))
        }
    };

    let now = Utc::now();

    // added_at compares in UTC; closed_at is naive-UTC.
    let intervals = membership_intervals(conn, cycle, start_at)?;

    // closed_at for every ticket that was ever a member. Completion is
    // gated on membership, so a removed ticket leaves both the scope and
    // completed lines together (honest under removals).
    let ticket_ids: Vec<i32> = intervals.keys().copied().collect();
    let closed_map: std::collections::HashMap<i32, Option<NaiveDateTime>> = tickets::table
        .filter(tickets::id.eq_any(&ticket_ids))
        .select((tickets::id, tickets::closed_at))
        .load::<(i32, Option<NaiveDateTime>)>(conn)?
        .into_iter()
        .collect();

    let member_at = |at: DateTime<Utc>, ivs: &[Interval]| {
        ivs.iter()
            .any(|iv| iv.start <= at && iv.end.map_or(true, |e| at < e))
    };

    let start_day = start_at.date_naive();
    let raw_end = cycle.end_at.unwrap_or(now).min(now).date_naive();
    // Cap the span to 120 days so the series stays bounded.
    let max_end = start_day + chrono::Duration::days(120);
    let end_day = raw_end.max(start_day).min(max_end);

    // Current scope = members open now; start scope = members at the
    // cycle's start (the gap up to final_scope is mid-cycle creep).
    let final_scope = intervals.values().filter(|ivs| member_at(now, ivs)).count();
    let start_scope = intervals
        .values()
        .filter(|ivs| member_at(start_at, ivs))
        .count();

    let mut points = Vec::new();
    let mut day = start_day;
    while day <= end_day {
        // End-of-day boundary; added_at compares in UTC, closed_at is
        // naive-UTC so it compares to the naive day_end.
        let day_end = day
            .and_hms_opt(23, 59, 59)
            .expect("23:59:59 is a valid time");
        let day_end_utc = DateTime::<Utc>::from_naive_utc_and_offset(day_end, Utc);

        let mut scope = 0usize;
        let mut completed = 0usize;
        for (ticket_id, ivs) in &intervals {
            if !member_at(day_end_utc, ivs) {
                continue;
            }
            scope += 1;
            if closed_map
                .get(ticket_id)
                .and_then(|c| *c)
                .is_some_and(|c| c <= day_end)
            {
                completed += 1;
            }
        }

        points.push(json!({
            "day": day.format("%Y-%m-%d").to_string(),
            "scope": scope,
            "completed": completed,
        }));
        day += chrono::Duration::days(1);
    }

    Ok(json!({
        "start": start_at.to_rfc3339(),
        "end": cycle.end_at.unwrap_or(now).to_rfc3339(),
        "final_scope": final_scope,
        "start_scope": start_scope,
        "points": points,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::sync_actions;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn make_cycle(project_id: i32, name: &str, state: &str) -> NewCycle {
        let now = Utc::now();
        NewCycle {
            project_id,
            name: name.into(),
            start_at: Some(now),
            end_at: Some(now + chrono::Duration::days(14)),
            state: state.into(),
            created_by: None,
        }
    }

    fn _seed_user_and_project(conn: &mut DbConnection, label: &str) -> (Uuid, i32) {
        let user = TestFixtures::create_user(conn, label, "user");
        let project = TestFixtures::create_project(conn, label);
        (user.uuid, project.id)
    }

    #[test]
    fn one_active_cycle_per_project() {
        let mut conn = setup_test_connection();
        let (_user, pid) = _seed_user_and_project(&mut conn, "cyc_active");

        let _planned = create(&mut conn, make_cycle(pid, "first", "planned")).unwrap();
        let active = create(&mut conn, make_cycle(pid, "second", "active")).unwrap();

        // A second active cycle in the same project should fail
        // the partial unique index.
        let dup = create(&mut conn, make_cycle(pid, "third", "active"));
        assert!(dup.is_err(), "expected unique violation, got {:?}", dup);
        assert_eq!(active.state, "active");
    }

    #[test]
    fn ticket_membership_is_exclusive() {
        let mut conn = setup_test_connection();
        let (user, pid) = _seed_user_and_project(&mut conn, "cyc_member");
        let cycle_a = create(&mut conn, make_cycle(pid, "a", "planned")).unwrap();
        let cycle_b = create(&mut conn, make_cycle(pid, "b", "planned")).unwrap();
        let ticket = TestFixtures::create_ticket(&mut conn, "test ticket", Some(user), None);

        add_ticket(&mut conn, cycle_a.id, ticket.id, Some(user)).unwrap();
        assert_eq!(
            cycle_id_for_ticket(&mut conn, ticket.id).unwrap(),
            Some(cycle_a.id)
        );

        add_ticket(&mut conn, cycle_b.id, ticket.id, Some(user)).unwrap();
        // The second add removes the first membership.
        assert_eq!(
            cycle_id_for_ticket(&mut conn, ticket.id).unwrap(),
            Some(cycle_b.id)
        );
    }

    /// Move a ticket to the lowest-position state in a category so the
    /// completion snapshot / carryover sees it as terminal (Done) or
    /// not (Backlog stays non-terminal).
    fn set_ticket_category(
        conn: &mut DbConnection,
        ticket_id: i32,
        category: WorkflowStateCategory,
    ) {
        let state = crate::repository::workflow_states::first_in_category(conn, category).unwrap();
        diesel::update(tickets::table.find(ticket_id))
            .set(tickets::workflow_state_id.eq(state.id))
            .execute(conn)
            .unwrap();
    }

    #[test]
    fn carryover_moves_incomplete_to_next_cycle() {
        let mut conn = setup_test_connection();
        let (user, pid) = _seed_user_and_project(&mut conn, "cyc_carry");

        // Cycle A is active and starts now; cycle B is planned and
        // starts later, so it's the next cycle to receive carryover.
        let now = Utc::now();
        let mut a_def = make_cycle(pid, "a", "active");
        a_def.start_at = Some(now);
        let cycle_a = create(&mut conn, a_def).unwrap();
        let mut b_def = make_cycle(pid, "b", "planned");
        b_def.start_at = Some(now + chrono::Duration::days(14));
        let cycle_b = create(&mut conn, b_def).unwrap();

        let done = TestFixtures::create_ticket(&mut conn, "done", Some(user), None);
        let open1 = TestFixtures::create_ticket(&mut conn, "open1", Some(user), None);
        let open2 = TestFixtures::create_ticket(&mut conn, "open2", Some(user), None);
        for t in [&done, &open1, &open2] {
            add_ticket(&mut conn, cycle_a.id, t.id, Some(user)).unwrap();
        }
        set_ticket_category(&mut conn, done.id, WorkflowStateCategory::Done);

        // Complete A via the repo path: snapshot first, then carryover,
        // then complete.
        let mut snapshot = build_completion_snapshot(&mut conn, &cycle_a).unwrap();
        assert_eq!(snapshot["tickets"], 3);
        assert_eq!(snapshot["completed"], 1);
        let carried = carry_over_incomplete(&mut conn, &cycle_a).unwrap();
        assert_eq!(carried, 2);
        snapshot["carried_over"] = json!(carried);
        complete(&mut conn, cycle_a.uuid, snapshot).unwrap();

        // The two open tickets now belong to B; the done ticket left A
        // (no longer a member of any cycle).
        assert_eq!(
            cycle_id_for_ticket(&mut conn, open1.id).unwrap(),
            Some(cycle_b.id)
        );
        assert_eq!(
            cycle_id_for_ticket(&mut conn, open2.id).unwrap(),
            Some(cycle_b.id)
        );
        // The done ticket is terminal, so carryover leaves it in A.
        assert_eq!(
            cycle_id_for_ticket(&mut conn, done.id).unwrap(),
            Some(cycle_a.id)
        );
        assert_eq!(
            ticket_ids_for_cycle(&mut conn, cycle_a.id).unwrap(),
            vec![done.id]
        );
    }

    #[test]
    fn burnup_reconstructs_scope_and_completed_series() {
        let mut conn = setup_test_connection();
        let (user, pid) = _seed_user_and_project(&mut conn, "cyc_burnup");

        // Cycle spans roughly 3 days ago to 3 days ahead.
        let now = Utc::now();
        let mut def = make_cycle(pid, "burnup", "active");
        def.start_at = Some(now - chrono::Duration::days(3));
        def.end_at = Some(now + chrono::Duration::days(3));
        let cycle = create(&mut conn, def).unwrap();

        // Three tickets join the cycle. Backdate their added_at so the
        // scope ramps across the window: t1 two days ago, t2 and t3 one
        // day ago.
        let t1 = TestFixtures::create_ticket(&mut conn, "t1", Some(user), None);
        let t2 = TestFixtures::create_ticket(&mut conn, "t2", Some(user), None);
        let t3 = TestFixtures::create_ticket(&mut conn, "t3", Some(user), None);
        for t in [&t1, &t2, &t3] {
            add_ticket(&mut conn, cycle.id, t.id, Some(user)).unwrap();
        }
        diesel::update(cycle_tickets::table.filter(cycle_tickets::ticket_id.eq(t1.id)))
            .set(cycle_tickets::added_at.eq(now - chrono::Duration::days(2)))
            .execute(&mut conn)
            .unwrap();
        diesel::update(
            cycle_tickets::table.filter(cycle_tickets::ticket_id.eq_any([t2.id, t3.id])),
        )
        .set(cycle_tickets::added_at.eq(now - chrono::Duration::days(1)))
        .execute(&mut conn)
        .unwrap();

        // build_burnup reconstructs membership intervals from the
        // `cycle_ticket.added` events in sync_actions (occurred_at), NOT from
        // cycle_tickets.added_at, so the backdate above only bites if it also
        // lands on the event timestamps. The events are stamped with the DB
        // clock (~now), which can run slightly AHEAD of the process clock that
        // build_burnup samples for "now"; backdating them by whole days both
        // makes the scope ramp real and removes that sub-second skew as a
        // source of flakiness (a ticket added "now" but recorded one tick in
        // the future would otherwise drop out of final_scope).
        diesel::update(
            sync_actions::table
                .filter(sync_actions::aggregate.eq(SyncAggregate::CycleTicket))
                .filter(sync_actions::aggregate_id.eq(format!("{}:{}", cycle.id, t1.id)))
                .filter(sync_actions::event_type.eq("cycle_ticket.added")),
        )
        .set(sync_actions::occurred_at.eq(now - chrono::Duration::days(2)))
        .execute(&mut conn)
        .unwrap();
        diesel::update(
            sync_actions::table
                .filter(sync_actions::aggregate.eq(SyncAggregate::CycleTicket))
                .filter(sync_actions::aggregate_id.eq_any([
                    format!("{}:{}", cycle.id, t2.id),
                    format!("{}:{}", cycle.id, t3.id),
                ]))
                .filter(sync_actions::event_type.eq("cycle_ticket.added")),
        )
        .set(sync_actions::occurred_at.eq(now - chrono::Duration::days(1)))
        .execute(&mut conn)
        .unwrap();

        // Close t1 so completed picks up at least one ticket. Close at
        // the ticket's own created_at rather than the `now` captured at
        // the top of the test: the fixtures inserted at the DB clock,
        // which is strictly after that `now`, so `closed_at = now` can
        // trip the tickets_dates_valid check (closed_at < created_at)
        // when the suite runs under load. created_at is ~now (today),
        // so it still lands in the final daily bucket.
        diesel::update(tickets::table.find(t1.id))
            .set(tickets::closed_at.eq(Some(t1.created_at)))
            .execute(&mut conn)
            .unwrap();

        let series = build_burnup(&mut conn, &cycle).unwrap();
        let points = series["points"].as_array().unwrap();

        // start_day .. end_day (capped at now) inclusive: 3 days ago to
        // today is 4 days.
        assert_eq!(points.len(), 4, "expected 4 daily points");
        assert_eq!(series["final_scope"], 3);

        // Last point reflects full scope, and completed is monotonic
        // non-decreasing across the series.
        let last = points.last().unwrap();
        assert_eq!(last["scope"], 3);
        let mut prev = 0i64;
        for p in points {
            let c = p["completed"].as_i64().unwrap();
            assert!(c >= prev, "completed must be monotonic non-decreasing");
            prev = c;
        }
        assert_eq!(last["completed"], 1, "t1 closed within the window");
    }

    #[test]
    fn burnup_scope_counts_removed_tickets_historically() {
        // The current-members snapshot would understate past scope after
        // a mid-cycle removal; the event replay should keep the removed
        // ticket on the scope line for the days it was a member.
        let mut conn = setup_test_connection();
        let (user, pid) = _seed_user_and_project(&mut conn, "cyc_burnup_rm");

        let now = Utc::now();
        let mut def = make_cycle(pid, "burnup_rm", "active");
        def.start_at = Some(now - chrono::Duration::days(3));
        def.end_at = Some(now + chrono::Duration::days(3));
        let cycle = create(&mut conn, def).unwrap();

        let t1 = TestFixtures::create_ticket(&mut conn, "rm1", Some(user), None);
        let t2 = TestFixtures::create_ticket(&mut conn, "rm2", Some(user), None);
        add_ticket(&mut conn, cycle.id, t1.id, Some(user)).unwrap();
        add_ticket(&mut conn, cycle.id, t2.id, Some(user)).unwrap();

        // Backdate both joins to two days ago, then remove t1 a day ago.
        // (Membership is replayed from the events' occurred_at.)
        diesel::update(
            crate::schema::sync_actions::table
                .filter(crate::schema::sync_actions::event_type.eq("cycle_ticket.added"))
                .filter(crate::schema::sync_actions::aggregate_id.like(format!("{}:%", cycle.id))),
        )
        .set(crate::schema::sync_actions::occurred_at.eq(now - chrono::Duration::days(2)))
        .execute(&mut conn)
        .unwrap();

        remove_ticket(&mut conn, t1.id).unwrap();
        diesel::update(
            crate::schema::sync_actions::table
                .filter(crate::schema::sync_actions::event_type.eq("cycle_ticket.removed"))
                .filter(
                    crate::schema::sync_actions::aggregate_id.eq(format!("{}:{}", cycle.id, t1.id)),
                ),
        )
        .set(crate::schema::sync_actions::occurred_at.eq(now - chrono::Duration::days(1)))
        .execute(&mut conn)
        .unwrap();

        let series = build_burnup(&mut conn, &cycle).unwrap();
        let points = series["points"].as_array().unwrap();

        // t1 left the cycle, so current scope is just t2 ...
        assert_eq!(series["final_scope"], 1);
        assert_eq!(points.last().unwrap()["scope"], 1);
        // ... but on the days both were members, scope reached 2.
        let max_scope = points
            .iter()
            .map(|p| p["scope"].as_i64().unwrap())
            .max()
            .unwrap();
        assert_eq!(
            max_scope, 2,
            "removed ticket should count while it was a member"
        );
    }

    #[test]
    fn burnup_empty_without_start_at() {
        let mut conn = setup_test_connection();
        let (_user, pid) = _seed_user_and_project(&mut conn, "cyc_burnup_nostart");
        let mut def = make_cycle(pid, "nostart", "planned");
        def.start_at = None;
        let cycle = create(&mut conn, def).unwrap();

        let series = build_burnup(&mut conn, &cycle).unwrap();
        assert!(series["points"].as_array().unwrap().is_empty());
        assert_eq!(series["final_scope"], 0);
        assert!(series["start"].is_null());
    }

    #[test]
    fn carryover_unlinks_when_no_next_cycle() {
        let mut conn = setup_test_connection();
        let (user, pid) = _seed_user_and_project(&mut conn, "cyc_unlink");

        let cycle = create(&mut conn, make_cycle(pid, "only", "active")).unwrap();
        let open = TestFixtures::create_ticket(&mut conn, "open", Some(user), None);
        add_ticket(&mut conn, cycle.id, open.id, Some(user)).unwrap();

        let carried = carry_over_incomplete(&mut conn, &cycle).unwrap();
        assert_eq!(carried, 1);
        // No target cycle: the ticket unlinks back to the backlog.
        assert_eq!(cycle_id_for_ticket(&mut conn, open.id).unwrap(), None);
    }
}
