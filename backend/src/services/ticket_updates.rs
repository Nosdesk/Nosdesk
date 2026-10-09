//! What follows a ticket update, whichever way it arrives. The REST PATCH and
//! sync push (how the app saves ticket edits) both call [`after_update`] once
//! the update has committed.

use std::sync::Arc;

use tracing::info;

use crate::db::DbConnection;
use crate::extractors::TenantConn;
use crate::models::{AssignmentTrigger, Ticket, TicketUpdate};
use crate::repository;
use crate::repository::tickets::TicketUpdatedObserver;
use crate::services::assignment::AssignmentEngine;
use crate::services::search::SearchService;
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;

/// Runs a closure in the caller's workspace, as its actor.
pub trait InWorkspace {
    fn run<T, E: From<diesel::result::Error>>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E>;
}

impl InWorkspace for TenantConn {
    fn run<T, E: From<diesel::result::Error>>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E> {
        TenantConn::run_result(self, f)
    }
}

/// A held connection and the actor to run as, for sync push.
pub struct ActorConn<'a> {
    pub conn: &'a mut DbConnection,
    pub actor: &'a ActorContext,
}

impl InWorkspace for ActorConn<'_> {
    fn run<T, E: From<diesel::result::Error>>(
        &mut self,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E> {
        with_actor_context(self.conn, self.actor, f)
    }
}

/// After `updated` was saved: an unassigned ticket whose category changed goes
/// through the assignment rules. It logs and carries on if that fails, since
/// the update itself has committed. (A recurring ticket's next occurrence comes
/// from the write that closes it, in `update_ticket_partial`.)
pub fn after_update(
    db: &mut impl InWorkspace,
    search: Option<&Arc<SearchService>>,
    updated: &Ticket,
    category_changed: bool,
) {
    if category_changed && updated.assignee_uuid.is_none() {
        assign_on_category_change(db, search, updated);
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
        .run(|conn| {
            Ok::<_, diesel::result::Error>(AssignmentEngine::evaluate_rules(conn, ticket, trigger))
        })
        .ok()
        .flatten()?;
    let assigned_uuid = result.assigned_user_uuid?;
    let assign = TicketUpdate {
        assignee_uuid: Some(Some(assigned_uuid)),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };
    let observer = search.map(|s| s as &dyn TicketUpdatedObserver);
    // The rule made this change, not whoever created or edited the ticket:
    // the write and its sync action are credited to the rule as a system
    // actor (`assignment_rule:<id>`), for the rest of this transaction.
    let rule_actor =
        crate::sync::actor::ActorContext::system(format!("assignment_rule:{}", result.rule_id))
            .with_workspace(ticket.workspace_id);
    let updated = db
        .run(|conn| {
            crate::sync::session::set_actor(conn, &rule_actor)?;
            repository::update_ticket_partial(conn, ticket.id, assign, observer)
        })
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
