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
//! A time rule that emails the requester is a warning to every later rule
//! on its clock that closes the ticket (the auto-close warning to the
//! close): the close first fires on a ticket no sooner after its nearest
//! warning than its threshold is after the warning's. A later rule that
//! doesn't close keeps its own time. A ticket warned on time is closed on
//! time; one already past both when they are turned on is warned first and
//! closed that long after. Whether it was warned is read from the warning's
//! application rows, so a rebuilt queue, or a warning paused since, still
//! holds the close. A warning that won't run on the ticket (its conditions
//! aren't met) holds nothing, and the next nearest is asked. The tags a
//! warning adds after its email mark the ticket as warned: they go on only
//! when that email is sent (`repository::rules`), and come off once its
//! clock starts again on the open ticket ([`clear_lapsed_warnings`]). A rule
//! that emails the requester with no close after it keeps its tags either
//! way.
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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::models::{
    NewRuleApplication, Rule, RuleApplicationStatus, RuleState, RuleTriggerKind,
    WorkflowStateCategory,
};
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

/// A time-elapsed rule that isn't archived, with settings it can run on. It
/// waits on its clock while live or in dry run; a paused or draft rule's
/// past runs still count, such as a warning paused after it warned. A rule
/// saved before its settings were checked waits on nothing.
pub struct TimeRule {
    pub rule: Rule,
    pub trigger: TimeTrigger,
    /// Whether it waits on its clock: live or in dry run.
    waits: bool,
    /// Whether it emails the requester, which can make it a warning.
    emails_requester: bool,
    /// Whether it closes the ticket, which makes it wait on a warning
    /// ([`TimeRule::set_closes`]).
    closes: bool,
}

impl TimeRule {
    /// The rule when it waits on its clock.
    pub fn of(rule: Rule) -> Option<Self> {
        Self::parse(rule).filter(|r| r.waits)
    }

    /// The rule when it is a time-elapsed rule that isn't archived, in any
    /// state.
    fn parse(rule: Rule) -> Option<Self> {
        if rule.trigger_kind != RuleTriggerKind::TimeElapsed || rule.archived_at.is_some() {
            return None;
        }
        let trigger = clocks::parse_trigger(&rule.trigger_config).ok()?;
        let emails_requester = rule
            .actions
            .as_array()
            .into_iter()
            .flatten()
            .any(repo::step_emails_requester);
        let waits = matches!(rule.state, RuleState::Live | RuleState::DryRun);
        Some(Self {
            rule,
            trigger,
            waits,
            emails_requester,
            closes: false,
        })
    }

    /// Read whether the rule closes the ticket: a step moves it to a state
    /// in a closing category (done or cancelled), by `categories` of state
    /// id.
    fn set_closes(&mut self, categories: &HashMap<i32, WorkflowStateCategory>) {
        self.closes = self
            .rule
            .actions
            .as_array()
            .into_iter()
            .flatten()
            .filter(|step| step.get("kind").and_then(Value::as_str) == Some("set_status"))
            .filter_map(|step| step.get("config")?.get("workflow_state_id")?.as_i64())
            .filter_map(|id| categories.get(&i32::try_from(id).ok()?))
            .any(|category| category.closes_ticket());
    }

