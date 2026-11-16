//! CRUD for the unified rules engine. Phase 1 surface only: list,
//! find, create, update, archive, hard-delete, plus the version /
//! application read paths and the application insert path. The
//! atomic apply lifecycle lives in `repository::rules::apply` (Wave
//! 5); this module is the data access layer the lifecycle and the
//! HTTP handlers call into.
//!
//! Workspace isolation is enforced by Postgres RLS (the
//! `rules_workspace_isolation` and sibling policies in the
//! migration), so the queries below don't add explicit
//! `workspace_id = ?` filters — the session's `app.workspace_id`
//! GUC scopes every read and the `WITH CHECK` clause rejects any
//! write to another workspace. Callers in handler code already
//! resolve the GUC via the request actor context; the helpers here
//! just need the workspace_id to populate `NewRule.workspace_id`
//! on insert.
//!
//! The `reads_set` / `writes_set` derivation tables in plan §11.2
//! live in this module so the self-referential save linter (Wave
//! 4) reads them off a single source of truth.

use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel::QueryResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{ConditionField, SyncAggregate, SyncOp};
use crate::models::{
    NewComment, NewRule, NewRuleApplication, Rule, RuleApplication, RuleApplicationStatus,
    RuleState, RuleTriggerKind, RuleUpdate, RuleVersion, TicketPriority, TicketUpdate,
};
use crate::repository::comments;
use crate::services::rules::conditions::{ConditionRefs, Predicate, TicketFacts};
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::sync::session::with_actor_context;

// =====================================================================
// reads_set / writes_set derivation (plan §11.2). Pure helpers; the
// repository calls them on every save so the columns stay in sync
// with the conditions and actions trees.
// =====================================================================

/// Walk `conditions` and return the sorted, deduplicated list of
/// `ConditionField` strings the rule reads. Manual rules pass `[]`
/// and get an empty `reads_set` back. Nested-field expansion
/// (`ticket.requester.email` adding both itself and
/// `ticket.requester_uuid`) is applied per the plan's derivation
/// table.
///
/// The return type is `Vec<Option<String>>` because Postgres TEXT[]
/// columns reach Rust through Diesel with nullable elements. The
/// derivation only ever produces real strings; the inner Option is
/// the schema boundary, not a semantic null.
pub fn derive_reads_set(conditions: &Value) -> Vec<Option<String>> {
    let mut out = std::collections::BTreeSet::new();
    collect_condition_fields(conditions, &mut out);
    out.into_iter().map(Some).collect()
}

fn collect_condition_fields(node: &Value, out: &mut std::collections::BTreeSet<String>) {
    // Manual rules: conditions is `[]`. Nothing to walk.
    if node.is_array() && node.as_array().is_some_and(|a| a.is_empty()) {
        return;
    }
    let Some(obj) = node.as_object() else { return };
    let kind = obj.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    match kind {
        "leaf" => {
            if let Some(field) = obj.get("field").and_then(|f| f.as_str()) {
                for expanded in expand_condition_field(field) {
                    out.insert(expanded);
                }
            }
        }
        "and" | "or" => {
            if let Some(children) = obj.get("children").and_then(|c| c.as_array()) {
                for child in children {
                    collect_condition_fields(child, out);
                }
            }
        }
        "not" => {
            if let Some(child) = obj.get("child") {
                collect_condition_fields(child, out);
            }
        }
        _ => {}
    }
}

/// Plan §11.2 reads_set derivation table, from
/// [`ConditionField::reads`](crate::models::ConditionField::reads). A
/// field name the model doesn't know maps to itself.
fn expand_condition_field(field: &str) -> Vec<String> {
    match ConditionField::parse(field) {
        Some(known) => known.reads().iter().map(|r| r.to_string()).collect(),
        None => vec![field.to_string()],
    }
}

/// Walk `actions` and return the sorted, deduplicated list of
/// ticket-state fields the rule writes. Plan §11.2 derivation
/// table; the synthetic `ticket.comments` key flags reply-style
/// actions so Phase 2's `ticket_replied` trigger can detect
/// rule-replies-to-its-own-reply self-loops.
///
/// Same `Vec<Option<String>>` schema-boundary shape as
/// [`derive_reads_set`].
pub fn derive_writes_set(actions: &Value) -> Vec<Option<String>> {
    let mut out = std::collections::BTreeSet::new();
    let Some(arr) = actions.as_array() else {
        return Vec::new();
    };
    for action in arr {
        let Some(obj) = action.as_object() else {
            continue;
        };
        let Some(kind) = obj.get("kind").and_then(|k| k.as_str()) else {
            continue;
        };
        for w in writes_for_action_kind(kind) {
            out.insert(w.to_string());
        }
    }
    out.into_iter().map(Some).collect()
}

fn writes_for_action_kind(kind: &str) -> &'static [&'static str] {
    match kind {
        "reply" => &[
            "ticket.first_response_at",
            "ticket.updated_at",
            "ticket.comments",
        ],
        "set_status" => &[
            "ticket.workflow_state_id",
            "ticket.workflow_state.category",
            "ticket.resolved_at",
            "ticket.updated_at",
        ],
        "assign" => &["ticket.assignee_uuid", "ticket.updated_at"],
        "unassign" => &["ticket.assignee_uuid", "ticket.updated_at"],
        "add_tags" => &["ticket.tag_ids", "ticket.updated_at"],
        "remove_tags" => &["ticket.tag_ids", "ticket.updated_at"],
        "set_priority" => &["ticket.priority", "ticket.updated_at"],
        // The asset steps write assets and loans, never the ticket, so no
        // condition on a ticket field reads what they change.
        "assign_assets_to_requester" => &["asset.custody"],
        "lend_assets" => &["asset.loan"],
        "return_requester_assets" => &["asset.custody", "asset.loan"],
        "apply_macro_template" => &[
            "ticket.first_response_at",
            "ticket.updated_at",
            "ticket.comments",
        ],
        // notify, webhook, stop_processing have no ticket-state
        // writes (outbound side-effects only or engine control).
        _ => &[],
    }
}

// =====================================================================
// List / find. Workspace scoping is via RLS — the queries below
// rely on the session's app.workspace_id GUC and don't repeat it
// in WHERE clauses (matching the existing tenant-table pattern).
// =====================================================================

/// Filter shape for [`list`]. The handler unpacks query params into
/// this; the picker call site uses [`list_pickable_manual`] which
/// hardcodes its own filter.
#[derive(Debug, Default, Clone)]
pub struct ListFilter {
    pub trigger_kind: Option<RuleTriggerKind>,
    pub state: Option<RuleState>,
    pub include_archived: bool,
    pub name_query: Option<String>,
}

/// List rules visible in the current workspace, ordered by priority
/// then name. `archived_at IS NULL` unless `include_archived` is
/// explicitly set (the admin "show archived" toggle).
pub fn list(conn: &mut DbConnection, filter: ListFilter) -> QueryResult<Vec<Rule>> {
    use crate::schema::rules::dsl;
    let mut query = dsl::rules.into_boxed();
    if !filter.include_archived {
        query = query.filter(dsl::archived_at.is_null());
    }
    if let Some(kind) = filter.trigger_kind {
        query = query.filter(dsl::trigger_kind.eq(kind));
    }
    if let Some(state) = filter.state {
        query = query.filter(dsl::state.eq(state));
    }
    if let Some(q) = filter.name_query {
        // `ILIKE %q%` style match for the admin search box. The
        // bound % wrappers go through Diesel's escape path so a
        // literal `%` in the query is treated as literal.
        let pattern = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
        query = query.filter(dsl::name.ilike(pattern));
    }
    query
        .order((dsl::priority.asc(), dsl::name.asc()))
        .load(conn)
}

/// Live, non-archived, manual-trigger rules for the agent toolbar
/// picker (plan §13.4). Phase 1 has no per-ticket condition
/// evaluation since manual rules carry `conditions = []` by the
/// `rules_manual_no_conditions` CHECK.
pub fn list_pickable_manual(conn: &mut DbConnection) -> QueryResult<Vec<Rule>> {
    use crate::schema::rules::dsl;
    dsl::rules
        .filter(dsl::archived_at.is_null())
        .filter(dsl::state.eq(RuleState::Live))
        .filter(dsl::trigger_kind.eq(RuleTriggerKind::Manual))
        .order((dsl::priority.asc(), dsl::name.asc()))
        .load(conn)
}

pub fn find(conn: &mut DbConnection, id: i32) -> QueryResult<Option<Rule>> {
    use crate::schema::rules::dsl;
    dsl::rules.find(id).first(conn).optional()
}

// =====================================================================
// Create / update / archive / hard-delete.
// =====================================================================

/// Result type for write paths that need to distinguish "row not
/// found" and "row not in the right state for this op" from a
/// transient DB error.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("rule {0} not found")]
    NotFound(i32),
    #[error("rule {0} must be archived before hard delete")]
    NotArchived(i32),
    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),
}

// sync-pending-wire: needs sync aggregate wiring (rules.created on Insert)
/// Insert a new rule. `reads_set` and `writes_set` are computed
/// from `conditions` / `actions` before insert so the columns stay
/// in sync with the JSONB trees they derive from. Manual rules
/// pass `conditions = []` and get an empty `reads_set` populated.
pub fn create(conn: &mut DbConnection, mut new: NewRule) -> QueryResult<Rule> {
    use crate::schema::rules::dsl;
    new.reads_set = derive_reads_set(&new.conditions);
    new.writes_set = derive_writes_set(&new.actions);
    diesel::insert_into(dsl::rules)
        .values(&new)
        .get_result(conn)
}

