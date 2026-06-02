//! HTTP handlers for the rules engine. Phase 1 surface: admin CRUD
//! on the `rules` resource, the recent-applications log, the agent
//! picker endpoint, and the self-referential save linter (a check
//! that runs at create / update time, not a separate endpoint).
//!
//! The apply endpoint (`POST /api/rules/{id}/apply`) lives in Wave
//! 6 alongside the atomic lifecycle in `repository::rules::apply`.
//! Routes are wired in `main.rs` so the literal `/rules/{id}/apply`
//! and `/rules/{id}/state` register before the wildcard
//! `/rules/{id}` PATCH/DELETE per memory note
//! `project_actix_route_shadowing`.
//!
//! Workspace isolation is via RLS + `require_workspace_role`. The
//! repository layer doesn't add explicit `workspace_id = ?` filters
//! to its queries; the session GUC the cookie-auth + workspace
//! middleware sets up handles that uniformly.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::errors;
use crate::middleware::request_context::RequestContext;
use crate::models::{
    NewRule, Rule, RuleApplicationStatus, RuleState, RuleTriggerKind, RuleUpdate, RuleVersion,
    WorkspaceRole,
};
use crate::repository::rules;
use crate::utils::rbac::require_workspace_role;

// =====================================================================
// Request / response DTOs.
// =====================================================================

/// Body of `POST /api/rules`. Only admins call this. `reads_set` /
/// `writes_set` are derived server-side; clients don't send them.
#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    #[serde(default = "empty_object")]
    pub trigger_config: Value,
    #[serde(default = "empty_array")]
    pub conditions: Value,
    pub actions: Value,
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Optional override for the self-referential save linter. The
    /// editor sets this to `true` only when the admin clicks the
    /// "I understand this rule may loop" checkbox. The server still
    /// computes `reads_set` / `writes_set` and stores them; the flag
    /// just changes whether an intersection blocks the save.
    #[serde(default)]
    pub override_self_reference: bool,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}
fn empty_array() -> Value {
    Value::Array(Vec::new())
}
fn default_priority() -> i32 {
    100
}

/// Body of `PUT /api/rules/{id}`. Every field is Option-wrapped so
/// the editor's partial PATCH-equivalent round-trips without
/// clobbering unsent fields. Same `override_self_reference` knob as
/// create.
#[derive(Debug, Deserialize)]
pub struct UpdateRuleRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub trigger_kind: Option<RuleTriggerKind>,
    #[serde(default)]
    pub trigger_config: Option<Value>,
    #[serde(default)]
    pub conditions: Option<Value>,
    #[serde(default)]
    pub actions: Option<Value>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub override_self_reference: bool,
}

/// Body of `PATCH /api/rules/{id}/state`. The state machine is
/// validated server-side; illegal transitions return 409 with the
/// `RULE_STATE_TRANSITION` code so the editor can surface the
/// allowed next states.
#[derive(Debug, Deserialize)]
pub struct StateTransitionRequest {
    pub state: RuleState,
}

/// Filters for `GET /api/rules`. Pulled from query string.
#[derive(Debug, Default, Deserialize)]
pub struct ListRulesQuery {
    pub trigger_kind: Option<RuleTriggerKind>,
    pub state: Option<RuleState>,
    pub q: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
}

/// Filters for `GET /api/rule-applications`. Date bounds are
/// optional RFC-3339 strings; the repo converts to `chrono`.
#[derive(Debug, Default, Deserialize)]
pub struct ListApplicationsQuery {
    pub rule_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub status: Option<RuleApplicationStatus>,
    pub actor_uuid: Option<Uuid>,
    pub from: Option<chrono::DateTime<Utc>>,
    pub to: Option<chrono::DateTime<Utc>>,
    #[serde(default = "default_application_limit")]
    pub limit: i64,
}

fn default_application_limit() -> i64 {
    50
}

/// Query-string knob on `DELETE /api/rules/{id}`. `?hard=true`
/// permanently deletes; otherwise the row is soft-archived (sets
/// `archived_at`). Decision 32 in the plan: hard delete is only
/// permitted on a row that's already archived.
#[derive(Debug, Default, Deserialize)]
pub struct DeleteRuleQuery {
    #[serde(default)]
    pub hard: bool,
}

