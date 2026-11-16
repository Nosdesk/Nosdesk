//! The rules engine's event consumer: runs the rules with an event trigger
//! when a ticket is created, updated or replied to.
//!
//! `tr_sync_actions_rule_event_outbox` queues the `sync_actions` rows a rule
//! trigger reads, in a workspace with a live or dry-run event rule. This loop
//! claims them the way the notification outbox's dispatcher does (`FOR UPDATE
//! SKIP LOCKED`, a lease, backoff, a dead letter) and, for each event, runs the
//! rules of its trigger in priority order. Each rule runs in a transaction of
//! its own as the system actor `rules_engine:<rule_id>` (see
//! [`repository::rules::run_rule_on_ticket`]).
//!
//! An event maps to one trigger: `ticket.created` to `ticket_created`, a
//! ticket field write to `ticket_updated` (which runs only the rules whose
//! `trigger_config.changed_fields_include` names a changed field, or is
//! empty), and a public `comment.created` to `ticket_replied` (by
//! `trigger_config.reply_kind`: customer, agent or any). The message a
//! ticket was opened with is not a reply.
//!
//! A ticket held for approval or merged is turned away before any rule runs;
//! that leaves no application row, only a count on each rule it would have
//! run. So does a live rule whose conditions aren't met. A dry-run rule runs
//! in shadow: it records what it would do (a skip too) and changes nothing,
//! and its `stop_processing` stops nothing.
//!
//! A chain of events (rule A's write is rule B's event) keeps the root
//! event's id as its correlation id, and each rule runs at most once per
//! chain on a ticket (decision D4, `ticket_rule_runs`).
//!
//! Delivery is at least once. A transient database error (a lost connection,
//! a serialization failure, a deadlock, a lock or statement timeout, an
//! exhausted pool) stops the event where it is and retries it with backoff,
//! leaving no row; the rules that already ran hold their budget slots for the
//! event and are skipped on the retry. On the last attempt the error is
//! recorded as a `failed` row like any other and the event is dead-lettered.
//! A deterministic error (a refused step, an assignee who can't work tickets)
//! writes a `failed` row at once, and the next rule runs.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{
    Array, BigInt, Bool, Integer, Jsonb, Nullable, SmallInt, Text, Timestamptz,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_postgres::AsyncMessage;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::models::{NewRuleApplication, Rule, RuleApplicationStatus, RuleState, RuleTriggerKind};
use crate::repository::rules::{
    self as repo, ApplyContext, ApplyError, ApplySource, OriginatingEvent, RunBudget, SkipCount,
};
use crate::services::rules::conditions::{self, EventFacts, TicketFacts};
use crate::services::search::SearchService;
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_context;

const POLL_SECS: u64 = 2;
const BATCH: i64 = 100;
pub const MAX_ATTEMPTS: i16 = 8;
const STALE_CLAIM_SECS: i64 = 300;
/// An event recorded this long before it was queued was backdated (seed-demo
/// rewrites `occurred_at`, and moving a row across a month re-fires the
/// trigger), so it is acknowledged without running. A rerun is exempt.
const BACKDATED_SECS: i64 = 600;
/// An event queued longer ago than this is acknowledged without running:
/// rules act on the ticket as it is now, and a day-late run surprises more
/// than it helps.
const MAX_AGE_SECS: i64 = 24 * 60 * 60;
/// How long the budget remembers a rule's run in a chain of events: past an
/// event's whole retry span and [`MAX_AGE_SECS`], so a retried or late event
/// still finds what its chain already ran.
pub const BUDGET_RETENTION_HOURS: i64 = 48;
/// How long a rule run waits for the ticket's lock before giving up with a
/// transient error.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

/// Backoff after the `attempts`-th failure.
fn backoff_secs(attempts: i16) -> i64 {
    match attempts {
        ..=1 => 1,
        2 => 10,
        3 => 60,
        4 => 600,
        _ => 3600,
    }
}

