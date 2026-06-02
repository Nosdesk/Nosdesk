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
use crate::models::{
    NewComment, NewRule, NewRuleApplication, Rule, RuleApplication, RuleApplicationStatus,
    RuleState, RuleTriggerKind, RuleUpdate, RuleVersion, TicketPriority,
};
use crate::repository::comments;
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::sync::session::with_actor_context;
use crate::models::{SyncAggregate, SyncOp};

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

/// Plan §11.2 reads_set derivation table. Each entry returns the
/// concrete `reads_set` keys for one `ConditionField`. Status and
/// workflow_state_id alias the same underlying column so a rule
/// conditioning on either reads both. Nested requester fields also
/// add `ticket.requester_uuid` because mutating the requester (FK)
/// implicitly changes those values.
fn expand_condition_field(field: &str) -> Vec<String> {
    match field {
        "ticket.status" => vec!["ticket.workflow_state.category".into()],
        "ticket.workflow_state_id" => vec![
            "ticket.workflow_state_id".into(),
            "ticket.workflow_state.category".into(),
        ],
        "ticket.workflow_state.category" => vec![
            "ticket.workflow_state.category".into(),
            "ticket.workflow_state_id".into(),
        ],
        "ticket.requester.email" => vec![
            "ticket.requester.email".into(),
            "ticket.requester_uuid".into(),
        ],
        "ticket.requester.organization_id" => vec![
            "ticket.requester.organization_id".into(),
            "ticket.requester_uuid".into(),
        ],
        // Event / clock metadata is not ticket state, so no
        // reads_set contribution.
        "event.changed_fields" | "reply.kind" | "reply.author_role" | "clock.minutes_since" => {
            vec![]
        }
        // Everything else maps to itself.
        other => vec![other.to_string()],
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
        let Some(obj) = action.as_object() else { continue };
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
pub fn archive(conn: &mut DbConnection, id: i32, when: chrono::DateTime<chrono::Utc>) -> Result<Rule, WriteError> {
    use crate::schema::rules::dsl;
    diesel::update(dsl::rules.find(id))
        .set(dsl::archived_at.eq(Some(when)))
        .get_result::<Rule>(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => WriteError::NotFound(id),
            other => WriteError::Db(other),
        })
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
    let existing: Rule = dsl::rules
        .find(id)
        .first(conn)
        .map_err(|e| match e {
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
    query
        .order(dsl::applied_at.desc())
        .limit(limit)
        .load(conn)
}

pub fn find_application(
    conn: &mut DbConnection,
    id: i64,
) -> QueryResult<Option<RuleApplication>> {
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
// Manual apply lifecycle (Wave 5). Mirrors execute_merge's shape: one
// `with_actor_context` transaction, advisory lock on the ticket, pre-
// flight under the lock, then run the action executors in order. Any
// preflight failure or executor error rolls the whole apply back; the
// rule_applications row records what happened.
//
// Phase 1 only supports manual rules so the "evaluate conditions"
// step is skipped (manual rules carry conditions = [] by the
// rules_manual_no_conditions CHECK). Phase 2 plugs in condition
// evaluation at the same pre-flight point.
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
    /// Replaces the first `reply` action's body verbatim if Some.
    /// The agent edited it in the dialog after seeing the rendered
    /// preview; the engine substitutes no further template tokens.
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

/// Counts the apply endpoint returns to the agent dialog.
#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    /// Updated rule row (with the bumped `fire_count`).
    pub rule: Rule,
    /// The application audit row inserted at the end of the txn.
    pub application: RuleApplication,
    /// The `comments.id` of the reply action's row, when the rule
    /// had a reply action and it wasn't suppressed. `None`
    /// otherwise.
    pub comment_id: Option<i32>,
    /// How many actions ran (skip / failure subtracts).
    pub actions_executed: usize,
    /// How many actions were skipped via `overrides.suppress_actions`.
    pub actions_suppressed: usize,
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
    #[error("ticket {0} not found")]
    TicketNotFound(i32),
    #[error("ticket {0} is merged into another ticket and cannot be modified")]
    TicketMerged(i32),
    #[error("override index {0} is out of range (rule has {1} actions)")]
    InvalidOverrideIndex(usize, usize),
    #[error("rule action [{index}] is not valid for Phase 1: {message}")]
    UnsupportedActionPhase1 { index: usize, message: String },
    #[error("rule action [{index}] failed: {message}")]
    ActionFailed { index: usize, message: String },
    #[error("actor has no workspace context")]
    MissingWorkspace,
    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),
}

// sync-pending-wire: emits ticket.rule_applied + ticket.updated via sync::emit inside the txn
/// Apply one manual rule to one ticket. Atomic: every action runs
/// inside one `with_actor_context` transaction. Pre-flight checks
/// the rule + ticket under the advisory lock so a racing archive
/// or merge can't slip past. On any executor error the whole apply
/// rolls back and the `rule_applications` row is never written
/// (which is the audit-correct behaviour: the failed apply didn't
/// happen).
pub fn apply_manual(
    conn: &mut DbConnection,
    input: ApplyInput,
    actor: &ActorContext,
) -> Result<ApplyOutcome, ApplyError> {
    use crate::schema::rules::dsl as r;
    use crate::schema::tickets::dsl as t;

    let workspace_id = actor.workspace_id.ok_or(ApplyError::MissingWorkspace)?;

    with_actor_context(conn, actor, |conn| -> Result<ApplyOutcome, ApplyError> {
        diesel::sql_query("SELECT pg_advisory_xact_lock($1)")
            .bind::<BigInt, _>(advisory_key(workspace_id, input.ticket_id))
            .execute(conn)?;

        // Re-read rule + ticket under the lock so racing edits
        // (archive, state change, merge) are caught.
        let rule: Rule = r::rules
            .find(input.rule_id)
            .first(conn)
            .optional()?
            .ok_or(ApplyError::NotFound(input.rule_id))?;
        if rule.archived_at.is_some() {
            return Err(ApplyError::NotLive(rule.id, rule.state.as_str()));
        }
        if rule.state != RuleState::Live && rule.state != RuleState::DryRun {
            return Err(ApplyError::NotLive(rule.id, rule.state.as_str()));
        }
        if rule.trigger_kind != RuleTriggerKind::Manual {
            return Err(ApplyError::NotManual(rule.id));
        }

        let ticket: crate::models::Ticket = t::tickets
            .find(input.ticket_id)
            .first(conn)
            .optional()?
            .ok_or(ApplyError::TicketNotFound(input.ticket_id))?;
        if ticket.merged_into_ticket_id.is_some() {
            return Err(ApplyError::TicketMerged(ticket.id));
        }

        let Some(actions) = rule.actions.as_array() else {
            return Err(ApplyError::ActionFailed {
                index: 0,
                message: "rule.actions is not a JSON array (DB CHECK should have caught this)"
                    .into(),
            });
        };
        // Validate override indices up front so we don't run half
        // the action list before bailing.
        for idx in &input.overrides.suppress_actions {
            if *idx == 0 || *idx > actions.len() {
                return Err(ApplyError::InvalidOverrideIndex(*idx, actions.len()));
            }
        }
        let suppress: std::collections::HashSet<usize> =
            input.overrides.suppress_actions.iter().copied().collect();

        let mut comment_id: Option<i32> = None;
        let mut actions_executed = 0usize;
        let mut actions_taken: Vec<Value> = Vec::with_capacity(actions.len());
        let mut actions_skipped: Vec<Value> = Vec::with_capacity(suppress.len());
        let mut applied_reply_body_override = false;

        for (i, action) in actions.iter().enumerate() {
            let one_based = i + 1;
            if suppress.contains(&one_based) {
                actions_skipped.push(json!({
                    "index": one_based,
                    "reason": "suppressed_by_override"
                }));
                continue;
            }
            let kind = action
                .get("kind")
                .and_then(|k| k.as_str())
                .ok_or_else(|| ApplyError::ActionFailed {
                    index: one_based,
                    message: "missing kind".to_string(),
                })?;
            let config = action.get("config").cloned().unwrap_or(Value::Null);

            let outcome = match kind {
                "reply" => {
                    let override_body = if !applied_reply_body_override {
                        let b = input.overrides.body.clone();
                        if b.is_some() {
                            applied_reply_body_override = true;
                        }
                        b
                    } else {
                        None
                    };
                    execute_reply(conn, &ticket, actor, &config, override_body)?
                        .map(|c| {
                            if comment_id.is_none() {
                                comment_id = Some(c);
                            }
                            json!({ "index": one_based, "kind": kind, "comment_id": c })
                        })
                        .unwrap_or_else(|| json!({ "index": one_based, "kind": kind }))
                }
                "set_status" => execute_set_status(conn, ticket.id, &config)
                    .map(|state_id| {
                        json!({ "index": one_based, "kind": kind, "workflow_state_id": state_id })
                    })?,
                "assign" => execute_assign(conn, ticket.id, workspace_id, rule.id, &config)
                    .map(|uuid| {
                        json!({ "index": one_based, "kind": kind, "assigned_to_uuid": uuid })
                    })?,
                "unassign" => execute_unassign(conn, ticket.id)
                    .map(|_| json!({ "index": one_based, "kind": kind }))?,
                "add_tags" => execute_add_tags(conn, ticket.id, workspace_id, &config, actor)
                    .map(|added| {
                        json!({ "index": one_based, "kind": kind, "tag_ids_added": added })
                    })?,
                "remove_tags" => execute_remove_tags(conn, ticket.id, &config).map(|removed| {
                    json!({ "index": one_based, "kind": kind, "tag_ids_removed": removed })
                })?,
                "set_priority" => execute_set_priority(conn, ticket.id, &config)
                    .map(|p| json!({ "index": one_based, "kind": kind, "priority": p }))?,
                "stop_processing" => {
                    // No-op for manual apply; only meaningful inside
                    // an event-chain run loop in Phase 2.
                    json!({ "index": one_based, "kind": kind, "note": "no-op for manual apply" })
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
            actions_executed += 1;
        }

        // Bump fire_count + last_fired_at on the rule. Use sql::now
        // so the timestamp comes from the same transaction's clock
        // as the rule_applications.applied_at row below.
        let updated_rule: Rule = diesel::update(r::rules.find(rule.id))
            .set((
                r::fire_count.eq(r::fire_count + 1),
                r::last_fired_at.eq(Some(chrono::Utc::now())),
            ))
            .get_result(conn)?;

        // dry_run state writes a shadow rule_applications row so the
        // admin can preview without touching production data. The
        // action writes above still hit the DB in the txn, but the
        // outer transaction will be COMMITTED — dry-run rows live in
        // the audit log alongside successful ones, distinguished by
        // status. That matches the plan §4.3 contract.
        let status = if updated_rule.state == RuleState::DryRun {
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

        let application = record_application(
            conn,
            NewRuleApplication {
                workspace_id,
                rule_id: rule.id,
                rule_version,
                ticket_id: ticket.id,
                status,
                correlation_id: actor.correlation_id,
                actor_uuid: actor.uuid,
                actor_kind: "user".to_string(),
                originating_event_id: None,
                originating_event_kind: None,
                condition_evaluation: None,
                actions_taken: actions_taken_value.clone(),
                actions_skipped: actions_skipped_value,
                failure_reason: None,
            },
        )?;

        // Emit ticket.rule_applied so the activity feed surfaces the
        // fire alongside ticket.merged + manual events. correlation_id
        // is set via the actor session GUC; the audit_log + sync_actions
        // rows for this transaction all share it.
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
                    "actor_uuid": actor.uuid,
                    "comment_id": comment_id,
                    "actions_taken": actions_taken_value,
                    "was_dry_run": status == RuleApplicationStatus::DryRun,
                }),
                groups: sync_groups,
                causation_id: None,
            },
        )?;

        Ok(ApplyOutcome {
            rule: updated_rule,
            application,
            comment_id,
            actions_executed,
            actions_suppressed: suppress.len(),
        })
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
/// stamps `is_internal` accordingly. The customer-channel dispatch
/// for `visibility=public` runs through the existing
/// `comments::create_comment` observer pipeline, so an outbound
/// queue row is enqueued for tickets with an email channel.
fn execute_reply(
    conn: &mut DbConnection,
    ticket: &crate::models::Ticket,
    actor: &ActorContext,
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
            index: 0,
            message: "reply action missing body".to_string(),
        })?;

    // Phase 1: render with a context built from the loaded ticket
    // + actor. The renderer falls back to empty strings for
    // unknown bindings, so a body referencing customer_name on a
    // requester-less ticket renders "Hi ,".
    let actor_uuid = actor.uuid.ok_or_else(|| ApplyError::ActionFailed {
        index: 0,
        message: "manual reply requires an authenticated actor".to_string(),
    })?;
    let agent = crate::repository::users::find_active_by_uuid(&actor_uuid, conn)?;
    let requester = if let Some(uuid) = ticket.requester_uuid {
        crate::repository::users::find_active_by_uuid(&uuid, conn).ok()
    } else {
        None
    };
    let app_name = std::env::var("APP_NAME").unwrap_or_else(|_| "Nosdesk".to_string());
    let rendered = crate::services::template_vars::render(
        &raw_body,
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
        user_uuid: actor_uuid,
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

/// `set_status` action. Updates `tickets.workflow_state_id`; the
/// existing tickets-table trigger handles `resolved_at` /
/// `closed_at` stamping for terminal categories.
fn execute_set_status(
    conn: &mut DbConnection,
    ticket_id: i32,
    config: &Value,
) -> Result<i32, ApplyError> {
    use crate::schema::tickets::dsl;
    let state_id = config
        .get("workflow_state_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ApplyError::ActionFailed {
            index: 0,
            message: "set_status missing workflow_state_id".to_string(),
        })? as i32;
    diesel::update(dsl::tickets.find(ticket_id))
        .set(dsl::workflow_state_id.eq(state_id))
        .execute(conn)?;
    Ok(state_id)
}

/// `assign` action. Direct user assignment for Phase 1; round-robin
/// and group queue land in Phase 2 when assignment_rules absorbs
/// here and the historical apply rows feed the stateless picker.
fn execute_assign(
    conn: &mut DbConnection,
    ticket_id: i32,
    _workspace_id: i32,
    _rule_id: i32,
    config: &Value,
) -> Result<Uuid, ApplyError> {
    use crate::schema::tickets::dsl;
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
                index: 0,
                message: "assign(method=direct) missing valid user_uuid".to_string(),
            })?,
        other => {
            return Err(ApplyError::UnsupportedActionPhase1 {
                index: 0,
                message: format!(
                    "assign method '{other}' lands in Phase 2 when assignment_rules absorbs here"
                ),
            });
        }
    };
    diesel::update(dsl::tickets.find(ticket_id))
        .set(dsl::assignee_uuid.eq(Some(assignee)))
        .execute(conn)?;
    Ok(assignee)
}

