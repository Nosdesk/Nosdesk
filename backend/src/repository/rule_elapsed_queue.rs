//! The time-elapsed rules' clock queue (`ticket_elapsed_queue`) and what the
//! clocks read from a ticket. The queue caches when each live or dry-run
//! time-elapsed rule is next due on each open ticket; the rules engine
//! derives every row from the ticket ([`clock_inputs`]) and the rule's
//! application rows ([`last_fires`]), so a row can always be rebuilt (see
//! `services::rules::elapsed`).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{
    Array, BigInt, Bool, Double, Integer, Nullable, SmallInt, Text, Timestamptz,
};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{RuleState, RuleTriggerKind, WorkflowStateCategory};
use crate::services::rules::clocks::{ClockInputs, Eligibility, Wait};

#[derive(QueryableByName)]
struct InputRow {
    #[diesel(sql_type = Integer)]
    ticket_id: i32,
    #[diesel(sql_type = Timestamptz)]
    created_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    due_date: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    assignee_uuid: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    submitted_via: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    verification_state: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    approval_state: Option<String>,
    #[diesel(sql_type = Nullable<crate::schema::sql_types::WorkflowStateCategory>)]
    category: Option<WorkflowStateCategory>,
    #[diesel(sql_type = Bool)]
    merged: bool,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    state_changed_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    assigned_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    agent_reply_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    customer_reply_at: Option<DateTime<Utc>>,
}

// sync-audit-only: read-only
/// What the clocks read from each of `ticket_ids` in the pinned workspace.
///
/// A due date is a calendar day, due at the end of that day in the
/// workspace's timezone ([`crate::services::ticket_dates::due_instant`]).
///
/// A state or assignee change is read from the ticket's `sync_actions`, by
/// the fields an update changed (an update that changes both records one
/// event); a ticket with none, or whose events were pruned, counts from its
/// creation. A reply is a public comment that isn't deleted, from staff (an
/// owner, admin or agent of the workspace, or a platform admin) or from
/// anyone else. A reply a rule wrote on its own (`channel_metadata.rule_id`)
/// is no staff reply: an automated warning doesn't restart the clock it was
/// sent on. A reply an agent sent by applying a rule is theirs.
pub fn clock_inputs(conn: &mut DbConnection, ticket_ids: &[i32]) -> QueryResult<Vec<ClockInputs>> {
    if ticket_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<InputRow> = diesel::sql_query(
        r#"
        SELECT t.id AS ticket_id, t.created_at, t.due_date, t.assignee_uuid,
               t.submitted_via::text AS submitted_via,
               t.verification_state::text AS verification_state,
               t.approval_state,
               ws.category,
               EXISTS (SELECT 1 FROM ticket_merges m WHERE m.ticket_id = t.id) AS merged,
               (SELECT max(s.occurred_at) FROM sync_actions s
                 WHERE s.aggregate = 'ticket' AND s.aggregate_id = t.id::text
                   AND s.workspace_id = t.workspace_id
                   AND CASE WHEN s.data ? 'changed_fields'
                            THEN s.data->'changed_fields' ? 'ticket.workflow_state_id'
                            ELSE s.event_type = 'ticket.workflow_state_changed' END
               ) AS state_changed_at,
               (SELECT max(s.occurred_at) FROM sync_actions s
                 WHERE s.aggregate = 'ticket' AND s.aggregate_id = t.id::text
                   AND s.workspace_id = t.workspace_id
                   AND CASE WHEN s.data ? 'changed_fields'
                            THEN s.data->'changed_fields' ? 'ticket.assignee_uuid'
                            ELSE s.event_type = 'ticket.assignee_changed' END
               ) AS assigned_at,
               (SELECT max(c.created_at) FROM comments c
                 WHERE c.ticket_id = t.id AND NOT c.is_internal AND c.deleted_at IS NULL
                   AND NOT coalesce(c.channel_metadata ? 'rule_id', false)
                   AND EXISTS (
                       SELECT 1 FROM users u
                        WHERE u.uuid = c.user_uuid
                          AND (u.platform_role = 'platform_admin'
                               OR EXISTS (SELECT 1 FROM workspace_members wm
                                           WHERE wm.user_uuid = u.uuid
                                             AND wm.workspace_id = t.workspace_id
                                             AND wm.role IN ('owner', 'admin', 'agent')
                                             AND wm.removed_at IS NULL)))
               ) AS agent_reply_at,
               (SELECT max(c.created_at) FROM comments c
                 WHERE c.ticket_id = t.id AND NOT c.is_internal AND c.deleted_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM users u
                        WHERE u.uuid = c.user_uuid
                          AND (u.platform_role = 'platform_admin'
                               OR EXISTS (SELECT 1 FROM workspace_members wm
                                           WHERE wm.user_uuid = u.uuid
                                             AND wm.workspace_id = t.workspace_id
                                             AND wm.role IN ('owner', 'admin', 'agent')
                                             AND wm.removed_at IS NULL)))
               ) AS customer_reply_at
          FROM tickets t
          LEFT JOIN workflow_states ws ON ws.id = t.workflow_state_id
         WHERE t.id = ANY($1)
         ORDER BY t.id
        "#,
    )
    .bind::<Array<Integer>, _>(ticket_ids)
    .load(conn)?;
    let pending = crate::repository::ticket_approvals::PENDING;
    let holds_waiting = if rows
        .iter()
        .any(|r| r.approval_state.as_deref() == Some(pending))
    {
        crate::repository::ticket_approvals::holds_waiting_requests(conn)?
    } else {
        false
    };
    // A due date is a calendar day; it is due at the end of that day in the
    // workspace's timezone.
    let tz = if rows.iter().any(|r| r.due_date.is_some()) {
        use crate::schema::site_settings::dsl as ss;
        let zone: Option<String> = ss::site_settings
            .select(ss::default_timezone)
            .first(conn)
            .optional()?;
        crate::utils::locale::effective_timezone(None, zone.as_deref().unwrap_or("UTC"))
    } else {
        chrono_tz::Tz::UTC
    };
    Ok(rows
        .into_iter()
        .map(|r| {
            let runs_clocks = Eligibility {
                category: r.category,
                submitted_via: r.submitted_via.as_deref(),
                verification_state: r.verification_state.as_deref(),
                merged: r.merged,
                held_for_approval: holds_waiting && r.approval_state.as_deref() == Some(pending),
            }
            .runs_clocks();
            ClockInputs {
                ticket_id: r.ticket_id,
                created_at: r.created_at,
                due_at: r
                    .due_date
                    .map(|d| crate::services::ticket_dates::due_instant(d, tz)),
                assignee_uuid: r.assignee_uuid,
                state_changed_at: r.state_changed_at,
                assigned_at: r.assigned_at,
                agent_reply_at: r.agent_reply_at,
                customer_reply_at: r.customer_reply_at,
                runs_clocks,
            }
        })
        .collect())
}