#[derive(QueryableByName, Debug)]
pub struct Claimed {
    #[diesel(sql_type = BigInt)]
    pub sync_id: i64,
    #[diesel(sql_type = SmallInt)]
    pub attempts: i16,
    #[diesel(sql_type = Bool)]
    pub rerun: bool,
    #[diesel(sql_type = Timestamptz)]
    pub enqueued_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Integer>)]
    pub workspace_id: Option<i32>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    pub event_uuid: Option<Uuid>,
    #[diesel(sql_type = Nullable<Text>)]
    pub event_type: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    pub aggregate_id: Option<String>,
    #[diesel(sql_type = Nullable<Jsonb>)]
    pub data: Option<Value>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    pub correlation_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub occurred_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Array<Nullable<Text>>>)]
    pub groups: Option<Vec<Option<String>>>,
    #[diesel(sql_type = Nullable<BigInt>)]
    pub xid8: Option<i64>,
}

/// Claim due rows and load their source events in one short transaction. A
/// row whose `sync_actions` source is gone comes back with null columns and
/// is acknowledged.
pub fn claim_batch(conn: &mut DbConnection) -> QueryResult<Vec<Claimed>> {
    diesel::sql_query(
        r#"
        WITH due AS (
            SELECT sync_id FROM rule_event_outbox
            WHERE dead_at IS NULL
              AND next_attempt_at <= now()
              AND (claimed_at IS NULL OR claimed_at < now() - make_interval(secs => $1))
            ORDER BY sync_id
            LIMIT $2
            FOR UPDATE SKIP LOCKED
        ),
        claimed AS (
            UPDATE rule_event_outbox o
               SET claimed_at = now(), attempts = o.attempts + 1
              FROM due
             WHERE o.sync_id = due.sync_id
         RETURNING o.sync_id, o.attempts, o.rerun, o.enqueued_at, o.workspace_id
        )
        SELECT c.sync_id, c.attempts, c.rerun, c.enqueued_at,
               s.workspace_id, s.event_uuid, s.event_type, s.aggregate_id, s.data,
               s.correlation_id, s.occurred_at, s.groups, s.xid8
          FROM claimed c
          LEFT JOIN sync_actions s
                 ON s.sync_id = c.sync_id AND s.workspace_id = c.workspace_id
         ORDER BY c.sync_id
        "#,
    )
    .bind::<diesel::sql_types::Double, _>(STALE_CLAIM_SECS as f64)
    .bind::<BigInt, _>(BATCH)
    .load(conn)
}

/// Delete handled rows.
pub fn ack(conn: &mut DbConnection, sync_ids: &[i64]) -> QueryResult<usize> {
    use crate::schema::rule_event_outbox::dsl::*;
    if sync_ids.is_empty() {
        return Ok(0);
    }
    diesel::delete(rule_event_outbox.filter(sync_id.eq_any(sync_ids))).execute(conn)
}

/// Reschedule a row after its `attempt`-th failure, or dead-letter it.
pub fn fail(
    conn: &mut DbConnection,
    id: i64,
    attempt: i16,
    error_kind: &str,
) -> QueryResult<usize> {
    use crate::schema::rule_event_outbox::dsl::*;
    let dead = attempt >= MAX_ATTEMPTS;
    diesel::update(rule_event_outbox.filter(sync_id.eq(id)))
        .set((
            claimed_at.eq(None::<DateTime<Utc>>),
            next_attempt_at.eq(Utc::now() + chrono::Duration::seconds(backoff_secs(attempt))),
            last_error.eq(Some(error_kind.to_string())),
            dead_at.eq(if dead { Some(Utc::now()) } else { None }),
        ))
        .execute(conn)
}

/// Run `kind`'s rules on a ticket as if its event had just happened: a
/// request held for approval runs its `ticket_created` rules once it is
/// approved, since the approval records no event a trigger reads. Queues the
/// ticket's `ticket.created` for the engine again, in the caller's
/// transaction, so it runs once that commits. Only `ticket_created` has an
/// event to run again; other kinds queue nothing. Returns whether a run was
/// queued.
pub fn run_trigger_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    kind: RuleTriggerKind,
) -> QueryResult<bool> {
    if kind != RuleTriggerKind::TicketCreated {
        return Ok(false);
    }
    let Some(sync_id) = repo::created_event(conn, ticket_id)? else {
        return Ok(false);
    };
    let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
    Ok(repo::queue_rerun(conn, sync_id, ticket.workspace_id)? > 0)
}