    fn clock_name(&self) -> &'static str {
        self.trigger.clock.as_str()
    }

    /// When the rule last fired on `ticket`, as counts for its state.
    fn last_fire(&self, ticket_id: i32, fires: &[LastFire]) -> Option<DateTime<Utc>> {
        fires
            .iter()
            .find(|f| f.rule_id == self.rule.id && f.ticket_id == ticket_id)
            .and_then(|f| f.for_state(self.rule.state))
    }

    /// When the rule is next due on `ticket` by its own clock alone.
    fn wait(&self, ticket: &ClockInputs, fires: &[LastFire]) -> Option<Wait> {
        clocks::next_wait(
            &self.trigger,
            ticket,
            self.last_fire(ticket.ticket_id, fires),
        )
    }

    /// Whether this rule is a warning before `later`: it emails the
    /// requester, on the same clock counting up, once per start and sooner,
    /// and `later` closes the ticket. A rule that doesn't close keeps its own
    /// time, so a late reminder doesn't hold an escalation.
    fn warns_before(&self, later: &TimeRule) -> bool {
        self.emails_requester
            && later.closes
            && self.rule.id != later.rule.id
            && self.trigger.clock == later.trigger.clock
            && !self.trigger.clock.counts_down()
            && self.trigger.repeat_every_minutes.is_none()
            && self.trigger.minutes < later.trigger.minutes
    }

    /// Whether this rule is a warning at all: one of `rules` that waits
    /// closes the ticket after it. Only a warning's tags mark the ticket as
    /// warned.
    fn is_warning(&self, rules: &[TimeRule]) -> bool {
        rules.iter().any(|r| r.waits && self.warns_before(r))
    }

    /// Whether this rule, not yet fired since its clock started, is still
    /// to warn before `later`: it waits, live, or in dry run before a rule
    /// in dry run, whose shadow runs show what the pair would do.
    fn still_to_warn(&self, later: &TimeRule) -> bool {
        self.waits && (self.rule.state == RuleState::Live || later.rule.state == RuleState::DryRun)
    }

    /// Whether the rule's conditions take `facts` after `minutes` on its
    /// clock. Conditions that can't be read count as met, so the fire
    /// records why the rule can't run.
    fn would_run(&self, facts: &TicketFacts, minutes: i64) -> bool {
        conditions::compile(&self.rule.conditions, RuleTriggerKind::TimeElapsed)
            .map_or(true, |p| evaluate(&p, facts, minutes))
    }
}

/// When `rule` is next due on `ticket`: its own clock's wait, held for its
/// nearest warning among `rules` ([`TimeRule::warns_before`], the largest
/// threshold below its own) until as long after that warning as its
/// threshold is after the warning's. A warning that fired since the clock
/// started (as counts for `rule`'s state, whatever the warning's state now)
/// holds it from that fire; one still to come
/// ([`TimeRule::still_to_warn`]), from when it falls due, or from now when
/// that has passed. A warning that won't run (paused before it warned, or
/// past due with conditions `facts` don't meet) holds nothing, and the next
/// nearest is asked; without `facts`, one past due holds. Only the rule's
/// first fire per start is held; a repeat follows its last fire.
fn due(
    rule: &TimeRule,
    rules: &[TimeRule],
    ticket: &ClockInputs,
    fires: &[LastFire],
    facts: Option<&TicketFacts>,
    now: DateTime<Utc>,
) -> Option<Wait> {
    let mut wait = rule.wait(ticket, fires)?;
    if rule
        .last_fire(ticket.ticket_id, fires)
        .is_some_and(|fired| fired >= wait.started_at)
    {
        return Some(wait);
    }
    let mut warnings: Vec<&TimeRule> = rules.iter().filter(|w| w.warns_before(rule)).collect();
    warnings.sort_by_key(|w| std::cmp::Reverse(w.trigger.minutes));
    for warning in warnings {
        let warned = fires
            .iter()
            .find(|f| f.rule_id == warning.rule.id && f.ticket_id == ticket.ticket_id)
            .and_then(|f| f.for_state(rule.rule.state))
            .filter(|fired| *fired >= wait.started_at);
        let from = match warned {
            Some(warned) => warned,
            None if !warning.still_to_warn(rule) => continue,
            None => {
                let Some(pending) = warning.wait(ticket, fires) else {
                    continue;
                };
                if pending.due_at > now {
                    pending.due_at
                } else if facts.is_some_and(|f| {
                    !warning.would_run(f, clocks::minutes_since(pending.started_at, now))
                }) {
                    continue;
                } else {
                    now
                }
            }
        };
        let gap = chrono::Duration::minutes(rule.trigger.minutes - warning.trigger.minutes);
        wait.due_at = wait.due_at.max(from + gap);
        break;
    }
    Some(wait)
}

