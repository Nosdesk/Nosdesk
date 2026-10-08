//! Ticket approvals (`ticket_approvals`, `tickets.approval_state`).
//!
//! A request in a type that needs approval waits for its approvers: the type's
//! named approvers and, when the type asks, the requester's manager. With the
//! `any` rule the first approval decides; with `all` every approver must
//! approve. Any decline declines. Staff can skip a waiting approval (the
//! handler decides who may), and every decision is a row plus a
//! `ticket.approval_decided` event, so the activity feed, notifications and
//! the audit log all see it.
//!
//! A request whose type needs approval but has nobody to approve it (no named
//! approvers and no manager on file) still waits: silently letting it through
//! would defeat an organisation that asked for approvals. The event says so
//! and staff skip it.

use chrono::Utc;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    SyncAggregate, SyncOp, Ticket, TicketApproval, TicketCategory, TicketUpdate,
    WorkflowStateCategory,
};
use crate::schema::{category_approvers, ticket_approvals, tickets};
use crate::sync::{emit, emit::SyncEmit, groups};

pub const PENDING: &str = "pending";
pub const APPROVED: &str = "approved";
pub const DECLINED: &str = "declined";
pub const SKIPPED: &str = "skipped";

/// Why a decision wasn't recorded.
#[derive(Debug, PartialEq, Eq)]
pub enum DecideError {
    /// No waiting approval for this person on this ticket.
    NotWaiting,
    /// A decline needs a reason the requester can read.
    CommentRequired,
    Db(String),
}

impl From<diesel::result::Error> for DecideError {
    fn from(e: diesel::result::Error) -> Self {
        DecideError::Db(e.to_string())
    }
}

/// What staff are told when they try to resolve a request still waiting.
pub const WAITING_MESSAGE: &str =
    "This request is waiting for approval. Approve it or skip the approval first.";

/// Whether moving `ticket_id` to `state_id` would resolve a request that is
/// still waiting for approval (the fulfilment is gated, the conversation isn't:
/// every other change is allowed).
pub fn blocks_resolution(
    conn: &mut DbConnection,
    ticket_id: i32,
    state_id: i32,
) -> QueryResult<bool> {
    let state: Option<String> = tickets::table
        .find(ticket_id)
        .select(tickets::approval_state)
        .first(conn)?;
    if state.as_deref() != Some(PENDING) {
        return Ok(false);
    }
    Ok(
        crate::repository::workflow_states::category_of(conn, state_id)?
            == Some(WorkflowStateCategory::Done),
    )
}

/// The overall state of a round, from its rows.
pub fn outcome(rule: &str, rows: &[TicketApproval]) -> &'static str {
    let decided = |d: &str| {
        rows.iter()
            .filter(|r| r.decision.as_deref() == Some(d))
            .count()
    };
    if decided(DECLINED) > 0 {
        return DECLINED;
    }
    let approved = decided(APPROVED);
    let done = if rule == "all" {
        !rows.is_empty() && approved == rows.len()
    } else {
        approved > 0
    };
    if done {
        APPROVED
    } else {
        PENDING
    }
}

/// The current (latest) round's rows for a ticket.
pub fn current_round(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<TicketApproval>> {
    let round: Option<i32> = ticket_approvals::table
        .filter(ticket_approvals::ticket_id.eq(ticket_id))
        .select(diesel::dsl::max(ticket_approvals::round))
        .first(conn)?;
    let Some(round) = round else {
        return Ok(Vec::new());
    };
    ticket_approvals::table
        .filter(ticket_approvals::ticket_id.eq(ticket_id))
        .filter(ticket_approvals::round.eq(round))
        .order(ticket_approvals::id.asc())
        .select(TicketApproval::as_select())
        .load(conn)
}

/// Who approves a request of `category` from `requester`: the named approvers
/// and, when the type asks, the requester's manager.
fn approvers_for(
    conn: &mut DbConnection,
    category: &TicketCategory,
    requester: Option<Uuid>,
) -> QueryResult<Vec<Uuid>> {
    let mut out: Vec<Uuid> = category_approvers::table
        .filter(category_approvers::category_id.eq(category.id))
        .select(category_approvers::user_uuid)
        .order(category_approvers::created_at.asc())
        .load(conn)?;
    if category.approval_by_manager {
        if let Some(manager) = requester
            .map(|r| crate::repository::user_contact::manager_of(conn, r))
            .transpose()?
            .flatten()
        {
            if !out.contains(&manager) {
                out.push(manager);
            }
        }
    }
    Ok(out)
}

fn emit_event(
    conn: &mut DbConnection,
    ticket: &Ticket,
    event_type: &str,
    data: serde_json::Value,
) -> QueryResult<()> {
    let groups = groups::for_ticket(conn, ticket)?;
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::Ticket,
            aggregate_id: ticket.id.to_string(),
            op: SyncOp::Update,
            event_type,
            data,
            groups,
            causation_id: None,
        },
    )?;
    Ok(())
}