/// `Rule` as it appears in API responses. Strips the
/// `Vec<Option<String>>` schema-boundary on `reads_set` /
/// `writes_set` (Diesel's nullable-elements view) into a flat
/// `Vec<String>` for clients.
#[derive(Debug, Serialize)]
pub struct RuleDto {
    pub id: i32,
    pub workspace_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: Value,
    pub conditions: Value,
    pub actions: Value,
    pub reads_set: Vec<String>,
    pub writes_set: Vec<String>,
    pub state: RuleState,
    pub priority: i32,
    pub last_fired_at: Option<chrono::DateTime<Utc>>,
    pub fire_count: i32,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub archived_at: Option<chrono::DateTime<Utc>>,
}

impl From<Rule> for RuleDto {
    fn from(r: Rule) -> Self {
        Self {
            id: r.id,
            workspace_id: r.workspace_id,
            name: r.name,
            description: r.description,
            trigger_kind: r.trigger_kind,
            trigger_config: r.trigger_config,
            conditions: r.conditions,
            actions: r.actions,
            reads_set: r.reads_set.into_iter().flatten().collect(),
            writes_set: r.writes_set.into_iter().flatten().collect(),
            state: r.state,
            priority: r.priority,
            last_fired_at: r.last_fired_at,
            fire_count: r.fire_count,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
            archived_at: r.archived_at,
        }
    }
}

/// `RuleVersion` as it appears in API responses. Same flat shape as
/// `RuleDto` for the columns it shares; `reads_set` / `writes_set`
/// aren't on `rule_versions` because the migration's trigger only
/// snapshots the user-facing fields.
#[derive(Debug, Serialize)]
pub struct RuleVersionDto {
    pub id: i32,
    pub rule_id: i32,
    pub workspace_id: i32,
    pub version: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: Value,
    pub conditions: Value,
    pub actions: Value,
    pub state: RuleState,
    pub priority: i32,
    pub saved_by: Option<Uuid>,
    pub saved_at: chrono::DateTime<Utc>,
}

impl From<RuleVersion> for RuleVersionDto {
    fn from(v: RuleVersion) -> Self {
        Self {
            id: v.id,
            rule_id: v.rule_id,
            workspace_id: v.workspace_id,
            version: v.version,
            name: v.name,
            description: v.description,
            trigger_kind: v.trigger_kind,
            trigger_config: v.trigger_config,
            conditions: v.conditions,
            actions: v.actions,
            state: v.state,
            priority: v.priority,
            saved_by: v.saved_by,
            saved_at: v.saved_at,
        }
    }
}

// =====================================================================
// Self-referential save linter (decision 8 in the plan / unit-11).
// Lives in this module because it's a save-time HTTP-layer check, not
// a repository invariant.
// =====================================================================

#[derive(Debug)]
struct SelfReferentialError {
    fields: Vec<String>,
}

/// Returns `Err` when the rule's `reads_set ∩ writes_set ≠ ∅` AND
/// the caller didn't opt out via `override_self_reference`. The
/// repository's `derive_*` helpers produce `Vec<Option<String>>`
/// because the column is `Array<Nullable<Text>>`; we strip the
/// `Option` wrapper at this boundary.
fn check_self_referential(
    reads: &[Option<String>],
    writes: &[Option<String>],
    override_flag: bool,
) -> Result<(), SelfReferentialError> {
    if override_flag {
        return Ok(());
    }
    let writes_set: std::collections::HashSet<&str> = writes
        .iter()
        .filter_map(|w| w.as_deref())
        .collect();
    let overlap: Vec<String> = reads
        .iter()
        .filter_map(|r| r.as_deref())
        .filter(|r| writes_set.contains(r))
        .map(|s| s.to_string())
        .collect();
    if overlap.is_empty() {
        Ok(())
    } else {
        Err(SelfReferentialError { fields: overlap })
    }
}

// =====================================================================
// Helpers.
// =====================================================================

fn actor_workspace_id(req: &HttpRequest) -> Option<i32> {
    req.extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.workspace_id)
        .unwrap_or(None)
}

fn actor_uuid(req: &HttpRequest) -> Option<Uuid> {
    req.extensions()
        .get::<RequestContext>()
        .and_then(|c| c.actor.uuid)
}

/// Cheap "this transition is one of the legal ones" check. The
/// state machine: draft ↔ dry_run ↔ live → archived. From archived
/// the row stays put; restore-to-draft would be a separate
/// endpoint when we add it (out of scope for Phase 1 per decision
/// 32).
fn legal_state_transition(from: RuleState, to: RuleState) -> bool {
    use RuleState::*;
    matches!(
        (from, to),
        (Draft, DryRun)
            | (Draft, Live)
            | (Draft, Archived)
            | (DryRun, Live)
            | (DryRun, Draft)
            | (DryRun, Archived)
            | (Live, DryRun)
            | (Live, Archived)
    )
}

