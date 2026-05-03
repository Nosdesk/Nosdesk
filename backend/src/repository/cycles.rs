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

use chrono::Utc;
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
        query = query.filter(cycles::state.eq_any(states.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
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

pub fn update(
    conn: &mut DbConnection,
    uuid: Uuid,
    patch: CycleUpdate,
) -> QueryResult<Cycle> {
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
            emit_cycle_ticket_event(conn, prev_cycle_id, ticket_id, SyncOp::Delete, "cycle_ticket.removed", None)?;
        }
        let row: CycleTicket = diesel::insert_into(cycle_tickets::table)
            .values(&NewCycleTicket {
                cycle_id,
                ticket_id,
                added_by: actor,
            })
            .get_result(conn)?;
        emit_cycle_ticket_event(conn, cycle_id, ticket_id, SyncOp::Insert, "cycle_ticket.added", actor)?;
        Ok(row)
    })
}

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
            emit_cycle_ticket_event(conn, cycle_id, ticket_id, SyncOp::Delete, "cycle_ticket.removed", None)?;
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

/// Build the completion snapshot that gets frozen on cycle.complete.
/// Counts the cycle's tickets and breaks them down by workflow
/// state category. Burndown reads this snapshot for completed
/// cycles so post-completion edits don't move the line.
pub fn build_completion_snapshot(
    conn: &mut DbConnection,
    cycle_id: i32,
) -> QueryResult<serde_json::Value> {
    let rows: Vec<(i32, WorkflowStateCategory)> = cycle_tickets::table
        .inner_join(tickets::table.on(tickets::id.eq(cycle_tickets::ticket_id)))
        .inner_join(
            workflow_states::table.on(workflow_states::id.eq(tickets::workflow_state_id)),
        )
        .filter(cycle_tickets::cycle_id.eq(cycle_id))
        .select((tickets::id, workflow_states::category))
        .load(conn)?;

    let total = rows.len();
    let mut by_category: std::collections::BTreeMap<String, i32> = Default::default();
    let mut completed = 0i32;
    for (_, cat) in &rows {
        let key = cat.as_str().to_string();
        *by_category.entry(key.clone()).or_insert(0) += 1;
        if matches!(cat, WorkflowStateCategory::Done) {
            completed += 1;
        }
    }
    Ok(json!({
        "frozen_at": Utc::now().to_rfc3339(),
        "tickets": total,
        "completed": completed,
        "by_category": by_category,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
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
        let user = TestFixtures::create_user(conn, label, UserRole::User);
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
        assert_eq!(cycle_id_for_ticket(&mut conn, ticket.id).unwrap(), Some(cycle_a.id));

        add_ticket(&mut conn, cycle_b.id, ticket.id, Some(user)).unwrap();
        // The second add removes the first membership.
        assert_eq!(cycle_id_for_ticket(&mut conn, ticket.id).unwrap(), Some(cycle_b.id));
    }
}