// sync-pending-wire: needs sync aggregate wiring (rules.updated on Update)
/// Apply a partial update. If `conditions` or `actions` move,
/// `reads_set` / `writes_set` are recomputed and overwritten on
/// the change set; any explicit set the caller passed is ignored.
/// The migration's BEFORE UPDATE trigger writes a `rule_versions`
/// snapshot and bumps `updated_at`.
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    mut change: RuleUpdate,
) -> Result<Rule, WriteError> {
    use crate::schema::rules::dsl;
    if let Some(ref conditions) = change.conditions {
        change.reads_set = Some(derive_reads_set(conditions));
    }
    if let Some(ref actions) = change.actions {
        change.writes_set = Some(derive_writes_set(actions));
    }
    diesel::update(dsl::rules.find(id))
        .set(&change)
        .get_result::<Rule>(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => WriteError::NotFound(id),
            other => WriteError::Db(other),
        })
}

// sync-pending-wire: needs sync aggregate wiring (rules.archived on Update)
/// Soft-archive. Sets `archived_at = NOW()` and is the only path
/// the admin "Delete" button drives; hard delete requires
/// archived_at to already be set per decision 32 in the plan.
pub fn archive(
    conn: &mut DbConnection,
    id: i32,
    when: chrono::DateTime<chrono::Utc>,
) -> Result<Rule, WriteError> {
    use crate::schema::rules::dsl;
    diesel::update(dsl::rules.find(id))
        .set(dsl::archived_at.eq(Some(when)))
        .get_result::<Rule>(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => WriteError::NotFound(id),
            other => WriteError::Db(other),
        })
}

// sync-pending-wire: needs sync aggregate wiring (rules.restored on Update)
/// Bring an archived rule back as a draft: clears `archived_at` and sets the
/// state to draft, so it can be edited and has to go live again before
/// agents see it. A rule that isn't archived comes back unchanged.
pub fn restore(conn: &mut DbConnection, id: i32) -> Result<Rule, WriteError> {
    use crate::schema::rules::dsl;
    let existing: Rule = dsl::rules.find(id).first(conn).map_err(|e| match e {
        diesel::result::Error::NotFound => WriteError::NotFound(id),
        other => WriteError::Db(other),
    })?;
    if !existing.is_archived() {
        return Ok(existing);
    }
    diesel::update(dsl::rules.find(id))
        .set((
            dsl::archived_at.eq(None::<chrono::DateTime<chrono::Utc>>),
            dsl::state.eq(RuleState::Draft),
        ))
        .get_result::<Rule>(conn)
        .map_err(WriteError::Db)
}

// sync-pending-wire: needs sync aggregate wiring (rules.deleted on Delete)
/// Permanently delete a rule. The caller must have already
/// archived it; we return `NotArchived` rather than implicitly
/// soft-archiving so the admin can't bypass the "review recent
/// activity before destroying it" UX gate by hitting the API
/// directly. Cascades into `rule_versions` and `rule_applications`
/// via the FK.
pub fn hard_delete(conn: &mut DbConnection, id: i32) -> Result<(), WriteError> {
    use crate::schema::rules::dsl;
    let existing: Rule = dsl::rules.find(id).first(conn).map_err(|e| match e {
        diesel::result::Error::NotFound => WriteError::NotFound(id),
        other => WriteError::Db(other),
    })?;
    if existing.archived_at.is_none() {
        return Err(WriteError::NotArchived(id));
    }
    diesel::delete(dsl::rules.find(id)).execute(conn)?;
    Ok(())
}

// =====================================================================
// Versions (read-only; the migration's INSERT/UPDATE triggers do
// the writes).
// =====================================================================

pub fn list_versions(conn: &mut DbConnection, rule_id: i32) -> QueryResult<Vec<RuleVersion>> {
    use crate::schema::rule_versions::dsl;
    dsl::rule_versions
        .filter(dsl::rule_id.eq(rule_id))
        .order(dsl::version.desc())
        .load(conn)
}

pub fn find_version(
    conn: &mut DbConnection,
    rule_id: i32,
    version: i32,
) -> QueryResult<Option<RuleVersion>> {
    use crate::schema::rule_versions::dsl;
    dsl::rule_versions
        .filter(dsl::rule_id.eq(rule_id))
        .filter(dsl::version.eq(version))
        .first(conn)
        .optional()
}

// =====================================================================
// Applications (the audit / recent-activity log).
// =====================================================================

/// Filter shape for [`list_applications`]. The recent-activity
/// admin endpoint unpacks query params into this.
#[derive(Debug, Default, Clone)]
pub struct ApplicationFilter {
    pub rule_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub status: Option<RuleApplicationStatus>,
    pub actor_uuid: Option<Uuid>,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    pub limit: i64,
}

pub fn list_applications(
    conn: &mut DbConnection,
    filter: ApplicationFilter,
) -> QueryResult<Vec<RuleApplication>> {
    use crate::schema::rule_applications::dsl;
    let mut query = dsl::rule_applications.into_boxed();
    if let Some(rid) = filter.rule_id {
        query = query.filter(dsl::rule_id.eq(rid));
    }
    if let Some(tid) = filter.ticket_id {
        query = query.filter(dsl::ticket_id.eq(tid));
    }
    if let Some(status) = filter.status {
        query = query.filter(dsl::status.eq(status));
    }
    if let Some(actor) = filter.actor_uuid {
        query = query.filter(dsl::actor_uuid.eq(actor));
    }
    if let Some(from) = filter.from {
        query = query.filter(dsl::applied_at.ge(from));
    }
    if let Some(to) = filter.to {
        query = query.filter(dsl::applied_at.le(to));
    }
    let limit = filter.limit.clamp(1, 500);
    query.order(dsl::applied_at.desc()).limit(limit).load(conn)
}

pub fn find_application(conn: &mut DbConnection, id: i64) -> QueryResult<Option<RuleApplication>> {
    use crate::schema::rule_applications::dsl;
    dsl::rule_applications.find(id).first(conn).optional()
}

pub fn list_applications_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<RuleApplication>> {
    use crate::schema::rule_applications::dsl;
    dsl::rule_applications
        .filter(dsl::ticket_id.eq(ticket_id))
        .order(dsl::applied_at.desc())
        .load(conn)
}

// sync-audit-only: rule-application audit row, no entity sync
/// Insert one application row. Called from inside the apply
/// lifecycle's transaction so the row's `correlation_id` matches
/// the audit_log + sync_actions rows the same apply produced.
pub fn record_application(
    conn: &mut DbConnection,
    new: NewRuleApplication,
) -> QueryResult<RuleApplication> {
    use crate::schema::rule_applications::dsl;
    diesel::insert_into(dsl::rule_applications)
        .values(&new)
        .get_result(conn)
}

// =====================================================================
// Apply lifecycle. Mirrors execute_merge's shape: one
// `with_actor_context` transaction per rule and ticket, advisory lock on
// the ticket, preflight under the lock, then the action executors in
// order. Any preflight failure or executor error rolls the whole run
// back; the rule_applications row records what happened.
//
// An agent applies a manual rule ([`apply_manual`]); the rules engine
// runs an automated one as a system actor once its conditions match
// ([`run_rule_on_ticket`]). A rule in dry run runs in shadow: it records
// what it would do and writes nothing else. An automated run that fails
// past its preflight leaves a `failed` row, written after the rollback.
// =====================================================================

/// Pack `(workspace_id, ticket_id)` into a collision-free int64
/// advisory-lock key. Same formula as
/// `repository::ticket_merge::advisory_key`; the helper there is
/// private. We redefine locally rather than expose another crate's
/// private helper through a pub(crate) escape hatch — single
/// expression, no behavioural drift risk.
fn advisory_key(workspace_id: i32, ticket_id: i32) -> i64 {
    ((workspace_id as i64) << 32) | (ticket_id as i64 & 0xffff_ffff)
}

/// One-indexed action position the agent dialog ticks off to skip.
/// `Vec<usize>` of positions, validated against the rule's action
/// list length at the API boundary.
#[derive(Debug, Clone, Default)]
pub struct ApplyOverrides {
    /// Replaces the first `reply` action's body if Some. It is the
    /// template the agent edited in the dialog, so its variable tokens
    /// are filled in like the rule's own body.
    pub body: Option<String>,
    /// Action positions to skip (1-indexed per decision 33).
    pub suppress_actions: Vec<usize>,
}

/// Input to [`apply_manual`]. Built by the handler from the
/// request body + the resolved actor.
#[derive(Debug, Clone)]
pub struct ApplyInput {
    pub rule_id: i32,
    pub ticket_id: i32,
    pub overrides: ApplyOverrides,
}

/// What started a rule run. Each source runs only the trigger kinds it
/// serves: an agent applies manual rules, ticket events run the event
/// triggers, and the clock runs time-elapsed rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplySource {
    Manual,
    Event,
    Time,
}

impl ApplySource {
    /// Whether a rule with this trigger runs from this source.
    pub fn runs(self, kind: RuleTriggerKind) -> bool {
        match self {
            Self::Manual => kind == RuleTriggerKind::Manual,
            Self::Event => matches!(
                kind,
                RuleTriggerKind::TicketCreated
                    | RuleTriggerKind::TicketUpdated
                    | RuleTriggerKind::TicketReplied
            ),
            Self::Time => kind == RuleTriggerKind::TimeElapsed,
        }
    }
}

/// The event an automated run reacts to, recorded on its application row.
#[derive(Debug, Clone)]
pub struct OriginatingEvent {
    pub id: Uuid,
    pub kind: String,
}