fn set_state(conn: &mut DbConnection, ticket_id: i32, state: &str) -> QueryResult<Ticket> {
    diesel::update(tickets::table.find(ticket_id))
        .set(tickets::approval_state.eq(Some(state)))
        .get_result(conn)
}

// sync-audit-only: approval rows are audited; the ticket-level outcome emits ticket.approval_requested / ticket.approval_decided below
/// Start an approval round if the ticket's request type needs one and it
/// hasn't got one yet. Returns the ticket's approval state afterwards (`None`
/// when no approval is involved). Called when a ticket is created (or released
/// from guest verification) and when its request type changes.
pub fn start_if_required(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<Option<String>> {
    if ticket.approval_state.is_some() {
        return Ok(ticket.approval_state.clone());
    }
    let Some(category_id) = ticket.category_id else {
        return Ok(None);
    };
    let category =
        match crate::repository::categories::get_category_by_id(conn, category_id).optional()? {
            Some(c) if c.approval_required => c,
            _ => return Ok(None),
        };
    start_round(conn, ticket, &category, 1).map(Some)
}

fn start_round(
    conn: &mut DbConnection,
    ticket: &Ticket,
    category: &TicketCategory,
    round: i32,
) -> QueryResult<String> {
    let approvers = approvers_for(conn, category, ticket.requester_uuid)?;
    let now = Utc::now();
    for approver in &approvers {
        // Someone approving their own request has already said yes.
        let own = Some(*approver) == ticket.requester_uuid;
        diesel::insert_into(ticket_approvals::table)
            .values((
                ticket_approvals::ticket_id.eq(ticket.id),
                ticket_approvals::approver_uuid.eq(approver),
                ticket_approvals::round.eq(round),
                ticket_approvals::decision.eq(own.then_some(APPROVED)),
                ticket_approvals::channel.eq(own.then_some("self")),
                ticket_approvals::decided_by.eq(own.then_some(*approver)),
                ticket_approvals::decided_at.eq(own.then_some(now)),
            ))
            .on_conflict_do_nothing()
            .execute(conn)?;
    }
    let rows = current_round(conn, ticket.id)?;
    let state = outcome(&category.approval_rule, &rows);
    let ticket = set_state(conn, ticket.id, state)?;
    let waiting: Vec<Uuid> = rows
        .iter()
        .filter(|r| r.decision.is_none())
        .map(|r| r.approver_uuid)
        .collect();
    emit_event(
        conn,
        &ticket,
        "ticket.approval_requested",
        json!({
            "id": ticket.id,
            "title": ticket.title,
            "requester_uuid": ticket.requester_uuid,
            "approval_state": state,
            "approval_rule": category.approval_rule,
            "round": round,
            "approver_uuids": waiting,
            // Nobody to ask: staff have to skip it.
            "no_approver": rows.is_empty(),
        }),
    )?;
    Ok(state.to_string())
}

// sync-audit-only: approval rows are audited; the new round emits ticket.approval_requested
/// A declined request that was reopened (the requester replied) starts a new
/// approval round; the old rows stay as history.
pub fn restart_if_declined(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<()> {
    if ticket.approval_state.as_deref() != Some(DECLINED) {
        return Ok(());
    }
    let Some(category) = ticket
        .category_id
        .map(|id| crate::repository::categories::get_category_by_id(conn, id).optional())
        .transpose()?
        .flatten()
        .filter(|c| c.approval_required)
    else {
        return Ok(());
    };
    let round: i32 = ticket_approvals::table
        .filter(ticket_approvals::ticket_id.eq(ticket.id))
        .select(diesel::dsl::max(ticket_approvals::round))
        .first::<Option<i32>>(conn)?
        .unwrap_or(0)
        + 1;
    start_round(conn, ticket, &category, round).map(|_| ())
}

// sync-audit-only: approval rows are audited; the outcome emits ticket.approval_decided below
/// Record `approver`'s decision on the ticket's current round. Returns the
/// ticket's approval state afterwards. A decline moves the request to the
/// first cancelled state (if the workspace has one) with the reason.
pub fn decide(
    conn: &mut DbConnection,
    ticket_id: i32,
    approver: Uuid,
    approve: bool,
    comment: Option<&str>,
    channel: &str,
    observer: Option<&dyn crate::repository::tickets::TicketUpdatedObserver>,
) -> Result<String, DecideError> {
    let comment = comment.map(str::trim).filter(|c| !c.is_empty());
    if !approve && comment.is_none() {
        return Err(DecideError::CommentRequired);
    }
    conn.transaction(|conn| {
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        if ticket.approval_state.as_deref() != Some(PENDING) {
            return Err(DecideError::NotWaiting);
        }
        let rows = current_round(conn, ticket_id)?;
        let Some(row) = rows
            .iter()
            .find(|r| r.approver_uuid == approver && r.decision.is_none())
        else {
            return Err(DecideError::NotWaiting);
        };
        let decision = if approve { APPROVED } else { DECLINED };
        diesel::update(ticket_approvals::table.find(row.id))
            .set((
                ticket_approvals::decision.eq(decision),
                ticket_approvals::comment.eq(comment),
                ticket_approvals::channel.eq(channel),
                ticket_approvals::decided_by.eq(approver),
                ticket_approvals::decided_at.eq(Utc::now()),
            ))
            .execute(conn)?;
        let rule = rule_for(conn, &ticket)?;
        let rows = current_round(conn, ticket_id)?;
        let state = outcome(&rule, &rows);
        conclude(
            conn,
            &ticket,
            state,
            decision,
            Some(approver),
            comment,
            channel,
            observer,
        )?;
        Ok(state.to_string())
    })
}

// sync-audit-only: approval rows are audited; the outcome emits ticket.approval_decided below
/// Staff skip a waiting approval (whether they may is the caller's decision).
/// Every waiting row is closed as skipped, with who and why.
pub fn skip(
    conn: &mut DbConnection,
    ticket_id: i32,
    actor: Uuid,
    comment: Option<&str>,
) -> Result<(), DecideError> {
    let comment = comment.map(str::trim).filter(|c| !c.is_empty());
    conn.transaction(|conn| {
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        if ticket.approval_state.as_deref() != Some(PENDING) {
            return Err(DecideError::NotWaiting);
        }
        let rows = current_round(conn, ticket_id)?;
        if rows.is_empty() {
            // Nobody was asked: record the skip against the person who made
            // it, so the decision has a row (and an audit entry) like any other.
            diesel::insert_into(ticket_approvals::table)
                .values((
                    ticket_approvals::ticket_id.eq(ticket_id),
                    ticket_approvals::approver_uuid.eq(actor),
                    ticket_approvals::round.eq(1),
                ))
                .execute(conn)?;
        }
        let rows = current_round(conn, ticket_id)?;
        let ids: Vec<i32> = rows
            .iter()
            .filter(|r| r.decision.is_none())
            .map(|r| r.id)
            .collect();
        diesel::update(ticket_approvals::table.filter(ticket_approvals::id.eq_any(&ids)))
            .set((
                ticket_approvals::decision.eq(SKIPPED),
                ticket_approvals::comment.eq(comment),
                ticket_approvals::channel.eq("skip"),
                ticket_approvals::decided_by.eq(actor),
                ticket_approvals::decided_at.eq(Utc::now()),
            ))
            .execute(conn)?;
        conclude(
            conn,
            &ticket,
            SKIPPED,
            SKIPPED,
            Some(actor),
            comment,
            "skip",
            None,
        )?;
        Ok(())
    })
}

fn rule_for(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<String> {
    Ok(match ticket.category_id {
        Some(id) => crate::repository::categories::get_category_by_id(conn, id)
            .optional()?
            .map(|c| c.approval_rule)
            .unwrap_or_else(|| "any".into()),
        None => "any".into(),
    })
}

/// Record one decision's effect: the ticket's state, the event, and for a
/// decline, the move to cancelled.
fn conclude(
    conn: &mut DbConnection,
    ticket: &Ticket,
    state: &str,
    decision: &str,
    actor: Option<Uuid>,
    comment: Option<&str>,
    channel: &str,
    observer: Option<&dyn crate::repository::tickets::TicketUpdatedObserver>,
) -> QueryResult<()> {
    let mut ticket = if state != PENDING {
        set_state(conn, ticket.id, state)?
    } else {
        ticket.clone()
    };
    emit_event(
        conn,
        &ticket,
        "ticket.approval_decided",
        json!({
            "id": ticket.id,
            "title": ticket.title,
            "requester_uuid": ticket.requester_uuid,
            "approval_state": state,
            "decision": decision,
            "decided_by": actor,
            "comment": comment,
            "channel": channel,
        }),
    )?;
    if state == DECLINED {
        // No cancelled state configured: the approval state alone says it.
        if let Some(cancelled) = crate::repository::workflow_states::first_in_category(
            conn,
            WorkflowStateCategory::Cancelled,
        )
        .optional()?
        {
            let now = Utc::now().naive_utc();
            ticket = crate::repository::tickets::update_ticket_partial(
                conn,
                ticket.id,
                TicketUpdate {
                    workflow_state_id: Some(cancelled.id),
                    updated_at: Some(now),
                    ..Default::default()
                },
                observer,
            )?;
        }
    }
    let _ = ticket;
    Ok(())
}

#[derive(diesel::QueryableByName)]
struct TimedOut {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    workspace_id: i32,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    ticket_id: i32,
}

/// `(workspace_id, ticket_id)` of requests whose approval has waited longer
/// than their workspace's automatic-approval period. Cross-workspace: run under
/// the bypass context.
// sync-audit-only: a read-only scan (raw SELECT), no write
pub fn timed_out(conn: &mut DbConnection, limit: i64) -> QueryResult<Vec<(i32, i32)>> {
    let rows: Vec<TimedOut> = diesel::sql_query(
        "SELECT t.workspace_id, t.id AS ticket_id \
         FROM tickets t \
         JOIN site_settings s ON s.workspace_id = t.workspace_id \
         WHERE t.approval_state = 'pending' \
           AND s.approval_auto_approve_days IS NOT NULL \
           AND EXISTS (SELECT 1 FROM ticket_approvals a \
                       WHERE a.ticket_id = t.id AND a.decision IS NULL \
                         AND a.created_at < now() - make_interval(days => s.approval_auto_approve_days)) \
         ORDER BY t.id LIMIT $1",
    )
    .bind::<diesel::sql_types::BigInt, _>(limit)
    .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.workspace_id, r.ticket_id))
        .collect())
}

