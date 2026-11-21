//! Time-elapsed rules fire: the clock queue's upkeep and the worker that
//! runs the rules whose clocks are due.
//!
//! Each live or dry-run time-elapsed rule has a row in `ticket_elapsed_queue`
//! for every open ticket it waits on, saying when it is next due
//! ([`clocks::next_wait`]). The rows are derived, never the source of truth,
//! so three paths keep them in line by recomputing them from the tickets:
//!
//! - the event consumer, for a ticket an event changed (a reply, a change of
//!   state, assignee or due date, a close): [`maintain_ticket`];
//! - saving, pausing or putting live a time-elapsed rule, for every open
//!   ticket in batches: [`requeue_batch`];
//! - a daily reconcile across every workspace that repairs drift (a deleted
//!   comment, a merge, a missed event): [`reconcile`].
//!
//! A row already past due is queued only while the rule's conditions are
//! met, so a rule whose conditions rule most tickets out doesn't refill the
//! queue with work that would be turned away.
//!
//! The fire job ([`fire_due`]) claims due rows across workspaces (`FOR
//! UPDATE SKIP LOCKED` with a lease, capped per tick and per workspace) and
//! for each, pinned to its workspace: reads the clock and the conditions
//! again, then runs the rule as the system actor `rules_engine:<rule_id>`
//! (a dry-run rule in shadow). Under the ticket's lock the run reads the
//! clock once more and moves the row on (deleted, or the next repeat) in the
//! run's own transaction, so two fires of one due clock run the rule once.
//! A failure that may pass leaves no row and tries again with backoff; any
//! other is recorded as a `failed` application row and counts as the fire.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use diesel::prelude::*;
use serde_json::{json, Value};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::models::{NewRuleApplication, Rule, RuleApplicationStatus, RuleState, RuleTriggerKind};
use crate::repository::rule_elapsed_queue::{self as queue, DueRow, LastFire};
use crate::repository::rules::{
    self as repo, ApplyContext, ApplyError, ApplySource, OriginatingEvent, SkipCount,
};
use crate::services::rules::clocks::{self, ClockInputs, TimeTrigger, Wait};
use crate::services::rules::conditions::{self, EventFacts, Predicate, TicketFacts};
use crate::services::rules::engine::{after_live_run, backoff_secs, MAX_ATTEMPTS};
use crate::services::search::SearchService;
use crate::sync::actor::ActorContext;
use crate::sync::session::{background_run, run_in_workspace, with_actor_context};

/// The most due rows one tick fires.
pub const TICK_CAP: i64 = 500;
/// The most due rows one tick fires in one workspace, so a backlog in one
/// (after an outage, a rule put live on many old tickets) doesn't hold up
/// the rest.
pub const WORKSPACE_CAP: i64 = 100;
/// Tickets per transaction when a rule's rows are rebuilt.
pub const BATCH: i64 = 500;
/// A claim older than this was lost (the worker died); the row is claimed
/// again.
const STALE_CLAIM_SECS: f64 = 300.0;
/// How long a fire waits for the ticket's lock before trying again later.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// A time-elapsed rule that waits: live or in dry run, with settings it can
/// run on. A rule saved before its settings were checked waits on nothing.
pub struct TimeRule {
    pub rule: Rule,
    pub trigger: TimeTrigger,
}

impl TimeRule {
    pub fn of(rule: Rule) -> Option<Self> {
        if rule.trigger_kind != RuleTriggerKind::TimeElapsed
            || rule.archived_at.is_some()
            || !matches!(rule.state, RuleState::Live | RuleState::DryRun)
        {
            return None;
        }
        let trigger = clocks::parse_trigger(&rule.trigger_config).ok()?;
        Some(Self { rule, trigger })
    }

    fn clock_name(&self) -> &'static str {
        self.trigger.clock.as_str()
    }

    /// When the rule is next due on `ticket`.
    fn wait(&self, ticket: &ClockInputs, fires: &[LastFire]) -> Option<Wait> {
        let last = fires
            .iter()
            .find(|f| f.rule_id == self.rule.id && f.ticket_id == ticket.ticket_id)
            .and_then(|f| f.for_state(self.rule.state));
        clocks::next_wait(&self.trigger, ticket, last)
    }
}