// =====================================================================
// Handlers.
// =====================================================================

/// `POST /api/rules` (admin). Create a new rule. Manual rules
/// require `conditions = []`; the `rules_manual_no_conditions`
/// CHECK enforces this at the DB but we early-reject here with a
/// clearer error.
pub async fn create_rule(
    req: HttpRequest,
    body: web::Json<CreateRuleRequest>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let Some(workspace_id) = actor_workspace_id(&req) else {
        return errors::unauthorized("Authentication required");
    };
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let CreateRuleRequest {
        name,
        description,
        trigger_kind,
        trigger_config,
        conditions,
        actions,
        priority,
        override_self_reference,
    } = body.into_inner();

    if name.trim().is_empty() {
        return errors::bad_request_with_code("name must not be empty", "RULE_VALIDATION");
    }
    if let Err(msg) = validate_actions(&actions) {
        return errors::bad_request_with_code(msg, "RULE_VALIDATION");
    }
    if trigger_kind == RuleTriggerKind::Manual && !is_empty_conditions(&conditions) {
        return errors::bad_request_with_code(
            "manual rules must have an empty conditions list",
            "RULE_VALIDATION",
        );
    }

    let reads = rules::derive_reads_set(&conditions);
    let writes = rules::derive_writes_set(&actions);

    if let Err(e) = check_self_referential(&reads, &writes, override_self_reference) {
        return errors::conflict_with_code(
            format!(
                "rule reads and writes the same fields: {} (set override_self_reference=true to bypass)",
                e.fields.join(", ")
            ),
            "RULE_SELF_REFERENTIAL",
        );
    }

    let new = NewRule {
        workspace_id,
        name,
        description,
        trigger_kind,
        trigger_config,
        conditions,
        actions,
        // derive_* output is what gets persisted; the repo's
        // create() recomputes from the JSONB trees so passing it
        // again here is redundant but explicit.
        reads_set: reads,
        writes_set: writes,
        state: RuleState::Draft,
        priority,
        created_by: actor_uuid(&req),
    };

    match rules::create(&mut conn, new) {
        Ok(rule) => HttpResponse::Created().json(RuleDto::from(rule)),
        Err(e) => {
            tracing::error!(error = ?e, "create_rule: repo error");
            errors::db_error(&e)
        }
    }
}

/// `GET /api/rules` (admin). List rules in the caller's workspace.
pub async fn list_rules(
    req: HttpRequest,
    query: web::Query<ListRulesQuery>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let q = query.into_inner();
    let filter = rules::ListFilter {
        trigger_kind: q.trigger_kind,
        state: q.state,
        include_archived: q.include_archived,
        name_query: q.q,
    };
    match rules::list(&mut conn, filter) {
        Ok(rs) => HttpResponse::Ok().json(rs.into_iter().map(RuleDto::from).collect::<Vec<_>>()),
        Err(e) => errors::db_error(&e),
    }
}

/// `GET /api/rules/{id}` (admin). Single rule.
pub async fn get_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match rules::find(&mut conn, id) {
        Ok(Some(rule)) => HttpResponse::Ok().json(RuleDto::from(rule)),
        Ok(None) => errors::not_found_msg(format!("rule {id} not found")),
        Err(e) => errors::db_error(&e),
    }
}