/// One rule run on one ticket: an agent's apply ([`apply_manual`]) or an
/// automated fire. Start from [`ApplyContext::new`] and set what the run
/// needs.
pub struct ApplyContext<'a> {
    /// Who the writes are credited to: the agent applying the rule, or the
    /// engine's system actor. Its correlation id goes on every row the run
    /// writes.
    pub actor: &'a ActorContext,
    pub rule_id: i32,
    pub ticket_id: i32,
    pub source: ApplySource,
    /// Record what the rule would do without doing it. A rule in dry run
    /// runs only in shadow, and only a rule in dry run does.
    pub shadow: bool,
    /// The agent's edits in the apply dialog.
    pub overrides: ApplyOverrides,
    /// Recorded on the application row.
    pub originating_event: Option<OriginatingEvent>,
    /// How the rule's conditions evaluated, recorded on the application row.
    pub condition_evaluation: Option<Value>,
    /// Causation of the `ticket.rule_applied` sync action.
    pub causation_id: Option<Uuid>,
    /// Indexes the ticket and, when a step closes a recurring one, its
    /// next occurrence.
    pub observer: Option<&'a dyn crate::repository::tickets::TicketUpdatedObserver>,
}

impl<'a> ApplyContext<'a> {
    /// A run that writes, with nothing else recorded.
    pub fn new(actor: &'a ActorContext, rule_id: i32, ticket_id: i32, source: ApplySource) -> Self {
        Self {
            actor,
            rule_id,
            ticket_id,
            source,
            shadow: false,
            overrides: ApplyOverrides::default(),
            originating_event: None,
            condition_evaluation: None,
            causation_id: None,
            observer: None,
        }
    }
}

/// Counts the apply endpoint returns to the agent dialog.
#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    /// Updated rule row (with the bumped `fire_count`; unchanged after a
    /// shadow run).
    pub rule: Rule,
    /// The application audit row inserted at the end of the txn.
    pub application: RuleApplication,
    /// The `comments.id` of the reply action's row, when the rule
    /// had a reply action and it wasn't suppressed. `None`
    /// otherwise.
    pub comment_id: Option<i32>,
    /// How many actions ran (skip / failure subtracts). None run in
    /// shadow; `application.actions_taken` lists what would have.
    pub actions_executed: usize,
    /// How many actions were skipped via `overrides.suppress_actions`.
    pub actions_suppressed: usize,
    /// The rule has a `stop_processing` step that wasn't suppressed, so
    /// no lower-priority rule runs for the same event.
    pub stop_processing: bool,
}

/// Why a run didn't apply.
#[derive(Debug)]
pub struct RunFailed {
    pub error: ApplyError,
    /// The `failed` application row. Only an automated run that passed its
    /// preflight leaves one: an agent sees the error instead, and a rule or
    /// ticket the preflight turned away (gone, no longer live, merged) has
    /// nothing to record. `None` too if writing the row failed (logged).
    pub recorded: Option<Box<RuleApplication>>,
}

/// Errors specific to the apply lifecycle. Distinct from
/// [`WriteError`] because handler callers map these to user-facing
/// 400/404/409 codes.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("rule {0} not found")]
    NotFound(i32),
    #[error("rule {0} is not live (current state: {1})")]
    NotLive(i32, &'static str),
    #[error("rule {0} is not a manual-trigger rule")]
    NotManual(i32),
    /// An automated run of a rule whose trigger its source doesn't serve.
    #[error("rule {0} has the {1} trigger, which this run doesn't serve")]
    WrongTrigger(i32, &'static str),
    #[error("ticket {0} not found")]
    TicketNotFound(i32),
    /// Carries the ticket's number.
    #[error("ticket #{0} is merged into another ticket and cannot be modified")]
    TicketMerged(i32),
    #[error("override index {0} is out of range (rule has {1} actions)")]
    InvalidOverrideIndex(usize, usize),
    #[error("rule action [{index}] is not valid for Phase 1: {message}")]
    UnsupportedActionPhase1 { index: usize, message: String },
    #[error("rule action [{index}] failed: {message}")]
    ActionFailed { index: usize, message: String },
    /// The actor's user row was not found or is soft-deleted. Distinct
    /// from a transient DB error so the handler can return 401/410
    /// instead of 500 when an agent's account is revoked between the
    /// JWT validation and the apply transaction.
    #[error("actor user {0} not found or revoked")]
    AgentRevoked(Uuid),
    #[error("actor has no workspace context")]
    MissingWorkspace,
    /// A direct assign step names someone who can't work tickets.
    #[error("{}", crate::repository::tickets::INELIGIBLE_ASSIGNEE)]
    IneligibleAssignee(Uuid),
    /// A team assign step's team has no one who can work tickets.
    #[error("Step {index}: the team has no one who can work tickets")]
    NoAssignableTeamMember { index: usize },
    /// An asset step needs the ticket's requester and the ticket has none.
    #[error("Step {index}: the ticket has no requester to give assets to or take them back from")]
    NoRequester { index: usize },
    /// An asset step met an asset in a state the step can't move it from.
    #[error("Step {index}: {message}")]
    AssetRefused { index: usize, message: String },
    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),
}

impl ApplyError {
    /// Whether the message already names the step it failed at.
    fn names_step(&self) -> bool {
        matches!(
            self,
            Self::ActionFailed { .. }
                | Self::UnsupportedActionPhase1 { .. }
                | Self::NoAssignableTeamMember { .. }
                | Self::NoRequester { .. }
                | Self::AssetRefused { .. }
        )
    }
}

/// Apply one manual rule to one ticket. Atomic: every action runs
/// inside one `with_actor_context` transaction. Pre-flight checks
/// the rule + ticket under the advisory lock so a racing archive
/// or merge can't slip past. On any executor error the whole apply
/// rolls back and the `rule_applications` row is never written
/// (which is the audit-correct behaviour: the failed apply didn't
/// happen). Only a live rule applies: a draft, a paused or an archived
/// one is refused before anything runs, whoever calls.
pub fn apply_manual(
    conn: &mut DbConnection,
    input: ApplyInput,
    actor: &ActorContext,
    observer: Option<&dyn crate::repository::tickets::TicketUpdatedObserver>,
) -> Result<ApplyOutcome, ApplyError> {
    let ctx = ApplyContext {
        overrides: input.overrides,
        observer,
        ..ApplyContext::new(actor, input.rule_id, input.ticket_id, ApplySource::Manual)
    };
    run_rule_on_ticket(conn, ctx).map_err(|failed| failed.error)
}

/// How far a run got before it failed.
#[derive(Default)]
struct RunProgress {
    /// The rule's version, once the preflight passed.
    rule_version: Option<i32>,
    /// The step running, one-indexed.
    step: Option<usize>,
}

/// Run one rule on one ticket, the way [`apply_manual`] does for an agent.
/// The rule must be live, or in dry run for a shadow run, and have a
/// trigger `ctx.source` serves. A shadow run checks each step as the real
/// run would and writes only its `dry_run` application row: no ticket
/// change, no fire count, no sync action. A failed automated run is rolled
/// back, then recorded as a `failed` row in a transaction of its own.
pub fn run_rule_on_ticket(
    conn: &mut DbConnection,
    ctx: ApplyContext,
) -> Result<ApplyOutcome, RunFailed> {
    let Some(workspace_id) = ctx.actor.workspace_id else {
        return Err(RunFailed {
            error: ApplyError::MissingWorkspace,
            recorded: None,
        });
    };
    let mut progress = RunProgress::default();
    let error = match with_actor_context(conn, ctx.actor, |conn| {
        run_locked(conn, &ctx, workspace_id, &mut progress)
    }) {
        Ok(outcome) => return Ok(outcome),
        Err(error) => error,
    };
    let recorded = match progress.rule_version {
        Some(rule_version) if ctx.source != ApplySource::Manual => record_failure(
            conn,
            &ctx,
            workspace_id,
            rule_version,
            &error,
            progress.step,
        ),
        _ => None,
    };
    Err(RunFailed { error, recorded })
}

/// `rule_applications.actor_kind` for `actor`: a person, or the system
/// (the engine, a plugin).
fn application_actor_kind(actor: &ActorContext) -> String {
    match actor.kind {
        crate::sync::actor::ActorKind::User => "user",
        _ => "system",
    }
    .to_string()
}

/// The `failed` row for a run that rolled back. Its own transaction, as
/// the run's actor, so it carries the run's correlation id.
fn record_failure(
    conn: &mut DbConnection,
    ctx: &ApplyContext,
    workspace_id: i32,
    rule_version: i32,
    error: &ApplyError,
    step: Option<usize>,
) -> Option<Box<RuleApplication>> {
    let failure_reason = match step {
        Some(n) if !error.names_step() => format!("Step {n}: {error}"),
        _ => error.to_string(),
    };
    let new = NewRuleApplication {
        workspace_id,
        rule_id: ctx.rule_id,
        rule_version,
        ticket_id: ctx.ticket_id,
        status: RuleApplicationStatus::Failed,
        correlation_id: ctx.actor.correlation_id,
        actor_uuid: ctx.actor.uuid,
        actor_kind: application_actor_kind(ctx.actor),
        originating_event_id: ctx.originating_event.as_ref().map(|e| e.id),
        originating_event_kind: ctx.originating_event.as_ref().map(|e| e.kind.clone()),
        condition_evaluation: ctx.condition_evaluation.clone(),
        actions_taken: None,
        actions_skipped: None,
        failure_reason: Some(failure_reason),
    };
    match with_actor_context(conn, ctx.actor, |conn| record_application(conn, new)) {
        Ok(row) => Some(Box::new(row)),
        Err(e) => {
            tracing::warn!(
                error = %e,
                ticket_id = ctx.ticket_id,
                "rule {} failed and its failed row could not be written",
                ctx.rule_id
            );
            None
        }
    }
}