/// The pinned workspace's time-elapsed rules that wait.
fn time_rules(conn: &mut DbConnection) -> QueryResult<Vec<TimeRule>> {
    Ok(repo::event_candidates(conn, RuleTriggerKind::TimeElapsed)?
        .into_iter()
        .filter_map(TimeRule::of)
        .collect())
}

/// How a rule's conditions read a ticket after `minutes` on its clock.
fn evaluate(predicate: &Predicate, facts: &TicketFacts, minutes: i64) -> bool {
    predicate.matches(
        facts,
        Some(&EventFacts {
            minutes_since: Some(minutes),
            ..Default::default()
        }),
    )
}

/// Bring `rules`' rows on `ticket_ids` in line with the tickets as they are
/// now, in the pinned workspace.
fn sync_rows(
    conn: &mut DbConnection,
    workspace_id: i32,
    rules: &[TimeRule],
    ticket_ids: &[i32],
) -> QueryResult<()> {
    if rules.is_empty() || ticket_ids.is_empty() {
        return Ok(());
    }
    let tickets = queue::clock_inputs(conn, ticket_ids)?;
    let rule_ids: Vec<i32> = rules.iter().map(|r| r.rule.id).collect();
    let fires = queue::last_fires(conn, &rule_ids, ticket_ids)?;
    let now = Utc::now();
    let mut waits: Vec<(i32, &TimeRule, Option<Wait>)> = Vec::new();
    for ticket in &tickets {
        for rule in rules {
            waits.push((ticket.ticket_id, rule, rule.wait(ticket, &fires)));
        }
    }
    // A wait already past due is kept only while the conditions are met.
    let overdue: Vec<i32> = {
        let mut ids: Vec<i32> = waits
            .iter()
            .filter(|(_, _, w)| w.is_some_and(|w| w.due_at <= now))
            .map(|(id, _, _)| *id)
            .collect();
        ids.dedup();
        ids
    };
    let facts = if overdue.is_empty() {
        Vec::new()
    } else {
        repo::load_ticket_facts(conn, &overdue)?
    };
    for (ticket_id, rule, wait) in waits {
        let wait = wait.filter(|w| {
            if w.due_at > now {
                return true;
            }
            let Some(facts) = facts.iter().find(|f| f.id == ticket_id) else {
                return false;
            };
            // Conditions that can't be read are queued anyway, so the fire
            // records why the rule can't run.
            conditions::compile(&rule.rule.conditions, RuleTriggerKind::TimeElapsed)
                .map_or(true, |p| {
                    evaluate(&p, facts, clocks::minutes_since(w.started_at, now))
                })
        });
        queue::set_wait(
            conn,
            workspace_id,
            ticket_id,
            rule.rule.id,
            rule.clock_name(),
            wait,
        )?;
    }
    Ok(())
}

/// Bring the time-elapsed rules' rows on a ticket in line with it, in the
/// pinned workspace: after an event changed the ticket.
pub fn maintain_ticket(
    conn: &mut DbConnection,
    workspace_id: i32,
    ticket_id: i32,
) -> QueryResult<()> {
    let rules = time_rules(conn)?;
    sync_rows(conn, workspace_id, &rules, &[ticket_id])
}

/// Rebuild one batch of a rule's rows, in the pinned workspace: the next
/// [`BATCH`] tickets after `after` it may wait on. Returns where the next
/// batch starts, or `None` when this was the last. A rule that doesn't wait
/// (paused, archived, draft, or not time-elapsed) loses all its rows in one.
pub fn requeue_batch(
    conn: &mut DbConnection,
    rule_id: i32,
    after: i32,
) -> QueryResult<Option<i32>> {
    let Some(rule) = repo::find(conn, rule_id)? else {
        return Ok(None);
    };
    let Some(rule) = TimeRule::of(rule) else {
        queue::clear_rule(conn, rule_id)?;
        return Ok(None);
    };
    let ids = queue::candidate_tickets(conn, rule_id, after, BATCH)?;
    sync_rows(
        conn,
        rule.rule.workspace_id,
        std::slice::from_ref(&rule),
        &ids,
    )?;
    Ok(if ids.len() as i64 >= BATCH {
        ids.last().copied()
    } else {
        None
    })
}