/// Relay a rule's public reply to the requester's channel once the run has
/// committed: the enqueue a reply posted from the ticket gets. Loads the
/// ticket and comment fresh in the ticket's workspace. Internal notes and
/// replies the rule didn't mark public don't relay.
pub fn relay_reply(
    pool: &Pool,
    workspace_id: i32,
    ticket_id: i32,
    comment_id: i32,
) -> Result<(), String> {
    let (comment, ticket) =
        crate::sync::session::run_in_workspace(pool, "rules:reply_relay", workspace_id, |conn| {
            Ok((
                crate::repository::comments::get_comment_by_id(conn, comment_id)?,
                crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let public = comment
        .channel_metadata
        .as_ref()
        .and_then(|m| m.get("visibility"))
        .and_then(Value::as_str)
        == Some("public");
    if comment.is_internal || !public {
        return Ok(());
    }
    crate::services::channels::outbound::enqueue_for_comment(ticket, comment, pool.clone());
    debug!(comment_id, "rule reply enqueued for channel relay");
    Ok(())
}

/// What a claimed event came to.
enum Handled {
    /// Every rule it runs had its turn; acknowledge it.
    Done,
    /// Try it again later, or dead-letter it on its last attempt.
    Again(&'static str),
}

/// The trigger an event serves and what its rules read from it.
struct Event {
    kind: RuleTriggerKind,
    ticket_id: i32,
    /// For a reply, the comment and its author.
    comment: Option<(i32, Uuid)>,
    changed_fields: Vec<String>,
}

impl Event {
    /// `None` for an event no trigger runs on: an internal note, a ticket
    /// update that changed nothing, an event the outbox doesn't queue.
    fn read(event_type: &str, aggregate_id: Option<&str>, data: &Value) -> Option<Self> {
        let int = |v: Option<&Value>| {
            v.and_then(Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
        };
        let ticket_id = || aggregate_id.and_then(|id| id.parse::<i32>().ok());
        match event_type {
            "ticket.created" => Some(Self {
                kind: RuleTriggerKind::TicketCreated,
                ticket_id: ticket_id()?,
                comment: None,
                changed_fields: Vec::new(),
            }),
            "comment.created" => {
                if data.get("is_internal").and_then(Value::as_bool) != Some(false) {
                    return None;
                }
                let author = data
                    .get("user_uuid")
                    .and_then(Value::as_str)
                    .and_then(|u| Uuid::parse_str(u).ok())?;
                Some(Self {
                    kind: RuleTriggerKind::TicketReplied,
                    ticket_id: int(data.get("ticket_id"))?,
                    comment: Some((int(data.get("id"))?, author)),
                    changed_fields: Vec::new(),
                })
            }
            t if t.starts_with("ticket.") => {
                let changed_fields: Vec<String> = data
                    .get("changed_fields")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect();
                // An update that set fields to the values they had changed
                // nothing a rule could react to.
                if changed_fields.is_empty() {
                    return None;
                }
                Some(Self {
                    kind: RuleTriggerKind::TicketUpdated,
                    ticket_id: ticket_id()?,
                    comment: None,
                    changed_fields,
                })
            }
            _ => None,
        }
    }
}

/// Whether `rule`'s trigger settings take an event with these changed
/// fields and reply kind. An update rule runs when the update changed a
/// field its trigger names. With none named it runs on any change to a
/// field its conditions read, since only such a change can bring a ticket
/// into its conditions (plan, "Lifecycle: event-triggered fire"); a rule
/// whose conditions read no ticket field runs on any change. A reply rule
/// with no reply kind takes the editor's default, a requester's reply.
fn trigger_takes(rule: &Rule, changed_fields: &[String], reply_kind: Option<&str>) -> bool {
    let changed = |field: &str| changed_fields.iter().any(|c| c == field);
    match rule.trigger_kind {
        RuleTriggerKind::TicketUpdated => {
            let named: Vec<&str> = rule
                .trigger_config
                .get("changed_fields_include")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if !named.is_empty() {
                return named.into_iter().any(changed);
            }
            let reads: Vec<&str> = rule
                .reads_set
                .iter()
                .flatten()
                .map(String::as_str)
                .collect();
            reads.is_empty() || reads.into_iter().any(changed)
        }
        RuleTriggerKind::TicketReplied => {
            match rule
                .trigger_config
                .get("reply_kind")
                .and_then(Value::as_str)
                .unwrap_or("customer")
            {
                "any" => true,
                wanted => Some(wanted) == reply_kind,
            }
        }
        _ => true,
    }
}

/// An event's ticket, ready for its rules.
struct Prepared {
    rules: Vec<Rule>,
    facts: TicketFacts,
    event: EventFacts,
    /// The event came from an automated email.
    automated_source: bool,
}

pub struct Dispatcher {
    pool: Pool,
    search: Option<Arc<SearchService>>,
    lock_timeout: Duration,
}

impl Dispatcher {
    pub fn new(pool: Pool, search: Option<Arc<SearchService>>) -> Self {
        Self {
            pool,
            search,
            lock_timeout: LOCK_TIMEOUT,
        }
    }

    /// Wait at most `timeout` for a ticket's lock before a rule run gives up
    /// with a transient error.
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    pub fn spawn(
        self,
        database_url: Option<String>,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("rules engine dispatcher started");
            tokio::select! {
                _ = shutdown.cancelled() => info!("rules engine dispatcher: shutting down"),
                _ = self.run(database_url) => {}
            }
        })
    }

    async fn run(&self, database_url: Option<String>) {
        let (wake_tx, mut wake_rx) = mpsc::channel::<()>(8);
        if let Some(url) = database_url {
            tokio::spawn(listen(url, wake_tx));
        }
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_SECS));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                w = wake_rx.recv() => { if w.is_none() { interval.tick().await; } }
            }
            // Drain until a batch comes back short.
            loop {
                match self.dispatch_batch().await {
                    Ok(n) if n as i64 >= BATCH => continue,
                    Ok(_) => break,
                    Err(e) => {
                        warn!(error = %e, "rules engine: batch failed");
                        break;
                    }
                }
            }
        }
    }

    /// Claim due events, run their rules, acknowledge or reschedule them.
    /// Returns how many were claimed, so the caller knows whether to go
    /// again.
    pub async fn dispatch_batch(&self) -> Result<usize, String> {
        // cross-tenant: the outbox is a queue across every workspace; each
        // event runs under its own workspace pin below.
        let claimed = crate::sync::session::background_run(
            &self.pool,
            "background:rule_event_outbox_claim",
            claim_batch,
        )
        .map_err(|e| e.to_string())?;
        if claimed.is_empty() {
            return Ok(0);
        }
        let count = claimed.len();
        let mut done: Vec<i64> = Vec::new();
        let mut again: Vec<(i64, i16, &'static str)> = Vec::new();
        for row in &claimed {
            match self.handle(row) {
                Handled::Done => done.push(row.sync_id),
                Handled::Again(kind) => again.push((row.sync_id, row.attempts, kind)),
            }
        }
        // cross-tenant: acknowledgements and reschedules are queue rows in
        // every workspace.
        crate::sync::session::background_run(
            &self.pool,
            "background:rule_event_outbox_ack",
            move |conn| {
                ack(conn, &done)?;
                for (sync_id, attempts, kind) in &again {
                    fail(conn, *sync_id, *attempts, kind)?;
                }
                Ok(())
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(count)
    }

    /// One claimed event, start to end.
    fn handle(&self, row: &Claimed) -> Handled {
        let (Some(workspace_id), Some(event_uuid), Some(event_type), Some(data), Some(occurred_at)) = (
            row.workspace_id,
            row.event_uuid,
            row.event_type.as_deref(),
            row.data.as_ref(),
            row.occurred_at,
        ) else {
            // The source row is gone; there is nothing to run on.
            return Handled::Done;
        };
        // Not a workspace event yet (a guest ticket waiting for its submitter
        // to confirm): the workspace sees its events once it's confirmed.
        if !crate::sync::groups::has_workspace_audience(row.groups.as_deref().unwrap_or(&[])) {
            return Handled::Done;
        }
        let now = Utc::now();
        if !row.rerun && (row.enqueued_at - occurred_at).num_seconds() > BACKDATED_SECS {
            debug!(
                sync_id = row.sync_id,
                "rules engine: backdated event; acknowledged"
            );
            return Handled::Done;
        }
        if (now - row.enqueued_at).num_seconds() > MAX_AGE_SECS {
            debug!(
                sync_id = row.sync_id,
                "rules engine: event too old; acknowledged"
            );
            return Handled::Done;
        }
        let Some(event) = Event::read(event_type, row.aggregate_id.as_deref(), data) else {
            return Handled::Done;
        };
        let mut conn = match self.pool.get() {
            Ok(conn) => conn,
            Err(e) => {
                warn!(error = %e, "rules engine: no connection");
                return Handled::Again("pool");
            }
        };
        let reader = ActorContext::system("rules_engine").with_workspace(workspace_id);
        let prepared =
            match with_actor_context(&mut conn, &reader, |c| prepare(c, &event, row.xid8)) {
                Ok(Some(prepared)) => prepared,
                Ok(None) => return Handled::Done,
                Err(e) => {
                    warn!(sync_id = row.sync_id, error = %e, "rules engine: event not read");
                    return Handled::Again("prepare");
                }
            };
        let run = Run {
            workspace_id,
            sync_id: row.sync_id,
            event_uuid,
            event_type,
            // The chain's root: the event a cascade started from.
            root_id: row.correlation_id.unwrap_or(event_uuid),
            last_attempt: row.attempts >= MAX_ATTEMPTS,
        };
        self.run_rules(&mut conn, &run, &event, prepared)
    }

    /// Run the event's rules in order.
    fn run_rules(
        &self,
        conn: &mut DbConnection,
        run: &Run,
        event: &Event,
        prepared: Prepared,
    ) -> Handled {
        let Prepared {
            rules,
            mut facts,
            event: event_facts,
            automated_source,
        } = prepared;
        let reader = ActorContext::system("rules_engine").with_workspace(run.workspace_id);
        let observer = self
            .search
            .as_ref()
            .map(|s| s as &dyn crate::repository::tickets::TicketUpdatedObserver);
        let mut gave_up = false;
        for rule in &rules {
            let shadow = rule.state == RuleState::DryRun;
            let actor = run.actor(rule.id);
            let predicate = match conditions::compile(&rule.conditions, rule.trigger_kind) {
                Ok(p) => p,
                Err(e) => {
                    // Checked at save, so only a rule saved before the check.
                    let reason = format!("The conditions can't be evaluated: {e}");
                    if let Err(e) = record(
                        conn,
                        &actor,
                        run,
                        rule,
                        event.ticket_id,
                        RuleApplicationStatus::Failed,
                        None,
                        Some(reason),
                    ) {
                        return again(run, "record", &e);
                    }
                    continue;
                }
            };
            let matched = predicate.matches(&facts, Some(&event_facts));
            let evaluation = json!({
                "matched": matched,
                "trigger": rule.trigger_kind.as_str(),
                "event_type": run.event_type,
                "changed_fields": event_facts.changed_fields,
                "reply_kind": event_facts.reply_kind,
            });
            if !matched {
                let recorded = if shadow {
                    record(
                        conn,
                        &actor,
                        run,
                        rule,
                        event.ticket_id,
                        RuleApplicationStatus::SkippedConditionUnmet,
                        Some(evaluation),
                        None,
                    )
                } else {
                    with_actor_context(conn, &reader, |c| {
                        repo::count_skips(c, &[rule.id], SkipCount::ConditionUnmet).map(|_| ())
                    })
                };
                if let Err(e) = recorded {
                    return again(run, "record", &e);
                }
                continue;
            }
            let ctx = ApplyContext {
                shadow,
                originating_event: Some(OriginatingEvent {
                    id: run.event_uuid,
                    kind: run.event_type.to_string(),
                }),
                condition_evaluation: Some(evaluation.clone()),
                causation_id: Some(run.event_uuid),
                observer,
                budget: Some(RunBudget {
                    root_id: run.root_id,
                    sync_id: run.sync_id,
                }),
                lock_timeout: Some(self.lock_timeout),
                retries: !run.last_attempt,
                automated_source,
                ..ApplyContext::new(&actor, rule.id, event.ticket_id, ApplySource::Event)
            };
            match repo::run_rule_on_ticket(conn, ctx) {
                Ok(outcome) => {
                    if shadow {
                        continue;
                    }
                    self.after_run(
                        conn,
                        run,
                        event.ticket_id,
                        outcome.comment_id,
                        outcome.actions_executed,
                    );
                    if outcome.stop_processing {
                        break;
                    }
                    if outcome.actions_executed > 0 {
                        // The next rule reads the ticket as this one left it.
                        match with_actor_context(conn, &reader, |c| {
                            repo::load_ticket_facts(c, &[event.ticket_id])
                        }) {
                            Ok(mut reloaded) if !reloaded.is_empty() => facts = reloaded.remove(0),
                            Ok(_) => break,
                            Err(e) => return again(run, "prepare", &e),
                        }
                    }
                }
                Err(failed) => match failed.error {
                    // Ran on an earlier attempt at this event and stopped the
                    // rules after it then; otherwise the next rule runs.
                    ApplyError::AlreadyRan(_) if !shadow && stops_processing(rule) => break,
                    ApplyError::TicketNotFound(_) | ApplyError::TicketMerged(_) => break,
                    e if e.is_transient() && !run.last_attempt => {
                        warn!(sync_id = run.sync_id, rule_id = rule.id, error = %e, "rules engine: rule run failed; retrying the event");
                        return Handled::Again("transient");
                    }
                    e if e.is_transient() => {
                        warn!(sync_id = run.sync_id, rule_id = rule.id, error = %e, "rules engine: rule run failed on the last attempt");
                        gave_up = true;
                        // Failed before its preflight (waiting for the
                        // ticket's lock), so the run left no row.
                        if failed.recorded.is_none() {
                            if let Err(e) = record(
                                conn,
                                &actor,
                                run,
                                rule,
                                event.ticket_id,
                                RuleApplicationStatus::Failed,
                                Some(evaluation),
                                Some(e.to_string()),
                            ) {
                                warn!(rule_id = rule.id, error = %e, "rules engine: failed row not written");
                            }
                        }
                    }
                    // Recorded (failed, suppressed) or turned away at the
                    // rule's preflight (no longer live): the next rule runs.
                    _ => {}
                },
            }
        }
        if gave_up {
            Handled::Again("transient")
        } else {
            Handled::Done
        }
    }

    /// After a live run commits: relay its public reply and reindex the
    /// ticket. Failures are logged; the run stands.
    fn after_run(
        &self,
        conn: &mut DbConnection,
        run: &Run,
        ticket_id: i32,
        comment_id: Option<i32>,
        actions_executed: usize,
    ) {
        if let Some(comment_id) = comment_id {
            if let Err(e) = relay_reply(&self.pool, run.workspace_id, ticket_id, comment_id) {
                warn!(error = %e, comment_id, "rules engine: reply relay enqueue failed");
            }
        }
        let Some(search) = &self.search else {
            return;
        };
        if actions_executed == 0 {
            return;
        }
        let reader = ActorContext::system("rules_engine").with_workspace(run.workspace_id);
        match with_actor_context(conn, &reader, |c| {
            let ticket = crate::repository::tickets::get_ticket_by_id(c, ticket_id)?;
            let article =
                crate::repository::article_content::get_article_content_by_ticket_id(c, ticket_id)
                    .optional()?;
            Ok::<_, diesel::result::Error>((ticket, article))
        }) {
            Ok((ticket, article)) => {
                use crate::repository::tickets::TicketUpdatedObserver;
                search.ticket_updated(&ticket, article.as_ref())
            }
            Err(e) => warn!(error = %e, ticket_id, "rules engine: post-run reload failed"),
        }
    }
}

/// One event's run through its rules.
struct Run<'a> {
    workspace_id: i32,
    sync_id: i64,
    event_uuid: Uuid,
    event_type: &'a str,
    root_id: Uuid,
    last_attempt: bool,
}

impl Run<'_> {
    /// The system actor a rule's writes are credited to, carrying the
    /// chain's root as its correlation id.
    fn actor(&self, rule_id: i32) -> ActorContext {
        let mut actor = ActorContext::system(format!("rules_engine:{rule_id}"))
            .with_workspace(self.workspace_id);
        actor.correlation_id = Some(self.root_id);
        actor
    }
}

/// Retry the event after a database error outside a rule's run.
fn again(run: &Run, kind: &'static str, error: &diesel::result::Error) -> Handled {
    warn!(sync_id = run.sync_id, error = %error, "rules engine: {kind} failed; retrying the event");
    Handled::Again(kind)
}

/// Whether a rule ends with `stop_processing`, so no rule after it runs.
fn stops_processing(rule: &Rule) -> bool {
    rule.actions
        .as_array()
        .into_iter()
        .flatten()
        .any(|step| step.get("kind").and_then(Value::as_str) == Some("stop_processing"))
}

/// The event's ticket and the rules it runs, or `None` when none run: no
/// rule takes the event, the ticket is gone, the comment opened the ticket,
/// or the ticket is held for approval or merged (counted on each rule).
fn prepare(
    conn: &mut DbConnection,
    event: &Event,
    xid8: Option<i64>,
) -> QueryResult<Option<Prepared>> {
    let Some(ticket) =
        crate::repository::tickets::get_ticket_by_id(conn, event.ticket_id).optional()?
    else {
        return Ok(None);
    };
    let mut reply_kind = None;
    let mut reply_author_role = None;
    if let Some((_, author)) = event.comment {
        if let Some(xid8) = xid8 {
            if repo::written_with_ticket(conn, ticket.id, xid8)? {
                return Ok(None);
            }
        }
        let staff =
            crate::repository::comments::is_workspace_staff(conn, author, ticket.workspace_id);
        reply_kind = Some(if staff { "agent" } else { "customer" }.to_string());
        reply_author_role =
            crate::repository::workspaces::get_membership_role(conn, ticket.workspace_id, author)?;
    }
    let rules: Vec<Rule> = repo::event_candidates(conn, event.kind)?
        .into_iter()
        .filter(|rule| trigger_takes(rule, &event.changed_fields, reply_kind.as_deref()))
        .collect();
    if rules.is_empty() {
        return Ok(None);
    }
    if crate::repository::ticket_approvals::held_for_approval(conn, &ticket)?
        || crate::repository::ticket_merge::is_merge_source(conn, ticket.id)?
    {
        let ids: Vec<i32> = rules.iter().map(|r| r.id).collect();
        repo::count_skips(conn, &ids, SkipCount::Preflight)?;
        return Ok(None);
    }
    let Some(facts) = repo::load_ticket_facts(conn, &[ticket.id])?.pop() else {
        return Ok(None);
    };
    let automated_source = match event.kind {
        RuleTriggerKind::TicketCreated | RuleTriggerKind::TicketReplied => {
            crate::repository::channels::automated_inbound(
                conn,
                ticket.id,
                event.comment.map(|(id, _)| id),
            )?
        }
        _ => false,
    };
    Ok(Some(Prepared {
        rules,
        facts,
        event: EventFacts {
            changed_fields: event.changed_fields.clone(),
            reply_kind,
            reply_author_role,
            minutes_since: None,
        },
        automated_source,
    }))
}

/// An application row the engine writes itself, outside a rule's run: a dry
/// run's condition skip, a rule whose conditions can't be evaluated.
#[allow(clippy::too_many_arguments)]
fn record(
    conn: &mut DbConnection,
    actor: &ActorContext,
    run: &Run,
    rule: &Rule,
    ticket_id: i32,
    status: RuleApplicationStatus,
    condition_evaluation: Option<Value>,
    failure_reason: Option<String>,
) -> QueryResult<()> {
    with_actor_context(conn, actor, |c| {
        let rule_version = repo::current_version(c, rule.id)?;
        repo::record_application(
            c,
            NewRuleApplication {
                workspace_id: run.workspace_id,
                rule_id: rule.id,
                rule_version,
                ticket_id,
                status,
                correlation_id: actor.correlation_id,
                actor_uuid: None,
                actor_kind: "system".to_string(),
                originating_event_id: Some(run.event_uuid),
                originating_event_kind: Some(run.event_type.to_string()),
                condition_evaluation,
                actions_taken: None,
                actions_skipped: None,
                failure_reason,
            },
        )
        .map(|_| ())
    })
}

/// Hold a LISTEN connection on `rule_event_outbox_new` and nudge the
/// dispatcher on every notification. Reconnects with backoff; the poll
/// covers the gaps.
async fn listen(database_url: String, wake: mpsc::Sender<()>) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match listen_once(&database_url, &wake).await {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(e) => {
                warn!(error = %e, "rules engine listener disconnected; reconnecting");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn listen_once(database_url: &str, wake: &mpsc::Sender<()>) -> Result<(), anyhow::Error> {
    let (client, mut messages) = crate::db::listen::connect(database_url).await?;
    let wake = wake.clone();
    let driver = tokio::spawn(async move {
        use futures::StreamExt;
        while let Some(msg) = messages.next().await {
            match msg {
                Ok(AsyncMessage::Notification(_)) => {
                    // A full channel means a wake is already pending.
                    let _ = wake.try_send(());
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    client.batch_execute("LISTEN rule_event_outbox_new").await?;
    debug!("rules engine listening on `rule_event_outbox_new`");
    let _ = driver.await;
    Err(anyhow::anyhow!("notification stream closed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: RuleTriggerKind, trigger_config: Value) -> Rule {
        Rule {
            id: 1,
            workspace_id: 1,
            name: "r".into(),
            description: None,
            trigger_kind: kind,
            trigger_config,
            conditions: json!([]),
            actions: json!([{ "kind": "stop_processing" }]),
            reads_set: Vec::new(),
            writes_set: Vec::new(),
            state: RuleState::Live,
            priority: 100,
            last_fired_at: None,
            fire_count: 0,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
            skipped_preflight_count: 0,
            skipped_condition_unmet_count: 0,
        }
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 10);
        assert_eq!(backoff_secs(5), 3600);
        assert_eq!(backoff_secs(50), 3600);
    }

    #[test]
    fn events_map_to_the_trigger_they_serve() {
        let created = Event::read("ticket.created", Some("7"), &json!({ "id": 7 })).unwrap();
        assert_eq!(
            (created.kind, created.ticket_id),
            (RuleTriggerKind::TicketCreated, 7)
        );

        let author = Uuid::now_v7();
        let reply = Event::read(
            "comment.created",
            Some("31"),
            &json!({ "id": 31, "ticket_id": 7, "user_uuid": author, "is_internal": false }),
        )
        .unwrap();
        assert_eq!(reply.kind, RuleTriggerKind::TicketReplied);
        assert_eq!((reply.ticket_id, reply.comment), (7, Some((31, author))));

        let note = json!({ "id": 32, "ticket_id": 7, "user_uuid": author, "is_internal": true });
        assert!(Event::read("comment.created", Some("32"), &note).is_none());

        let update = Event::read(
            "ticket.priority_changed",
            Some("7"),
            &json!({ "changed_fields": ["ticket.priority"] }),
        )
        .unwrap();
        assert_eq!(update.kind, RuleTriggerKind::TicketUpdated);
        assert_eq!(update.changed_fields, vec!["ticket.priority".to_string()]);

        let unchanged = json!({ "changed_fields": [] });
        assert!(Event::read("ticket.updated", Some("7"), &unchanged).is_none());
    }

    #[test]
    fn an_update_rule_runs_on_the_fields_it_names_or_on_any_change() {
        let changed = vec!["ticket.priority".to_string()];
        let any = rule(RuleTriggerKind::TicketUpdated, json!({}));
        let empty = rule(
            RuleTriggerKind::TicketUpdated,
            json!({ "changed_fields_include": [] }),
        );
        let priority = rule(
            RuleTriggerKind::TicketUpdated,
            json!({ "changed_fields_include": ["ticket.title", "ticket.priority"] }),
        );
        let title = rule(
            RuleTriggerKind::TicketUpdated,
            json!({ "changed_fields_include": ["ticket.title"] }),
        );
        assert!(trigger_takes(&any, &changed, None));
        assert!(trigger_takes(&empty, &changed, None));
        assert!(trigger_takes(&priority, &changed, None));
        assert!(!trigger_takes(&title, &changed, None));
    }

    #[test]
    fn with_no_fields_named_an_update_rule_runs_on_a_change_its_conditions_read() {
        let reads = |fields: &[&str]| {
            let mut r = rule(RuleTriggerKind::TicketUpdated, json!({}));
            r.reads_set = fields.iter().map(|f| Some(f.to_string())).collect();
            r
        };
        let changed = vec!["ticket.assignee_uuid".to_string()];
        assert!(!trigger_takes(&reads(&["ticket.priority"]), &changed, None));
        assert!(trigger_takes(
            &reads(&["ticket.assignee_uuid"]),
            &changed,
            None
        ));
        // A field the trigger names runs it whatever the conditions read.
        let mut named = reads(&["ticket.priority"]);
        named.trigger_config = json!({ "changed_fields_include": ["ticket.assignee_uuid"] });
        assert!(trigger_takes(&named, &changed, None));
    }

    #[test]
    fn a_reply_rule_runs_on_the_reply_kind_it_names() {
        let kinds = |config: Value| {
            let r = rule(RuleTriggerKind::TicketReplied, config);
            (
                trigger_takes(&r, &[], Some("customer")),
                trigger_takes(&r, &[], Some("agent")),
            )
        };
        assert_eq!(kinds(json!({ "reply_kind": "customer" })), (true, false));
        assert_eq!(kinds(json!({ "reply_kind": "agent" })), (false, true));
        assert_eq!(kinds(json!({ "reply_kind": "any" })), (true, true));
        assert_eq!(kinds(json!({})), (true, false), "the editor's default");
    }
}