/// The body of [`run_rule_on_ticket`], inside its transaction.
fn run_locked(
    conn: &mut DbConnection,
    ctx: &ApplyContext,
    workspace_id: i32,
    progress: &mut RunProgress,
) -> Result<ApplyOutcome, ApplyError> {
    use crate::schema::rules::dsl as r;
    use crate::schema::tickets::dsl as t;

    let shadow = ctx.shadow;
    diesel::sql_query("SELECT pg_advisory_xact_lock($1)")
        .bind::<BigInt, _>(advisory_key(workspace_id, ctx.ticket_id))
        .execute(conn)?;

    // Re-read rule + ticket under the lock so racing edits
    // (archive, state change, merge) are caught.
    let rule: Rule = r::rules
        .find(ctx.rule_id)
        .first(conn)
        .optional()?
        .ok_or(ApplyError::NotFound(ctx.rule_id))?;
    if rule.archived_at.is_some() {
        return Err(ApplyError::NotLive(rule.id, rule.state.as_str()));
    }
    let runnable_state = if shadow {
        RuleState::DryRun
    } else {
        RuleState::Live
    };
    if rule.state != runnable_state {
        return Err(ApplyError::NotLive(rule.id, rule.state.as_str()));
    }
    if !ctx.source.runs(rule.trigger_kind) {
        return Err(match ctx.source {
            ApplySource::Manual => ApplyError::NotManual(rule.id),
            _ => ApplyError::WrongTrigger(rule.id, rule.trigger_kind.as_str()),
        });
    }

    let ticket: crate::models::Ticket = t::tickets
        .find(ctx.ticket_id)
        .first(conn)
        .optional()?
        .ok_or(ApplyError::TicketNotFound(ctx.ticket_id))?;
    // Merge metadata now lives in the `ticket_merges` satellite; a row
    // there means the ticket is a merge source (terminal, read-only).
    let is_merged: bool = diesel::select(diesel::dsl::exists(
        crate::schema::ticket_merges::table.find(ticket.id),
    ))
    .get_result(conn)?;
    if is_merged {
        return Err(ApplyError::TicketMerged(ticket.number));
    }

    // rule_version is the current version after the last save;
    // pull from rule_versions max(version) for this rule. The
    // migration trigger guarantees a v1 row at minimum.
    let rule_version: i32 = {
        use crate::schema::rule_versions::dsl;
        dsl::rule_versions
            .filter(dsl::rule_id.eq(rule.id))
            .select(diesel::dsl::max(dsl::version))
            .first::<Option<i32>>(conn)?
            .unwrap_or(1)
    };
    progress.rule_version = Some(rule_version);

    let Some(actions) = rule.actions.as_array() else {
        return Err(ApplyError::ActionFailed {
            index: 0,
            message: "rule.actions is not a JSON array (DB CHECK should have caught this)".into(),
        });
    };
    // Validate override indices up front so we don't run half
    // the action list before bailing.
    for idx in &ctx.overrides.suppress_actions {
        if *idx == 0 || *idx > actions.len() {
            return Err(ApplyError::InvalidOverrideIndex(*idx, actions.len()));
        }
    }
    let suppress: std::collections::HashSet<usize> =
        ctx.overrides.suppress_actions.iter().copied().collect();

    let mut comment_id: Option<i32> = None;
    let mut actions_executed = 0usize;
    let mut stop_processing = false;
    let mut actions_taken: Vec<Value> = Vec::with_capacity(actions.len());
    let mut actions_skipped: Vec<Value> = Vec::with_capacity(suppress.len());
    let mut applied_reply_body_override = false;

    for (i, action) in actions.iter().enumerate() {
        // Executors receive `one_based` so any ActionFailed they
        // raise carries the index of the failing action rather
        // than a placeholder. The error envelope downstream
        // shows the admin which position in the rule's action
        // list to investigate.
        let one_based = i + 1;
        if suppress.contains(&one_based) {
            actions_skipped.push(json!({
                "index": one_based,
                "reason": "suppressed_by_override"
            }));
            continue;
        }
        progress.step = Some(one_based);
        let kind = action.get("kind").and_then(|k| k.as_str()).ok_or_else(|| {
            ApplyError::ActionFailed {
                index: one_based,
                message: "missing kind".to_string(),
            }
        })?;
        let config = action.get("config").cloned().unwrap_or(Value::Null);

        let outcome = match kind {
            "reply" => {
                let override_body = if !applied_reply_body_override {
                    let b = ctx.overrides.body.clone();
                    if b.is_some() {
                        applied_reply_body_override = true;
                    }
                    b
                } else {
                    None
                };
                execute_reply(conn, one_based, &ticket, ctx, &rule, &config, override_body)?
                    .map(|c| {
                        if comment_id.is_none() {
                            comment_id = Some(c);
                        }
                        json!({ "index": one_based, "kind": kind, "comment_id": c })
                    })
                    .unwrap_or_else(|| json!({ "index": one_based, "kind": kind }))
            }
            "set_status" => {
                execute_set_status(conn, one_based, ticket.id, &config, ctx.observer, shadow)
                    .map(|state_id| {
                        json!({ "index": one_based, "kind": kind, "workflow_state_id": state_id })
                    })?
            }
            "assign" => execute_assign(conn, one_based, ticket.id, &config, shadow).map(|uuid| {
                json!({ "index": one_based, "kind": kind, "assigned_to_uuid": uuid })
            })?,
            "unassign" => execute_unassign(conn, ticket.id, shadow)
                .map(|_| json!({ "index": one_based, "kind": kind }))?,
            "add_tags" => execute_add_tags(conn, ticket.id, &config, ctx.actor, shadow)
                .map(|added| json!({ "index": one_based, "kind": kind, "tag_ids_added": added }))?,
            "remove_tags" => execute_remove_tags(conn, ticket.id, &config, ctx.actor, shadow)
                .map(|removed| {
                    json!({ "index": one_based, "kind": kind, "tag_ids_removed": removed })
                })?,
            "set_priority" => execute_set_priority(conn, one_based, ticket.id, &config, shadow)
                .map(|p| json!({ "index": one_based, "kind": kind, "priority": p }))?,
            "assign_assets_to_requester" => {
                let mut done = execute_assign_assets(conn, one_based, &ticket, ctx, shadow)?;
                stamp_step(&mut done, one_based, kind);
                done
            }
            "lend_assets" => {
                let mut done =
                    execute_lend_assets(conn, one_based, &ticket, ctx, &config, shadow)?;
                stamp_step(&mut done, one_based, kind);
                done
            }
            "return_requester_assets" => {
                let mut done = execute_return_assets(conn, one_based, &ticket, ctx, shadow)?;
                stamp_step(&mut done, one_based, kind);
                done
            }
            "stop_processing" => {
                stop_processing = true;
                if ctx.source == ApplySource::Manual {
                    // No-op for manual apply; only meaningful inside
                    // an event-chain run loop.
                    json!({ "index": one_based, "kind": kind, "note": "no-op for manual apply" })
                } else {
                    json!({ "index": one_based, "kind": kind })
                }
            }
            "notify" | "apply_macro_template" | "webhook" => {
                return Err(ApplyError::UnsupportedActionPhase1 {
                    index: one_based,
                    message: format!(
                        "{kind} action is scheduled for a later phase; admin should archive this rule or remove the action"
                    ),
                });
            }
            other => {
                return Err(ApplyError::ActionFailed {
                    index: one_based,
                    message: format!("unknown action kind: {other}"),
                });
            }
        };
        actions_taken.push(outcome);
        if !shadow {
            actions_executed += 1;
        }
    }
    progress.step = None;

    // A shadow run changes nothing on the rule: it didn't fire.
    let updated_rule: Rule = if shadow {
        rule.clone()
    } else {
        // Bump fire_count + last_fired_at on the rule. Use sql::now
        // so the timestamp comes from the same transaction's clock
        // as the rule_applications.applied_at row below.
        diesel::update(r::rules.find(rule.id))
            .set((
                r::fire_count.eq(r::fire_count + 1),
                r::last_fired_at.eq(Some(chrono::Utc::now())),
            ))
            .get_result(conn)?
    };

    let status = if shadow {
        RuleApplicationStatus::DryRun
    } else {
        RuleApplicationStatus::Succeeded
    };

    let actions_taken_value = if actions_taken.is_empty() {
        None
    } else {
        Some(Value::Array(actions_taken))
    };
    let actions_skipped_value = if actions_skipped.is_empty() {
        None
    } else {
        Some(Value::Array(actions_skipped))
    };

    let application = record_application(
        conn,
        NewRuleApplication {
            workspace_id,
            rule_id: rule.id,
            rule_version,
            ticket_id: ticket.id,
            status,
            correlation_id: ctx.actor.correlation_id,
            actor_uuid: ctx.actor.uuid,
            actor_kind: application_actor_kind(ctx.actor),
            originating_event_id: ctx.originating_event.as_ref().map(|e| e.id),
            originating_event_kind: ctx.originating_event.as_ref().map(|e| e.kind.clone()),
            condition_evaluation: ctx.condition_evaluation.clone(),
            actions_taken: actions_taken_value.clone(),
            actions_skipped: actions_skipped_value,
            failure_reason: None,
        },
    )?;

    // Emit ticket.rule_applied so the activity feed surfaces the
    // fire alongside ticket.merged + manual events. correlation_id
    // is set via the actor session GUC; the audit_log + sync_actions
    // rows for this transaction all share it. A shadow run did nothing
    // a client would show.
    if !shadow {
        let sync_groups = groups::for_ticket(conn, &ticket)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket.id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.rule_applied",
                data: json!({
                    "rule_id": rule.id,
                    "rule_version": rule_version,
                    "rule_name": updated_rule.name,
                    "actor_uuid": ctx.actor.uuid,
                    "comment_id": comment_id,
                    "actions_taken": actions_taken_value,
                    // Always false now; kept for readers of older events.
                    "was_dry_run": false,
                }),
                groups: sync_groups,
                causation_id: ctx.causation_id,
            },
        )?;
    }

    Ok(ApplyOutcome {
        rule: updated_rule,
        application,
        comment_id,
        actions_executed,
        actions_suppressed: suppress.len(),
        stop_processing,
    })
}