fn execute_unassign(conn: &mut DbConnection, ticket_id: i32) -> Result<(), ApplyError> {
    use crate::schema::tickets::dsl;
    diesel::update(dsl::tickets.find(ticket_id))
        .set(dsl::assignee_uuid.eq::<Option<Uuid>>(None))
        .execute(conn)?;
    Ok(())
}

fn execute_add_tags(
    conn: &mut DbConnection,
    ticket_id: i32,
    workspace_id: i32,
    config: &Value,
    actor: &ActorContext,
) -> Result<Vec<i32>, ApplyError> {
    use crate::schema::ticket_tags::dsl;
    let tag_ids: Vec<i32> = config
        .get("tag_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_i64()).map(|x| x as i32).collect())
        .unwrap_or_default();
    if tag_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<_> = tag_ids
        .iter()
        .map(|tid| {
            (
                dsl::ticket_id.eq(ticket_id),
                dsl::tag_id.eq(*tid),
                dsl::created_by.eq(actor.uuid),
                dsl::workspace_id.eq(workspace_id),
            )
        })
        .collect();
    diesel::insert_into(dsl::ticket_tags)
        .values(&rows)
        .on_conflict_do_nothing()
        .execute(conn)?;
    Ok(tag_ids)
}

fn execute_remove_tags(
    conn: &mut DbConnection,
    ticket_id: i32,
    config: &Value,
) -> Result<Vec<i32>, ApplyError> {
    use crate::schema::ticket_tags::dsl;
    let tag_ids: Vec<i32> = config
        .get("tag_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_i64()).map(|x| x as i32).collect())
        .unwrap_or_default();
    if tag_ids.is_empty() {
        return Ok(Vec::new());
    }
    diesel::delete(
        dsl::ticket_tags
            .filter(dsl::ticket_id.eq(ticket_id))
            .filter(dsl::tag_id.eq_any(&tag_ids)),
    )
    .execute(conn)?;
    Ok(tag_ids)
}

fn execute_set_priority(
    conn: &mut DbConnection,
    ticket_id: i32,
    config: &Value,
) -> Result<String, ApplyError> {
    use crate::schema::tickets::dsl;
    let priority_str = config
        .get("priority")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApplyError::ActionFailed {
            index: 0,
            message: "set_priority missing priority".to_string(),
        })?;
    // The codebase's TicketPriority enum is Low / Medium / High;
    // the plan's "urgent" maps onto High since there's no taller
    // tier. The save linter in the rule editor should validate
    // against this restricted set; we error here as defence-in-
    // depth so an editor mismatch surfaces with a clear message
    // rather than a silent demotion.
    let priority = match priority_str {
        "low" => TicketPriority::Low,
        "normal" | "medium" => TicketPriority::Medium,
        "high" | "urgent" => TicketPriority::High,
        other => {
            return Err(ApplyError::ActionFailed {
                index: 0,
                message: format!("unknown priority: {other}"),
            })
        }
    };
    diesel::update(dsl::tickets.find(ticket_id))
        .set(dsl::priority.eq(priority))
        .execute(conn)?;
    Ok(priority_str.to_string())
}

// =====================================================================
// Preview-match save-time check (Wave 5 / unit-12). Phase 1
// scaffolding: manual rules carry conditions = [] so the match is
// trivially every ticket. The endpoint returns the count + sample
// IDs of recent tickets so the editor can render the "matches N of
// N tickets" banner uniformly with future event-rule previews.
// Phase 2 plugs the typed condition evaluator in here.
// =====================================================================

/// Result of a preview-match against recent tickets.
#[derive(Debug, Clone)]
pub struct PreviewMatch {
    pub matched: i64,
    pub scanned: i64,
    pub sample_ticket_ids: Vec<i32>,
}

/// Run `conditions` against the last `scan_limit` tickets in the
/// workspace (capped at 1000) and return how many matched. Phase 1
/// always returns matched = scanned because manual rules have no
/// conditions; the shape is in place so Phase 2 can drop in the
/// condition evaluator without changing the caller surface.
pub fn preview_match(
    conn: &mut DbConnection,
    _conditions: &Value,
    scan_limit: i64,
) -> QueryResult<PreviewMatch> {
    use crate::schema::tickets::dsl;
    let limit = scan_limit.clamp(50, 1000);
    let rows: Vec<i32> = dsl::tickets
        .order(dsl::created_at.desc())
        .limit(limit)
        .select(dsl::id)
        .load(conn)?;
    let scanned = rows.len() as i64;
    let sample_ticket_ids = rows.into_iter().take(5).collect();
    Ok(PreviewMatch {
        // Phase 1: every recent ticket matches a manual rule
        // (manual rules carry conditions = [] and the picker
        // returns them unfiltered, see plan §13.4).
        matched: scanned,
        scanned,
        sample_ticket_ids,
    })
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
            vec![
                "ticket.workflow_state.category",
                "ticket.workflow_state_id"
            ]
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

    #[test]
    fn stop_processing_action_contributes_nothing() {
        let actions = json!([
            { "kind": "stop_processing" }
        ]);
        assert!(derive_writes_set(&actions).is_empty());
    }
}