/// `PUT /api/rules/{id}` (admin). Apply a partial update; the
/// migration's BEFORE UPDATE trigger writes a `rule_versions` row
/// and bumps `updated_at`. `reads_set` / `writes_set` are
/// recomputed by the repo when `conditions` or `actions` move.
pub async fn update_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<UpdateRuleRequest>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let UpdateRuleRequest {
        name,
        description,
        trigger_kind,
        trigger_config,
        conditions,
        actions,
        priority,
        override_self_reference,
    } = body.into_inner();

    if let Some(ref n) = name {
        if n.trim().is_empty() {
            return errors::bad_request_with_code("name must not be empty", "RULE_VALIDATION");
        }
    }
    if let Some(ref a) = actions {
        if let Err(msg) = validate_actions(a) {
            return errors::bad_request_with_code(msg, "RULE_VALIDATION");
        }
    }

    // Self-ref check needs the post-update conditions / actions, which
    // means reading the existing row first if either is omitted. Same
    // pattern the merge handler uses for optimistic-lock fetches.
    let existing = match rules::find(&mut conn, id) {
        Ok(Some(r)) => r,
        Ok(None) => return errors::not_found_msg(format!("rule {id} not found")),
        Err(e) => return errors::db_error(&e),
    };
    let effective_conditions = conditions.clone().unwrap_or(existing.conditions);
    let effective_actions = actions.clone().unwrap_or(existing.actions);
    let effective_trigger_kind = trigger_kind.unwrap_or(existing.trigger_kind);
    if effective_trigger_kind == RuleTriggerKind::Manual
        && !is_empty_conditions(&effective_conditions)
    {
        return errors::bad_request_with_code(
            "manual rules must have an empty conditions list",
            "RULE_VALIDATION",
        );
    }
    let reads = rules::derive_reads_set(&effective_conditions);
    let writes = rules::derive_writes_set(&effective_actions);
    if let Err(e) = check_self_referential(&reads, &writes, override_self_reference) {
        return errors::conflict_with_code(
            format!(
                "rule reads and writes the same fields: {} (set override_self_reference=true to bypass)",
                e.fields.join(", ")
            ),
            "RULE_SELF_REFERENTIAL",
        );
    }

    let change = RuleUpdate {
        name,
        description,
        trigger_kind,
        trigger_config,
        conditions,
        actions,
        reads_set: None,
        writes_set: None,
        state: None,
        priority,
        last_fired_at: None,
        fire_count: None,
        archived_at: None,
    };

    match rules::update(&mut conn, id, change) {
        Ok(rule) => HttpResponse::Ok().json(RuleDto::from(rule)),
        Err(rules::WriteError::NotFound(_)) => {
            errors::not_found_msg(format!("rule {id} not found"))
        }
        Err(rules::WriteError::Db(e)) => errors::db_error(&e),
        Err(rules::WriteError::NotArchived(_)) => {
            // Not reachable from update(); only hard_delete returns
            // NotArchived. Defensive 500 if it ever leaks.
            errors::internal("unexpected NotArchived from update")
        }
    }
}

/// `PATCH /api/rules/{id}/state` (admin). State transition with
/// the legal-transition table; everything else returns 409
/// `RULE_STATE_TRANSITION`.
pub async fn transition_state(
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<StateTransitionRequest>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let target = body.into_inner().state;

    let existing = match rules::find(&mut conn, id) {
        Ok(Some(r)) => r,
        Ok(None) => return errors::not_found_msg(format!("rule {id} not found")),
        Err(e) => return errors::db_error(&e),
    };
    if existing.state == target {
        return HttpResponse::Ok().json(RuleDto::from(existing));
    }
    if !legal_state_transition(existing.state, target) {
        return errors::conflict_with_code(
            format!(
                "cannot transition rule from {} to {}",
                existing.state.as_str(),
                target.as_str()
            ),
            "RULE_STATE_TRANSITION",
        );
    }
    // Archive transitions stamp archived_at so the
    // archived-vs-live filter in list_pickable_manual stays
    // consistent with the state column. The reverse (un-archive)
    // isn't a v1 transition; out-of-scope per decision 32.
    let archived_at = match target {
        RuleState::Archived => Some(Some(Utc::now())),
        _ => None,
    };
    let change = RuleUpdate {
        state: Some(target),
        archived_at,
        ..Default::default()
    };
    match rules::update(&mut conn, id, change) {
        Ok(rule) => HttpResponse::Ok().json(RuleDto::from(rule)),
        Err(rules::WriteError::NotFound(_)) => {
            errors::not_found_msg(format!("rule {id} not found"))
        }
        Err(rules::WriteError::Db(e)) => errors::db_error(&e),
        Err(rules::WriteError::NotArchived(_)) => {
            errors::internal("unexpected NotArchived from state transition")
        }
    }
}

/// `DELETE /api/rules/{id}` (admin). Soft-archive by default;
/// `?hard=true` permanently deletes but only if `archived_at` is
/// already set. Returns 204 on hard delete, 200 with the archived
/// rule on soft.
pub async fn delete_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    query: web::Query<DeleteRuleQuery>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    if query.hard {
        match rules::hard_delete(&mut conn, id) {
            Ok(()) => HttpResponse::NoContent().finish(),
            Err(rules::WriteError::NotFound(_)) => {
                errors::not_found_msg(format!("rule {id} not found"))
            }
            Err(rules::WriteError::NotArchived(_)) => errors::conflict_with_code(
                "rule must be archived before hard delete",
                "RULE_NOT_ARCHIVED",
            ),
            Err(rules::WriteError::Db(e)) => errors::db_error(&e),
        }
    } else {
        match rules::archive(&mut conn, id, Utc::now()) {
            Ok(rule) => HttpResponse::Ok().json(RuleDto::from(rule)),
            Err(rules::WriteError::NotFound(_)) => {
                errors::not_found_msg(format!("rule {id} not found"))
            }
            Err(rules::WriteError::Db(e)) => errors::db_error(&e),
            Err(rules::WriteError::NotArchived(_)) => {
                errors::internal("unexpected NotArchived from archive")
            }
        }
    }
}