// =====================================================================
// Action executors (Wave 5 / unit-06). Each returns the relevant
// detail the caller folds into the actions_taken JSONB blob.
// =====================================================================

/// `reply` action. Renders the body via services::template_vars
/// (when the caller doesn't pass an override), inserts a comment,
/// and returns the new comment id. Internal vs public is the
/// action's `visibility` flag (see plan §5.3); the comment row
/// stamps `is_internal` accordingly. Outbound channel dispatch
/// (customer email relay) is the apply handler's
/// post-commit job; this executor only writes the comment row +
/// emits the comment.created sync event via
/// `comments::create_comment`.
///
/// Substituted template variable values are HTML-escaped before
/// the substitution so an untrusted value (e.g. a requester whose
/// IdP-synced display name carries HTML metacharacters) cannot
/// inject markup into the rendered HTML comment body. The body
/// template itself (admin-authored, vetted at save) is not
/// escaped — admins routinely write <br> / <b> deliberately.
///
/// An agent's apply replies as the agent. An automated run replies as
/// the step's `send_as_uuid`, else the rule's author, who must still
/// be able to work tickets here. A shadow run checks the body and the
/// author and posts nothing.
fn execute_reply(
    conn: &mut DbConnection,
    action_index: usize,
    ticket: &crate::models::Ticket,
    ctx: &ApplyContext,
    rule: &Rule,
    config: &Value,
    override_body: Option<String>,
) -> Result<Option<i32>, ApplyError> {
    let visibility = config
        .get("visibility")
        .and_then(|v| v.as_str())
        .unwrap_or("public");
    let raw_body = override_body
        .or_else(|| {
            config
                .get("body")
                .and_then(|b| b.as_str())
                .map(|s| s.to_string())
        })
        .ok_or_else(|| ApplyError::ActionFailed {
            index: action_index,
            message: "reply action missing body".to_string(),
        })?;

    let agent = reply_author(conn, action_index, ticket, ctx, rule, config)?;
    if ctx.shadow {
        return Ok(None);
    }
    // Requester is optional — anonymous tickets and revoked
    // requesters both surface as None and the renderer falls back
    // to empty bindings. Same .ok() shape as the lazy OIDC path.
    let requester = if let Some(uuid) = ticket.requester_uuid {
        crate::repository::users::find_active_by_uuid(&uuid, conn).ok()
    } else {
        None
    };
    // Pull the workspace-scoped app_name from site_settings; this
    // mirrors how email signatures and password-reset notices
    // resolve the brand string. Falls back to "Nosdesk" only if
    // the row genuinely doesn't carry an app_name.
    // SiteSettings.app_name is String (not Option) and the row is
    // singleton-id=1; fall back to "Nosdesk" only if the row read
    // genuinely fails (extremely rare — get_site_settings returns
    // Err only on disconnected DB).
    let app_name = crate::repository::site_settings::get_site_settings(conn)
        .map(|s| s.app_name)
        .unwrap_or_else(|_| "Nosdesk".to_string());
    // A plain-text template keeps its line breaks in the HTML comment.
    let rendered = crate::services::template_vars::render(
        &crate::utils::content::reply_template_html(&raw_body),
        &crate::services::template_vars::TemplateContext {
            ticket,
            requester: requester.as_ref(),
            agent: &agent,
            app_name: &app_name,
            event: None,
            reply: None,
        },
    );

    let is_internal = visibility == "internal";
    let new_comment = NewComment {
        content: rendered,
        ticket_id: ticket.id,
        user_uuid: agent.uuid,
        channel_metadata: Some(json!({
            "kind": "rule_reply",
            "visibility": visibility,
        })),
        is_internal,
        content_format: crate::models::ContentFormat::Html,
        ..NewComment::default()
    };
    let comment = comments::create_comment(conn, new_comment, None)?;
    Ok(Some(comment.id))
}

/// Who a `reply` step posts as (decision D5): the agent applying a
/// manual rule, or for an automated run the step's `send_as_uuid`, else
/// the rule's author.
fn reply_author(
    conn: &mut DbConnection,
    action_index: usize,
    ticket: &crate::models::Ticket,
    ctx: &ApplyContext,
    rule: &Rule,
    config: &Value,
) -> Result<crate::models::User, ApplyError> {
    if ctx.source == ApplySource::Manual {
        let actor_uuid = ctx.actor.uuid.ok_or_else(|| ApplyError::ActionFailed {
            index: action_index,
            message: "manual reply requires an authenticated actor".to_string(),
        })?;
        // The agent's row is required (we need their display name for
        // template rendering); NotFound (revoked / soft-deleted)
        // surfaces as AgentRevoked → 401 rather than Db → 500.
        return crate::repository::users::find_active_by_uuid(&actor_uuid, conn).map_err(
            |e| match e {
                diesel::result::Error::NotFound => ApplyError::AgentRevoked(actor_uuid),
                other => ApplyError::Db(other),
            },
        );
    }
    let failed = |message: &str| ApplyError::ActionFailed {
        index: action_index,
        message: message.to_string(),
    };
    let send_as = config
        .get("send_as_uuid")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .or(rule.created_by)
        .ok_or_else(|| failed("the reply has no agent to send as"))?;
    let agent = crate::repository::users::find_active_by_uuid(&send_as, conn).optional()?;
    match agent {
        Some(agent)
            if crate::repository::assignees::is_assignable(conn, ticket.workspace_id, send_as)? =>
        {
            Ok(agent)
        }
        _ => Err(failed(
            "the agent the reply sends as can no longer work tickets here",
        )),
    }
}

/// The team member with the fewest open tickets assigned, among those who can
/// work tickets. Ties go to the earlier member in the team's order, so an idle
/// team fills up in turn. Counting is workspace-scoped through the caller's
/// tenant connection.
fn least_loaded_member(conn: &mut DbConnection, group_id: i32) -> QueryResult<Option<Uuid>> {
    use crate::models::WorkflowStateCategory;
    use crate::schema::{tickets, workflow_states};
    let members: Vec<Uuid> = crate::repository::assignees::eligible_members(conn, group_id)?;
    if members.is_empty() {
        return Ok(None);
    }
    let open: Vec<(Option<Uuid>, i64)> = tickets::table
        .inner_join(workflow_states::table.on(workflow_states::id.eq(tickets::workflow_state_id)))
        .filter(tickets::assignee_uuid.eq_any(&members))
        .filter(workflow_states::category.ne_all(vec![
            WorkflowStateCategory::Done,
            WorkflowStateCategory::Cancelled,
            WorkflowStateCategory::Merged,
        ]))
        .group_by(tickets::assignee_uuid)
        .select((tickets::assignee_uuid, diesel::dsl::count_star()))
        .load(conn)?;
    let count = |uuid: &Uuid| {
        open.iter()
            .find(|(assignee, _)| assignee.as_ref() == Some(uuid))
            .map_or(0, |(_, n)| *n)
    };
    Ok(members.iter().min_by_key(|u| count(u)).copied())
}

/// A rule step's change to the ticket row, made the way a manual edit makes
/// it: through `update_ticket_partial`, which records the previous assignee
/// and status, recomputes the SLA and emits the `ticket.*` sync action that
/// clients, notifications and webhooks read. In `shadow` nothing is written;
/// a new assignee is still refused if they can't work tickets, as the write
/// would refuse them.
fn update_ticket_fields(
    conn: &mut DbConnection,
    ticket_id: i32,
    update: TicketUpdate,
    observer: Option<&dyn crate::repository::tickets::TicketUpdatedObserver>,
    shadow: bool,
) -> Result<(), ApplyError> {
    use crate::repository::tickets::TicketWriteError;
    let written = if shadow {
        match update.assignee_uuid {
            Some(Some(user)) => crate::repository::assignees::ensure_assignable_here(conn, user),
            _ => Ok(()),
        }
    } else {
        crate::repository::tickets::update_ticket_partial(conn, ticket_id, update, observer)
            .map(|_| ())
    };
    match written {
        Ok(_) => Ok(()),
        Err(TicketWriteError::IneligibleAssignee(user)) => {
            Err(ApplyError::IneligibleAssignee(user))
        }
        Err(TicketWriteError::Merged) => Err(ApplyError::TicketMerged(
            crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?.number,
        )),
        Err(TicketWriteError::Database(e)) => Err(ApplyError::Db(e)),
    }
}