/// When a rule last ran on a ticket.
#[derive(Debug, Clone, QueryableByName)]
pub struct LastFire {
    #[diesel(sql_type = Integer)]
    pub rule_id: i32,
    #[diesel(sql_type = Integer)]
    pub ticket_id: i32,
    /// Any run that wasn't a skip: a live or a shadow run, a failure, a
    /// suppression.
    #[diesel(sql_type = Timestamptz)]
    pub any_run: DateTime<Utc>,
    /// The same, shadow runs left out.
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub live_run: Option<DateTime<Utc>>,
}

impl LastFire {
    /// The last run that counts for a rule in `state`: a dry-run rule's
    /// shadow runs count while it is in dry run, but a rule that goes live
    /// runs for real on the tickets it shadowed.
    pub fn for_state(&self, state: RuleState) -> Option<DateTime<Utc>> {
        if state == RuleState::DryRun {
            Some(self.any_run)
        } else {
            self.live_run
        }
    }
}

// sync-audit-only: read-only
/// When each of `rule_ids` last ran on each of `ticket_ids`, skips left out.
pub fn last_fires(
    conn: &mut DbConnection,
    rule_ids: &[i32],
    ticket_ids: &[i32],
) -> QueryResult<Vec<LastFire>> {
    if rule_ids.is_empty() || ticket_ids.is_empty() {
        return Ok(Vec::new());
    }
    diesel::sql_query(
        "SELECT rule_id, ticket_id, max(applied_at) AS any_run, \
                max(applied_at) FILTER (WHERE status <> 'dry_run') AS live_run \
           FROM rule_applications \
          WHERE rule_id = ANY($1) AND ticket_id = ANY($2) \
            AND status NOT IN ('skipped_preflight', 'skipped_condition_unmet') \
          GROUP BY rule_id, ticket_id",
    )
    .bind::<Array<Integer>, _>(rule_ids)
    .bind::<Array<Integer>, _>(ticket_ids)
    .load(conn)
}