/// `GET /api/rules/{id}/versions` (admin). The activity-feed
/// inspector deep-links into a specific version; the editor's
/// "version history" sidebar reads this.
pub async fn list_rule_versions(
    req: HttpRequest,
    path: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let rule_id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match rules::list_versions(&mut conn, rule_id) {
        Ok(rs) => {
            HttpResponse::Ok().json(rs.into_iter().map(RuleVersionDto::from).collect::<Vec<_>>())
        }
        Err(e) => errors::db_error(&e),
    }
}

/// `GET /api/rules/{id}/versions/{version}` (admin). The single-
/// version snapshot the inspector renders.
pub async fn get_rule_version(
    req: HttpRequest,
    path: web::Path<(i32, i32)>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let (rule_id, version) = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match rules::find_version(&mut conn, rule_id, version) {
        Ok(Some(v)) => HttpResponse::Ok().json(RuleVersionDto::from(v)),
        Ok(None) => errors::not_found_msg(format!(
            "rule {rule_id} version {version} not found"
        )),
        Err(e) => errors::db_error(&e),
    }
}

// =====================================================================
// Recent-activity log (unit-09).
// =====================================================================

/// `GET /api/rule-applications` (admin). The workspace-wide
/// recent-activity feed. Default page size 50, max 500 (the repo
/// clamps).
pub async fn list_rule_applications(
    req: HttpRequest,
    query: web::Query<ListApplicationsQuery>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let q = query.into_inner();
    let filter = rules::ApplicationFilter {
        rule_id: q.rule_id,
        ticket_id: q.ticket_id,
        status: q.status,
        actor_uuid: q.actor_uuid,
        from: q.from,
        to: q.to,
        limit: q.limit,
    };
    match rules::list_applications(&mut conn, filter) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => errors::db_error(&e),
    }
}

/// `GET /api/rule-applications/{id}` (admin). Full row with the
/// `condition_evaluation` / `actions_taken` / `actions_skipped` /
/// `failure_reason` payloads the inspector renders.
pub async fn get_rule_application(
    req: HttpRequest,
    path: web::Path<i64>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match rules::find_application(&mut conn, id) {
        Ok(Some(row)) => HttpResponse::Ok().json(row),
        Ok(None) => errors::not_found_msg(format!("rule_application {id} not found")),
        Err(e) => errors::db_error(&e),
    }
}

/// `GET /api/tickets/{ticket_id}/rule-applications` (agent). The
/// per-ticket activity slice the open-ticket-view's history panel
/// reads. Agent-readable so the agent can see "rules that fired on
/// this ticket" without needing admin access to the workspace-wide
/// log.
pub async fn list_ticket_rule_applications(
    req: HttpRequest,
    path: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }
    let ticket_id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match rules::list_applications_for_ticket(&mut conn, ticket_id) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => errors::db_error(&e),
    }
}

// =====================================================================
// Manual apply (Wave 6 / unit-08). Agent role; the lifecycle in
// repository::rules::apply_manual owns the transaction.
// =====================================================================

#[derive(Debug, Deserialize)]
pub struct ApplyRuleRequest {
    pub ticket_id: i32,
    #[serde(default)]
    pub overrides: ApplyRuleOverrides,
}

#[derive(Debug, Default, Deserialize)]
pub struct ApplyRuleOverrides {
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub suppress_actions: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub struct ApplyRuleResponse {
    pub rule: RuleDto,
    pub application_id: i64,
    pub correlation_id: Option<Uuid>,
    pub comment_id: Option<i32>,
    pub actions_executed: usize,
    pub actions_suppressed: usize,
}

/// `POST /api/rules/{id}/apply`. Agent role minimum. The route is
/// registered before the wildcard `/rules/{id}` PUT/DELETE in
/// main.rs to dodge actix's route-shadowing footgun.
///
/// Post-commit side-effects (channel relay for public replies,
/// SSE field broadcasts) live in this handler, not in the apply
/// lifecycle, so a slow downstream cannot roll back a committed
/// rule fire. Errors here are logged and swallowed.
pub async fn apply_rule(
    req: HttpRequest,
    path: web::Path<i32>,
    body: web::Json<ApplyRuleRequest>,
    pool: web::Data<Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }
    let Some(actor) = req
        .extensions()
        .get::<RequestContext>()
        .map(|c| c.actor.clone())
    else {
        return errors::unauthorized("Authentication required");
    };
    let rule_id = path.into_inner();
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let body = body.into_inner();
    let ticket_id = body.ticket_id;
    let input = rules::ApplyInput {
        rule_id,
        ticket_id,
        overrides: rules::ApplyOverrides {
            body: body.overrides.body,
            suppress_actions: body.overrides.suppress_actions,
        },
    };