/// `set_status` action. `closed_at` and `closed_by` follow the new state in
/// the database (`ticket_closed_follows_state`).
/// `observer` indexes the ticket and, when this closes a recurring one, its
/// next occurrence.
fn execute_set_status(
    conn: &mut DbConnection,
    action_index: usize,
    ticket_id: i32,
    config: &Value,
    observer: Option<&dyn crate::repository::tickets::TicketUpdatedObserver>,
    shadow: bool,
) -> Result<i32, ApplyError> {
    let state_id = config
        .get("workflow_state_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ApplyError::ActionFailed {
            index: action_index,
            message: "set_status missing workflow_state_id".to_string(),
        })? as i32;
    if crate::repository::ticket_approvals::blocks_resolution(conn, ticket_id, state_id)? {
        return Err(ApplyError::ActionFailed {
            index: action_index,
            message: crate::repository::ticket_approvals::WAITING_MESSAGE.to_string(),
        });
    }
    update_ticket_fields(
        conn,
        ticket_id,
        TicketUpdate {
            workflow_state_id: Some(state_id),
            ..Default::default()
        },
        observer,
        shadow,
    )?;
    Ok(state_id)
}

/// `assign` action: a named person (`direct`), or whoever on a team
/// (`group`) has the fewest open tickets.
fn execute_assign(
    conn: &mut DbConnection,
    action_index: usize,
    ticket_id: i32,
    config: &Value,
    shadow: bool,
) -> Result<Uuid, ApplyError> {
    let method = config
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("direct");
    let assignee = match method {
        "direct" => config
            .get("user_uuid")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| ApplyError::ActionFailed {
                index: action_index,
                message: "assign(method=direct) missing valid user_uuid".to_string(),
            })?,
        "group" => {
            let group_id = config
                .get("group_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| ApplyError::ActionFailed {
                    index: action_index,
                    message: "assign(method=group) missing group_id".to_string(),
                })? as i32;
            least_loaded_member(conn, group_id)?.ok_or(ApplyError::NoAssignableTeamMember {
                index: action_index,
            })?
        }
        other => {
            return Err(ApplyError::UnsupportedActionPhase1 {
                index: action_index,
                message: format!(
                    "assign method '{other}' lands in Phase 2 when assignment_rules absorbs here"
                ),
            });
        }
    };
    update_ticket_fields(
        conn,
        ticket_id,
        TicketUpdate {
            assignee_uuid: Some(Some(assignee)),
            ..Default::default()
        },
        None,
        shadow,
    )?;
    Ok(assignee)
}

fn execute_unassign(
    conn: &mut DbConnection,
    ticket_id: i32,
    shadow: bool,
) -> Result<(), ApplyError> {
    update_ticket_fields(
        conn,
        ticket_id,
        TicketUpdate {
            assignee_uuid: Some(None),
            ..Default::default()
        },
        None,
        shadow,
    )
}

fn tag_ids_in(config: &Value) -> Vec<i32> {
    config
        .get("tag_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_i64())
                .map(|x| x as i32)
                .collect()
        })
        .unwrap_or_default()
}

/// `add_tags` action, through `set_tags_for_ticket` so the ticket's
/// `ticket.tags_changed` sync action goes out like a manual edit's.
/// Returns the tags it actually added (would add, in `shadow`).
fn execute_add_tags(
    conn: &mut DbConnection,
    ticket_id: i32,
    config: &Value,
    actor: &ActorContext,
    shadow: bool,
) -> Result<Vec<i32>, ApplyError> {
    let current = crate::repository::tags::tag_ids_for_ticket(conn, ticket_id)?;
    let added: Vec<i32> = tag_ids_in(config)
        .into_iter()
        .filter(|t| !current.contains(t))
        .collect();
    if !added.is_empty() && !shadow {
        let desired: Vec<i32> = current.iter().chain(&added).copied().collect();
        crate::repository::tags::set_tags_for_ticket(conn, ticket_id, &desired, actor.uuid)?;
    }
    Ok(added)
}

/// `remove_tags` action. Returns the tags it actually removed (would
/// remove, in `shadow`).
fn execute_remove_tags(
    conn: &mut DbConnection,
    ticket_id: i32,
    config: &Value,
    actor: &ActorContext,
    shadow: bool,
) -> Result<Vec<i32>, ApplyError> {
    let current = crate::repository::tags::tag_ids_for_ticket(conn, ticket_id)?;
    let to_remove = tag_ids_in(config);
    let removed: Vec<i32> = current
        .iter()
        .filter(|t| to_remove.contains(t))
        .copied()
        .collect();
    if !removed.is_empty() && !shadow {
        let desired: Vec<i32> = current
            .iter()
            .filter(|t| !removed.contains(t))
            .copied()
            .collect();
        crate::repository::tags::set_tags_for_ticket(conn, ticket_id, &desired, actor.uuid)?;
    }
    Ok(removed)
}

fn execute_set_priority(
    conn: &mut DbConnection,
    action_index: usize,
    ticket_id: i32,
    config: &Value,
    shadow: bool,
) -> Result<String, ApplyError> {
    let priority_str = config
        .get("priority")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApplyError::ActionFailed {
            index: action_index,
            message: "set_priority missing priority".to_string(),
        })?;
    // TicketPriority includes distinct `urgent` and `none` tiers
    // (migration 2025-06-08-120000_add_ticket_priority_urgent_none).
    // The save linter in the rule editor should validate against
    // this set; we error here as defence-in-depth so an editor
    // mismatch surfaces with a clear message
    // rather than a silent demotion.
    let priority = match TicketPriority::parse(priority_str) {
        Some(priority) => priority,
        None => {
            return Err(ApplyError::ActionFailed {
                index: action_index,
                message: format!("unknown priority: {priority_str}"),
            })
        }
    };
    update_ticket_fields(
        conn,
        ticket_id,
        TicketUpdate {
            priority: Some(priority),
            ..Default::default()
        },
        None,
        shadow,
    )?;
    Ok(priority.as_str().to_string())
}

// =====================================================================
// Asset steps: the ticket's linked assets go to its requester, for good
// or on loan, and an offboarding takes back what the requester has. They
// write through the asset, loan and lifecycle repositories, so every change
// sends its own `asset*` sync actions, none of which the rule event outbox
// takes. Custody here is the asset's user (`primary_user_uuid`); the
// "managed by" custodian is left alone. An asset synced from another system
// gets its user from that system, so these steps leave it alone too.
// =====================================================================

/// Lifecycle reason on an asset a rule puts in service for the requester.
pub const RULE_ASSIGNED_REASON: &str = "Assigned to the requester by a rule";
/// Lifecycle reason on a loan or asset a rule takes back from the requester.
pub const RULE_RETURNED_REASON: &str = "Taken back from the requester by a rule";

/// Statuses an assign step puts in service. An asset in repair, on loan,
/// retired, lost or disposed isn't one to hand out.
pub const ASSIGNABLE_FROM: &[&str] = &["in_service", "in_stock", "on_order", "in_transit"];

/// Longest loan a lend step sets, in days.
pub const MAX_LOAN_DAYS: i64 = 3650;

fn stamp_step(done: &mut Value, index: usize, kind: &str) {
    if let Some(obj) = done.as_object_mut() {
        obj.insert("index".into(), json!(index));
        obj.insert("kind".into(), json!(kind));
    }
}