// sync-audit-only: clock queue bookkeeping, no entity sync
/// Make `rule_id`'s rows on `ticket_id` say `wait` on clock `clock_name`:
/// one row while the rule waits, none when it doesn't. A row that changes
/// gives up any claim on it; one whose clock restarted starts its attempts
/// again.
pub fn set_wait(
    conn: &mut DbConnection,
    workspace_id: i32,
    ticket_id: i32,
    rule_id: i32,
    clock_name: &str,
    wait: Option<Wait>,
) -> QueryResult<()> {
    use crate::schema::ticket_elapsed_queue::dsl as q;
    let rows = q::ticket_elapsed_queue
        .filter(q::ticket_id.eq(ticket_id))
        .filter(q::rule_id.eq(rule_id));
    let Some(wait) = wait else {
        diesel::delete(rows).execute(conn)?;
        return Ok(());
    };
    // A rule edited to wait on another clock.
    diesel::delete(rows.filter(q::clock_name.ne(clock_name))).execute(conn)?;
    diesel::sql_query(
        "INSERT INTO ticket_elapsed_queue \
             (workspace_id, ticket_id, rule_id, clock_name, clock_started_at, due_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (ticket_id, rule_id, clock_name) DO UPDATE \
            SET clock_started_at = EXCLUDED.clock_started_at, \
                due_at = EXCLUDED.due_at, \
                claimed_at = NULL, \
                attempts = CASE WHEN ticket_elapsed_queue.clock_started_at = EXCLUDED.clock_started_at \
                                THEN ticket_elapsed_queue.attempts ELSE 0 END \
          WHERE ticket_elapsed_queue.clock_started_at IS DISTINCT FROM EXCLUDED.clock_started_at \
             OR ticket_elapsed_queue.due_at IS DISTINCT FROM EXCLUDED.due_at",
    )
    .bind::<Integer, _>(workspace_id)
    .bind::<Integer, _>(ticket_id)
    .bind::<Integer, _>(rule_id)
    .bind::<Text, _>(clock_name)
    .bind::<Timestamptz, _>(wait.started_at)
    .bind::<Timestamptz, _>(wait.due_at)
    .execute(conn)?;
    Ok(())
}