    let outcome = match rules::apply_manual(&mut conn, input, &actor) {
        Ok(o) => o,
        Err(e) => return map_apply_error(e),
    };

    // Channel relay: when the apply produced a public-visibility
    // reply, enqueue the outbound message the same way the regular
    // comment-create handler does. enqueue_for_comment spawns a
    // background task, so the response returns immediately. Internal
    // notes never relay.
    if let Some(cid) = outcome.comment_id {
        if let Err(e) =
            dispatch_rule_reply_for_relay(&pool, ticket_id, cid, &outcome.application).await
        {
            tracing::warn!(error = %e, comment_id = cid, "rule reply channel relay enqueue failed");
        }
    }

    // SSE: broadcast field names the frontend's useTicketSSE
    // composable already handles. workflow_state_id needs to
    // resolve to the state's category string (the frontend's
    // ticket.status); assignee_uuid maps to the 'assignee' key.
    // Tag changes have no matching per-field handler today; the
    // ticket.rule_applied sync event drives activity-feed refresh,
    // which is the surface that needs to update for tag changes.
    let actor_uuid_str = actor.uuid.map(|u| u.to_string()).unwrap_or_default();
    if let Some(taken) = outcome
        .application
        .actions_taken
        .as_ref()
        .and_then(|v| v.as_array())
    {
        for action in taken {
            let Some(kind) = action.get("kind").and_then(|k| k.as_str()) else {
                continue;
            };
            let (field, value) = match kind {
                "set_status" => {
                    let state_id = action
                        .get("workflow_state_id")
                        .and_then(|v| v.as_i64())
                        .map(|i| i as i32);
                    let category = state_id
                        .and_then(|id| resolve_workflow_state_category(&pool, id).ok())
                        .unwrap_or_default();
                    ("status", Value::String(category))
                }
                "assign" => (
                    "assignee",
                    action.get("assigned_to_uuid").cloned().unwrap_or(Value::Null),
                ),
                "unassign" => ("assignee", Value::Null),
                "set_priority" => (
                    "priority",
                    action.get("priority").cloned().unwrap_or(Value::Null),
                ),
                // add_tags / remove_tags / reply / stop_processing
                // have no useTicketSSE handler today; activity feed
                // refresh via ticket.rule_applied covers them.
                _ => continue,
            };
            sse_state
                .broadcast_event(crate::handlers::sse::SseEvent::TicketUpdated {
                    ticket_id,
                    field: field.to_string(),
                    value,
                    updated_by: actor_uuid_str.clone(),
                    timestamp: chrono::Utc::now(),
                })
                .await;
        }
    }

    HttpResponse::Ok().json(ApplyRuleResponse {
        rule: RuleDto::from(outcome.rule),
        application_id: outcome.application.id,
        correlation_id: outcome.application.correlation_id,
        comment_id: outcome.comment_id,
        actions_executed: outcome.actions_executed,
        actions_suppressed: outcome.actions_suppressed,
    })
}

/// Resolve a workflow_state row's category string for the SSE
/// payload. Best-effort: a missing row returns the empty string,
/// the frontend's ticket.status setter accepts that as "unknown"
/// and falls back to a refetch on the next interaction. Done
/// outside the apply transaction so RLS-friendliness comes from
/// the regular pool acquire.
fn resolve_workflow_state_category(
    pool: &web::Data<Pool>,
    state_id: i32,
) -> Result<String, diesel::result::Error> {
    use crate::models::WorkflowStateCategory;
    use crate::schema::workflow_states::dsl as ws;
    use diesel::prelude::*;
    let mut conn = pool
        .get()
        .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
    let category: WorkflowStateCategory = ws::workflow_states
        .find(state_id)
        .select(ws::category)
        .first(&mut conn)?;
    Ok(category.as_str().to_string())
}