/// The ticket's linked assets, lowest id first. A link's key is workspace
/// scoped, so these are all the ticket's own workspace's.
fn linked_assets(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<crate::models::Asset>> {
    let mut assets = crate::repository::tickets::get_devices_for_ticket(conn, ticket_id)?;
    assets.sort_by_key(|a| a.id);
    Ok(assets)
}

fn requester_of(ticket: &crate::models::Ticket, index: usize) -> Result<Uuid, ApplyError> {
    ticket
        .requester_uuid
        .ok_or(ApplyError::NoRequester { index })
}

fn asset_refused(index: usize, asset: &crate::models::Asset, why: &str) -> ApplyError {
    ApplyError::AssetRefused {
        index,
        message: format!("\"{}\" {why}", asset.name),
    }
}

/// `in_repair` as "in repair", for a refusal's message.
fn status_words(status: &str) -> String {
    status.replace('_', " ")
}

fn transition_failed(
    index: usize,
    asset: &crate::models::Asset,
    e: crate::repository::asset_lifecycle::TransitionError,
) -> ApplyError {
    use crate::repository::asset_lifecycle::TransitionError;
    match e {
        TransitionError::Db(e) => ApplyError::Db(e),
        other => asset_refused(index, asset, &format!("can't change status ({other:?})")),
    }
}

fn loan_failed(
    index: usize,
    asset: &crate::models::Asset,
    e: crate::repository::asset_loans::LoanError,
) -> ApplyError {
    use crate::repository::asset_loans::LoanError;
    match e {
        LoanError::Db(e) => ApplyError::Db(e),
        LoanError::AlreadyOnLoan => asset_refused(index, asset, "is already on loan"),
        LoanError::NotLoanable(status) => asset_refused(
            index,
            asset,
            &format!("is {}, so it can't be lent", status_words(&status)),
        ),
        other => asset_refused(index, asset, &format!("can't change its loan ({other:?})")),
    }
}

/// Set (or clear) an asset's user, sending `asset.updated`.
fn set_asset_user(
    conn: &mut DbConnection,
    asset_id: i32,
    user: Option<Uuid>,
) -> Result<(), ApplyError> {
    crate::repository::assets::update_device(
        conn,
        asset_id,
        crate::models::AssetUpdate {
            primary_user_uuid: Some(user),
            ..Default::default()
        },
    )?;
    Ok(())
}

/// Move an asset to `to_status` through the lifecycle, linked to the ticket.
fn move_asset(
    conn: &mut DbConnection,
    index: usize,
    ctx: &ApplyContext,
    ticket_id: i32,
    asset: &crate::models::Asset,
    to_status: &str,
    reason: &str,
) -> Result<(), ApplyError> {
    crate::repository::asset_lifecycle::transition(
        conn,
        crate::repository::asset_lifecycle::TransitionInput {
            asset_id: asset.id,
            to_status: to_status.to_string(),
            reason: Some(reason.to_string()),
            ticket_id: Some(ticket_id),
            metadata: json!({ "rule_id": ctx.rule_id }),
            actor_uuid: ctx.actor.uuid,
            disposal: None,
        },
    )
    .map(|_| ())
    .map_err(|e| transition_failed(index, asset, e))
}

/// `assign_assets_to_requester` action. Each linked asset gets the requester
/// as its user and, when it isn't in service yet, goes in service. The
/// assets are all checked before any changes, so one that can't be handed
/// out refuses the step by name. No linked assets is a no-op. Returns what
/// changed (would change, in `shadow`).
fn execute_assign_assets(
    conn: &mut DbConnection,
    index: usize,
    ticket: &crate::models::Ticket,
    ctx: &ApplyContext,
    shadow: bool,
) -> Result<Value, ApplyError> {
    let assets = linked_assets(conn, ticket.id)?;
    if assets.is_empty() {
        return Ok(json!({ "asset_ids": [] }));
    }
    let requester = requester_of(ticket, index)?;
    for asset in &assets {
        if asset.external_sync_source.is_none() && !ASSIGNABLE_FROM.contains(&asset.status.as_str())
        {
            return Err(asset_refused(
                index,
                asset,
                &format!(
                    "is {}, so it can't be assigned",
                    status_words(&asset.status)
                ),
            ));
        }
    }
    let mut assigned = Vec::new();
    let mut unchanged = Vec::new();
    let mut synced = Vec::new();
    for asset in &assets {
        if asset.external_sync_source.is_some() {
            synced.push(asset.id);
            continue;
        }
        let sets_user = asset.primary_user_uuid != Some(requester);
        let goes_in_service = asset.status != crate::models::AssetStatus::InService.as_str();
        if !sets_user && !goes_in_service {
            unchanged.push(asset.id);
            continue;
        }
        assigned.push(asset.id);
        if shadow {
            continue;
        }
        if sets_user {
            set_asset_user(conn, asset.id, Some(requester))?;
        }
        if goes_in_service {
            move_asset(
                conn,
                index,
                ctx,
                ticket.id,
                asset,
                crate::models::AssetStatus::InService.as_str(),
                RULE_ASSIGNED_REASON,
            )?;
        }
    }
    Ok(json!({
        "requester_uuid": requester,
        "asset_ids": assigned,
        "unchanged_asset_ids": unchanged,
        "synced_asset_ids": synced,
    }))
}

/// `lend_assets` action. Each linked asset goes out on loan to the
/// requester, the loan linked to the ticket and due back `due_in_days` from
/// today when the step sets it. An asset already lent to the requester stays
/// as it is, so applying the rule again lends nothing twice. The assets are
/// all checked first; one lent to someone else, or in a status loans don't
/// start from, refuses the step by name. No linked assets is a no-op.
fn execute_lend_assets(
    conn: &mut DbConnection,
    index: usize,
    ticket: &crate::models::Ticket,
    ctx: &ApplyContext,
    config: &Value,
    shadow: bool,
) -> Result<Value, ApplyError> {
    use crate::repository::asset_loans;

    let assets = linked_assets(conn, ticket.id)?;
    if assets.is_empty() {
        return Ok(json!({ "asset_ids": [], "loan_ids": [] }));
    }
    let requester = requester_of(ticket, index)?;
    let due_back = config
        .get("due_in_days")
        .and_then(Value::as_i64)
        .filter(|days| (1..=MAX_LOAN_DAYS).contains(days))
        .and_then(|days| {
            chrono::Utc::now()
                .date_naive()
                .checked_add_signed(chrono::Duration::days(days))
        });

    let mut to_lend = Vec::new();
    let mut already_lent = Vec::new();
    for asset in &assets {
        if let Some(loan) = asset_loans::active_for_asset(conn, asset.id)? {
            if loan.borrower_user_uuid == Some(requester) {
                already_lent.push(asset.id);
                continue;
            }
            return Err(asset_refused(
                index,
                asset,
                "is on loan to someone else, so it can't be lent",
            ));
        }
        if !asset_loans::LOANABLE_FROM.contains(&asset.status.as_str()) {
            return Err(asset_refused(
                index,
                asset,
                &format!("is {}, so it can't be lent", status_words(&asset.status)),
            ));
        }
        to_lend.push(asset);
    }

    let mut loan_ids = Vec::new();
    if !shadow {
        for asset in &to_lend {
            let loan = asset_loans::issue(
                conn,
                asset_loans::IssueLoan {
                    asset_id: asset.id,
                    borrower_user_uuid: requester,
                    loaned_at: None,
                    due_back,
                    ticket_id: Some(ticket.id),
                    notes: None,
                    actor_uuid: ctx.actor.uuid,
                },
            )
            .map_err(|e| loan_failed(index, asset, e))?;
            loan_ids.push(loan.id);
        }
    }
    Ok(json!({
        "requester_uuid": requester,
        "asset_ids": to_lend.iter().map(|a| a.id).collect::<Vec<_>>(),
        "loan_ids": loan_ids,
        "due_back": due_back,
        "already_lent_asset_ids": already_lent,
    }))
}

/// `return_requester_assets` action, for offboarding. Every loan the
/// requester has out in this workspace comes back, whichever ticket it was
/// lent from, and every asset they're the user of loses them as its user and,
/// if it was in service, goes back in stock, linked to this ticket. A
/// requester with nothing out is a no-op.
fn execute_return_assets(
    conn: &mut DbConnection,
    index: usize,
    ticket: &crate::models::Ticket,
    ctx: &ApplyContext,
    shadow: bool,
) -> Result<Value, ApplyError> {
    use crate::repository::asset_loans;

    let requester = requester_of(ticket, index)?;
    let mut returned_loans = Vec::new();
    let mut returned_assets = Vec::new();
    for (loan, asset) in asset_loans::active_for_borrower(conn, &requester)? {
        returned_loans.push(loan.id);
        returned_assets.push(asset.id);
        if !shadow {
            asset_loans::return_loan_logged(
                conn,
                asset.id,
                loan.id,
                chrono::Utc::now(),
                ctx.actor.uuid,
                None,
                Some(RULE_RETURNED_REASON),
            )
            .map_err(|e| loan_failed(index, &asset, e))?;
        }
    }

    // Read after the returns, so each asset's status is the one it went back to.
    let mut held = crate::repository::assets::get_devices_for_user(conn, &requester)?;
    held.sort_by_key(|a| a.id);
    let mut cleared = Vec::new();
    let mut synced = Vec::new();
    for asset in &held {
        if asset.external_sync_source.is_some() {
            synced.push(asset.id);
            continue;
        }
        cleared.push(asset.id);
        if shadow {
            continue;
        }
        set_asset_user(conn, asset.id, None)?;
        if asset.status == crate::models::AssetStatus::InService.as_str() {
            move_asset(
                conn,
                index,
                ctx,
                ticket.id,
                asset,
                crate::models::AssetStatus::InStock.as_str(),
                RULE_RETURNED_REASON,
            )?;
        }
    }
    Ok(json!({
        "requester_uuid": requester,
        "loan_ids": returned_loans,
        "returned_asset_ids": returned_assets,
        "cleared_asset_ids": cleared,
        "synced_asset_ids": synced,
    }))
}

// =====================================================================
// Save-time preview (plan, "Save-time mandatory check"): how many recent tickets a rule's
// conditions match, and the ids a condition tree names.
// =====================================================================

/// How many recent tickets a save previews a rule against.
pub const PREVIEW_SCAN: i64 = 200;

/// Result of a preview-match against recent tickets.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PreviewMatch {
    pub matched: i64,
    pub scanned: i64,
    /// Up to five matching tickets, newest first.
    pub sample_ticket_ids: Vec<i32>,
}

/// Run `predicate` against the workspace's `scan_limit` newest tickets
/// (at most 1000) and count the matches. Merged tickets and blank ones
/// nobody has touched since "New ticket" made them are left out: neither
/// is a request a rule would act on. Leaves on event fields count as met (see
/// [`Predicate::matches`]).
pub fn preview_match(
    conn: &mut DbConnection,
    predicate: &Predicate,
    scan_limit: i64,
) -> QueryResult<PreviewMatch> {
    use crate::models::WorkflowStateCategory;
    use crate::schema::tickets::dsl;
    use diesel::PgExpressionMethods;

    let categories = crate::repository::workflow_states::categories(conn)?;
    let merged: Vec<i32> = categories
        .iter()
        .filter(|(_, c)| **c == WorkflowStateCategory::Merged)
        .map(|(id, _)| *id)
        .collect();
    let ids: Vec<i32> = dsl::tickets
        .filter(dsl::workflow_state_id.ne_all(merged))
        .filter(dsl::blank_at.is_distinct_from(dsl::updated_at.nullable()))
        .order((dsl::created_at.desc(), dsl::id.desc()))
        .limit(scan_limit.clamp(1, 1000))
        .select(dsl::id)
        .load(conn)?;
    let facts = load_ticket_facts_with(conn, &ids, &categories)?;
    let matching: Vec<i32> = facts
        .iter()
        .filter(|t| predicate.matches(t, None))
        .map(|t| t.id)
        .collect();
    Ok(PreviewMatch {
        matched: matching.len() as i64,
        scanned: facts.len() as i64,
        sample_ticket_ids: matching.into_iter().take(5).collect(),
    })
}