// sync-audit-only: clock queue bookkeeping, no entity sync
/// Delete every row of `rule_id`: it no longer waits on any ticket.
pub fn clear_rule(conn: &mut DbConnection, rule_id: i32) -> QueryResult<usize> {
    use crate::schema::ticket_elapsed_queue::dsl as q;
    diesel::delete(q::ticket_elapsed_queue.filter(q::rule_id.eq(rule_id))).execute(conn)
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

// sync-audit-only: read-only
/// The next `limit` tickets after `after`, by id, whose clocks `rule_id` may
/// wait on: those in an open workflow state, and those it has a row for.
pub fn candidate_tickets(
    conn: &mut DbConnection,
    rule_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<i32>> {
    Ok(diesel::sql_query(
        "SELECT t.id FROM tickets t \
           JOIN workflow_states ws ON ws.id = t.workflow_state_id \
          WHERE t.id > $1 \
            AND (ws.category NOT IN ('done', 'cancelled', 'merged') \
                 OR EXISTS (SELECT 1 FROM ticket_elapsed_queue q \
                             WHERE q.ticket_id = t.id AND q.rule_id = $2)) \
          ORDER BY t.id \
          LIMIT $3",
    )
    .bind::<Integer, _>(after)
    .bind::<Integer, _>(rule_id)
    .bind::<BigInt, _>(limit)
    .load::<Id>(conn)?
    .into_iter()
    .map(|r| r.id)
    .collect())
}

/// A due row the fire job claimed.
#[derive(Debug, Clone, QueryableByName)]
pub struct DueRow {
    #[diesel(sql_type = Integer)]
    pub workspace_id: i32,
    #[diesel(sql_type = Integer)]
    pub ticket_id: i32,
    #[diesel(sql_type = Integer)]
    pub rule_id: i32,
    #[diesel(sql_type = Text)]
    pub clock_name: String,
    #[diesel(sql_type = Timestamptz)]
    pub clock_started_at: DateTime<Utc>,
    #[diesel(sql_type = Timestamptz)]
    pub due_at: DateTime<Utc>,
    #[diesel(sql_type = SmallInt)]
    pub attempts: i16,
}

// sync-audit-only: clock queue claim, no entity sync
/// Claim up to `limit` due rows across every workspace, the earliest
/// first and at most `per_workspace` from one workspace, skipping rows
/// another claim holds and taking back claims older than
/// `stale_claim_secs`. Every workspace's, so the caller runs it with row
/// security bypassed; each row's work then runs pinned to its workspace.
pub fn claim_due(
    conn: &mut DbConnection,
    limit: i64,
    per_workspace: i64,
    stale_claim_secs: f64,
) -> QueryResult<Vec<DueRow>> {
    // Postgres takes no row lock alongside a window function, so the cap per
    // workspace is ranked first and the rows are locked after.
    diesel::sql_query(
        r#"
        WITH ranked AS (
            SELECT ticket_id, rule_id, clock_name,
                   row_number() OVER (PARTITION BY workspace_id ORDER BY due_at) AS n
              FROM ticket_elapsed_queue
             WHERE due_at <= now()
               AND (claimed_at IS NULL OR claimed_at < now() - make_interval(secs => $1))
        ),
        due AS (
            SELECT q.ticket_id, q.rule_id, q.clock_name
              FROM ticket_elapsed_queue q
              JOIN ranked r
                ON r.ticket_id = q.ticket_id AND r.rule_id = q.rule_id
               AND r.clock_name = q.clock_name
             WHERE r.n <= $2
               AND q.due_at <= now()
               AND (q.claimed_at IS NULL OR q.claimed_at < now() - make_interval(secs => $1))
             ORDER BY q.due_at
             LIMIT $3
               FOR UPDATE OF q SKIP LOCKED
        )
        UPDATE ticket_elapsed_queue q
           SET claimed_at = now(), attempts = q.attempts + 1
          FROM due
         WHERE q.ticket_id = due.ticket_id AND q.rule_id = due.rule_id
           AND q.clock_name = due.clock_name
     RETURNING q.workspace_id, q.ticket_id, q.rule_id, q.clock_name, q.clock_started_at,
               q.due_at, q.attempts
        "#,
    )
    .bind::<Double, _>(stale_claim_secs)
    .bind::<BigInt, _>(per_workspace)
    .bind::<BigInt, _>(limit)
    .load(conn)
}

// sync-audit-only: clock queue bookkeeping, no entity sync
/// Give up a claimed row's claim and try it again at `retry_at`, after a
/// failure that may pass.
pub fn retry_at(
    conn: &mut DbConnection,
    row: &DueRow,
    retry_at: DateTime<Utc>,
) -> QueryResult<usize> {
    use crate::schema::ticket_elapsed_queue::dsl as q;
    diesel::update(
        q::ticket_elapsed_queue
            .filter(q::ticket_id.eq(row.ticket_id))
            .filter(q::rule_id.eq(row.rule_id))
            .filter(q::clock_name.eq(&row.clock_name)),
    )
    .set((
        q::due_at.eq(retry_at),
        q::claimed_at.eq(None::<DateTime<Utc>>),
    ))
    .execute(conn)
}

/// The workspaces with a live or dry-run time-elapsed rule. Every
/// workspace's, so the caller runs it with row security bypassed.
pub fn workspaces_with_time_rules(conn: &mut DbConnection) -> QueryResult<Vec<i32>> {
    use crate::schema::rules::dsl as r;
    r::rules
        .filter(r::trigger_kind.eq(RuleTriggerKind::TimeElapsed))
        .filter(r::state.eq_any([RuleState::Live, RuleState::DryRun]))
        .filter(r::archived_at.is_null())
        .select(r::workspace_id)
        .distinct()
        .order(r::workspace_id)
        .load(conn)
}

// sync-audit-only: clock queue bookkeeping, no entity sync
/// Delete the rows of rules that no longer wait on anything: archived,
/// paused or draft, or no longer time-elapsed. Every workspace's, so the
/// caller runs it with row security bypassed.
pub fn clear_idle_rules(conn: &mut DbConnection) -> QueryResult<usize> {
    diesel::sql_query(
        "DELETE FROM ticket_elapsed_queue q \
          USING rules r \
          WHERE r.id = q.rule_id AND r.workspace_id = q.workspace_id \
            AND (r.archived_at IS NOT NULL \
                 OR r.state NOT IN ('live', 'dry_run') \
                 OR r.trigger_kind <> 'time_elapsed')",
    )
    .execute(conn)
}