/// Mirror the comment-handler's channel-relay enqueue for a rule-
/// driven public reply. Loads the ticket + comment fresh from the
/// pool so the spawn body owns them; the apply transaction has
/// already committed by this point, so the rows are stable.
async fn dispatch_rule_reply_for_relay(
    pool: &web::Data<Pool>,
    ticket_id: i32,
    comment_id: i32,
    application: &crate::models::RuleApplication,
) -> Result<(), String> {
    // Internal notes do not relay; reply visibility lives on the
    // action's channel_metadata blob. Re-read the comment so we
    // see the canonical row exactly as it was stored.
    use crate::schema::comments::dsl as c;
    use crate::schema::tickets::dsl as t;
    use diesel::prelude::*;
    let mut conn = pool.get().map_err(|e| e.to_string())?;
    let comment: crate::models::Comment = c::comments
        .find(comment_id)
        .first(&mut conn)
        .map_err(|e| e.to_string())?;
    if comment.is_internal {
        return Ok(());
    }
    // The action's channel_metadata kind=="rule_reply" carries the
    // visibility we used at apply time. Public + non-internal is
    // the relay condition; anything else skips silently.
    let visibility_public = comment
        .channel_metadata
        .as_ref()
        .and_then(|m| m.get("visibility"))
        .and_then(|v| v.as_str())
        .map(|v| v == "public")
        .unwrap_or(false);
    if !visibility_public {
        return Ok(());
    }
    let ticket: crate::models::Ticket = t::tickets
        .find(ticket_id)
        .first(&mut conn)
        .map_err(|e| e.to_string())?;
    // Spawn — same shape as the regular comment handler. The
    // outbound queue worker handles SMTP dispatch + retry.
    crate::services::channels::outbound::enqueue_for_comment(
        ticket,
        comment,
        pool.get_ref().clone(),
    );
    tracing::debug!(
        application_id = application.id,
        comment_id,
        "rule reply enqueued for channel relay"
    );
    Ok(())
}

fn map_apply_error(err: rules::ApplyError) -> HttpResponse {
    let message = err.to_string();
    use rules::ApplyError::*;
    match err {
        NotFound(_) | TicketNotFound(_) => errors::not_found_msg(message),
        NotLive(..) => errors::conflict_with_code(message, "RULE_NOT_LIVE"),
        NotManual(_) => errors::bad_request_with_code(message, "RULE_NOT_MANUAL"),
        TicketMerged(_) => errors::bad_request_with_code(message, "RULE_TICKET_MERGED"),
        InvalidOverrideIndex(..) => errors::bad_request_with_code(message, "MANUAL_APPLY_VALIDATION"),
        UnsupportedActionPhase1 { .. } => {
            errors::bad_request_with_code(message, "RULE_ACTION_UNSUPPORTED")
        }
        ActionFailed { .. } => errors::internal_with_code(message, "RULE_ACTION_FAILED"),
        // Soft-deleted / revoked agents land here distinctly from a
        // transient DB error so the frontend can clear stale auth
        // state and prompt for re-login instead of showing a generic
        // 500.
        AgentRevoked(_) => errors::unauthorized_with_code(message, "ACTOR_REVOKED"),
        MissingWorkspace => errors::internal("missing workspace context"),
        Db(e) => errors::db_error(&e),
    }
}

// =====================================================================
// Starter catalog (Wave 8 / unit-19). Admin-only browse of the
// pre-built rules baked into the binary.
// =====================================================================

#[derive(Debug, Serialize)]
pub struct StarterRuleDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger_kind: String,
    pub conditions: Value,
    pub actions: Value,
}

/// `GET /api/rules/starter-catalog`. Admin only. Returns the
/// localised catalog the rules-page "Browse starters" affordance
/// renders. Locale comes from `Accept-Language`; falls back to
/// English when the requested locale isn't represented.
pub async fn list_starter_catalog(req: HttpRequest) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let locale = req
        .headers()
        .get(actix_web::http::header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "en".to_string());

    let dtos: Vec<StarterRuleDto> = crate::services::starter_catalog::list()
        .iter()
        .map(|r| StarterRuleDto {
            id: r.id.clone(),
            name: r.name_for(&locale).to_string(),
            description: r.description_for(&locale).to_string(),
            trigger_kind: r.trigger_kind.clone(),
            conditions: r.conditions.clone(),
            actions: r.actions.clone(),
        })
        .collect();
    HttpResponse::Ok().json(dtos)
}

// =====================================================================
// Agent picker (unit-10).
// =====================================================================