// sync-audit-only: approval rows are audited; the outcome emits ticket.approval_decided below
/// Nobody answered in time: approve the waiting rows automatically, recorded
/// as such. `false` when it was no longer waiting.
pub fn auto_approve(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<bool> {
    conn.transaction(|conn| {
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        if ticket.approval_state.as_deref() != Some(PENDING) {
            return Ok(false);
        }
        let rows = current_round(conn, ticket_id)?;
        let ids: Vec<i32> = rows
            .iter()
            .filter(|r| r.decision.is_none())
            .map(|r| r.id)
            .collect();
        if ids.is_empty() {
            return Ok(false);
        }
        diesel::update(ticket_approvals::table.filter(ticket_approvals::id.eq_any(&ids)))
            .set((
                ticket_approvals::decision.eq(APPROVED),
                ticket_approvals::channel.eq("timeout"),
                ticket_approvals::decided_at.eq(Utc::now()),
            ))
            .execute(conn)?;
        conclude(
            conn, &ticket, APPROVED, APPROVED, None, None, "timeout", None,
        )?;
        Ok(true)
    })
}

/// Approvals waiting for `approver`, newest first, with their tickets.
pub fn waiting_for(
    conn: &mut DbConnection,
    approver: Uuid,
) -> QueryResult<Vec<(TicketApproval, Ticket)>> {
    ticket_approvals::table
        .inner_join(tickets::table)
        .filter(ticket_approvals::approver_uuid.eq(approver))
        .filter(ticket_approvals::decision.is_null())
        .filter(tickets::approval_state.eq(PENDING))
        .order(ticket_approvals::created_at.desc())
        .limit(100)
        .select((TicketApproval::as_select(), tickets::all_columns))
        .load(conn)
}

/// `approver`'s waiting or decided row in the ticket's current round.
pub fn for_approver(
    conn: &mut DbConnection,
    ticket_id: i32,
    approver: Uuid,
) -> QueryResult<Option<TicketApproval>> {
    Ok(current_round(conn, ticket_id)?
        .into_iter()
        .find(|r| r.approver_uuid == approver))
}

/// What an approver needs to decide: the request, who asked, and what for.
#[derive(Debug, serde::Serialize)]
pub struct ApprovalSummary {
    pub ticket_id: i32,
    pub ticket_number: i32,
    pub title: String,
    pub request_type: Option<String>,
    pub requester_name: Option<String>,
    /// The requester's own words (their first public message), plain text.
    pub description: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub approval_state: Option<String>,
    /// Everyone in the current round, and where each stands.
    pub approvers: Vec<ApproverStatus>,
}

#[derive(Debug, serde::Serialize)]
pub struct ApproverStatus {
    pub uuid: Uuid,
    pub name: String,
    pub decision: Option<String>,
    pub comment: Option<String>,
    pub decided_at: Option<chrono::DateTime<Utc>>,
}

pub fn summary(conn: &mut DbConnection, ticket: &Ticket) -> QueryResult<ApprovalSummary> {
    use crate::schema::{comments, ticket_categories, users};
    let request_type = match ticket.category_id {
        Some(id) => ticket_categories::table
            .find(id)
            .select(ticket_categories::name)
            .first::<String>(conn)
            .optional()?,
        None => None,
    };
    let requester_name = match ticket.requester_uuid {
        Some(r) => users::table
            .find(r)
            .select(users::name)
            .first::<String>(conn)
            .optional()?,
        None => None,
    };
    let description = comments::table
        .filter(comments::ticket_id.eq(ticket.id))
        .filter(comments::is_internal.eq(false))
        .filter(comments::deleted_at.is_null())
        .order(comments::created_at.asc())
        .select((comments::body_text, comments::content))
        .first::<(Option<String>, String)>(conn)
        .optional()?
        .map(|(text, content)| text.unwrap_or(content));
    let rows = current_round(conn, ticket.id)?;
    let uuids: Vec<Uuid> = rows.iter().map(|r| r.approver_uuid).collect();
    let names: std::collections::HashMap<Uuid, String> = users::table
        .filter(users::uuid.eq_any(&uuids))
        .select((users::uuid, users::name))
        .load::<(Uuid, String)>(conn)?
        .into_iter()
        .collect();
    Ok(ApprovalSummary {
        ticket_id: ticket.id,
        ticket_number: ticket.number,
        title: ticket.title.clone(),
        request_type,
        requester_name,
        description,
        created_at: ticket.created_at,
        approval_state: ticket.approval_state.clone(),
        approvers: rows
            .into_iter()
            .map(|r| ApproverStatus {
                name: names.get(&r.approver_uuid).cloned().unwrap_or_default(),
                uuid: r.approver_uuid,
                decision: r.decision,
                comment: r.comment,
                decided_at: r.decided_at,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn approval_type(conn: &mut DbConnection, rule: &str, by_manager: bool, named: &[Uuid]) -> i32 {
        let cat =
            TestFixtures::create_category(conn, &format!("Needs approval {rule} {by_manager}"));
        diesel::update(crate::schema::ticket_categories::table.find(cat.id))
            .set((
                crate::schema::ticket_categories::approval_required.eq(true),
                crate::schema::ticket_categories::approval_rule.eq(rule),
                crate::schema::ticket_categories::approval_by_manager.eq(by_manager),
            ))
            .execute(conn)
            .unwrap();
        crate::repository::categories::set_category_approvers(conn, cat.id, named).unwrap();
        cat.id
    }

    fn ticket_in(conn: &mut DbConnection, category: i32, requester: Uuid) -> Ticket {
        let t = TestFixtures::create_ticket(conn, "Laptop", Some(requester), Some(category));
        tickets::table.find(t.id).first(conn).unwrap()
    }

    #[test]
    fn a_request_waits_until_an_approver_decides() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "appr_req", "user");
        let boss = TestFixtures::create_user(&mut conn, "appr_boss", "user");
        let cat = approval_type(&mut conn, "any", false, &[boss.uuid]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);

        assert_eq!(
            start_if_required(&mut conn, &ticket).unwrap().as_deref(),
            Some(PENDING)
        );
        // Someone else can't decide it.
        assert_eq!(
            decide(
                &mut conn,
                ticket.id,
                requester.uuid,
                true,
                None,
                "portal",
                None
            ),
            Err(DecideError::NotWaiting)
        );
        assert_eq!(
            decide(&mut conn, ticket.id, boss.uuid, true, None, "portal", None).unwrap(),
            APPROVED
        );
        let t: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        assert_eq!(t.approval_state.as_deref(), Some(APPROVED));
        // Once decided, nothing more to decide.
        assert_eq!(
            decide(
                &mut conn,
                ticket.id,
                boss.uuid,
                false,
                Some("no"),
                "portal",
                None
            ),
            Err(DecideError::NotWaiting)
        );
    }

    #[test]
    fn every_approver_must_approve_under_all_and_a_decline_needs_a_reason() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "all_req", "user");
        let a = TestFixtures::create_user(&mut conn, "all_a", "user");
        let b = TestFixtures::create_user(&mut conn, "all_b", "user");
        let cat = approval_type(&mut conn, "all", false, &[a.uuid, b.uuid]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);
        start_if_required(&mut conn, &ticket).unwrap();

        assert_eq!(
            decide(&mut conn, ticket.id, a.uuid, true, None, "email", None).unwrap(),
            PENDING
        );
        assert_eq!(
            decide(
                &mut conn,
                ticket.id,
                b.uuid,
                false,
                Some("  "),
                "email",
                None
            ),
            Err(DecideError::CommentRequired)
        );
        assert_eq!(
            decide(
                &mut conn,
                ticket.id,
                b.uuid,
                false,
                Some("Over budget"),
                "email",
                None
            )
            .unwrap(),
            DECLINED
        );
        let t: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        let category =
            crate::repository::workflow_states::category_of(&mut conn, t.workflow_state_id)
                .unwrap();
        assert_eq!(
            category,
            Some(WorkflowStateCategory::Cancelled),
            "declined requests close"
        );
    }

    #[test]
    fn the_manager_approves_and_an_approver_who_is_the_requester_already_has() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "mgr_req", "user");
        let manager = TestFixtures::create_user(&mut conn, "mgr_mgr", "user");
        crate::repository::user_contact::set_manager(
            &mut conn,
            requester.uuid,
            Some(manager.uuid),
            None,
        )
        .unwrap();
        let cat = approval_type(&mut conn, "any", true, &[]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);
        start_if_required(&mut conn, &ticket).unwrap();
        let rows = current_round(&mut conn, ticket.id).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.approver_uuid).collect::<Vec<_>>(),
            vec![manager.uuid]
        );

        // The requester is a named approver of their own request type.
        let cat2 = approval_type(&mut conn, "any", false, &[requester.uuid]);
        let own = ticket_in(&mut conn, cat2, requester.uuid);
        assert_eq!(
            start_if_required(&mut conn, &own).unwrap().as_deref(),
            Some(APPROVED)
        );
    }

    #[test]
    fn with_nobody_to_ask_it_still_waits_and_staff_can_skip_it() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "none_req", "user");
        let agent = TestFixtures::create_user(&mut conn, "none_agent", "technician");
        // Manager approval, but no manager on file and no named fallback.
        let cat = approval_type(&mut conn, "any", true, &[]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);
        assert_eq!(
            start_if_required(&mut conn, &ticket).unwrap().as_deref(),
            Some(PENDING)
        );
        assert!(current_round(&mut conn, ticket.id).unwrap().is_empty());

        skip(&mut conn, ticket.id, agent.uuid, Some("Approved by phone")).unwrap();
        let t: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        assert_eq!(t.approval_state.as_deref(), Some(SKIPPED));
        let rows = current_round(&mut conn, ticket.id).unwrap();
        assert_eq!(rows.len(), 1, "the skip is recorded as a row");
        assert_eq!(rows[0].decided_by, Some(agent.uuid));
        assert_eq!(rows[0].comment.as_deref(), Some("Approved by phone"));
        assert_eq!(
            skip(&mut conn, ticket.id, agent.uuid, None),
            Err(DecideError::NotWaiting)
        );
    }

    #[test]
    fn a_waiting_request_cant_be_resolved_and_a_reopened_decline_asks_again() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "guard_req", "user");
        let boss = TestFixtures::create_user(&mut conn, "guard_boss", "user");
        let cat = approval_type(&mut conn, "any", false, &[boss.uuid]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);
        start_if_required(&mut conn, &ticket).unwrap();
        let done = crate::repository::workflow_states::first_in_category(
            &mut conn,
            WorkflowStateCategory::Done,
        )
        .unwrap();
        assert!(blocks_resolution(&mut conn, ticket.id, done.id).unwrap());

        decide(
            &mut conn,
            ticket.id,
            boss.uuid,
            false,
            Some("Not now"),
            "portal",
            None,
        )
        .unwrap();
        assert!(
            !blocks_resolution(&mut conn, ticket.id, done.id).unwrap(),
            "no longer waiting"
        );
        let declined: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        restart_if_declined(&mut conn, &declined).unwrap();
        let t: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        assert_eq!(t.approval_state.as_deref(), Some(PENDING));
        let rows = current_round(&mut conn, ticket.id).unwrap();
        assert_eq!(rows[0].round, 2);
        assert_eq!(rows[0].decision, None);
    }

    #[test]
    fn nobody_answering_in_time_approves_it_automatically() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "late_req", "user");
        let boss = TestFixtures::create_user(&mut conn, "late_boss", "user");
        let cat = approval_type(&mut conn, "all", false, &[boss.uuid]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);
        start_if_required(&mut conn, &ticket).unwrap();
        assert!(auto_approve(&mut conn, ticket.id).unwrap());
        let t: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        assert_eq!(t.approval_state.as_deref(), Some(APPROVED));
        assert_eq!(
            current_round(&mut conn, ticket.id).unwrap()[0]
                .channel
                .as_deref(),
            Some("timeout")
        );
        assert!(!auto_approve(&mut conn, ticket.id).unwrap(), "only once");
    }

    #[test]
    fn only_approvals_older_than_the_workspace_period_time_out() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "scan_req", "user");
        let boss = TestFixtures::create_user(&mut conn, "scan_boss", "user");
        let cat = approval_type(&mut conn, "any", false, &[boss.uuid]);
        let ticket = ticket_in(&mut conn, cat, requester.uuid);
        start_if_required(&mut conn, &ticket).unwrap();
        crate::repository::site_settings::get_site_settings(&mut conn).unwrap();

        let set_days = |conn: &mut DbConnection, days: Option<i32>| {
            diesel::update(crate::schema::site_settings::table)
                .set(crate::schema::site_settings::approval_auto_approve_days.eq(days))
                .execute(conn)
                .unwrap();
        };
        let found = |conn: &mut DbConnection| {
            timed_out(conn, 1000)
                .unwrap()
                .iter()
                .any(|(_, id)| *id == ticket.id)
        };
        set_days(&mut conn, None);
        assert!(!found(&mut conn), "off by default");
        set_days(&mut conn, Some(2));
        assert!(!found(&mut conn), "too recent");
        diesel::update(ticket_approvals::table.filter(ticket_approvals::ticket_id.eq(ticket.id)))
            .set(ticket_approvals::created_at.eq(Utc::now() - chrono::Duration::days(3)))
            .execute(&mut conn)
            .unwrap();
        assert!(found(&mut conn));
    }

    #[test]
    fn a_type_without_approval_leaves_the_ticket_alone() {
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "plain_req", "user");
        let cat = TestFixtures::create_category(&mut conn, "Plain");
        let ticket = ticket_in(&mut conn, cat.id, requester.uuid);
        assert_eq!(start_if_required(&mut conn, &ticket).unwrap(), None);
    }
}