/// Rebuild every row of `rule_id` in `workspace_id`, one batch per
/// transaction.
pub fn requeue_rule(pool: &Pool, workspace_id: i32, rule_id: i32) -> Result<(), String> {
    let mut after = 0;
    loop {
        match run_in_workspace(pool, "rules_engine:requeue", workspace_id, |c| {
            requeue_batch(c, rule_id, after)
        }) {
            Ok(Some(next)) => after = next,
            Ok(None) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The daily repair: drop the rows of rules that no longer wait, then
/// rebuild every waiting rule's rows in every workspace that has one.
pub fn reconcile(pool: &Pool) -> Result<usize, String> {
    // cross-tenant: the clock queue holds every workspace's rows; a rule
    // that no longer waits loses its rows wherever it is.
    background_run(pool, "background:rules_elapsed_reconcile", |c| {
        queue::clear_idle_rules(c)
    })
    .map_err(|e| e.to_string())?;
    // cross-tenant: which workspaces have a waiting rule; each is rebuilt
    // pinned to its own workspace below.
    let workspaces = background_run(pool, "background:rules_elapsed_reconcile", |c| {
        queue::workspaces_with_time_rules(c)
    })
    .map_err(|e| e.to_string())?;
    let mut rebuilt = 0;
    for workspace_id in workspaces {
        let rule_ids: Vec<i32> =
            run_in_workspace(pool, "rules_engine:reconcile", workspace_id, |c| {
                Ok(time_rules(c)?.into_iter().map(|r| r.rule.id).collect())
            })
            .map_err(|e| e.to_string())?;
        for rule_id in rule_ids {
            if let Err(e) = requeue_rule(pool, workspace_id, rule_id) {
                warn!(rule_id, workspace_id, error = %e, "time-elapsed rules: a rule's clocks were not rebuilt");
                continue;
            }
            rebuilt += 1;
        }
    }
    Ok(rebuilt)
}

/// What became of a claimed row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fired {
    /// The rule ran (in shadow for a dry-run rule), or ran and failed, which
    /// counts as its fire.
    Ran,
    /// The conditions weren't met; the row is gone until the ticket changes.
    Unmet,
    /// Not due after all: the clock restarted or stopped, the rule already
    /// fired for it, or the rule no longer waits. The row was moved on.
    NotDue,
    /// A failure that may pass; it is tried again later.
    Retry,
}

/// Claim due rows across every workspace and fire each. Returns how many
/// were claimed.
pub fn fire_due(pool: &Pool, search: Option<&Arc<SearchService>>) -> Result<usize, String> {
    // cross-tenant: the clock queue holds every workspace's due rows; each
    // row's work runs pinned to its own workspace in `fire_claimed`.
    let claimed = background_run(pool, "background:rules_elapsed_claim", |c| {
        queue::claim_due(c, TICK_CAP, WORKSPACE_CAP, STALE_CLAIM_SECS)
    })
    .map_err(|e| e.to_string())?;
    for row in &claimed {
        fire_claimed(pool, search, row);
    }
    if !claimed.is_empty() {
        info!(
            count = claimed.len(),
            "time-elapsed rules: due clocks handled"
        );
    }
    Ok(claimed.len())
}

/// Fire one claimed row. A database failure outside the rule's run leaves
/// the claim to lapse, and the row is claimed again.
pub fn fire_claimed(pool: &Pool, search: Option<&Arc<SearchService>>, row: &DueRow) -> Fired {
    match try_fire(pool, search, row) {
        Ok(fired) => fired,
        Err(e) => {
            warn!(
                rule_id = row.rule_id,
                ticket_id = row.ticket_id,
                error = %e,
                "time-elapsed rules: fire failed; tried again later"
            );
            Fired::Retry
        }
    }
}

/// A claimed row, read again before the run.
enum Ready {
    /// Not due; the row was moved on.
    NotDue,
    Due {
        rule: TimeRule,
        wait: Wait,
        facts: TicketFacts,
    },
}

fn try_fire(
    pool: &Pool,
    search: Option<&Arc<SearchService>>,
    row: &DueRow,
) -> Result<Fired, diesel::result::Error> {
    let workspace_id = row.workspace_id;
    let ticket_id = row.ticket_id;
    let mut conn = pool
        .get()
        .map_err(|e| diesel::result::Error::QueryBuilderError(format!("pool: {e}").into()))?;
    let reader = ActorContext::system("rules_engine").with_workspace(workspace_id);

    let ready = with_actor_context(&mut conn, &reader, |c| {
        let Some(rule) = repo::find(c, row.rule_id)?.and_then(TimeRule::of) else {
            queue::clear_rule(c, row.rule_id)?;
            return Ok(Ready::NotDue);
        };
        let Some(ticket) = queue::clock_inputs(c, &[ticket_id])?.pop() else {
            // The ticket is gone, and its rows with it.
            return Ok(Ready::NotDue);
        };
        let fires = queue::last_fires(c, &[rule.rule.id], &[ticket_id])?;
        let wait = rule.wait(&ticket, &fires);
        let due = wait.filter(|w| w.due_at <= Utc::now());
        let (Some(wait), Some(facts)) = (due, repo::load_ticket_facts(c, &[ticket_id])?.pop())
        else {
            queue::set_wait(
                c,
                workspace_id,
                ticket_id,
                rule.rule.id,
                rule.clock_name(),
                wait,
            )?;
            return Ok(Ready::NotDue);
        };
        Ok::<_, diesel::result::Error>(Ready::Due { rule, wait, facts })
    })?;
    let Ready::Due { rule, wait, facts } = ready else {
        return Ok(Fired::NotDue);
    };

    let shadow = rule.rule.state == RuleState::DryRun;
    let fire_id = Uuid::now_v7();
    let mut actor =
        ActorContext::system(format!("rules_engine:{}", rule.rule.id)).with_workspace(workspace_id);
    // A fire starts a chain of events of its own: the event rules its writes
    // set off share its id as their root.
    actor.correlation_id = Some(fire_id);
    let now = Utc::now();
    let minutes = clocks::minutes_since(wait.started_at, now);
    let mut evaluation = json!({
        "trigger": RuleTriggerKind::TimeElapsed.as_str(),
        "clock": rule.clock_name(),
        "clock_started_at": wait.started_at,
        "minutes_since": minutes,
    });
    let originating = OriginatingEvent {
        id: fire_id,
        kind: format!("clock.{}", rule.clock_name()),
    };

    let predicate = match conditions::compile(&rule.rule.conditions, RuleTriggerKind::TimeElapsed) {
        Ok(p) => p,
        Err(e) => {
            // Checked at save, so only a rule saved before the check. The
            // failed row counts as the fire.
            record(
                &mut conn,
                &actor,
                &rule.rule,
                ticket_id,
                &originating,
                RuleApplicationStatus::Failed,
                evaluation,
                Some(format!("The conditions can't be evaluated: {e}")),
            )?;
            resync(&mut conn, &reader, workspace_id, &rule, ticket_id)?;
            return Ok(Fired::Ran);
        }
    };
    let matched = evaluate(&predicate, &facts, minutes);
    evaluation["matched"] = json!(matched);
    if !matched {
        if shadow {
            record(
                &mut conn,
                &actor,
                &rule.rule,
                ticket_id,
                &originating,
                RuleApplicationStatus::SkippedConditionUnmet,
                evaluation,
                None,
            )?;
        } else {
            with_actor_context(&mut conn, &reader, |c| {
                repo::count_skips(c, &[rule.rule.id], SkipCount::ConditionUnmet)
            })?;
        }
        // Queued again when an event brings the ticket into the conditions.
        with_actor_context(&mut conn, &reader, |c| {
            queue::set_wait(
                c,
                workspace_id,
                ticket_id,
                rule.rule.id,
                rule.clock_name(),
                None,
            )
        })?;
        return Ok(Fired::Unmet);
    }

    // Under the ticket's lock, in the run's transaction: the clock and the
    // conditions are read once more, and the row moves on as if the rule has
    // fired, which it has when the run commits. Anything changed since the
    // read above ends the run with nothing recorded.
    let recheck = |c: &mut DbConnection| -> QueryResult<bool> {
        let Some(ticket) = queue::clock_inputs(c, &[ticket_id])?.pop() else {
            return Ok(false);
        };
        let fires = queue::last_fires(c, &[rule.rule.id], &[ticket_id])?;
        let now = Utc::now();
        let Some(wait) = rule.wait(&ticket, &fires).filter(|w| w.due_at <= now) else {
            return Ok(false);
        };
        let Some(facts) = repo::load_ticket_facts(c, &[ticket_id])?.pop() else {
            return Ok(false);
        };
        if !evaluate(
            &predicate,
            &facts,
            clocks::minutes_since(wait.started_at, now),
        ) {
            return Ok(false);
        }
        let next = clocks::next_wait(&rule.trigger, &ticket, Some(now));
        queue::set_wait(
            c,
            workspace_id,
            ticket_id,
            rule.rule.id,
            rule.clock_name(),
            next,
        )?;
        Ok(true)
    };
    let observer = search.map(|s| s as &dyn crate::repository::tickets::TicketUpdatedObserver);
    let retries = row.attempts < MAX_ATTEMPTS;
    let ctx = ApplyContext {
        shadow,
        originating_event: Some(originating),
        condition_evaluation: Some(evaluation),
        observer,
        lock_timeout: Some(LOCK_TIMEOUT),
        retries,
        recheck: Some(&recheck),
        ..ApplyContext::new(&actor, rule.rule.id, ticket_id, ApplySource::Time)
    };
    match repo::run_rule_on_ticket(&mut conn, ctx) {
        Ok(outcome) => {
            if !shadow {
                after_live_run(
                    pool,
                    search,
                    &mut conn,
                    workspace_id,
                    ticket_id,
                    outcome.comment_id,
                    outcome.actions_executed,
                );
            }
            Ok(Fired::Ran)
        }
        Err(failed) => match failed.error {
            ApplyError::NoLongerDue => {
                debug!(
                    rule_id = rule.rule.id,
                    ticket_id, "time-elapsed rules: no longer due under the lock"
                );
                resync(&mut conn, &reader, workspace_id, &rule, ticket_id)?;
                Ok(Fired::NotDue)
            }
            e if e.is_transient() && retries => {
                warn!(rule_id = rule.rule.id, ticket_id, error = %e, "time-elapsed rules: run failed; retrying");
                let retry_at = Utc::now() + chrono::Duration::seconds(backoff_secs(row.attempts));
                with_actor_context(&mut conn, &reader, |c| queue::retry_at(c, row, retry_at))?;
                Ok(Fired::Retry)
            }
            e => {
                // Recorded as failed (or suppressed), or turned away at the
                // preflight (no longer live, the ticket merged): the row
                // follows what is now true.
                if failed.recorded.is_none() {
                    debug!(rule_id = rule.rule.id, ticket_id, error = %e, "time-elapsed rules: run turned away");
                }
                resync(&mut conn, &reader, workspace_id, &rule, ticket_id)?;
                Ok(Fired::Ran)
            }
        },
    }
}

/// Recompute one rule's row on one ticket.
fn resync(
    conn: &mut DbConnection,
    reader: &ActorContext,
    workspace_id: i32,
    rule: &TimeRule,
    ticket_id: i32,
) -> QueryResult<()> {
    with_actor_context(conn, reader, |c| {
        // Read again: the run may have left the rule paused or archived.
        match repo::find(c, rule.rule.id)?.and_then(TimeRule::of) {
            Some(rule) => sync_rows(c, workspace_id, std::slice::from_ref(&rule), &[ticket_id]),
            None => queue::clear_rule(c, rule.rule.id).map(|_| ()),
        }
    })
}

/// An application row the worker writes itself, outside a rule's run: a
/// dry run's condition skip, a rule whose conditions can't be evaluated.
#[allow(clippy::too_many_arguments)]
fn record(
    conn: &mut DbConnection,
    actor: &ActorContext,
    rule: &Rule,
    ticket_id: i32,
    originating: &OriginatingEvent,
    status: RuleApplicationStatus,
    condition_evaluation: Value,
    failure_reason: Option<String>,
) -> QueryResult<()> {
    with_actor_context(conn, actor, |c| {
        let rule_version = repo::current_version(c, rule.id)?;
        repo::record_application(
            c,
            NewRuleApplication {
                workspace_id: rule.workspace_id,
                rule_id: rule.id,
                rule_version,
                ticket_id,
                status,
                correlation_id: actor.correlation_id,
                actor_uuid: None,
                actor_kind: "system".to_string(),
                originating_event_id: Some(originating.id),
                originating_event_kind: Some(originating.kind.clone()),
                condition_evaluation: Some(condition_evaluation),
                actions_taken: None,
                actions_skipped: None,
                failure_reason,
            },
        )
        .map(|_| ())
    })
}