/// `GET /api/tickets/{id}/applicable-actions` (Agent role minimum).
/// Returns the live manual rules in the caller's workspace. Per
/// decision 30, manual rules have empty conditions so we don't
/// per-ticket-condition-evaluate; the picker is category-organised
/// in the UI, but the endpoint returns the unfiltered manual-rules
/// list and the frontend groups it.
pub async fn list_applicable_actions(
    req: HttpRequest,
    _path: web::Path<i32>,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }
    let mut conn = match errors::db_conn(&pool) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    match rules::list_pickable_manual(&mut conn) {
        Ok(rs) => HttpResponse::Ok().json(rs.into_iter().map(RuleDto::from).collect::<Vec<_>>()),
        Err(e) => errors::db_error(&e),
    }
}

// =====================================================================
// Local validation helpers used by create / update.
// =====================================================================

/// Surface-level sanity check on the actions JSONB. Full per-action
/// config validation lives with the executor in Wave 5; here we
/// just confirm the array shape and that every element has a
/// recognised `kind` so the editor's typo'd kind doesn't reach the
/// engine.
fn validate_actions(actions: &Value) -> Result<(), String> {
    let Some(arr) = actions.as_array() else {
        return Err("actions must be a JSON array".to_string());
    };
    if arr.is_empty() {
        return Err("actions must contain at least one entry".to_string());
    }
    const KNOWN: &[&str] = &[
        "reply",
        "set_status",
        "assign",
        "unassign",
        "add_tags",
        "remove_tags",
        "set_priority",
        "notify",
        "apply_macro_template",
        "webhook",
        "stop_processing",
    ];
    for (i, action) in arr.iter().enumerate() {
        let Some(obj) = action.as_object() else {
            return Err(format!("actions[{i}] must be a JSON object"));
        };
        let kind = obj
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| format!("actions[{i}] missing kind"))?;
        if !KNOWN.contains(&kind) {
            return Err(format!("actions[{i}] has unknown kind: {kind}"));
        }
    }
    Ok(())
}

fn is_empty_conditions(value: &Value) -> bool {
    matches!(value, Value::Array(a) if a.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_transitions_match_state_machine() {
        use RuleState::*;
        assert!(legal_state_transition(Draft, DryRun));
        assert!(legal_state_transition(DryRun, Live));
        assert!(legal_state_transition(Live, DryRun));
        assert!(legal_state_transition(Live, Archived));
        // Skip-to-Live ladder for the "I verified manually" admin
        // path is the same legal_state_transition, with the
        // editor-side checkbox being the gate.
        assert!(legal_state_transition(Draft, Live));
        // Unarchive isn't a Phase 1 transition.
        assert!(!legal_state_transition(Archived, Draft));
        assert!(!legal_state_transition(Archived, Live));
        assert!(!legal_state_transition(Archived, DryRun));
    }

    #[test]
    fn self_ref_check_flags_overlap() {
        let reads = vec![Some("ticket.priority".to_string())];
        let writes = vec![Some("ticket.priority".to_string()), Some("ticket.updated_at".to_string())];
        let err = check_self_referential(&reads, &writes, false).unwrap_err();
        assert_eq!(err.fields, vec!["ticket.priority"]);
    }

    #[test]
    fn self_ref_check_passes_with_override() {
        let reads = vec![Some("ticket.priority".to_string())];
        let writes = vec![Some("ticket.priority".to_string())];
        assert!(check_self_referential(&reads, &writes, true).is_ok());
    }

    #[test]
    fn self_ref_check_passes_when_disjoint() {
        let reads = vec![Some("ticket.title".to_string())];
        let writes = vec![Some("ticket.priority".to_string())];
        assert!(check_self_referential(&reads, &writes, false).is_ok());
    }

    #[test]
    fn validate_actions_rejects_empty_array() {
        let actions = serde_json::json!([]);
        assert!(validate_actions(&actions).is_err());
    }

    #[test]
    fn validate_actions_rejects_unknown_kind() {
        let actions = serde_json::json!([{ "kind": "obliterate" }]);
        let err = validate_actions(&actions).unwrap_err();
        assert!(err.contains("unknown kind"));
    }

    #[test]
    fn validate_actions_accepts_known_kinds() {
        let actions = serde_json::json!([
            { "kind": "reply", "config": { "visibility": "public", "body": "hi" } },
            { "kind": "stop_processing" }
        ]);
        assert!(validate_actions(&actions).is_ok());
    }

    #[test]
    fn is_empty_conditions_matches_array_only() {
        assert!(is_empty_conditions(&serde_json::json!([])));
        assert!(!is_empty_conditions(&serde_json::json!({})));
        assert!(!is_empty_conditions(&serde_json::json!({"kind": "leaf"})));
        assert!(!is_empty_conditions(&serde_json::json!([{"x": 1}])));
    }
}