/// The pinned workspace's time-elapsed rules that aren't archived: those
/// that wait, and the paused and draft ones whose past runs still count.
fn time_rules(conn: &mut DbConnection) -> QueryResult<Vec<TimeRule>> {
    let mut rules: Vec<TimeRule> = repo::time_elapsed_rules(conn)?
        .into_iter()
        .filter_map(TimeRule::parse)
        .collect();
    if !rules.is_empty() {
        let categories = crate::repository::workflow_states::categories(conn)?;
        for rule in &mut rules {
            rule.set_closes(&categories);
        }
    }
    Ok(rules)
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

/// Bring the rows on `ticket_ids` of the waiting `rules`, or of the one
/// rule `only` among them, in line with the tickets as they are now, in the
/// pinned workspace. `rules` are every time rule there ([`time_rules`]),
/// the warnings a rule waits on among them.
fn sync_rows(
    conn: &mut DbConnection,
    workspace_id: i32,
    rules: &[TimeRule],
    only: Option<i32>,
    ticket_ids: &[i32],
) -> QueryResult<()> {
    let synced: Vec<&TimeRule> = rules
        .iter()
        .filter(|r| r.waits && only.is_none_or(|id| r.rule.id == id))
        .collect();
    if synced.is_empty() || ticket_ids.is_empty() {
        return Ok(());
    }
    let tickets = queue::clock_inputs(conn, ticket_ids)?;
    let rule_ids: Vec<i32> = rules.iter().map(|r| r.rule.id).collect();
    let fires = queue::last_fires(conn, &rule_ids, ticket_ids)?;
    let now = Utc::now();
    // What a rule past due reads: its own conditions, or a warning's that
    // holds a later rule.
    let overdue: Vec<i32> = tickets
        .iter()
        .filter(|t| {
            rules
                .iter()
                .filter(|r| r.waits)
                .any(|r| r.wait(t, &fires).is_some_and(|w| w.due_at <= now))
        })
        .map(|t| t.ticket_id)
        .collect();
    let facts = if overdue.is_empty() {
        Vec::new()
    } else {
        repo::load_ticket_facts(conn, &overdue)?
    };
    for ticket in &tickets {
        let facts = facts.iter().find(|f| f.id == ticket.ticket_id);
        for rule in &synced {
            // A wait already past due is kept only while the conditions are
            // met.
            let wait = due(rule, rules, ticket, &fires, facts, now).filter(|w| {
                w.due_at > now
                    || facts.is_some_and(|f| {
                        rule.would_run(f, clocks::minutes_since(w.started_at, now))
                    })
            });
            queue::set_wait(
                conn,
                workspace_id,
                ticket.ticket_id,
                rule.rule.id,
                rule.clock_name(),
                wait,
            )?;
        }
    }
    clear_lapsed_warnings(conn, rules, only, &tickets, &fires)
}

/// Take a warning's tags off each open ticket whose clock has started again
/// (or stopped) since the warning last ran there: the requester answered,
/// so the ticket is no longer warned. A warning paused or put in dry run
/// since it warned counts too. Only the tags that run added after its email
/// to the requester, and that are still on from then, come off; a closed
/// ticket keeps them, and so does one whose clock the warning's own run
/// started again (a step that changed its status or assignee). Among
/// `rules`, the warnings, or the one rule `only`.
fn clear_lapsed_warnings(
    conn: &mut DbConnection,
    rules: &[TimeRule],
    only: Option<i32>,
    tickets: &[ClockInputs],
    fires: &[LastFire],
) -> QueryResult<()> {
    let warnings: Vec<&TimeRule> = rules
        .iter()
        .filter(|r| only.is_none_or(|id| r.rule.id == id) && r.is_warning(rules))
        .collect();
    let mut lapsed: Vec<(i32, i32, DateTime<Utc>, Option<DateTime<Utc>>)> = Vec::new();
    for ticket in tickets.iter().filter(|t| t.runs_clocks) {
        for rule in &warnings {
            let Some(warned) = fires
                .iter()
                .find(|f| f.rule_id == rule.rule.id && f.ticket_id == ticket.ticket_id)
                .and_then(|f| f.live_run)
            else {
                continue;
            };
            let started = rule.trigger.clock.started_at(ticket);
            if started.is_none_or(|started| started > warned) {
                lapsed.push((rule.rule.id, ticket.ticket_id, warned, started));
            }
        }
    }
    if lapsed.is_empty() {
        return Ok(());
    }
    let ticket_ids: Vec<i32> = lapsed.iter().map(|l| l.1).collect();
    let tagged = crate::repository::tags::tag_ids_for_tickets(conn, &ticket_ids)?;
    lapsed.retain(|l| tagged.get(&l.1).is_some_and(|tags| !tags.is_empty()));
    if lapsed.is_empty() {
        return Ok(());
    }
    let rule_ids: Vec<i32> = lapsed.iter().map(|l| l.0).collect();
    let ticket_ids: Vec<i32> = lapsed.iter().map(|l| l.1).collect();
    let runs = queue::last_runs(conn, &rule_ids, &ticket_ids)?;
    for (rule_id, ticket_id, warned, started) in lapsed {
        // The run judged lapsed, not one that committed since.
        let Some(run) = runs
            .iter()
            .find(|r| r.rule_id == rule_id && r.ticket_id == ticket_id && r.applied_at == warned)
        else {
            continue;
        };
        let on = &tagged[&ticket_id];
        let added: Vec<i32> = run
            .tags_added_after_warning()
            .into_iter()
            .filter(|t| on.contains(t))
            .collect();
        if added.is_empty() {
            continue;
        }
        // A clock the warning's own run started again: its changes are
        // recorded a moment after the run's start.
        if let (Some(started), Some(correlation_id)) = (started, run.correlation_id) {
            if queue::changed_by_run_until(conn, ticket_id, correlation_id)?
                .is_some_and(|until| started <= until)
            {
                continue;
            }
        }
        let removed = crate::repository::tags::remove_ticket_tags_added_by(
            conn,
            ticket_id,
            &added,
            run.applied_at,
        )?;
        if !removed.is_empty() {
            debug!(
                rule_id,
                ticket_id, "time-elapsed rules: a lapsed warning's tags came off"
            );
        }
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
    sync_rows(conn, workspace_id, &rules, None, &[ticket_id])
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
    let rules = time_rules(conn)?;
    sync_rows(conn, rule.rule.workspace_id, &rules, Some(rule_id), &ids)?;
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
                Ok(time_rules(c)?
                    .into_iter()
                    .filter(|r| r.waits)
                    .map(|r| r.rule.id)
                    .collect())
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
        /// Every waiting time rule in the workspace: the warnings.
        rules: Vec<TimeRule>,
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
        let Some(mut rule) = repo::find(c, row.rule_id)?.and_then(TimeRule::of) else {
            queue::clear_rule(c, row.rule_id)?;
            return Ok(Ready::NotDue);
        };
        let Some(ticket) = queue::clock_inputs(c, &[ticket_id])?.pop() else {
            // The ticket is gone, and its rows with it.
            return Ok(Ready::NotDue);
        };
        let rules = time_rules(c)?;
        rule.closes = rules.iter().any(|r| r.rule.id == rule.rule.id && r.closes);
        let rule_ids: Vec<i32> = rules.iter().map(|r| r.rule.id).collect();
        let fires = queue::last_fires(c, &rule_ids, &[ticket_id])?;
        let facts = repo::load_ticket_facts(c, &[ticket_id])?.pop();
        let now = Utc::now();
        let wait = due(&rule, &rules, &ticket, &fires, facts.as_ref(), now);
        let (Some(wait), Some(facts)) = (wait.filter(|w| w.due_at <= now), facts) else {
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
        Ok::<_, diesel::result::Error>(Ready::Due {
            rule,
            rules,
            wait,
            facts,
        })
    })?;
    let Ready::Due {
        rule,
        rules,
        wait,
        facts,
    } = ready
    else {
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
        let rule_ids: Vec<i32> = rules.iter().map(|r| r.rule.id).collect();
        let fires = queue::last_fires(c, &rule_ids, &[ticket_id])?;
        let Some(facts) = repo::load_ticket_facts(c, &[ticket_id])?.pop() else {
            return Ok(false);
        };
        let now = Utc::now();
        let Some(wait) =
            due(&rule, &rules, &ticket, &fires, Some(&facts), now).filter(|w| w.due_at <= now)
        else {
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
        warning: rule.is_warning(&rules),
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
                after_live_run(pool, search, &mut conn, workspace_id, ticket_id, &outcome);
            }
            // A warning's run moves the rules that wait on it.
            if let Err(e) = resync(&mut conn, &reader, workspace_id, &rule, ticket_id) {
                warn!(rule_id = rule.rule.id, ticket_id, error = %e, "time-elapsed rules: clocks not updated after a fire");
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

/// Recompute the rows on one ticket after `rule` fired or was turned away
/// there: every waiting rule's, since a warning's fire moves the rules that
/// wait on it.
fn resync(
    conn: &mut DbConnection,
    reader: &ActorContext,
    workspace_id: i32,
    rule: &TimeRule,
    ticket_id: i32,
) -> QueryResult<()> {
    with_actor_context(conn, reader, |c| {
        // Read again: the run may have left the rule paused or archived.
        if repo::find(c, rule.rule.id)?
            .and_then(TimeRule::of)
            .is_none()
        {
            queue::clear_rule(c, rule.rule.id)?;
        }
        maintain_ticket(c, workspace_id, ticket_id)
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
