//! What follows a ticket update, whichever way it arrives. The REST PATCH and
//! sync push (how the app saves ticket edits) both call [`after_update`] once
//! the update has committed.

use std::sync::Arc;

use diesel::QueryResult;
use tracing::{info, warn};

use crate::db::DbConnection;
use crate::extractors::TenantConn;
use crate::models::{AssignmentTrigger, NewTicket, Ticket, TicketUpdate};
use crate::repository;
use crate::repository::tickets::TicketUpdatedObserver;
use crate::services::assignment::AssignmentEngine;
use crate::services::search::SearchService;
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;

/// Runs a closure in the caller's workspace, as its actor.
pub trait InWorkspace {
    fn run<T>(&mut self, f: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> QueryResult<T>;
}

impl InWorkspace for TenantConn {
    fn run<T>(&mut self, f: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> QueryResult<T> {
        TenantConn::run(self, f)
    }
}

/// A held connection and the actor to run as, for sync push.
pub struct ActorConn<'a> {
    pub conn: &'a mut DbConnection,
    pub actor: &'a ActorContext,
}

impl InWorkspace for ActorConn<'_> {
    fn run<T>(&mut self, f: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> QueryResult<T> {
        with_actor_context(self.conn, self.actor, f)
    }
}

/// After `updated` was saved: a recurring ticket that just closed gets its
/// next occurrence, and an unassigned ticket whose category changed goes
/// through the assignment rules. Each step logs and carries on if it fails,
/// since the update itself has committed.
pub fn after_update(
    db: &mut impl InWorkspace,
    search: Option<&Arc<SearchService>>,
    updated: &Ticket,
    category_changed: bool,
) {
    materialise_next_occurrence(db, updated);
    if category_changed && updated.assignee_uuid.is_none() {
        assign_on_category_change(db, search, updated);
    }
}

/// RRULE materialise-on-close: if the ticket is in a closed category and
/// carries a recurrence_rule, generate the next occurrence so the user sees it
/// land immediately. A malformed rule is logged and skipped rather than
/// failing the close.
fn materialise_next_occurrence(db: &mut impl InWorkspace, updated: &Ticket) {
    let Some(rule) = updated.recurrence_rule.as_ref() else {
        return;
    };
    let closed = db
        .run(|conn| repository::workflow_states::category_of(conn, updated.workflow_state_id))
        .ok()
        .flatten()
        .is_some_and(|c| c.closes_ticket());
    if !closed {
        return;
    }
    let after = updated
        .due_date
        .or(updated.closed_at)
        .unwrap_or(updated.created_at);
    match crate::services::recurrence::next_occurrence_naive(rule, updated.created_at, after) {
        Ok(Some(next_due)) => {
            // The new occurrence is a clean copy of the template, same title /
            // priority / category / assignee, with a fresh due_date and an
            // open workflow state. Carry the rule forward so the chain
            // continues; record the template id to keep audit lineage.
            let template_id = updated.recurrence_template_id.unwrap_or(updated.id);
            let open_state = match db.run(repository::workflow_states::default_state) {
                Ok(s) => s.id,
                Err(_) => updated.workflow_state_id,
            };
            let new_ticket = NewTicket {
                title: updated.title.clone(),
                workflow_state_id: open_state,
                priority: updated.priority,
                requester_uuid: updated.requester_uuid,
                assignee_uuid: updated.assignee_uuid,
                category_id: updated.category_id,
                due_date: Some(next_due),
                recurrence_rule: Some(rule.clone()),
                recurrence_template_id: Some(template_id),
                ..Default::default()
            };
            match db.run(|conn| repository::create_ticket(conn, new_ticket)) {
                Ok(_) => info!(
                    ticket_id = updated.id,
                    template_id,
                    next_due = %next_due,
                    "Materialised next recurring occurrence"
                ),
                Err(e) => warn!(
                    ticket_id = updated.id,
                    error = ?e,
                    "Failed to materialise next recurring occurrence"
                ),
            }
        }
        Ok(None) => {
            // Series ran out (UNTIL passed). Nothing to do.
        }
        Err(e) => {
            warn!(
                ticket_id = updated.id,
                rule = %rule,
                error = ?e,
                "Recurrence rule failed to parse on close",
            );
        }
    }
}

/// Run the assignment rules for a category change, and assign the ticket to
/// whoever they pick. The assignee's notification derives from the
/// ticket.assignee_changed sync action the write emits.
fn assign_on_category_change(
    db: &mut impl InWorkspace,
    search: Option<&Arc<SearchService>>,
    updated: &Ticket,
) {
    run_assignment_rules(db, search, updated, AssignmentTrigger::CategoryChanged);
}

/// A new ticket nobody is assigned to goes through the assignment rules,
/// whichever way it arrived: the app, the portal, a guest form or email.
/// Returns the ticket as it now stands.
pub fn assign_new_ticket(
    db: &mut impl InWorkspace,
    search: Option<&Arc<SearchService>>,
    ticket: Ticket,
) -> Ticket {
    if ticket.assignee_uuid.is_some() {
        return ticket;
    }
    run_assignment_rules(db, search, &ticket, AssignmentTrigger::TicketCreated).unwrap_or(ticket)
}

/// Runs the assignment rules for `trigger` and saves the assignee they pick,
/// through `update_ticket_partial` so the assignment notification derives
/// from its `ticket.assignee_changed` sync action. `None` when no rule
/// assigned anyone or the save failed.
fn run_assignment_rules(
    db: &mut impl InWorkspace,
    search: Option<&Arc<SearchService>>,
    ticket: &Ticket,
    trigger: AssignmentTrigger,
) -> Option<Ticket> {
    let result = db
        .run(|conn| Ok(AssignmentEngine::evaluate_rules(conn, ticket, trigger)))
        .ok()
        .flatten()?;
    let assigned_uuid = result.assigned_user_uuid?;
    let assign = TicketUpdate {
        assignee_uuid: Some(Some(assigned_uuid)),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };
    let observer = search.map(|s| s as &dyn TicketUpdatedObserver);
    let updated = db
        .run(|conn| repository::update_ticket_partial(conn, ticket.id, assign, observer))
        .ok()?;
    info!(
        ticket_id = ticket.id,
        assignee = %assigned_uuid,
        rule = %result.rule_name,
        method = %result.method,
        "Auto-assigned ticket by an assignment rule"
    );
    Some(updated)
}