/// What rule conditions read from each of `ticket_ids`, in the order
/// given. Ids the workspace can't see are skipped.
pub fn load_ticket_facts(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> QueryResult<Vec<TicketFacts>> {
    let categories = crate::repository::workflow_states::categories(conn)?;
    load_ticket_facts_with(conn, ticket_ids, &categories)
}

fn load_ticket_facts_with(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
    categories: &std::collections::HashMap<i32, crate::models::WorkflowStateCategory>,
) -> QueryResult<Vec<TicketFacts>> {
    use crate::schema::{tickets, user_profiles};
    use chrono::{DateTime, Utc};

    type Row = (
        i32,
        String,
        TicketPriority,
        Option<i32>,
        Option<Uuid>,
        Option<Uuid>,
        i32,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );
    let rows: Vec<Row> = tickets::table
        .filter(tickets::id.eq_any(ticket_ids))
        .select((
            tickets::id,
            tickets::title,
            tickets::priority,
            tickets::category_id,
            tickets::requester_uuid,
            tickets::assignee_uuid,
            tickets::workflow_state_id,
            tickets::created_at,
            tickets::updated_at,
            tickets::first_response_at,
            tickets::closed_at,
        ))
        .load(conn)?;
    let mut tags = crate::repository::tags::tag_ids_for_tickets(conn, ticket_ids)?;
    let requesters: Vec<Uuid> = rows.iter().filter_map(|r| r.4).collect();
    let emails = crate::repository::user_helpers::primary_emails(conn, &requesters)?;
    let organizations: std::collections::HashMap<Uuid, String> = user_profiles::table
        .filter(user_profiles::user_uuid.eq_any(&requesters))
        .filter(user_profiles::organization.is_not_null())
        .select((
            user_profiles::user_uuid,
            user_profiles::organization.assume_not_null(),
        ))
        .load::<(Uuid, String)>(conn)?
        .into_iter()
        .collect();
    let present = |s: Option<&String>| {
        s.map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let mut by_id: std::collections::HashMap<i32, TicketFacts> = rows
        .into_iter()
        .map(|r| {
            let facts = TicketFacts {
                id: r.0,
                title: r.1,
                priority: r.2,
                category_id: r.3,
                requester_uuid: r.4,
                assignee_uuid: r.5,
                workflow_state_id: r.6,
                workflow_category: categories.get(&r.6).copied(),
                tag_ids: tags.remove(&r.0).unwrap_or_default(),
                created_at: r.7,
                updated_at: r.8,
                first_response_at: r.9,
                resolved_at: r.10,
                requester_email: present(r.4.and_then(|u| emails.get(&u))),
                requester_organization: present(r.4.and_then(|u| organizations.get(&u))),
            };
            (facts.id, facts)
        })
        .collect();
    Ok(ticket_ids
        .iter()
        .filter_map(|id| by_id.remove(id))
        .collect())
}

/// The first id `refs` names that isn't in the workspace, as the
/// condition field naming it and the id.
pub fn unknown_condition_reference(
    conn: &mut DbConnection,
    refs: &ConditionRefs,
) -> QueryResult<Option<(ConditionField, i32)>> {
    use crate::schema::{tags, ticket_categories, workflow_states};

    let missing = |wanted: &std::collections::BTreeSet<i32>, found: Vec<i32>| {
        wanted.iter().copied().find(|id| !found.contains(id))
    };
    if !refs.category_ids.is_empty() {
        let found = ticket_categories::table
            .filter(ticket_categories::id.eq_any(&refs.category_ids))
            .select(ticket_categories::id)
            .load(conn)?;
        if let Some(id) = missing(&refs.category_ids, found) {
            return Ok(Some((ConditionField::CategoryId, id)));
        }
    }
    if !refs.workflow_state_ids.is_empty() {
        let found = workflow_states::table
            .filter(workflow_states::id.eq_any(&refs.workflow_state_ids))
            .select(workflow_states::id)
            .load(conn)?;
        if let Some(id) = missing(&refs.workflow_state_ids, found) {
            return Ok(Some((ConditionField::WorkflowStateId, id)));
        }
    }
    if !refs.tag_ids.is_empty() {
        let found = tags::table
            .filter(tags::id.eq_any(&refs.tag_ids))
            .select(tags::id)
            .load(conn)?;
        if let Some(id) = missing(&refs.tag_ids, found) {
            return Ok(Some((ConditionField::TagIds, id)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Strip the Option wrapper so test assertions stay readable.
    /// Production code keeps the Option<String> shape because that's
    /// what Diesel hands back for Array<Nullable<Text>> columns.
    fn flat(v: Vec<Option<String>>) -> Vec<String> {
        v.into_iter().flatten().collect()
    }

    #[test]
    fn empty_conditions_derive_empty_reads_set() {
        let conditions = json!([]);
        assert!(derive_reads_set(&conditions).is_empty());
    }

    #[test]
    fn single_leaf_condition_picks_one_field() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.priority",
            "op": "eq",
            "value": "high"
        });
        assert_eq!(flat(derive_reads_set(&conditions)), vec!["ticket.priority"]);
    }

    #[test]
    fn and_tree_collects_every_leaf() {
        let conditions = json!({
            "kind": "and",
            "children": [
                {"kind": "leaf", "field": "ticket.priority", "op": "eq", "value": "high"},
                {"kind": "leaf", "field": "ticket.category_id", "op": "eq", "value": 12}
            ]
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(reads, vec!["ticket.category_id", "ticket.priority"]);
    }

    #[test]
    fn not_wraps_and_traversal_continues() {
        let conditions = json!({
            "kind": "not",
            "child": {
                "kind": "or",
                "children": [
                    {"kind": "leaf", "field": "ticket.title", "op": "contains", "value": "spam"},
                    {"kind": "leaf", "field": "ticket.tag_ids", "op": "has_any", "value": [1]}
                ]
            }
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(reads, vec!["ticket.tag_ids", "ticket.title"]);
    }

    #[test]
    fn status_alias_expands_to_category() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.status",
            "op": "eq",
            "value": "open"
        });
        let reads = flat(derive_reads_set(&conditions));
        // ticket.status aliases the category column; conditioning
        // on either flags both for the linter so a rule that
        // mutates workflow_state_id while reading status is
        // caught as self-referential.
        assert_eq!(reads, vec!["ticket.workflow_state.category"]);
    }

    #[test]
    fn workflow_state_id_and_category_alias_each_other() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.workflow_state_id",
            "op": "eq",
            "value": 4
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(
            reads,
            vec!["ticket.workflow_state.category", "ticket.workflow_state_id"]
        );
    }

    #[test]
    fn requester_email_expands_to_parent_uuid() {
        let conditions = json!({
            "kind": "leaf",
            "field": "ticket.requester.email",
            "op": "ends_with",
            "value": "@vip.example"
        });
        let reads = flat(derive_reads_set(&conditions));
        assert_eq!(
            reads,
            vec!["ticket.requester.email", "ticket.requester_uuid"]
        );
    }

    #[test]
    fn event_and_clock_fields_do_not_count_as_ticket_reads() {
        // These read event / engine metadata, not ticket state.
        // The self-referential linter only cares about state-vs-
        // state intersections.
        let conditions = json!({
            "kind": "and",
            "children": [
                {"kind": "leaf", "field": "event.changed_fields", "op": "has_any", "value": ["status"]},
                {"kind": "leaf", "field": "clock.minutes_since", "op": "gt", "value": 60}
            ]
        });
        assert!(derive_reads_set(&conditions).is_empty());
    }

    #[test]
    fn writes_set_from_set_status_action() {
        let actions = json!([
            { "kind": "set_status", "config": { "workflow_state_id": 7 } }
        ]);
        let writes = flat(derive_writes_set(&actions));
        assert_eq!(
            writes,
            vec![
                "ticket.resolved_at",
                "ticket.updated_at",
                "ticket.workflow_state.category",
                "ticket.workflow_state_id"
            ]
        );
    }

    #[test]
    fn writes_set_dedupes_across_actions() {
        let actions = json!([
            { "kind": "add_tags", "config": { "tag_ids": [1, 2] } },
            { "kind": "remove_tags", "config": { "tag_ids": [3] } }
        ]);
        let writes = flat(derive_writes_set(&actions));
        assert_eq!(writes, vec!["ticket.tag_ids", "ticket.updated_at"]);
    }

    #[test]
    fn reply_action_includes_synthetic_comments_key() {
        let actions = json!([
            { "kind": "reply", "config": { "visibility": "public", "body": "hi" } }
        ]);
        let writes = flat(derive_writes_set(&actions));
        assert!(writes.iter().any(|s| s == "ticket.comments"));
    }

    #[test]
    fn notify_action_has_no_ticket_state_writes() {
        let actions = json!([
            { "kind": "notify", "config": { "recipient": "requester", "subject": "x", "body": "y" } }
        ]);
        assert!(derive_writes_set(&actions).is_empty());
    }

    /// The asset steps write assets and loans and no ticket field, so a
    /// rule that reads the ticket never sees them as writes it reads.
    #[test]
    fn asset_actions_write_asset_keys_only() {
        let writes = |kind: &str| flat(derive_writes_set(&json!([{ "kind": kind }])));
        assert_eq!(writes("assign_assets_to_requester"), vec!["asset.custody"]);
        assert_eq!(writes("lend_assets"), vec!["asset.loan"]);
        assert_eq!(
            writes("return_requester_assets"),
            vec!["asset.custody", "asset.loan"]
        );
    }

    #[test]
    fn stop_processing_action_contributes_nothing() {
        let actions = json!([
            { "kind": "stop_processing" }
        ]);
        assert!(derive_writes_set(&actions).is_empty());
    }
}
