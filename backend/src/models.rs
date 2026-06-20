// models.rs
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
// Removed unused import: use diesel::sql_types::Text;
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

// Simple UUID serialization helpers
fn serialize_optional_uuid_as_string<S>(
    uuid: &Option<Uuid>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&uuid.map(|u| u.to_string()).unwrap_or_default())
}

/// Fixed system-level workflow categories. The category vocabulary is the
/// stable contract that downstream code reasons in (SLA timers, dashboard
/// rollups, automation triggers); the user-visible state names live on
/// `workflow_states` and can be customised per workspace.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::WorkflowStateCategory)]
pub enum WorkflowStateCategory {
    #[serde(rename = "triage")]
    Triage,
    #[serde(rename = "backlog")]
    Backlog,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "in_review")]
    InReview,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "cancelled")]
    Cancelled,
    /// Terminal state for a ticket consumed by a merge. Distinct from
    /// `done` (resolved) and `cancelled` so list filters and the
    /// activity feed can tell "closed because merged" apart from
    /// "closed because finished". Pauses SLA via the per-row
    /// `pauses_sla` flag, the same as the other terminal categories.
    #[serde(rename = "merged")]
    Merged,
}

impl WorkflowStateCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowStateCategory::Triage => "triage",
            WorkflowStateCategory::Backlog => "backlog",
            WorkflowStateCategory::Active => "active",
            WorkflowStateCategory::InReview => "in_review",
            WorkflowStateCategory::Done => "done",
            WorkflowStateCategory::Cancelled => "cancelled",
            WorkflowStateCategory::Merged => "merged",
        }
    }

    /// Terminal categories don't transition further on their own. Used by
    /// SLA, rollup, and metric code that needs a "is this work finished?"
    /// answer without enumerating every named state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Cancelled | Self::Merged)
    }
}

impl ToSql<crate::schema::sql_types::WorkflowStateCategory, Pg> for WorkflowStateCategory {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::WorkflowStateCategory, Pg> for WorkflowStateCategory {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"triage" => Ok(Self::Triage),
            b"backlog" => Ok(Self::Backlog),
            b"active" => Ok(Self::Active),
            b"in_review" => Ok(Self::InReview),
            b"done" => Ok(Self::Done),
            b"cancelled" => Ok(Self::Cancelled),
            b"merged" => Ok(Self::Merged),
            other => Err(format!(
                "unknown workflow_state_category: {}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

/// Saved view: a per-user / per-project / workspace-wide preset
/// bundling a `ViewShape` and `FilterState`. The two JSON columns
/// are validated client-side; the server treats them as opaque so
/// plugin-defined view shapes round-trip without a wire change.
///
/// History note: earlier revisions carried `is_default` (with a
/// partial unique index enforcing one per scope) and `archived_at`
/// (soft delete). Both dropped 2026-05-09 in favour of: the single
/// built-in `MY_OPEN_VIEW` fallback for "what shows by default,"
/// and hard `DELETE` for "delete a view." Neither column had a
/// user-facing surface (no admin UI to set the default; no archived-
/// views browser or restore flow), and the `is_default` mechanism
// ===== WORKSPACE MODELS =====
// One row per tenant. Phase 1 of the multi-tenant migration
// created this table + bootstrapped the default workspace at
// id=1 for backward compatibility with the existing
// single-tenant deployment. Phase 2 introduces the
// WorkspaceContext extractor + middleware that resolves a
// workspace per request (subdomain in hosted mode, default in
// self-hosted). See docs/multi-tenant-migration-plan.md for
// the full picture.

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::workspaces)]
pub struct Workspace {
    pub id: i32,
    pub uuid: Uuid,
    pub slug: String,
    pub name: String,
    /// Opaque plan identifier (free / starter / pro / enterprise
    /// / self_hosted). Intentionally not CHECK-constrained at
    /// the DB layer — the billing surface churns these values.
    pub plan: String,
    pub settings: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    /// Nullable seam for a future MSP / enterprise
    /// organisations-as-parent-of-workspaces tier. NULL on
    /// every workspace today.
    pub organisation_id: Option<i32>,
    /// Customer-owned hostname (e.g. `support.acme.com`) that
    /// routes to this workspace. NULL when the workspace is
    /// reached via its `<slug>.nosdesk.app` subdomain only.
    /// Managed by the control plane via the
    /// `PATCH /api/internal/v1/workspaces/{slug}/custom-domain`
    /// endpoint (M5 Task 5).
    pub custom_domain: Option<String>,
    /// Staff-seat cap (NULL = unlimited). Set on a self-serve trial provision
    /// (to 5) and lifted to NULL on subscription activation. Only staff roles
    /// (owner/admin/agent) count against it. See `add_staff_membership_capped`.
    pub seat_limit: Option<i32>,
}

/// Insertable for a new workspace row. The product owns workspace
/// identity per the M5 locked decision: callers must pre-generate
/// the UUID rather than relying on the DB default so the same UUID
/// can be mirrored into the control plane's instance row.
/// `plan` is omitted so the DB column default (`'free'`) applies;
/// the control plane's plan-management surface mutates it later.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::workspaces)]
pub struct NewWorkspace {
    pub uuid: Uuid,
    pub slug: String,
    pub name: String,
    /// Staff-seat cap (NULL = unlimited). Set by the control plane on a
    /// self-serve trial provision (to 5) and lifted to NULL on activation;
    /// self-hosted / operator-provisioned workspaces leave it None.
    pub seat_limit: Option<i32>,
}

/// Per-workspace membership for a global user. A user can be a
/// member of multiple workspaces; the role here is workspace-
/// scoped and layered on top of the user's global role
/// (`UserRole`).
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::workspace_members)]
#[diesel(primary_key(workspace_id, user_uuid))]
pub struct WorkspaceMember {
    pub workspace_id: i32,
    pub user_uuid: Uuid,
    /// One of: owner, admin, member. CHECK-constrained at the
    /// schema layer.
    pub role: String,
    pub invited_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

/// invited the bug where a user could accidentally promote a view
/// to default and then have no way to find or change that setting.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::saved_views)]
pub struct SavedView {
    pub id: i32,
    pub uuid: Uuid,
    pub scope: String,
    pub scope_id: Option<String>,
    pub name: String,
    pub shape: serde_json::Value,
    pub filter: serde_json::Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Dataset the view applies to: 'tickets' | 'assets' |
    /// 'users'. Existing rows backfilled to 'tickets'. The
    /// handler refuses workspace/project scope on non-ticket
    /// datasets so the access model stays ticket-specific.
    pub dataset: String,
    pub workspace_id: i32,
    /// Renderer the dashboard SavedViewWidget shell uses for this
    /// view: 'list' (the default, no chart) | 'kpi_tile' | 'line' |
    /// 'horizontal_bar' | 'heatmap' | 'leaderboard' | 'table'. CHECK-
    /// constrained at the DB level; the handler validates the same
    /// allowlist before write.
    pub viz_type: String,
    /// Per-renderer config blob: measures, group-by, top-N, grain,
    /// chart_source tagged union, etc. The shape per viz_type lives
    /// in docs/dashboard-and-analytics-plan.md §4.2.
    pub viz_config: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::saved_views)]
pub struct NewSavedView {
    pub scope: String,
    pub scope_id: Option<String>,
    pub name: String,
    pub shape: serde_json::Value,
    pub filter: serde_json::Value,
    pub created_by: Uuid,
    pub dataset: String,
    pub viz_type: String,
    pub viz_config: serde_json::Value,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::saved_views)]
pub struct SavedViewUpdate {
    pub name: Option<String>,
    pub shape: Option<serde_json::Value>,
    pub filter: Option<serde_json::Value>,
    pub viz_type: Option<String>,
    pub viz_config: Option<serde_json::Value>,
}

/// Cycle: project-scoped, time-boxed bucket of tickets. Tickets
/// join via the `cycle_tickets` link table. The architecture spec
/// (§ 10 phase 6) targets TSTZRANGE + GIST for the span column;
/// v1 ships two TIMESTAMPTZ columns (start_at, end_at) so the
/// stock Diesel mapping does not need a tuple-of-bound shim. A
/// follow-up can add a real range column if calendar/gantt views
/// need the overlap GIST.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::cycles)]
pub struct Cycle {
    pub id: i32,
    pub uuid: Uuid,
    pub project_id: i32,
    pub name: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub state: String,
    pub completion_snapshot: Option<serde_json::Value>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::cycles)]
pub struct NewCycle {
    pub project_id: i32,
    pub name: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub state: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::cycles)]
pub struct CycleUpdate {
    pub name: Option<String>,
    pub start_at: Option<Option<DateTime<Utc>>>,
    pub end_at: Option<Option<DateTime<Utc>>>,
    pub state: Option<String>,
    pub completion_snapshot: Option<Option<serde_json::Value>>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
}

/// Many-to-many between cycles and tickets. The partial unique
/// index on `ticket_id` enforces "one cycle per ticket" until the
/// multi-cycle use case lands.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Insertable)]
#[diesel(primary_key(cycle_id, ticket_id))]
#[diesel(table_name = crate::schema::cycle_tickets)]
pub struct CycleTicket {
    pub cycle_id: i32,
    pub ticket_id: i32,
    pub added_at: DateTime<Utc>,
    pub added_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::cycle_tickets)]
pub struct NewCycleTicket {
    pub cycle_id: i32,
    pub ticket_id: i32,
    pub added_by: Option<Uuid>,
}

/// Working calendar — weekly schedule + timezone. Drives the SLA
/// engine's business-hours arithmetic. Rows are workspace-scoped;
/// `is_default = TRUE` is exactly one row, enforced by a partial
/// unique index.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::working_calendars)]
pub struct WorkingCalendar {
    pub id: i32,
    pub name: String,
    pub timezone: String,
    /// JSONB shape: `{ "mon": [["09:00","17:00"]], ... }`. Empty
    /// array for a day means non-working.
    pub schedule: serde_json::Value,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

/// Per-calendar holiday override. Days listed here count as
/// non-working regardless of what the weekly schedule says.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::working_calendar_holidays)]
pub struct WorkingCalendarHoliday {
    pub id: i32,
    pub calendar_id: i32,
    pub date: chrono::NaiveDate,
    pub label: Option<String>,
    pub workspace_id: i32,
    /// `"none"` (single date) or `"annual"` (MM-DD repeats every
    /// year). The engine expands annual rows into concrete dates at
    /// load time so the arithmetic keeps using a flat
    /// `HashSet<NaiveDate>`.
    pub recurrence: String,
}

/// SLA policy — applies to a ticket when its `priority_filter` /
/// `category_id_filter` match (NULL = wildcard). When more than one
/// policy could match, the highest-id policy wins (last-write); the
/// `is_default` row is the catch-all when nothing else matches.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sla_policies)]
pub struct SlaPolicy {
    pub id: i32,
    pub name: String,
    pub target_response_minutes: Option<i32>,
    pub target_resolution_minutes: Option<i32>,
    pub working_calendar_id: Option<i32>,
    pub priority_filter: Option<String>,
    pub category_id_filter: Option<i32>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
    pub assignee_group_id_filter: Option<i32>,
}

/// Operation kind recorded in `sync_actions.op`. The fourth variant
/// `Archive` distinguishes a soft-delete (row stays, marked archived)
/// from a hard delete; consumers that maintain projections need to
/// know the difference.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::SyncOp)]
pub enum SyncOp {
    #[serde(rename = "I")]
    Insert,
    #[serde(rename = "U")]
    Update,
    #[serde(rename = "D")]
    Delete,
    #[serde(rename = "A")]
    Archive,
}

impl SyncOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Insert => "I",
            Self::Update => "U",
            Self::Delete => "D",
            Self::Archive => "A",
        }
    }
}

impl ToSql<crate::schema::sql_types::SyncOp, Pg> for SyncOp {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::SyncOp, Pg> for SyncOp {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"I" => Ok(Self::Insert),
            b"U" => Ok(Self::Update),
            b"D" => Ok(Self::Delete),
            b"A" => Ok(Self::Archive),
            other => Err(format!("unknown sync_op: {}", String::from_utf8_lossy(other)).into()),
        }
    }
}

/// Aggregate kind recorded in `sync_actions.aggregate`. Adding a new
/// aggregate requires both an `ALTER TYPE` migration and a Rust
/// variant; the registry module is the single source of truth for
/// what each variant means.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::SyncAggregate)]
pub enum SyncAggregate {
    #[serde(rename = "ticket")]
    Ticket,
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "project_ticket")]
    ProjectTicket,
    #[serde(rename = "workflow_state")]
    WorkflowState,
    #[serde(rename = "comment")]
    Comment,
    #[serde(rename = "attachment")]
    Attachment,
    #[serde(rename = "assignment")]
    Assignment,
    #[serde(rename = "group_membership")]
    GroupMembership,
    #[serde(rename = "plugin")]
    Plugin,
    #[serde(rename = "cycle")]
    Cycle,
    #[serde(rename = "cycle_ticket")]
    CycleTicket,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "asset")]
    Asset,
    #[serde(rename = "asset_media")]
    AssetMedia,
    #[serde(rename = "asset_lifecycle_event")]
    AssetLifecycleEvent,
    #[serde(rename = "webhook")]
    Webhook,
    #[serde(rename = "channel")]
    Channel,
    #[serde(rename = "knowledge_gap")]
    KnowledgeGap,
    #[serde(rename = "documentation_page")]
    DocumentationPage,
    #[serde(rename = "documentation_collection")]
    DocumentationCollection,
    /// Synthetic aggregate for system/meta events that have no backing
    /// table, e.g. `data.audit.read` / `data.audit.exported` emitted
    /// when the audit surface is read or exported (Item C/W5, D5).
    #[serde(rename = "data")]
    Data,
    /// Per-recipient notification events. Emitted on notification
    /// creation, scoped to the recipient's private `user:<uuid>` group,
    /// so they fan out cross-machine via the sync stream.
    #[serde(rename = "notification")]
    Notification,
    /// Ticket<->asset link (junction `ticket_assets`). Composite key
    /// `ticket_id:asset_id`. Lets the pool-native ticket detail view
    /// derive a ticket's linked assets (Phase 2).
    #[serde(rename = "ticket_asset")]
    TicketAsset,
    /// Ticket<->ticket link (junction `linked_tickets`). Composite key
    /// `ticket_id:linked_ticket_id`, emitted in both directions.
    #[serde(rename = "linked_ticket")]
    LinkedTicket,
    /// Append-only asset usage ledger event (`asset_usage_log`). Op
    /// Insert; not pool-materialised — the usage-history panels react to
    /// it via `useSyncActions`. Cross-machine replacement for the old
    /// instance-local `SseEvent::AssetUsageRecorded`.
    #[serde(rename = "asset_usage")]
    AssetUsage,
    /// Append-only asset physical-count audit event (`asset_audits`).
    /// Same shape/intent as `asset_usage`.
    #[serde(rename = "asset_audit")]
    AssetAudit,
}

impl SyncAggregate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Project => "project",
            Self::ProjectTicket => "project_ticket",
            Self::WorkflowState => "workflow_state",
            Self::Comment => "comment",
            Self::Attachment => "attachment",
            Self::Assignment => "assignment",
            Self::GroupMembership => "group_membership",
            Self::Plugin => "plugin",
            Self::Cycle => "cycle",
            Self::CycleTicket => "cycle_ticket",
            Self::User => "user",
            Self::Asset => "asset",
            Self::AssetMedia => "asset_media",
            Self::AssetLifecycleEvent => "asset_lifecycle_event",
            Self::Webhook => "webhook",
            Self::Channel => "channel",
            Self::KnowledgeGap => "knowledge_gap",
            Self::DocumentationPage => "documentation_page",
            Self::DocumentationCollection => "documentation_collection",
            Self::Data => "data",
            Self::Notification => "notification",
            Self::TicketAsset => "ticket_asset",
            Self::LinkedTicket => "linked_ticket",
            Self::AssetUsage => "asset_usage",
            Self::AssetAudit => "asset_audit",
        }
    }
}

impl ToSql<crate::schema::sql_types::SyncAggregate, Pg> for SyncAggregate {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::SyncAggregate, Pg> for SyncAggregate {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"ticket" => Ok(Self::Ticket),
            b"project" => Ok(Self::Project),
            b"project_ticket" => Ok(Self::ProjectTicket),
            b"workflow_state" => Ok(Self::WorkflowState),
            b"comment" => Ok(Self::Comment),
            b"attachment" => Ok(Self::Attachment),
            b"assignment" => Ok(Self::Assignment),
            b"group_membership" => Ok(Self::GroupMembership),
            b"plugin" => Ok(Self::Plugin),
            b"cycle" => Ok(Self::Cycle),
            b"cycle_ticket" => Ok(Self::CycleTicket),
            b"user" => Ok(Self::User),
            b"asset" => Ok(Self::Asset),
            b"asset_media" => Ok(Self::AssetMedia),
            b"asset_lifecycle_event" => Ok(Self::AssetLifecycleEvent),
            b"webhook" => Ok(Self::Webhook),
            b"channel" => Ok(Self::Channel),
            b"knowledge_gap" => Ok(Self::KnowledgeGap),
            b"documentation_page" => Ok(Self::DocumentationPage),
            b"documentation_collection" => Ok(Self::DocumentationCollection),
            b"data" => Ok(Self::Data),
            b"notification" => Ok(Self::Notification),
            b"ticket_asset" => Ok(Self::TicketAsset),
            b"linked_ticket" => Ok(Self::LinkedTicket),
            b"asset_usage" => Ok(Self::AssetUsage),
            b"asset_audit" => Ok(Self::AssetAudit),
            other => {
                Err(format!("unknown sync_aggregate: {}", String::from_utf8_lossy(other)).into())
            }
        }
    }
}

// === Ticket watchers =========================================
//
// Lets a user opt into notifications for a ticket without being
// the requester or assignee. See migration
// `2026-05-09-320000_ticket_watchers`.

#[derive(Debug, Clone, Serialize, Deserialize, Insertable, Queryable)]
#[diesel(table_name = crate::schema::ticket_watchers)]
pub struct TicketWatcher {
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    pub created_at: DateTime<Utc>,
    /// `true` when the watcher was added implicitly (e.g.
    /// auto-watch on first comment), `false` when the user
    /// explicitly toggled the bell. Used by the future "stop
    /// auto-watching" preference.
    pub auto_added: bool,
    /// Per-watch preference. `true` (default) means the watcher
    /// is notified for both public replies and internal notes;
    /// `false` mutes internal-note notifications only. Mentions
    /// ignore this flag because they are explicit pings rather
    /// than implicit fan-out.
    pub notify_on_internal_notes: bool,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_watchers)]
pub struct NewTicketWatcher {
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    pub auto_added: bool,
}

// === Tags ====================================================
//
// Free-form, multi-valued labels on tickets — flexible second
// axis to the fixed `category_id`. Workspace-scoped namespace;
// admin / staff can create + assign. See migration
// `2026-05-09-310000_ticket_tags`.

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::tags)]
pub struct Tag {
    pub id: i32,
    pub name: String,
    /// Display colour token. Same vocabulary as workflow_states
    /// (slate / gray / blue / purple / green / amber / rose /
    /// subtle). NULL means "use the neutral default."
    pub color: Option<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    pub workspace_id: i32,
}

#[derive(Debug, Default, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::tags)]
pub struct NewTag {
    pub name: String,
    pub color: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::tags)]
pub struct TagUpdate {
    pub name: Option<String>,
    pub color: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

/// One row of the `ticket_tags` join. The repository never
/// returns these directly — it returns either `Tag` rows for a
/// ticket (via a join) or just the tag-id list.
#[derive(Debug, Clone, Serialize, Deserialize, Insertable, Queryable)]
#[diesel(table_name = crate::schema::ticket_tags)]
pub struct TicketTag {
    pub ticket_id: i32,
    pub tag_id: i32,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_tags)]
pub struct NewTicketTag {
    pub ticket_id: i32,
    pub tag_id: i32,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::workflow_states)]
pub struct WorkflowState {
    pub id: i32,
    pub name: String,
    pub category: WorkflowStateCategory,
    pub color: String,
    pub position: i32,
    pub is_default: bool,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
    /// When true, the SLA matcher stops the clock while a ticket is
    /// in this state. Per-row override of the legacy category-derived
    /// rule (active = running, everything else = paused), so an admin
    /// can keep a "Waiting on customer" status modelled under active
    /// while still pausing the timer.
    pub pauses_sla: bool,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::workflow_states)]
pub struct NewWorkflowState {
    pub name: String,
    pub category: WorkflowStateCategory,
    pub color: String,
    pub position: i32,
    pub is_default: bool,
    pub created_by: Option<Uuid>,
    pub pauses_sla: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::workflow_states)]
pub struct WorkflowStateUpdate {
    pub name: Option<String>,
    pub color: Option<String>,
    pub position: Option<i32>,
    pub is_default: Option<bool>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
    pub pauses_sla: Option<bool>,
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::TicketPriority)]
#[derive(Default)]
pub enum TicketPriority {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    #[default]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "urgent")]
    Urgent,
}

impl TicketPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            TicketPriority::None => "none",
            TicketPriority::Low => "low",
            TicketPriority::Medium => "medium",
            TicketPriority::High => "high",
            TicketPriority::Urgent => "urgent",
        }
    }
}

impl ToSql<crate::schema::sql_types::TicketPriority, Pg> for TicketPriority {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

/// Format the bytes in `comments.content` are stored as. Lets the
/// outbound dispatcher choose the right transformation when relaying a
/// comment through a channel that needs a specific representation.
///
/// Stored as `VARCHAR(16)` rather than a Postgres `ENUM` so a future
/// channel-specific value (e.g. Slack `mrkdwn`) can be added by Rust
/// code alone, without an `ALTER TYPE` migration. An unknown string in
/// the DB is treated as a hard error so a typo in the inbound pipeline
/// is caught at the boundary instead of corrupting reply rendering.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "lowercase")]
pub enum ContentFormat {
    /// Rich HTML — what the ProseMirror editor produces.
    Html,
    /// CommonMark Markdown. Reserved for chat / scripted senders that
    /// emit Markdown natively. Not currently produced by any code path.
    Markdown,
    /// Pre-formatted plaintext. Inbound emails store their `body_text`
    /// here directly; whitespace is significant.
    Plaintext,
}

impl ContentFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "markdown",
            Self::Plaintext => "plaintext",
        }
    }
}

impl Default for ContentFormat {
    /// HTML matches the ProseMirror editor — the only path that
    /// currently *creates* comments through the API.
    fn default() -> Self {
        Self::Html
    }
}

impl ToSql<diesel::sql_types::Text, Pg> for ContentFormat {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<diesel::sql_types::Text, Pg> for ContentFormat {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"html" => Ok(Self::Html),
            b"markdown" => Ok(Self::Markdown),
            b"plaintext" => Ok(Self::Plaintext),
            other => {
                Err(format!("unknown content_format: {}", String::from_utf8_lossy(other)).into())
            }
        }
    }
}

impl FromSql<crate::schema::sql_types::TicketPriority, Pg> for TicketPriority {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"none" => Ok(TicketPriority::None),
            b"low" => Ok(TicketPriority::Low),
            b"medium" => Ok(TicketPriority::Medium),
            b"high" => Ok(TicketPriority::High),
            b"urgent" => Ok(TicketPriority::Urgent),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::tickets)]
pub struct Ticket {
    pub id: i32,
    pub title: String,
    pub priority: TicketPriority,
    #[serde(
        serialize_with = "serialize_optional_uuid_as_string",
        rename = "requester"
    )]
    pub requester_uuid: Option<Uuid>,
    #[serde(
        serialize_with = "serialize_optional_uuid_as_string",
        rename = "assignee"
    )]
    pub assignee_uuid: Option<Uuid>,
    #[serde(rename = "created")] // Map to frontend field name
    pub created_at: NaiveDateTime,
    #[serde(rename = "modified")] // Map to frontend field name
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub closed_at: Option<NaiveDateTime>,
    pub closed_by: Option<Uuid>,
    pub category_id: Option<i32>,
    pub submitted_via: Option<String>,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub guest_lookup_token: Option<Uuid>,
    pub verification_state: Option<String>,
    /// FK to the channel this ticket originated from (email mailbox, Slack
    /// workspace, etc.). Null for tickets submitted via the normal UI or
    /// the guest web form.
    pub origin_channel_id: Option<i32>,
    pub workflow_state_id: i32,
    /// Triage lifecycle, independent of workflow_state. NULL means
    /// "not in the triage flow" (i.e. already triaged into a cycle
    /// or directly worked). The Triage saved view filters on
    /// `triage_state = 'untriaged' AND ticket not in any cycle`.
    pub triage_state: Option<String>,
    /// Calendar deadline. NULL when the ticket has no committed
    /// due date; calendar views only render the ones with a value.
    /// Uses NaiveDateTime to match the surrounding closed_at /
    /// created_at columns; the SQL column is TIMESTAMPTZ but
    /// timezone gets normalised at the API boundary.
    pub due_date: Option<NaiveDateTime>,
    /// RFC 5545 RRULE string. NULL means the ticket isn't on a
    /// recurring schedule. Closing a ticket with a rule spawns the
    /// next occurrence (services::recurrence::materialise_next).
    pub recurrence_rule: Option<String>,
    /// First ticket in the series. NULL on the original; subsequent
    /// occurrences point back at the template so the audit reads
    /// "this ticket was generated from #N".
    pub recurrence_template_id: Option<i32>,
    /// Free-text "what fixed this?" capture. Surfaced prominently
    /// on the detail view once the ticket lands in a terminal
    /// workflow state. Separate from the comment thread because
    /// the resolution is a structured fact, not a discussion. Empty
    /// string normalises to NULL at the API boundary so the UI can
    /// use a single null-check.
    pub resolution_notes: Option<String>,
    pub workspace_id: i32,
    /// Wall-clock moment of the first non-internal staff comment on
    /// this ticket. Stamped idempotently by `repository::comments`
    /// (UPDATE ... WHERE first_response_at IS NULL) so concurrent
    /// first replies don't race. Feeds the SLA engine's response
    /// timer: the response target is met when `first_response_at <=
    /// target_at`; before the first response, the timer counts down
    /// toward breach exactly like the resolution timer.
    pub first_response_at: Option<NaiveDateTime>,
    /// Materialised response-timer target — the wall-clock instant
    /// the response SLA breaches. NULL when the timer doesn't apply
    /// (no `target_response_minutes` configured), has already been
    /// met (`first_response_at` is set), or the ticket is paused
    /// (non-active workflow state). The breach-detection job scans
    /// `WHERE sla_response_target_at <= NOW() AND
    /// sla_response_breached_at IS NULL` via a partial index. Kept
    /// fresh by `services::sla::recompute_and_stamp_sla_for_ticket`
    /// on every mutation that could change it.
    pub sla_response_target_at: Option<NaiveDateTime>,
    /// Idempotency stamp for the response breach. NULL until the
    /// detection job first observes a breach; once set, the partial
    /// index excludes the row from the scan so a follow-up tick
    /// doesn't re-fire the notification.
    pub sla_response_breached_at: Option<NaiveDateTime>,
    /// Materialised resolution-timer target. Same semantics as
    /// `sla_response_target_at` but for the resolution SLA (no `met`
    /// concept — resolution is satisfied by closing the ticket,
    /// which is a separate concern).
    pub sla_resolution_target_at: Option<NaiveDateTime>,
    /// Idempotency stamp for the resolution breach.
    pub sla_resolution_breached_at: Option<NaiveDateTime>,
    /// Stable, never-recycled identity. Unlike the integer `id` (which
    /// a DB reset recycles), this UUID is minted once at creation, so
    /// it's the safe key for collaborative-document caches keyed
    /// `ws-{workspaceUuid}_ticket-{uuid}`.
    pub uuid: Uuid,
    /// True when the ticket opened from inbound mail the provider flagged as
    /// spam. The ticket still opens (we never drop a customer request) but is
    /// badged + low-priority for triage. Cleared via a normal ticket update
    /// ("not spam"). Must stay the LAST field to match `schema.rs` column
    /// order (positional Queryable).
    pub spam_suspected: bool,
}

/// Merge metadata for a ticket that was merged into another (the satellite of
/// the old `tickets.merged_*` columns). 1:1 with merge-source tickets, keyed
/// by the source `ticket_id`; absent for the ~99% of tickets never merged.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable, Insertable)]
#[diesel(table_name = crate::schema::ticket_merges)]
#[diesel(primary_key(ticket_id))]
pub struct TicketMerge {
    pub ticket_id: i32,
    pub merged_into_ticket_id: i32,
    pub merged_at: NaiveDateTime,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub merged_by_user_uuid: Option<Uuid>,
    pub merge_reason: Option<String>,
    pub workspace_id: i32,
}

/// Insert shape for a new merge record. `workspace_id` fills from the RLS GUC.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::ticket_merges)]
pub struct NewTicketMerge {
    pub ticket_id: i32,
    pub merged_into_ticket_id: i32,
    pub merged_at: NaiveDateTime,
    pub merged_by_user_uuid: Option<Uuid>,
    pub merge_reason: Option<String>,
}

// Ticket implementation removed - serialization now handled by serde attributes

/// Insert payload for `tickets`. `Default` is implemented so call
/// sites can write `NewTicket { title: ..., workflow_state_id: ...,
/// ..Default::default() }` without spelling out every nullable
/// field. Adding a new optional column on `tickets` then becomes a
/// one-line model change instead of a sweep across every caller.
#[derive(Debug, Default, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::tickets)]
pub struct NewTicket {
    pub title: String,
    pub workflow_state_id: i32,
    pub priority: TicketPriority,
    pub requester_uuid: Option<Uuid>,
    pub assignee_uuid: Option<Uuid>,
    pub category_id: Option<i32>,
    pub submitted_via: Option<String>,
    pub guest_lookup_token: Option<Uuid>,
    pub verification_state: Option<String>,
    pub origin_channel_id: Option<i32>,
    pub triage_state: Option<String>,
    pub due_date: Option<NaiveDateTime>,
    pub recurrence_rule: Option<String>,
    pub recurrence_template_id: Option<i32>,
    pub resolution_notes: Option<String>,
    /// Defaults false; set true by the inbound pipeline when the source
    /// message was flagged as spam.
    pub spam_suspected: bool,
}

// Add a new struct for partial ticket updates
#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::tickets)]
pub struct TicketUpdate {
    pub title: Option<String>,
    pub workflow_state_id: Option<i32>,
    pub priority: Option<TicketPriority>,
    pub requester_uuid: Option<Option<Uuid>>,
    pub assignee_uuid: Option<Option<Uuid>>,
    pub updated_at: Option<NaiveDateTime>,
    pub closed_at: Option<Option<NaiveDateTime>>,
    pub verification_state: Option<Option<String>>,
    pub origin_channel_id: Option<Option<i32>>,
    pub category_id: Option<Option<i32>>,
    pub triage_state: Option<Option<String>>,
    pub due_date: Option<Option<NaiveDateTime>>,
    pub recurrence_rule: Option<Option<String>>,
    pub recurrence_template_id: Option<Option<i32>>,
    /// `Option<Option<String>>` semantics — outer None = leave as-is,
    /// `Some(None)` = clear, `Some(Some(s))` = set. Empty string
    /// normalises to `Some(None)` at the handler boundary so the
    /// UI can post a single shape regardless of intent.
    pub resolution_notes: Option<Option<String>>,
    /// Cleared to `false` by the "not spam" action; never set true via the API
    /// (only the inbound pipeline flags spam).
    pub spam_suspected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::assets)]
pub struct Asset {
    pub id: i32,
    pub name: String,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub location: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    /// Discriminator into the `asset_kinds` registry. Existing
    /// rows default to `'device'` so the IT-desk lens keeps
    /// rendering them unchanged; non-device kinds (vehicle,
    /// license, material, ...) opt in by setting this field
    /// when created from the typed asset flow.
    pub kind: String,
    /// JSONB blob holding kind-specific attributes validated
    /// against the kind's `attribute_schema` at write time. Empty
    /// object for IT-desk devices that only use the structured
    /// columns above.
    pub attributes: serde_json::Value,
    /// Quantity for bulk materials / consumables (cable length
    /// in metres, ink-cartridge stock, screws by the hundred).
    /// Null on assets that are "one row per physical thing"
    /// like a laptop.
    pub quantity: Option<bigdecimal::BigDecimal>,
    /// Unit label paired with `quantity`. Free-text ('m', 'L',
    /// 'pcs', etc.) so we don't impose a unit ontology.
    pub unit: Option<String>,
    /// Identifier of the external system that owns this row,
    /// when it's synced from one. `Some("intune")` /
    /// `Some("entra")` for Microsoft-managed assets; `None`
    /// for assets managed inside Nosdesk. Drives the
    /// is_editable predicate that hides edit UI for external
    /// rows so admins don't make changes that the next sync
    /// will overwrite.
    pub external_sync_source: Option<String>,
    /// Optional low-stock threshold. When set on a
    /// stock-tracked asset (i.e. `quantity` is also Some), a
    /// current `quantity` at or below this value flags the
    /// asset as low-stock in the UI and emits an
    /// `asset.low_stock` SSE event after each usage decrement
    /// that crosses the threshold. NULL means "not configured"
    /// (no alerting).
    pub low_stock_threshold: Option<bigdecimal::BigDecimal>,
    pub workspace_id: i32,
    /// Lifecycle state, one of the `AssetStatus` values (defaults to
    /// `in_service`). Status only changes through the lifecycle
    /// transition flow, which records an `asset_lifecycle_events`
    /// row, so `AssetUpdate` deliberately omits it.
    pub status: String,
}

/// Canonical asset lifecycle states. Stored as snake_case strings in
/// `assets.status` and validated here rather than by a DB CHECK, so
/// adding a state is a code change, not a migration. State-specific
/// data (repair vendor / RMA / offsite, loan recipient / due-back)
/// lives in `asset_lifecycle_events.metadata`, never in new columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    InService,
    InStock,
    InRepair,
    OnLoan,
    Retired,
    Lost,
    Disposed,
}

impl AssetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InService => "in_service",
            Self::InStock => "in_stock",
            Self::InRepair => "in_repair",
            Self::OnLoan => "on_loan",
            Self::Retired => "retired",
            Self::Lost => "lost",
            Self::Disposed => "disposed",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "in_service" => Some(Self::InService),
            "in_stock" => Some(Self::InStock),
            "in_repair" => Some(Self::InRepair),
            "on_loan" => Some(Self::OnLoan),
            "retired" => Some(Self::Retired),
            "lost" => Some(Self::Lost),
            "disposed" => Some(Self::Disposed),
            _ => None,
        }
    }

    /// The canonical default for a freshly created asset. Mirrors the
    /// DB-level `DEFAULT 'in_service'` on `assets.status`.
    pub fn default_str() -> &'static str {
        Self::InService.as_str()
    }

    pub fn is_valid(s: &str) -> bool {
        Self::from_str_opt(s).is_some()
    }
}

/// Default kind for callers that omit `kind` from the JSON
/// payload. Mirrors the DB-level default on `assets.kind`
/// (`'generic'` as of 2026-05-20-130000) so a workspace-neutral
/// asset is the no-effort outcome. The IT-desk flow sets
/// `kind = 'device'` explicitly through the picker.
fn default_asset_kind() -> String {
    "generic".to_string()
}

/// Default `attributes` blob for legacy callers. JSON Schema
/// validation against the `device` builtin kind's empty
/// attribute_schema accepts an empty object.
fn default_asset_attributes() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::assets)]
pub struct NewAsset {
    pub name: String,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    #[serde(default = "default_asset_kind")]
    pub kind: String,
    #[serde(default = "default_asset_attributes")]
    pub attributes: serde_json::Value,
    #[serde(default)]
    pub quantity: Option<bigdecimal::BigDecimal>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub external_sync_source: Option<String>,
    #[serde(default)]
    pub low_stock_threshold: Option<bigdecimal::BigDecimal>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::assets)]
pub struct AssetUpdate {
    pub name: Option<String>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
    pub kind: Option<String>,
    pub attributes: Option<serde_json::Value>,
    pub quantity: Option<Option<bigdecimal::BigDecimal>>,
    pub unit: Option<Option<String>>,
    pub external_sync_source: Option<Option<String>>,
    pub low_stock_threshold: Option<Option<bigdecimal::BigDecimal>>,
}

/// Runtime-extensible asset-kind registry. `slug` is the value
/// stored on `assets.kind`; `attribute_schema` is a constrained
/// JSON Schema subset validated by `services::assets::kinds`.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_kinds)]
pub struct AssetKind {
    pub id: i32,
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub attribute_schema: serde_json::Value,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
    /// One of `it`, `logical`, `physical`, `bulk`, `generic`.
    /// The frontend toggles which IT-flavoured form fields and
    /// planner UI to render off this; the DB CHECK constraint
    /// enforces the closed set.
    pub category: String,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_kinds)]
pub struct NewAssetKind {
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub attribute_schema: serde_json::Value,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub created_by: Option<Uuid>,
    pub category: String,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::asset_kinds)]
pub struct AssetKindUpdate {
    pub label: Option<String>,
    pub description: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub attribute_schema: Option<serde_json::Value>,
    pub sort_order: Option<i32>,
    pub category: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One row of the asset usage ledger. Records how much of a
/// stock-tracked asset was consumed and when, optionally tied
/// to a ticket. Example: "5 m of cable consumed on ticket
/// #142". `unit` is stored on the row (not derived from
/// asset.unit at read time) so a later unit change on the
/// asset doesn't rewrite history.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_usage_log)]
pub struct AssetUsage {
    pub id: i64,
    pub asset_id: i32,
    pub ticket_id: Option<i32>,
    pub quantity_used: bigdecimal::BigDecimal,
    pub unit: String,
    pub recorded_by: Option<Uuid>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub notes: Option<String>,
    /// Direction discriminator. `"usage"` decrements the asset's
    /// quantity; `"restock"` increments it. Both kinds keep
    /// `quantity_used > 0`; the magnitude lives in
    /// `quantity_used`, the direction here. The DB CHECK
    /// constraint pins the enum to this closed set.
    pub event_kind: String,
    pub workspace_id: i32,
}

/// One row of the asset audit ledger. Records a physical-
/// count assertion: the admin counted `counted_quantity` units
/// on hand at `recorded_at`; the system held `previous_quantity`
/// at that moment. `delta` = counted - previous (signed). The
/// assets.quantity column is set to counted_quantity in the
/// same transaction, so this row is also the audit trail for
/// the corresponding correction.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_audits)]
pub struct AssetAudit {
    pub id: i64,
    pub asset_id: i32,
    pub counted_quantity: bigdecimal::BigDecimal,
    pub previous_quantity: bigdecimal::BigDecimal,
    pub delta: bigdecimal::BigDecimal,
    pub notes: Option<String>,
    pub recorded_by: Option<Uuid>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_audits)]
pub struct NewAssetAudit {
    pub asset_id: i32,
    pub counted_quantity: bigdecimal::BigDecimal,
    pub previous_quantity: bigdecimal::BigDecimal,
    pub delta: bigdecimal::BigDecimal,
    pub notes: Option<String>,
    pub recorded_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_usage_log)]
pub struct NewAssetUsage {
    pub asset_id: i32,
    pub ticket_id: Option<i32>,
    pub quantity_used: bigdecimal::BigDecimal,
    pub unit: String,
    pub recorded_by: Option<Uuid>,
    pub notes: Option<String>,
    pub event_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_media)]
#[diesel(belongs_to(Asset))]
pub struct AssetMedia {
    pub id: i32,
    pub asset_id: i32,
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub kind: String,
    pub sort_order: i32,
    pub caption: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
    // Field order mirrors `schema.rs`: the column was added by a later
    // migration, so it lands last. Diesel `Queryable` maps positionally.
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_media)]
pub struct NewAssetMedia {
    pub asset_id: i32,
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub kind: String,
    pub sort_order: i32,
    pub caption: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::asset_media)]
pub struct AssetMediaUpdate {
    pub sort_order: Option<i32>,
    pub caption: Option<Option<String>>,
}

/// One entry in an asset's append-only lifecycle log. Each row is a
/// status transition; `ticket_id` links it to the ticket that
/// captured the context (e.g. the repair), and `metadata` carries
/// state-specific fields without dedicated columns.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_lifecycle_events)]
#[diesel(belongs_to(Asset))]
pub struct AssetLifecycleEvent {
    pub id: i32,
    pub asset_id: i32,
    pub from_status: Option<String>,
    pub to_status: String,
    pub reason: Option<String>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<Uuid>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_lifecycle_events)]
pub struct NewAssetLifecycleEvent {
    pub asset_id: i32,
    pub from_status: Option<String>,
    pub to_status: String,
    pub reason: Option<String>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::ticket_assets)]
#[diesel(belongs_to(Ticket))]
#[diesel(belongs_to(Asset, foreign_key = asset_id))]
#[diesel(primary_key(ticket_id, asset_id))]
pub struct TicketAsset {
    pub ticket_id: i32,
    pub asset_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_assets)]
pub struct NewTicketAsset {
    pub ticket_id: i32,
    pub asset_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::comments)]
#[diesel(belongs_to(Ticket))]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
pub struct Comment {
    pub id: i32,
    pub content: String,
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub is_edited: bool,
    pub edit_count: i32,
    /// Free-form per-channel metadata (our emitted Message-ID for email,
    /// Slack thread_ts, Discord message id, etc.). Null for comments
    /// authored through the normal Nosdesk UI without channel context.
    pub channel_metadata: Option<serde_json::Value>,
    /// True = tech-to-tech note. Never shown to requesters in their
    /// portal view; never relayed back through the originating channel.
    pub is_internal: bool,
    /// Soft-delete marker. Set by future channel-edit/delete pipeline
    /// handlers when Slack/Teams/Discord signal a deleted message.
    pub deleted_at: Option<NaiveDateTime>,
    /// What the bytes in `content` are. Drives the outbound dispatcher's
    /// HTML / plaintext composition for replies.
    pub content_format: ContentFormat,
    /// Raw text/plain MIME part (or the plaintext body for plaintext-only
    /// inbound messages). NULL for non-email comments.
    ///
    /// Backend-only: `skip` keeps it off the wire because the
    /// renderer reads `new_content` / `quoted_content` instead, and
    /// shipping the full raw body on every comment list inflates
    /// payloads with no consumer.
    #[serde(skip)]
    pub body_text: Option<String>,
    /// Raw text/html MIME part. Pre-sanitisation; Pass 2 of the email
    /// rendering plan introduces a separate `sanitised_html` column for
    /// the render-ready form. NULL for non-email comments and for emails
    /// without an HTML alternative.
    ///
    /// Backend-only — same reasoning as `body_text`.
    #[serde(skip)]
    pub body_html: Option<String>,
    /// Just-the-reply extraction, output of the quote splitter at ingest.
    /// Plain text or HTML depending on which path the parser took
    /// (use `content_format` to disambiguate). NULL for non-email
    /// comments.
    pub new_content: Option<String>,
    /// Extracted prior-thread quoted block. NULL when nothing was
    /// detected or when the comment isn't email-derived. Same format
    /// rule as `new_content`.
    pub quoted_content: Option<String>,
    /// Storage path (not URL) to the persisted .eml. Powers "Show
    /// original message" and lets us re-run the splitter on policy
    /// change without re-fetching from the upstream mailbox. NULL for
    /// non-email comments and for email comments ingested before this
    /// column existed.
    ///
    /// `skip` because the storage path is internal infrastructure; the
    /// frontend constructs the public URL from the comment id
    /// (`/api/comments/{id}/raw.eml`) and doesn't need the backing
    /// path. Hiding it also avoids leaking storage layout (S3 keys,
    /// LocalStorage roots) in API responses.
    #[serde(skip)]
    pub raw_source_uri: Option<String>,
    pub workspace_id: i32,
    /// Native-first render tier set by the inbound pipeline:
    /// `text` / `simple` / `rich` (see `email_render_kind`). NULL for
    /// non-email comments (agent markdown) and email comments ingested
    /// before this column existed; the frontend falls back to its
    /// per-`content_format` rendering when NULL.
    pub render_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Insertable, Default)]
#[diesel(table_name = crate::schema::comments)]
pub struct NewComment {
    pub content: String,
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    #[serde(default)]
    pub channel_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub is_internal: bool,
    /// Defaults to HTML so the regular helpdesk UI (the only path that
    /// posts comments through the API today) doesn't have to opt in.
    /// Inbound channel adapters set this explicitly to match what they
    /// stored in `content`.
    #[serde(default)]
    pub content_format: ContentFormat,
    /// Inbound-email-only — see `Comment` field docs. UI-authored
    /// comments leave all four NULL and just fill `content`.
    #[serde(default)]
    pub body_text: Option<String>,
    #[serde(default)]
    pub body_html: Option<String>,
    #[serde(default)]
    pub new_content: Option<String>,
    #[serde(default)]
    pub quoted_content: Option<String>,
    #[serde(default)]
    pub raw_source_uri: Option<String>,
    /// Render tier (`text`/`simple`/`rich`) from `email_render_kind`.
    /// Inbound channel adapters set this; UI-authored comments leave it
    /// NULL.
    #[serde(default)]
    pub render_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::attachments)]
#[diesel(belongs_to(Comment))]
pub struct Attachment {
    pub id: i32,
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub comment_id: Option<i32>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub transcription: Option<String>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::attachments)]
pub struct NewAttachment {
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub comment_id: Option<i32>,
    pub uploaded_by: Option<Uuid>,
    pub transcription: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::article_contents)]
#[diesel(belongs_to(Ticket))]
pub struct ArticleContent {
    pub id: i32,
    pub ticket_id: Option<i32>,
    pub current_revision_number: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub updated_at: NaiveDateTime,
    pub updated_by: Option<Uuid>,
    // Yjs document state (current version) - snapshot-based persistence
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub workspace_id: i32,
    /// Fencing token from the per-document ownership claim (Phase 2
    /// affinity). The owning machine stamps its claim's monotonic token
    /// on each snapshot write; a conditional write rejects a stale owner
    /// whose token is lower. NULL on rows written in single-instance
    /// mode (no claim). See `docs/realtime-collab-affinity-design.md`.
    pub fence_token: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::article_contents)]
pub struct NewArticleContent {
    pub ticket_id: i32,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
}

/// Append-only crash-recovery checkpoint for a collaborative document.
/// Written by the collaboration checkpoint loop between the heavier
/// `article_contents` saves so a hard crash loses seconds, not the whole
/// save interval. `document_id` is the namespaced doc id (the same key
/// used for the Redis cache); `snapshot` is a full Yjs v1 update,
/// `state_vector` its encoded state vector. Workspace-scoped via RLS.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::yjs_snapshots)]
pub struct NewYjsSnapshot<'a> {
    pub workspace_id: i32,
    pub document_id: &'a str,
    pub snapshot: &'a [u8],
    pub state_vector: &'a [u8],
}

// Article Content Revision models for version history
// Simplified: removed redundant yjs_document_snapshot field (DRY principle)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::article_content_revisions)]
#[diesel(belongs_to(ArticleContent))]
pub struct ArticleContentRevision {
    pub id: i32,
    pub article_content_id: i32,
    pub revision_number: i32,
    pub yjs_state_vector: Vec<u8>,
    pub yjs_document_content: Vec<u8>,
    pub contributed_by: Vec<Option<Uuid>>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::article_content_revisions)]
pub struct NewArticleContentRevision {
    pub article_content_id: i32,
    pub revision_number: i32,
    pub yjs_state_vector: Vec<u8>,
    pub yjs_document_content: Vec<u8>,
    pub contributed_by: Vec<Option<Uuid>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArticleContentRevisionResponse {
    pub id: i32,
    pub article_content_id: i32,
    pub revision_number: i32,
    pub contributed_by: Vec<Option<Uuid>>,
    pub created_at: NaiveDateTime,
}

impl From<ArticleContentRevision> for ArticleContentRevisionResponse {
    fn from(revision: ArticleContentRevision) -> Self {
        ArticleContentRevisionResponse {
            id: revision.id,
            article_content_id: revision.article_content_id,
            revision_number: revision.revision_number,
            contributed_by: revision.contributed_by,
            created_at: revision.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompleteTicket {
    #[serde(flatten)]
    pub ticket: Ticket,
    pub requester_user: Option<UserInfoWithAvatar>, // Complete requester data
    pub assignee_user: Option<UserInfoWithAvatar>,  // Complete assignee data
    pub devices: Vec<Asset>,
    pub comments: Vec<CommentWithAttachments>,
    pub article_content: Option<String>,
    pub linked_tickets: Vec<i32>,
    pub projects: Vec<Project>,
    /// Cycle membership, when the ticket belongs to one. Embeds
    /// the cycle's name + state so the detail sidebar can render
    /// the chip without a separate `/api/cycles` round-trip
    /// (the frontend cycles store is per-project keyed and the
    /// detail view doesn't necessarily know the cycle's project
    /// up-front). `None` for tickets not in any cycle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<TicketCycleSummary>,
    /// SLA pill payload — same shape `services::sla::compute_pill`
    /// produces for the bootstrap stream. Null when no policy /
    /// calendar matches the ticket. Lets the detail sidebar
    /// render the countdown / breach state without a second
    /// round-trip to recompute.
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub sla: serde_json::Value,
    /// Tag ids attached to the ticket. Frontend resolves each
    /// id to a `Tag` row via the workspace tag store. Empty
    /// array when no tags are attached. Sorted ascending for
    /// stable rendering.
    pub tag_ids: Vec<i32>,
    /// Uuids of users watching the ticket. Drives the watch /
    /// unwatch toggle button + the watchers list in the sidebar.
    /// Sorted by watch-creation time so the list reads
    /// chronologically. Comment notifications fan out to this
    /// set in addition to the requester / assignee.
    pub watcher_uuids: Vec<uuid::Uuid>,
}

/// Trimmed cycle projection for embedding inside a ticket detail
/// response. Carries the fields the sidebar pill renders (name +
/// state) plus the ids needed for navigation. Mirrors what the
/// frontend's `Cycle` type exposes minus the heavy fields
/// (snapshots, holiday lists) the pill never reads.
#[derive(Debug, Serialize, Deserialize)]
pub struct TicketCycleSummary {
    pub id: i32,
    pub uuid: Uuid,
    pub project_id: i32,
    pub name: String,
    pub state: String,
}

// Simplified ticket for lists - includes user info but not heavy data like comments
#[derive(Debug, Serialize, Deserialize)]
pub struct TicketListItem {
    #[serde(flatten)]
    pub ticket: Ticket,
    pub requester_user: Option<UserInfoWithAvatar>, // Complete requester data
    pub assignee_user: Option<UserInfoWithAvatar>,  // Complete assignee data
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentWithAttachments {
    #[serde(flatten)]
    pub comment: Comment,
    pub attachments: Vec<Attachment>,
    pub user: Option<UserInfoWithAvatar>, // Use enhanced user info with avatar
    /// Sender's external address (email for IMAP; equivalent identity
    /// for chat channels). Sourced from the joined `channel_messages`
    /// row when the comment came from a channel; `None` for comments
    /// authored through the helpdesk UI. Surfaced as a top-level field
    /// rather than digging into `channel_metadata` so the frontend
    /// reads a single typed field instead of probing a JSON blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_address: Option<String>,
    /// Whether this comment has an archived raw RFC-822 source the
    /// frontend can fetch via `GET /api/comments/{id}/raw.eml`.
    /// Derived from `Comment::raw_source_uri`'s presence so the
    /// frontend can render the "Show original message" affordance
    /// conditionally without learning the storage path itself.
    pub has_raw_source: bool,
}

// JSON import struct that matches the structure in tickets.json
#[derive(Debug, Serialize, Deserialize)]
pub struct TicketJson {
    pub id: i32,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub created: String,
    pub modified: String,
    pub assignee: String,
    pub requester: String,
    pub device: Option<AssetJson>,
    pub comments: Option<Vec<CommentJson>>,
    pub article_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetJson {
    pub id: String,
    pub name: String,
    pub hostname: String,
    #[serde(rename = "serialNumber")]
    pub serial_number: String,
    pub model: String,
    #[serde(rename = "warrantyStatus")]
    pub warranty_status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentJson {
    pub id: i32,
    pub content: String,
    pub user_uuid: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub attachments: Vec<AttachmentJson>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentJson {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TicketsJson {
    pub tickets: Vec<TicketJson>,
}

// Documentation Status Enum
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::DocumentationStatus)]
pub enum DocumentationStatus {
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "published")]
    Published,
    #[serde(rename = "archived")]
    Archived,
    #[serde(rename = "deleted")]
    Deleted,
}

impl ToSql<crate::schema::sql_types::DocumentationStatus, Pg> for DocumentationStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            DocumentationStatus::Draft => "draft",
            DocumentationStatus::Published => "published",
            DocumentationStatus::Archived => "archived",
            DocumentationStatus::Deleted => "deleted",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::DocumentationStatus, Pg> for DocumentationStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"draft" => Ok(DocumentationStatus::Draft),
            b"published" => Ok(DocumentationStatus::Published),
            b"archived" => Ok(DocumentationStatus::Archived),
            b"deleted" => Ok(DocumentationStatus::Deleted),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// Documentation Page
#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::documentation_pages)]
pub struct DocumentationPage {
    pub id: i32,
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: DocumentationStatus,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub last_edited_by: Uuid,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub archived_at: Option<chrono::NaiveDateTime>,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: bool,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    /// User who last marked the page as verified, or None if the
    /// page has never been verified.
    pub verified_by: Option<Uuid>,
    /// Timestamp of the last verification. Combined with
    /// verify_interval_days this drives the staleness banner.
    pub verified_at: Option<chrono::NaiveDateTime>,
    /// Days after verified_at before the page is considered stale.
    /// None means verification doesn't expire (evergreen reference
    /// docs).
    pub verify_interval_days: Option<i32>,
    pub workspace_id: i32,
    /// Fencing token from the per-document ownership claim (Phase 2
    /// affinity); see the note on `ArticleContent::fence_token`.
    pub fence_token: Option<i64>,
}

// Documentation Page with Children
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentationPageWithChildren {
    pub page: DocumentationPage,
    pub children: Vec<DocumentationPage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PageOrder {
    pub page_id: i32,
    pub display_order: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionOrder {
    pub collection_id: i32,
    pub display_order: i32,
}

// =====================================================================
// Phase 4 W2: split-role model. PlatformRole + WorkspaceRole sit
// alongside the legacy UserRole during the sweep, then UserRole is
// deleted in the cleanup migration.
// =====================================================================

/// Platform-wide privilege role. Replaces the global `users.role`
/// for non-workspace gating. Only two values: `platform_admin`
/// (super-user across the instance) and `user` (default, no
/// platform privileges). Stored on `users.platform_role`
/// (VARCHAR(32)) — string at the DB layer, enum at the application
/// layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRole {
    PlatformAdmin,
    /// Read-only access to the instance-wide audit surface. Holds no
    /// write access to any business entity and no admin-panel access
    /// beyond the audit view. Replaces the legacy
    /// `"audit_reviewer"`; audit reads are still additionally
    /// gated on the `audit:read` token scope.
    AuditReviewer,
    User,
}

impl PlatformRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlatformAdmin => "platform_admin",
            Self::AuditReviewer => "audit_reviewer",
            Self::User => "user",
        }
    }

    /// Parse a database string into a `PlatformRole`. Unknown values
    /// fall back to `User` — defensive against legacy rows or a
    /// hand-edited CHECK that hasn't caught up. The CHECK constraint
    /// makes this branch unreachable in practice but the caller still
    /// gets a typed value either way.
    pub fn from_db(s: &str) -> Self {
        match s {
            "platform_admin" => Self::PlatformAdmin,
            "audit_reviewer" => Self::AuditReviewer,
            _ => Self::User,
        }
    }

    pub fn is_platform_admin(&self) -> bool {
        matches!(self, Self::PlatformAdmin)
    }

    /// True for principals allowed to read the instance audit surface:
    /// platform admins and the dedicated audit reviewer. This is the
    /// role half of the audit gate; callers still AND it with the
    /// `audit:read` token scope.
    pub fn can_read_audit(&self) -> bool {
        matches!(self, Self::PlatformAdmin | Self::AuditReviewer)
    }
}

/// Per-workspace privilege role. Stored on `workspace_members.role`
/// (VARCHAR(32) CHECK IN ('owner', 'admin', 'agent', 'member')).
/// The ordering implements escalation: `Owner > Admin > Agent > Member`,
/// so `require_workspace_role(Agent)` admits owners and admins as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    /// File tickets, read docs. Default for new workspace members.
    Member,
    /// Handle tickets (was global `technician` in the pre-W2 model).
    Agent,
    /// Manage workspace members + settings.
    Admin,
    /// One per workspace. Can delete the workspace.
    Owner,
}

impl WorkspaceRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Agent => "agent",
            Self::Member => "member",
        }
    }

    /// Parse a database string into a `WorkspaceRole`. Unknown
    /// values fall back to `Member` — same defensive shape as
    /// [`PlatformRole::from_db`].
    pub fn from_db(s: &str) -> Self {
        match s {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            "agent" => Self::Agent,
            _ => Self::Member,
        }
    }

    /// True if `self` meets or exceeds `min` per the role ordering
    /// (Owner > Admin > Agent > Member).
    pub fn meets(&self, min: WorkspaceRole) -> bool {
        *self >= min
    }

    /// True for staff roles (Owner / Admin / Agent) — the seats that count
    /// toward a workspace's `seat_limit`. End-user `Member` (ticket
    /// requesters) are uncapped.
    pub fn is_staff(&self) -> bool {
        self.meets(WorkspaceRole::Agent)
    }
}

// User model - updated to match the actual database schema
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::users)]
#[diesel(primary_key(uuid))]
pub struct User {
    pub uuid: Uuid,
    pub name: String,
    // Email removed - now stored in user_emails table only
    // `role` (UserRole) was the pre-W2 column. W2 split it into
    // `platform_role` (kept below) and per-workspace
    // `workspace_members.role`. The legacy projection lives on
    // `AuthContext::role` (derived) for handler code that branches
    // on staff vs end-user.
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub password_changed_at: Option<NaiveDateTime>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub mfa_enabled: bool,
    // Recovery codes moved to the dedicated `user_recovery_codes`
    // table (migration `2026-05-31-180000_decouple_user_recovery_codes`).
    // Use `repository::user_recovery_codes` for reads / writes; this
    // table no longer carries them.
    /// Per-user feature flag overrides merged on top of the
    /// workspace defaults at request time. Same JSONB shape as
    /// `site_settings.feature_flags`. Used to opt individuals into
    /// staged rollouts before flipping the workspace default.
    /// Stays on `users` (not in `user_preferences`) because it's
    /// an admin-set override, not a user-chosen preference.
    pub feature_flag_overrides: serde_json::Value,
    /// Set when an admin soft-deletes the user. Non-null rows are
    /// hidden from "find an active user" code paths (login, mention
    /// search, assignee pickers, the default paginated list) and
    /// scheduled for purge by the retention worker once
    /// `deleted_at + NOSDESK_USER_PURGE_GRACE_DAYS` has elapsed.
    /// Historical references (audit log, ticket history) keep
    /// rendering the user so the record stays coherent during the
    /// window.
    pub deleted_at: Option<NaiveDateTime>,
    /// Framed AES-256-GCM blob (`utils::encryption::Keyring` shape).
    /// AAD = `uuid.as_bytes()` so a row swap fails the tag check.
    /// `mfa_secret_kek_id` mirrors the version encoded in the blob;
    /// they MUST agree on read or the row is rejected.
    pub mfa_secret: Option<Vec<u8>>,
    pub mfa_secret_kek_id: Option<i16>,
    /// Platform-wide privilege role (Phase 4 W2). Values:
    /// `"platform_admin"` (super-user — workspace lifecycle,
    /// instance settings, hosted billing) or `"user"` (default).
    /// Read via [`PlatformRole::from_str`] for typed access. Will
    /// supersede [`UserRole`] for non-workspace privilege gating
    /// once the post-W2 sweep removes the legacy column.
    pub platform_role: String,
}

// New user for creation
// Note: Email is no longer part of NewUser - it's created separately in user_emails table
#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct NewUser {
    pub uuid: Uuid,
    pub name: String,
    // Email removed - handled separately via user_emails table
    // `role` removed by the W2 column drop. Callers thread the
    // intended `UserRole` through `create_user_with_email` as a
    // separate parameter and that helper derives platform_role +
    // seeds the workspace_members row from it.
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub mfa_secret: Option<Vec<u8>>,
    pub mfa_secret_kek_id: Option<i16>,
    pub mfa_enabled: bool,
    /// Phase 4 W2: platform-wide privilege role. `None` leaves the
    /// DB default (`'user'`) so existing callers don't need to
    /// thread the value through. W2-aware callers set
    /// `Some("platform_admin".into())` for the bootstrap / hosted
    /// signup paths.
    pub platform_role: Option<String>,
    // mfa_backup_codes lives in `user_recovery_codes` now.
}

// User update struct
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserUpdate {
    pub name: Option<String>,
    // Email removed - update via user_emails table instead
    // `role` dropped with the column; admin role changes now go
    // through workspace_members.role (admin_workspaces handlers).
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

// User update with password for admin/user management
#[derive(Debug, Serialize, Deserialize)]
pub struct UserUpdateWithPassword {
    pub name: Option<String>,
    // Email removed - update via user_emails table
    pub role: Option<String>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub password: Option<String>,
    /// Free-form text appended to outbound channel replies as the
    /// agent's signature. `None` in the payload → no change. Empty
    /// string clears it.
    pub signature: Option<String>,
    /// Dashboard layout JSON (see `UserUpdate::dashboard_layout`).
    #[serde(default)]
    pub dashboard_layout: Option<serde_json::Value>,
    /// BCP-47 locale preference. `None` in the payload = no change;
    /// empty string = clear back to "inherit site default".
    #[serde(default)]
    pub locale: Option<String>,
    /// IANA timezone preference. Same omission / empty-string
    /// semantics as `locale`.
    #[serde(default)]
    pub timezone: Option<String>,
}

// User profile update for profile management
#[derive(Debug, Serialize, Deserialize)]
pub struct UserProfileUpdate {
    pub name: Option<String>,
    // Email removed - update via user_emails table
    pub role: Option<String>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub password: Option<String>,
    /// Email signature appended to outbound channel replies.
    pub signature: Option<String>,
}

// User response with minimal information.
//
// `theme` / `dashboard_layout` / `signature` / `locale` /
// `timezone` are flattened in from the `user_preferences` row by
// `repository::user_helpers::get_user_with_primary_email` so the
// API shape stays stable for the frontend even though these
// fields now live in a separate table.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub uuid: Uuid,
    pub name: String,
    pub email: Option<String>, // Now optional - populated from user_emails table
    /// Platform-wide privilege role (platform_admin / audit_reviewer /
    /// user). Replaces the legacy derived `role`.
    pub platform_role: PlatformRole,
    /// The user's role in the workspace this response was built for
    /// (owner / admin / agent / member), or null when there's no
    /// membership / the response wasn't built with a workspace
    /// connection. The `From<User>` conversion (no DB access) always
    /// leaves this null; the populated builders fill it from
    /// `workspace_members`.
    pub workspace_role: Option<WorkspaceRole>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_ticket_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_count: Option<i64>,
    /// Per-user dashboard layout JSON, or null = client uses defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard_layout: Option<serde_json::Value>,
    /// Free-form email signature appended to outbound replies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// BCP-47 locale (e.g. en-US). None means "inherit from
    /// site default". Frontend reads this on app boot to decide
    /// the i18n bundle to load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// IANA timezone (e.g. Europe/Berlin). None means "inherit
    /// from site default".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Resolved locale after walking user pref -> site default ->
    /// hardcoded fallback. Populated only by /auth/me; admin user
    /// listings leave it None to avoid the extra site_settings
    /// fetch per row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_locale: Option<String>,
    /// Resolved timezone after the same fallback chain. Same /me-
    /// only population rule as `effective_locale`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_timezone: Option<String>,
}

// ============================================================================
// User preferences — split from `users` in 2026-05-14 once the
// preference set grew past the few-columns-on-the-main-table
// threshold. A row exists for every user (auto-created by trigger).
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::user_preferences)]
#[diesel(primary_key(user_uuid))]
pub struct UserPreferences {
    pub user_uuid: Uuid,
    pub theme: Option<String>,
    pub signature: Option<String>,
    pub dashboard_layout: Option<serde_json::Value>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Partial update payload. Each field uses the `Option<Option<T>>`
/// convention so the API can distinguish "leave as-is" (outer
/// None) from "clear back to site default / role default"
/// (Some(None)) from "set to this value" (Some(Some(_))).
#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_preferences)]
pub struct UpdateUserPreferences {
    pub theme: Option<Option<String>>,
    pub signature: Option<Option<String>>,
    pub dashboard_layout: Option<Option<serde_json::Value>>,
    pub locale: Option<Option<String>>,
    pub timezone: Option<Option<String>>,
}

// User info for comments - minimal user data to include with comments
#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub uuid: Uuid,
    pub name: String,
}

// Enhanced UserInfo with avatar data for efficient frontend display
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserInfoWithAvatar {
    pub uuid: Uuid,
    pub name: String,
    pub avatar_url: Option<String>,
    pub avatar_thumb: Option<String>,
}

// Convert a bare User to UserResponse.
//
// Email + preference fields (theme, signature, dashboard_layout,
// locale, timezone) all come from other tables; this From impl
// leaves them None. Callers that need the fully-populated shape
// use `repository::user_helpers::get_user_with_primary_email`,
// which does the joins and fills them in.
impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        // The conversion has no DB access, so it can only surface the
        // platform role (which lives on the User row). The
        // workspace_role is left None; callers that need it should
        // build UserResponse via
        // `repository::user_helpers::get_user_with_primary_email`
        // (which has a connection and looks up workspace_members).
        UserResponse {
            uuid: user.uuid,
            name: user.name,
            email: None,
            platform_role: PlatformRole::from_db(&user.platform_role),
            workspace_role: None,
            pronouns: user.pronouns,
            avatar_url: user.avatar_url,
            banner_url: user.banner_url,
            avatar_thumb: user.avatar_thumb,
            theme: None,
            microsoft_uuid: user.microsoft_uuid,
            created_at: user.created_at,
            updated_at: user.updated_at,
            open_ticket_count: None,
            device_count: None,
            dashboard_layout: None,
            signature: None,
            locale: None,
            timezone: None,
            effective_locale: None,
            effective_timezone: None,
        }
    }
}

// Convert User to UserInfo
impl From<User> for UserInfo {
    fn from(user: User) -> Self {
        UserInfo {
            uuid: user.uuid,
            name: user.name,
        }
    }
}

impl From<User> for UserInfoWithAvatar {
    fn from(user: User) -> Self {
        UserInfoWithAvatar {
            uuid: user.uuid,
            name: user.name,
            avatar_url: user.avatar_url,
            avatar_thumb: user.avatar_thumb,
        }
    }
}

// User Email models for storing multiple email addresses per user
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::user_emails)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
pub struct UserEmail {
    pub id: i32,
    pub user_uuid: Uuid,
    pub email: String,
    pub email_type: String,
    pub is_primary: bool,
    pub is_verified: bool,
    pub source: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_emails)]
pub struct NewUserEmail {
    pub user_uuid: Uuid,
    pub email: String,
    pub email_type: String,
    pub is_primary: bool,
    pub is_verified: bool,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_emails)]
pub struct UserEmailUpdate {
    pub is_primary: Option<bool>,
    pub is_verified: Option<bool>,
    pub updated_at: Option<NaiveDateTime>,
}

// Extended User response that includes all email addresses
#[derive(Debug, Serialize, Deserialize)]
pub struct UserWithEmails {
    #[serde(flatten)]
    pub user: UserResponse,
    pub emails: Vec<UserEmail>,
}

// Project Status Enum
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::ProjectStatus)]
pub enum ProjectStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "archived")]
    Archived,
}

impl ToSql<crate::schema::sql_types::ProjectStatus, Pg> for ProjectStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            ProjectStatus::Active => "active",
            ProjectStatus::Completed => "completed",
            ProjectStatus::Archived => "archived",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::ProjectStatus, Pg> for ProjectStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"active" => Ok(ProjectStatus::Active),
            b"completed" => Ok(ProjectStatus::Completed),
            b"archived" => Ok(ProjectStatus::Archived),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// Project model
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::projects)]
pub struct Project {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub owner_uuid: Option<Uuid>,
    pub workspace_id: i32,
}

// New Project for creating projects
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::projects)]
pub struct NewProject {
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

// Project Update for partial updates
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::projects)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<ProjectStatus>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub updated_at: Option<NaiveDateTime>,
}

// Project Ticket association
#[derive(Debug, Serialize, Deserialize, Identifiable, Associations, Queryable)]
#[diesel(belongs_to(Project))]
#[diesel(belongs_to(Ticket))]
#[diesel(table_name = crate::schema::project_tickets)]
#[diesel(primary_key(project_id, ticket_id))]
pub struct ProjectTicket {
    pub project_id: i32,
    pub ticket_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub display_order: i32,
    pub workspace_id: i32,
}

// New Project Ticket for creating associations
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::project_tickets)]
pub struct NewProjectTicket {
    pub project_id: i32,
    pub ticket_id: i32,
    pub display_order: i32,
}

// Project with ticket count for API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectWithTicketCount {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub ticket_count: i64,
    /// Optional embedded ticket list, populated only when the
    /// `GET /projects/{id}?embed=tickets` flag is set. Skipped from
    /// JSON when absent so the legacy unbundled response shape is
    /// unchanged for existing callers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tickets: Option<Vec<TicketListItem>>,
}

// LinkedTicket model
#[derive(Debug, Serialize, Deserialize, Identifiable, Associations, Queryable)]
#[diesel(table_name = crate::schema::linked_tickets)]
#[diesel(primary_key(ticket_id, linked_ticket_id))]
#[diesel(belongs_to(Ticket, foreign_key = ticket_id))]
pub struct LinkedTicket {
    pub ticket_id: i32,
    pub linked_ticket_id: i32,
    /// One of `blocks` / `blocked_by` / `related` / `duplicate_of`.
    /// Locked at the DB layer by `linked_tickets_relation_type_check`.
    pub relation_type: String,
    pub description: Option<String>,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::linked_tickets)]
pub struct NewLinkedTicket {
    pub ticket_id: i32,
    pub linked_ticket_id: i32,
    /// One of `blocks` / `blocked_by` / `related` / `duplicate_of`,
    /// locked by `linked_tickets_relation_type_check`. The DB default
    /// is `related`; the column is spelled out here so the merge path
    /// can write `duplicate_of` edges directly.
    pub relation_type: String,
    /// Optional context for the relationship (e.g. the merge reason).
    pub description: Option<String>,
    /// Actor who created the edge. NULL for system-created links.
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentData {
    pub id: Option<i32>,
    pub url: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewCommentWithAttachments {
    pub content: String,
    // user_id/user_uuid removed - extracted from JWT token for security
    pub attachments: Vec<AttachmentData>,
    /// Format the editor that produced `content` is sending. Optional
    /// from the wire so older clients keep working — the handler falls
    /// back to the `ContentFormat` default (HTML).
    #[serde(default)]
    pub content_format: ContentFormat,
}

// JWT Claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,   // Subject (user UUID as string for JWT compatibility)
    pub name: String,  // User's name
    pub email: String, // User's email
    /// Platform-wide privilege role: `"platform_admin"` /
    /// `"audit_reviewer"` / `"user"`. The per-workspace role is NOT
    /// carried in the token (the JWT stays workspace-independent); it
    /// is resolved per-request from `workspace_members`. Defaulted on
    /// deserialize so a stray pre-W2 token without the claim degrades
    /// to a plain user (and is re-minted with the real value on the
    /// next 15-minute refresh) rather than failing to parse.
    #[serde(default = "default_platform_role")]
    pub platform_role: String,
    #[serde(default = "default_scope")]
    // Default to "full" for backward compatibility with existing tokens
    pub scope: String, // Token scope: "full" for normal sessions
    #[serde(default)] // Session ID (UUID) — None for SSE/API tokens
    pub sid: Option<String>,
    /// Workspace selected when an SSE token was minted (Model C). EventSource
    /// can't send the `X-Nosdesk-Workspace` header, so the selected workspace
    /// is bound into the SSE token instead and the stream authorizes against
    /// it. `None` on session/API tokens (which resolve the workspace per
    /// request) and on SSE tokens minted before this claim existed (the stream
    /// falls back to the Host-derived context).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_uuid: Option<Uuid>,
    pub exp: usize, // Expiration time
    pub iat: usize, // Issued at
}

impl Claims {
    /// Parse the `sid` claim into a UUID. Returns None for SSE/API tokens.
    pub fn session_uuid(&self) -> Option<Uuid> {
        self.sid.as_deref().and_then(|s| s.parse().ok())
    }
}

// Default scope for backward compatibility
fn default_scope() -> String {
    "full".to_string()
}

// Default platform role for tokens minted before the claim existed.
fn default_platform_role() -> String {
    "user".to_string()
}

// Login request structure
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

// Login response structure - supports both standard login and MFA flow
// Note: tokens are now in httpOnly cookies, only CSRF token is in response body
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub mfa_required: Option<bool>,
    pub mfa_setup_required: Option<bool>,
    pub passkey_mfa_required: Option<bool>,
    pub user_uuid: Option<String>,
    pub csrf_token: Option<String>, // CSRF token for the frontend
    pub user: Option<UserResponse>,
    pub message: Option<String>,
    pub mfa_backup_code_used: Option<bool>,
    pub requires_backup_code_regeneration: Option<bool>,
    pub backup_codes: Option<Vec<String>>, // Present when MFA is enabled during login setup
}

/// Request for MFA verification during login
#[derive(Debug, Deserialize)]
pub struct MfaLoginRequest {
    pub email: String,
    pub password: String,
    pub mfa_token: String,
}

/// Request for recovery code login (passkey-MFA users who can't use their passkey)
#[derive(Debug, Deserialize)]
pub struct RecoveryLoginRequest {
    pub email: String,
    pub password: String,
    pub recovery_code: String,
}

/// Request for MFA setup during login (unauthenticated)
#[derive(Debug, Deserialize)]
pub struct MfaSetupLoginRequest {
    pub email: String,
    pub password: String,
}

/// Request for enabling MFA during login (unauthenticated).
///
/// The TOTP secret is intentionally NOT in this struct: the matching
/// `mfa_setup_login` call stashes it server-side and the enable
/// handler retrieves it from there. Accepting it from the client
/// would let an attacker who knew the victim's password substitute
/// their own attacker-controlled secret + code and enroll their
/// authenticator on the victim's account.
#[derive(Debug, Deserialize)]
pub struct MfaEnableLoginRequest {
    pub email: String,
    pub password: String,
    pub token: String,
}

/// Response for token refresh
/// Note: tokens are now in httpOnly cookies, only CSRF token is in response
#[derive(Debug, Serialize)]
pub struct RefreshTokenResponse {
    pub success: bool,
    pub csrf_token: String,
}

#[derive(Deserialize, Debug)]
pub struct PasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
}

// Authentication Provider models
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum AuthProviderType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "microsoft")]
    Microsoft,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "saml")]
    Saml,
}

impl ToSql<diesel::sql_types::Text, Pg> for AuthProviderType {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            AuthProviderType::Local => "local",
            AuthProviderType::Microsoft => "microsoft",
            AuthProviderType::Google => "google",
            AuthProviderType::Saml => "saml",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<diesel::sql_types::Text, Pg> for AuthProviderType {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"local" => Ok(AuthProviderType::Local),
            b"microsoft" => Ok(AuthProviderType::Microsoft),
            b"google" => Ok(AuthProviderType::Google),
            b"saml" => Ok(AuthProviderType::Saml),
            _ => Err("Unrecognized auth provider type".into()),
        }
    }
}

// Environment-based AuthProvider struct (replaces database-stored providers)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthProvider {
    pub id: i32,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub is_default: bool,
}

impl AuthProvider {
    pub fn new(
        id: i32,
        name: String,
        provider_type: String,
        enabled: bool,
        is_default: bool,
    ) -> Self {
        Self {
            id,
            name,
            provider_type,
            enabled,
            is_default,
        }
    }
}

// Request models for authentication
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderConfigRequest {
    pub provider_id: i32,
    pub configs: Vec<ConfigItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigItem {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

// Response model for client display
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderWithConfig {
    pub id: i32,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    pub is_default: bool,
    pub configs: Vec<AuthProviderConfigResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderConfigResponse {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

// OAuth state management
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthState {
    pub state: String,
    pub redirect_uri: String,
    pub provider_type: String,
    pub exp: usize,
    pub user_connection: Option<bool>,
    /// PKCE code verifier (for OIDC providers)
    pub pkce_verifier: Option<String>,
    /// Nonce for ID token validation (for OIDC providers)
    pub nonce: Option<String>,
    /// OAuth `redirect_uri` (the IdP callback) used for THIS flow, bound
    /// at initiation so the token exchange presents the identical value.
    /// In hosted mode each tenant authenticates on its own subdomain, so
    /// this is `https://<tenant-host>/api/auth/oauth/callback`, derived
    /// from the initiating request's `Host`. `None` means "use the
    /// statically configured `OIDC_REDIRECT_URI`" (self-hosted, and legacy
    /// in-flight tokens minted before this field existed).
    #[serde(default)]
    pub callback_redirect_uri: Option<String>,
    /// Per-flow random value bound to the initiating user-agent via the
    /// `oauth_state` cookie (RFC 9700 §2.1). The callback rejects unless the
    /// cookie matches this value, so an attacker can't CSRF their own
    /// `(code, state)` onto a victim (login-CSRF / session swap). `None` for
    /// legacy in-flight tokens minted before this field existed (a <=10-minute
    /// transition window, after which every state carries a binding).
    #[serde(default)]
    pub binding: Option<String>,
}

// OAuth Authentication request
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthRequest {
    pub provider_type: String,
    pub redirect_uri: Option<String>,
    pub user_connection: Option<bool>,
}

// OAuth callback/exchange parameters
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OAuthExchangeRequest {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

// Microsoft Entra specific models
#[derive(Debug, Serialize, Deserialize)]
pub struct MicrosoftAuthConfig {
    pub client_id: String,
    pub tenant_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

// Models for user authentication identities
#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::user_auth_identities)]
pub struct UserAuthIdentity {
    pub id: i32,
    pub user_uuid: Uuid,
    pub provider_type: String,
    pub external_id: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub password_hash: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_auth_identities)]
pub struct NewUserAuthIdentity {
    pub user_uuid: Uuid,
    pub provider_type: String,
    pub external_id: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub password_hash: Option<String>,
}

// For displaying auth identities in the user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct UserAuthIdentityDisplay {
    pub id: i32,
    pub provider_type: String,
    pub provider_name: String,
    pub email: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_pages)]
pub struct NewDocumentationPage {
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: DocumentationStatus,
    pub created_by: Uuid,
    pub last_edited_by: Uuid,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_pages)]
pub struct DocumentationPageUpdate {
    pub title: Option<String>,
    pub slug: Option<String>,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: Option<DocumentationStatus>,
    pub last_edited_by: Option<Uuid>,
    pub parent_id: Option<Option<i32>>,
    pub display_order: Option<i32>,
    pub is_public: Option<bool>,
    pub is_template: Option<bool>,
    pub archived_at: Option<Option<chrono::NaiveDateTime>>,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: Option<bool>,
    pub updated_at: Option<chrono::NaiveDateTime>,
    pub deleted_at: Option<Option<chrono::NaiveDateTime>>,
    pub verified_by: Option<Option<Uuid>>,
    pub verified_at: Option<Option<chrono::NaiveDateTime>>,
    pub verify_interval_days: Option<Option<i32>>,
}

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::documentation_revisions)]
pub struct DocumentationRevision {
    pub id: i32,
    pub page_id: i32,
    pub revision_number: i32,
    pub title: String,
    pub yjs_document_snapshot: Vec<u8>,
    pub yjs_state_vector: Vec<u8>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub change_summary: Option<String>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_revisions)]
pub struct NewDocumentationRevision {
    pub page_id: i32,
    pub revision_number: i32,
    pub title: String,
    pub yjs_document_snapshot: Vec<u8>,
    pub yjs_state_vector: Vec<u8>,
    pub created_by: Uuid,
    pub change_summary: Option<String>,
}

// Response models for API
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentationPageResponse {
    pub id: i32,
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: DocumentationStatus,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: UserInfoWithAvatar,
    pub last_edited_by: UserInfoWithAvatar,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub archived_at: Option<chrono::NaiveDateTime>,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    pub has_unsaved_changes: bool,
    pub children: Option<Vec<DocumentationPageResponse>>,
    pub content: Option<String>,
    /// Verifier (resolved with avatar). None if the page has never
    /// been verified.
    pub verified_by: Option<UserInfoWithAvatar>,
    pub verified_at: Option<chrono::NaiveDateTime>,
    pub verify_interval_days: Option<i32>,
    /// Computed convenience for the frontend: true when the page
    /// has been verified, has an interval set, and the verification
    /// has expired. Pages with no interval are never stale.
    pub is_stale: bool,
    /// True when any collection containing this page has
    /// `require_verification` set. Gates the "needs verification"
    /// prompt for never-verified pages; false by default so an
    /// unverified page reads as neutral, not unchecked.
    pub requires_verification: bool,
    /// Embedded ticket links, populated when the caller passes
    /// `?embed=tickets`. None means the field wasn't requested
    /// (which is different from "no links" — that's `Some(vec![])`).
    /// Skipped from JSON when None so list responses stay lean.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_tickets: Option<Vec<DocumentationPageTicketEmbed>>,
}

/// Slim hydrated ticket-link record returned inline on a page when
/// `?embed=tickets` is requested. Mirrors the standalone
/// PageTicketLinkResponse from the page-tickets endpoint, kept
/// in this module so the type lives next to its consumer.
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentationPageTicketEmbed {
    pub ticket_id: i32,
    pub link_type: String,
    pub created_at: NaiveDateTime,
    pub ticket_title: Option<String>,
    pub ticket_category: Option<WorkflowStateCategory>,
}

// Sync History Models
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct SyncHistory {
    pub id: i32,
    pub sync_type: String,
    pub status: String,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
    pub tenant_id: Option<String>,
    pub initiated_by: Option<Uuid>,
    pub is_delta: bool,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct NewSyncHistory {
    pub sync_type: String,
    pub status: String,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
    pub tenant_id: Option<String>,
    pub is_delta: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct SyncHistoryUpdate {
    pub status: Option<String>,
    pub completed_at: Option<Option<NaiveDateTime>>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
}

// Delta tokens for incremental sync (Microsoft Graph delta queries)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct SyncDeltaToken {
    pub id: i32,
    pub provider_type: String,
    pub entity_type: String,
    pub delta_link: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct NewSyncDeltaToken {
    pub provider_type: String,
    pub entity_type: String,
    pub delta_link: String,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct SyncDeltaTokenUpdate {
    pub delta_link: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProgressPoint {
    pub name: String,
    pub sort_order: i32,
}

// Onboarding models
#[derive(Debug, Serialize, Deserialize)]
pub struct OnboardingStatus {
    pub requires_setup: bool,
    pub user_count: i64,
    pub microsoft_auth_enabled: bool,
    pub oidc_enabled: bool,
    pub oidc_display_name: Option<String>,
    /// True when local credential auth (password + passkey) is disabled and
    /// the platform OIDC is the only sign-in path (hosted mode). The login
    /// UI hides the password/passkey forms and auto-initiates SSO.
    pub local_auth_disabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct AdminSetupRequest {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminSetupResponse {
    pub success: bool,
    pub message: String,
    pub user: Option<UserResponse>,
}

// Frontend-compatible version of CompleteTicket
#[derive(Debug, Serialize)]
pub struct CompleteTicketResponse {
    pub id: i32,
    pub title: String,
    /// Legacy three-bucket status string derived from the workflow state's
    /// category. Kept for frontend wire compatibility while the UI is
    /// migrated to read `workflow_state_id` and the joined `workflow_state`
    /// directly. Remove once the frontend stops reading it.
    pub status: String,
    pub workflow_state_id: i32,
    pub workflow_state: Option<WorkflowState>,
    pub priority: TicketPriority,
    pub requester: String,
    pub assignee: String,
    pub created: String,
    pub modified: String,
    pub devices: Vec<Asset>,
    pub comments: Vec<CommentWithAttachments>,
    pub article_content: Option<String>,
    pub linked_tickets: Vec<i32>,
    pub projects: Vec<Project>,
}

impl CompleteTicketResponse {}

// === MFA (Multi-Factor Authentication) Models ===

/// QR code matrix data for frontend rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrMatrix {
    /// Width/height of the QR code (always square)
    pub size: usize,
    /// Flattened boolean array (row-major order), true = dark module
    pub data: Vec<bool>,
}

/// Response for MFA setup request
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaSetupResponse {
    pub secret: String,
    pub qr_code: String,
    pub backup_codes: Vec<String>,
    /// QR code matrix data for animated rendering
    pub qr_matrix: Option<QrMatrix>,
}

/// Request for verifying MFA setup
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaVerifySetupRequest {
    pub token: String,
    pub secret: String,
}

/// Response for MFA setup verification
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaVerifySetupResponse {
    pub success: bool,
    pub backup_codes: Vec<String>,
}

/// Request for enabling MFA. The TOTP secret is intentionally NOT
/// in this struct (see `MfaEnableLoginRequest` for the threat model);
/// it lives in the server-side setup cache keyed by user uuid.
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaEnableRequest {
    pub token: String,
    pub backup_codes: Option<Vec<String>>,
}

/// Request for disabling MFA
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaDisableRequest {
    pub password: String,
}

/// Step-up credential for "sign out all other sessions". The caller
/// supplies whichever they have: a local password, or a TOTP / backup
/// code. Both optional so an OAuth-only account with no MFA (nothing to
/// step up with) can still call the endpoint on a full session.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RevokeOtherSessionsRequest {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub mfa_code: Option<String>,
}

/// Request for regenerating backup codes
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaRegenerateBackupCodesRequest {
    pub password: String,
}

/// Response for regenerating backup codes
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaRegenerateBackupCodesResponse {
    pub backup_codes: Vec<String>,
}

/// Response for MFA status
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaStatusResponse {
    pub enabled: bool,
    pub has_backup_codes: bool,
}

/// Update struct for user MFA fields. Recovery codes live in
/// `user_recovery_codes` now — see
/// `repository::user_recovery_codes::replace_all` for the atomic
/// "rotate codes" operation that used to be a JSONB array swap on
/// this row.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserMfaUpdate {
    pub mfa_secret: Option<Option<Vec<u8>>>,
    pub mfa_secret_kek_id: Option<Option<i16>>,
    pub mfa_enabled: Option<bool>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

// ===== USER RECOVERY CODES =====

/// One row of `user_recovery_codes`. Each MFA backup/recovery code
/// is stored as its own row keyed by a `BIGSERIAL` id, with the
/// hash opaque and a nullable `used_at` recording the moment a
/// successful verify consumed it.
///
/// The atomicity invariant is held by Postgres, not the app:
/// consumption is a single `UPDATE … WHERE id = $1 AND used_at IS
/// NULL RETURNING …` so two concurrent verifies racing the same
/// code resolve to one succeeded, one failed at the row-level
/// lock. The earlier JSONB-array design forced a read-modify-write
/// of the full array per consumption and lost concurrent
/// consumptions to last-write-wins.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::user_recovery_codes)]
pub struct UserRecoveryCode {
    pub id: i64,
    pub user_uuid: Uuid,
    pub code_hash: String,
    pub used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::user_recovery_codes)]
pub struct NewUserRecoveryCode {
    pub user_uuid: Uuid,
    pub code_hash: String,
}

// ===== PASSKEY CREDENTIAL MODELS =====

/// One row of `passkey_credentials`. Each WebAuthn credential is
/// stored as its own row with a unique `credential_id`, replacing
/// the earlier JSONB-blob-on-users design.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::passkey_credentials)]
pub struct PasskeyCredential {
    pub id: Uuid,
    pub user_uuid: Uuid,
    pub credential_id: String,
    pub name: String,
    pub credential: serde_json::Value,
    pub transports: Vec<Option<String>>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    /// WebAuthn sign counter (u32 in the spec, stored as i8/BIGINT to
    /// fit Postgres's signed integer types). Bumped on every
    /// successful authentication by `webauthn::update_credential_post_auth`.
    /// The counter inside `credential` JSONB is kept in lockstep so
    /// the library's regression check works on rehydrated `Passkey`.
    pub sign_count: i64,
    /// Set when the WebAuthn `backup_state` flag flips between
    /// authentications (e.g. credential synced to a new ecosystem).
    /// Pairs with the `passkey_backup_state_changed` security event.
    pub backup_state_changed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::passkey_credentials)]
pub struct NewPasskeyCredential {
    pub user_uuid: Uuid,
    pub credential_id: String,
    pub name: String,
    pub credential: serde_json::Value,
    pub transports: Vec<Option<String>>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    // `sign_count` defaults to 0 at the column level; no need to set
    // explicitly on insert. A freshly-registered credential's first
    // authentication will bump it past 0 (or stay at 0 for the
    // counter-less authenticator case, per WebAuthn §6.1.1).
}

/// Subset for updating mutable fields. Two clusters:
///   - `name` — user-initiated rename.
///   - `last_used_at`, `credential`, `sign_count`, `backup_state`,
///     `backup_state_changed_at` — written together by the post-auth
///     hook (`webauthn::update_credential_post_auth`) so the
///     denormalised columns and the JSONB blob never disagree.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::passkey_credentials)]
pub struct PasskeyCredentialUpdate {
    pub name: Option<String>,
    pub last_used_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    /// Full JSONB blob — written when the embedded counter changes
    /// so the library's `Passkey` deserialisation sees the current
    /// value.
    pub credential: Option<serde_json::Value>,
    /// Denormalised sign counter for fast clone-detection queries.
    pub sign_count: Option<i64>,
    /// Current backup state from the most recent assertion; flips
    /// here drive the `passkey_backup_state_changed` security event.
    pub backup_state: Option<bool>,
    /// Stamped at the moment a backup_state flip is observed.
    pub backup_state_changed_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
}

// ===== SESSION MANAGEMENT MODELS =====

/// Active user sessions for session management and revocation
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct ActiveSession {
    pub id: i32,
    pub user_uuid: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub is_current: bool,
    pub session_id: Uuid,
}

/// New active session for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct NewActiveSession {
    pub user_uuid: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub expires_at: chrono::NaiveDateTime,
    pub is_current: bool,
}

/// Update struct for active sessions
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct ActiveSessionUpdate {
    pub last_active: Option<chrono::NaiveDateTime>,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub is_current: Option<bool>,
}

/// Refresh token for JWT token rotation
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct RefreshToken {
    pub id: i32,
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub session_id: Option<Uuid>,
    pub family_id: Uuid,
    pub is_used: bool,
    pub used_at: Option<chrono::NaiveDateTime>,
    pub replaced_by_hash: Option<String>,
    pub grace_expires_at: Option<chrono::NaiveDateTime>,
}

/// New refresh token for creation
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct NewRefreshToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub expires_at: chrono::NaiveDateTime,
    pub session_id: Option<Uuid>,
    pub family_id: Uuid,
}

// ===== IDEMPOTENCY KEY MODELS =====

/// Cached response for a previously-seen `Idempotency-Key` header.
/// Looked up by the Idempotency middleware (M5 Task 2) to short-
/// circuit retries on POST / PUT / PATCH callbacks from the
/// control-plane provisioning worker.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::idempotency_keys)]
#[diesel(primary_key(key))]
pub struct IdempotencyRecord {
    pub key: String,
    pub response_body: serde_json::Value,
    pub response_status: i16,
    pub created_at: chrono::NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::idempotency_keys)]
pub struct NewIdempotencyRecord {
    pub key: String,
    pub response_body: serde_json::Value,
    pub response_status: i16,
}

// ===== API TOKEN MODELS =====

/// API token for programmatic access (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::api_tokens)]
pub struct ApiToken {
    pub id: i32,
    pub uuid: Uuid,
    pub token_hash: String,
    pub token_prefix: String,
    pub user_uuid: Uuid,
    pub name: String,
    pub scopes: Option<Vec<Option<String>>>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub last_used_at: Option<chrono::NaiveDateTime>,
    pub last_used_ip: Option<ipnetwork::IpNetwork>,
    pub workspace_id: i32,
}

/// New API token for insertion. All tokens are user-bound; the
/// control-plane provisioning surface authenticates with an EdDSA JWT
/// (see `extractors::PlatformAuth`), not an api_token.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::api_tokens)]
pub struct NewApiToken {
    pub token_hash: String,
    pub token_prefix: String,
    pub user_uuid: Uuid,
    pub name: String,
    pub scopes: Option<Vec<Option<String>>>,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// Request to create a new API token
#[derive(Debug, Deserialize)]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub user_uuid: Uuid,
    #[serde(default)]
    pub expires_in_days: Option<i64>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
}

/// Response when an API token is created (includes the raw token - only shown once!)
#[derive(Debug, Serialize)]
pub struct ApiTokenCreatedResponse {
    pub uuid: Uuid,
    pub token: String,
    pub token_prefix: String,
    pub name: String,
    pub user_uuid: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// API token info for listing (no sensitive data)
#[derive(Debug, Serialize)]
pub struct ApiTokenInfo {
    pub uuid: Uuid,
    pub token_prefix: String,
    pub name: String,
    pub user_uuid: Uuid,
    pub user_name: String,
    pub scopes: Vec<String>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by_name: String,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub last_used_at: Option<chrono::NaiveDateTime>,
}

/// Response model for active sessions in user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct ActiveSessionResponse {
    pub id: i32,
    pub session_id: String,
    pub device_name: Option<String>,
    pub location: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
    pub is_current: bool,
}

impl From<ActiveSession> for ActiveSessionResponse {
    fn from(session: ActiveSession) -> Self {
        ActiveSessionResponse {
            id: session.id,
            session_id: session.session_id.to_string(),
            device_name: session.device_name,
            location: session.location,
            ip_address: session.ip_address.map(|ip| ip.to_string()),
            created_at: session.created_at,
            last_active: session.last_active,
            is_current: session.is_current,
        }
    }
}

// ===== SECURITY EVENTS MODELS =====

/// Security events for MFA and authentication monitoring
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct SecurityEvent {
    pub id: i32,
    /// `None` for events not tied to a known account (e.g. a failed
    /// login against an unrecognised email; see C/W2). The attempted
    /// identifier is carried in `details` for those rows.
    pub user_uuid: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
    pub session_id: Option<i32>,
}

/// New security event for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct NewSecurityEvent {
    pub user_uuid: Option<Uuid>,
    pub event_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
    pub session_id: Option<i32>,
}

/// Security event types enum for type safety
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum SecurityEventType {
    #[serde(rename = "login_success")]
    LoginSuccess,
    #[serde(rename = "login_failed")]
    LoginFailed,
    #[serde(rename = "mfa_enabled")]
    MfaEnabled,
    #[serde(rename = "mfa_disabled")]
    MfaDisabled,
    #[serde(rename = "mfa_failed")]
    MfaFailed,
    #[serde(rename = "mfa_success")]
    MfaSuccess,
    #[serde(rename = "backup_codes_used")]
    BackupCodesUsed,
    #[serde(rename = "backup_codes_regenerated")]
    BackupCodesRegenerated,
    #[serde(rename = "password_changed")]
    PasswordChanged,
    #[serde(rename = "session_revoked")]
    SessionRevoked,
    #[serde(rename = "account_locked")]
    AccountLocked,
    #[serde(rename = "suspicious_activity")]
    SuspiciousActivity,
}

impl SecurityEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoginSuccess => "login_success",
            Self::LoginFailed => "login_failed",
            Self::MfaEnabled => "mfa_enabled",
            Self::MfaDisabled => "mfa_disabled",
            Self::MfaFailed => "mfa_failed",
            Self::MfaSuccess => "mfa_success",
            Self::BackupCodesUsed => "backup_codes_used",
            Self::BackupCodesRegenerated => "backup_codes_regenerated",
            Self::PasswordChanged => "password_changed",
            Self::SessionRevoked => "session_revoked",
            Self::AccountLocked => "account_locked",
            Self::SuspiciousActivity => "suspicious_activity",
        }
    }
}

impl std::fmt::Display for SecurityEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SecurityEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "login_success" => Ok(Self::LoginSuccess),
            "login_failed" => Ok(Self::LoginFailed),
            "mfa_enabled" => Ok(Self::MfaEnabled),
            "mfa_disabled" => Ok(Self::MfaDisabled),
            "mfa_failed" => Ok(Self::MfaFailed),
            "mfa_success" => Ok(Self::MfaSuccess),
            "backup_codes_used" => Ok(Self::BackupCodesUsed),
            "backup_codes_regenerated" => Ok(Self::BackupCodesRegenerated),
            "password_changed" => Ok(Self::PasswordChanged),
            "session_revoked" => Ok(Self::SessionRevoked),
            "account_locked" => Ok(Self::AccountLocked),
            "suspicious_activity" => Ok(Self::SuspiciousActivity),
            _ => Err(format!("Invalid security event type: {s}")),
        }
    }
}

/// Security event severity enum
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecurityEventSeverity {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "critical")]
    Critical,
}

impl SecurityEventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

impl std::fmt::Display for SecurityEventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SecurityEventSeverity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("Invalid security event severity: {s}")),
        }
    }
}

/// Response model for security events in user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityEventResponse {
    pub id: i32,
    pub event_type: String,
    pub ip_address: Option<String>,
    pub location: Option<String>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
    pub details: Option<serde_json::Value>,
}

impl From<SecurityEvent> for SecurityEventResponse {
    fn from(event: SecurityEvent) -> Self {
        SecurityEventResponse {
            id: event.id,
            event_type: event.event_type,
            ip_address: event.ip_address.map(|ip| ip.to_string()),
            location: event.location,
            severity: event.severity,
            created_at: event.created_at,
            details: event.details,
        }
    }
}

// ===== RESET TOKENS MODELS =====

/// Generic reset tokens for password resets, MFA resets, and other temporary tokens
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::reset_tokens)]
#[diesel(primary_key(token_hash))]
pub struct ResetToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub token_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub used_at: Option<chrono::NaiveDateTime>,
    pub is_used: bool,
    pub metadata: Option<serde_json::Value>,
}

/// New reset token for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::reset_tokens)]
pub struct NewResetToken<'a> {
    pub token_hash: &'a str,
    pub user_uuid: Uuid,
    pub token_type: &'a str,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<&'a str>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<serde_json::Value>,
}

/// Update struct for reset tokens
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::reset_tokens)]
pub struct ResetTokenUpdate {
    pub used_at: Option<chrono::NaiveDateTime>,
    pub is_used: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

// ===== PASSWORD RESET MODELS =====

/// Request to initiate password reset
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetRequest {
    pub email: String,
}

/// Response for password reset initiation
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetResponse {
    pub message: String,
}

/// Request to complete password reset with token
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetCompleteRequest {
    pub token: String,
    pub new_password: String,
}

/// Session revocation request
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionRevocationRequest {
    pub session_id: Option<i32>, // If None, revoke all others
}

// ===== INVITATION MODELS =====

/// Request to accept an invitation and set password
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub password: String,
}

/// Response for invitation acceptance
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptInvitationResponse {
    pub success: bool,
    pub message: String,
}

/// Request to validate an invitation token (check if it's valid before showing the form)
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateInvitationRequest {
    pub token: String,
}

/// Response for invitation validation
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateInvitationResponse {
    pub valid: bool,
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub message: Option<String>,
    /// Classification of the invitation's origin so the frontend can tailor
    /// copy ("confirm your ticket submission" vs generic onboarding).
    /// `"guest_ticket"` when the token was issued by a public ticket
    /// submission; `"invitation"` for an admin-sent invitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Response for session operations
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionResponse {
    pub message: String,
    pub sessions_revoked: usize,
}

// User ticket views for tracking recently viewed tickets
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(belongs_to(Ticket))]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct UserTicketView {
    pub id: i32,
    pub user_uuid: Uuid,
    pub ticket_id: i32,
    pub first_viewed_at: NaiveDateTime,
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct NewUserTicketView {
    pub user_uuid: Uuid,
    pub ticket_id: i32,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct UpdateUserTicketView {
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}

// Response structure for recent tickets API
#[derive(Debug, Serialize, Deserialize)]
pub struct RecentTicket {
    pub id: i32,
    pub title: String,
    /// The frontend resolves this to a category / colour via the
    /// workspace workflow-states store.
    pub workflow_state_id: i32,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub requester: Option<Uuid>,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub assignee: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}

// ============================================================================
// Site Settings - Branding and Customization
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct SiteSettings {
    pub id: i32,
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub updated_by: Option<Uuid>,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_default_priority: Option<String>,
    pub guest_ticket_rate_limit_per_hour: i32,
    pub guest_ticket_email_verification: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
    /// Whether to send a one-off "thanks, we got your message" reply
    /// when a channel message opens a fresh ticket. Defaults true.
    pub channel_auto_ack_enabled: bool,
    /// Admin-overridden template for the auto-ack body. `None` uses
    /// the built-in default (see
    /// [`crate::services::channels::auto_ack::DEFAULT_TEMPLATE`]).
    pub channel_auto_ack_template: Option<String>,
    /// Workspace-level feature flag defaults. JSONB shape
    /// `{ "<flag_name>": <boolean | string | object>, ... }`. Empty
    /// object = all flags at code-default. Per-user overrides on the
    /// `users` table merge on top at request time.
    pub feature_flags: serde_json::Value,
    /// System-wide default BCP-47 locale used when the user has no
    /// preference and (for guests) when the inbound mail's
    /// `Content-Language` was missing or unsupported. Defaults to
    /// `en-US`; operator can change via admin settings.
    pub default_locale: String,
    /// System-wide default IANA timezone used when the user has no
    /// preference. Defaults to `UTC`; operator typically sets this
    /// to the team's working zone (e.g. `Australia/Sydney`).
    pub default_timezone: String,
    pub workspace_id: i32,
    /// Workspace-wide default email signature. The outbound channel
    /// reply pipeline appends this when an agent has not set a
    /// personal signature in `user_preferences.signature`. `None` =
    /// no org default; reply goes out unsigned, matching the pre-
    /// migration behaviour.
    pub signature_default: Option<String>,
    /// Whether to render the anti-phishing security note in the
    /// transactional email footer. Defaults false: the note is
    /// brand-specific, so it stays opt-in until an admin enables it.
    pub email_security_note_enabled: bool,
    /// Admin-overridden security-note body. `None` uses the built-in
    /// localized default (FTL key `email-security-note-default`).
    /// Supports `{{app_name}}` and `{{domain}}` placeholders.
    pub email_security_note_template: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct UpdateSiteSettings {
    pub app_name: Option<String>,
    pub logo_url: Option<Option<String>>,
    pub logo_light_url: Option<Option<String>>,
    pub favicon_url: Option<Option<String>>,
    pub primary_color: Option<Option<String>>,
    pub updated_by: Option<Uuid>,
    pub guest_tickets_enabled: Option<bool>,
    pub guest_public_docs_enabled: Option<bool>,
    pub guest_kb_search_enabled: Option<bool>,
    pub guest_ticket_lookup_enabled: Option<bool>,
    pub guest_help_page_enabled: Option<bool>,
    pub guest_ticket_default_priority: Option<Option<String>>,
    pub guest_ticket_rate_limit_per_hour: Option<i32>,
    pub guest_ticket_email_verification: Option<bool>,
    pub guest_ticket_attachments_enabled: Option<bool>,
    pub guest_ticket_intro_message: Option<Option<String>>,
    pub channel_auto_ack_enabled: Option<bool>,
    pub channel_auto_ack_template: Option<Option<String>>,
    pub default_locale: Option<String>,
    pub default_timezone: Option<String>,
    /// `Option<Option<String>>`: outer `None` = leave as-is,
    /// `Some(None)` = clear back to NULL (no org default),
    /// `Some(Some(_))` = set the org-wide template.
    pub signature_default: Option<Option<String>>,
    pub email_security_note_enabled: Option<bool>,
    /// Same `Option<Option<String>>` clear semantics as the auto-ack
    /// template: `Some(None)` reverts to the built-in default.
    pub email_security_note_template: Option<Option<String>>,
}

// API response for site settings (without internal fields)
#[derive(Debug, Serialize, Deserialize)]
pub struct SiteSettingsResponse {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub updated_at: NaiveDateTime,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_default_priority: Option<String>,
    pub guest_ticket_rate_limit_per_hour: i32,
    pub guest_ticket_email_verification: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
    /// Workspace-wide default email signature. Admin-visible only;
    /// excluded from `PublicSiteSettings` since it isn't relevant
    /// to anonymous guest views.
    pub signature_default: Option<String>,
    /// Whether to send the "we got your message" auto-acknowledgement
    /// when a channel message opens a new ticket. See
    /// `services::channels::auto_ack`.
    pub channel_auto_ack_enabled: bool,
    /// Admin-overridden template for the auto-ack body. `None` =
    /// use the built-in FTL default for the resolved locale.
    pub channel_auto_ack_template: Option<String>,
    /// Whether the anti-phishing security note renders in the email
    /// footer. See `utils::email_branding::resolve_security_note`.
    pub email_security_note_enabled: bool,
    /// Admin-overridden security-note body. `None` = use the built-in
    /// localized default.
    pub email_security_note_template: Option<String>,
}

impl From<SiteSettings> for SiteSettingsResponse {
    fn from(settings: SiteSettings) -> Self {
        SiteSettingsResponse {
            app_name: settings.app_name,
            logo_url: settings.logo_url,
            logo_light_url: settings.logo_light_url,
            favicon_url: settings.favicon_url,
            primary_color: settings.primary_color,
            updated_at: settings.updated_at,
            guest_tickets_enabled: settings.guest_tickets_enabled,
            guest_public_docs_enabled: settings.guest_public_docs_enabled,
            guest_kb_search_enabled: settings.guest_kb_search_enabled,
            guest_ticket_lookup_enabled: settings.guest_ticket_lookup_enabled,
            guest_help_page_enabled: settings.guest_help_page_enabled,
            guest_ticket_default_priority: settings.guest_ticket_default_priority,
            guest_ticket_rate_limit_per_hour: settings.guest_ticket_rate_limit_per_hour,
            guest_ticket_email_verification: settings.guest_ticket_email_verification,
            guest_ticket_attachments_enabled: settings.guest_ticket_attachments_enabled,
            guest_ticket_intro_message: settings.guest_ticket_intro_message,
            signature_default: settings.signature_default,
            channel_auto_ack_enabled: settings.channel_auto_ack_enabled,
            channel_auto_ack_template: settings.channel_auto_ack_template,
            email_security_note_enabled: settings.email_security_note_enabled,
            email_security_note_template: settings.email_security_note_template,
        }
    }
}

// Public subset — safe to expose on /api/public/settings (no auth required)
#[derive(Debug, Serialize, Deserialize)]
pub struct PublicSiteSettings {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
}

impl From<&SiteSettings> for PublicSiteSettings {
    fn from(s: &SiteSettings) -> Self {
        PublicSiteSettings {
            app_name: s.app_name.clone(),
            logo_url: s.logo_url.clone(),
            logo_light_url: s.logo_light_url.clone(),
            favicon_url: s.favicon_url.clone(),
            primary_color: s.primary_color.clone(),
            guest_tickets_enabled: s.guest_tickets_enabled,
            guest_public_docs_enabled: s.guest_public_docs_enabled,
            guest_kb_search_enabled: s.guest_kb_search_enabled,
            guest_ticket_lookup_enabled: s.guest_ticket_lookup_enabled,
            guest_help_page_enabled: s.guest_help_page_enabled,
            guest_ticket_attachments_enabled: s.guest_ticket_attachments_enabled,
            guest_ticket_intro_message: s.guest_ticket_intro_message.clone(),
        }
    }
}

// ============================================================================
// Backup Jobs - System Backup and Restore
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct BackupJob {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct NewBackupJob {
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct BackupJobUpdate {
    pub status: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub completed_at: Option<NaiveDateTime>,
}

/// CSV import job. Two-phase: rows go through dry-run (parse +
/// validate, write `summary`) before the admin commits. The
/// audit row outlives the request that triggered the upload so
/// the admin UI can resume a job they navigated away from.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct ImportJob {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub filename: String,
    pub file_path: String,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub summary: Option<serde_json::Value>,
    pub records_committed: Option<i32>,
    pub error_message: Option<String>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct NewImportJob {
    pub job_type: String,
    pub filename: String,
    pub file_path: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::import_jobs)]
pub struct ImportJobUpdate {
    pub status: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub summary: Option<Option<serde_json::Value>>,
    pub records_committed: Option<Option<i32>>,
    pub error_message: Option<Option<String>>,
}

// API response for backup jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupJobResponse {
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_by: Option<String>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
}

impl From<BackupJob> for BackupJobResponse {
    fn from(job: BackupJob) -> Self {
        BackupJobResponse {
            id: job.id.to_string(),
            job_type: job.job_type,
            status: job.status,
            include_sensitive: job.include_sensitive,
            file_path: job.file_path,
            file_size: job.file_size,
            error_message: job.error_message,
            created_by: job.created_by.map(|u| u.to_string()),
            created_at: job.created_at,
            completed_at: job.completed_at,
        }
    }
}

// Request to start an export backup
#[derive(Debug, Serialize, Deserialize)]
pub struct StartBackupExportRequest {
    pub include_sensitive: bool,
    pub password: Option<String>,
}

// Request to execute a restore
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecuteRestoreRequest {
    pub password: Option<String>,
}

// Backup manifest for archive metadata.
//
// Lives inside the zip (encrypted-or-not) as `manifest.json`.
// The encryption envelope is OUTSIDE the manifest — when a
// backup is password-protected, the entire zip is wrapped in an
// AES-GCM container whose header carries the salt + nonce; this
// struct is read after that decryption.
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Bumped on every breaking change to the on-disk shape.
    /// Restorers refuse archives with an unknown version (the
    /// pg_dump `K_VERS_*` pattern). Starts at 1.
    pub backup_format_version: u32,
    /// `CARGO_PKG_VERSION` of the binary that wrote the backup.
    /// Operator-readable, not gate-load-bearing.
    pub nosdesk_version: String,
    /// The migrations-derived hash computed at build time via
    /// `env!("NOSDESK_SCHEMA_HASH")`. Restore refuses by default
    /// when this doesn't match the running server; the CLI's
    /// `--ignore-schema-mismatch` is the explicit override.
    pub schema_hash: String,
    pub created_at: String,
    pub tables: std::collections::HashMap<String, TableManifest>,
    pub files: FilesManifest,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableManifest {
    pub count: i64,
    /// Hex SHA-256 of the table's `data/<name>.json` payload as
    /// stored in the zip. Restore recomputes and refuses on
    /// mismatch — catches truncated downloads, corrupt storage,
    /// and post-creation tampering.
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FilesManifest {
    pub total_count: i64,
    pub total_size_bytes: i64,
}

// Restore preview response
#[derive(Debug, Serialize, Deserialize)]
pub struct RestorePreview {
    pub manifest: BackupManifest,
    /// True when the source file uses the encrypted wrapper.
    /// The preview can still be returned without the password
    /// only for unencrypted archives; encrypted previews
    /// require the password to be passed in to decrypt the
    /// manifest first.
    pub encrypted: bool,
    pub warnings: Vec<String>,
}

// ============================================================================
// Groups - User Group Management
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::groups)]
pub struct Group {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub group_type: Option<String>,
    pub mail_enabled: bool,
    pub security_enabled: bool,
    pub last_synced_at: Option<NaiveDateTime>,
    pub sync_enabled: bool,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::groups)]
pub struct NewGroup {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::groups)]
pub struct NewExternalGroup {
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub group_type: Option<String>,
    pub mail_enabled: bool,
    pub security_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::groups)]
pub struct GroupUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::groups)]
pub struct ExternalGroupUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub group_type: Option<String>,
    pub mail_enabled: Option<bool>,
    pub security_enabled: Option<bool>,
    pub last_synced_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

// Group include (composite group membership)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::group_includes)]
#[diesel(primary_key(parent_group_id, child_group_id))]
pub struct GroupInclude {
    pub parent_group_id: i32,
    pub child_group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::group_includes)]
pub struct NewGroupInclude {
    pub parent_group_id: i32,
    pub child_group_id: i32,
    pub created_by: Option<Uuid>,
}

// Lightweight group summary for include display
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupSummary {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub external_source: Option<String>,
    pub member_count: i64,
    pub members: Vec<UserInfoWithAvatar>,
}

// Group with member count for list views
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupWithMemberCount {
    #[serde(flatten)]
    pub group: Group,
    pub member_count: i64,
    pub device_count: i64,
    pub included_group_count: i64,
}

// Group with full member details
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupWithMembers {
    #[serde(flatten)]
    pub group: Group,
    pub members: Vec<UserInfoWithAvatar>,
}

// Group with members and devices (for detail view)
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupDetails {
    #[serde(flatten)]
    pub group: Group,
    pub members: Vec<UserInfoWithAvatar>,
    pub devices: Vec<Asset>,
    pub included_groups: Vec<GroupSummary>,
    pub included_in: Vec<GroupSummary>,
}

// User-Group junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::user_groups)]
#[diesel(belongs_to(Group))]
#[diesel(primary_key(user_uuid, group_id))]
pub struct UserGroup {
    pub user_uuid: Uuid,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_groups)]
pub struct NewUserGroup {
    pub user_uuid: Uuid,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
}

// Asset-Group junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_groups)]
#[diesel(belongs_to(Group))]
#[diesel(belongs_to(Asset, foreign_key = asset_id))]
#[diesel(primary_key(asset_id, group_id))]
pub struct AssetGroup {
    pub asset_id: i32,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub external_source: Option<String>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_groups)]
pub struct NewAssetGroup {
    pub asset_id: i32,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
    pub external_source: Option<String>,
}

// ============================================================================
// Ticket Categories - Category Management
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::ticket_categories)]
pub struct TicketCategory {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub display_order: i32,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_categories)]
pub struct NewTicketCategory {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub display_order: i32,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::ticket_categories)]
pub struct TicketCategoryUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub display_order: Option<i32>,
    pub is_active: Option<bool>,
    pub updated_at: Option<NaiveDateTime>,
}

// Category with visibility information for admin views
#[derive(Debug, Serialize, Deserialize)]
pub struct CategoryWithVisibility {
    #[serde(flatten)]
    pub category: TicketCategory,
    pub visible_to_groups: Vec<Group>,
    pub is_public: bool, // true if no group restrictions (visible to all)
}

// Category-Group visibility junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::category_group_visibility)]
#[diesel(belongs_to(TicketCategory, foreign_key = category_id))]
#[diesel(belongs_to(Group))]
#[diesel(primary_key(category_id, group_id))]
pub struct CategoryGroupVisibility {
    pub category_id: i32,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::category_group_visibility)]
pub struct NewCategoryGroupVisibility {
    pub category_id: i32,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
}

// ============================================================================
// Documentation Collections
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct DocumentationCollection {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub slug: String,
    /// Short tagline shown above the rich description editor.
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub is_system: bool,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub display_order: i32,
    /// Yjs binary state for the collection's rich description.
    /// Replaces the old root_page_id pattern: the collection owns
    /// its overview content directly instead of pointing at a
    /// special "main page".
    pub description_yjs: Option<Vec<u8>>,
    pub description_state_vector: Option<Vec<u8>>,
    /// Plain-text projection of `description_yjs` for search.
    pub description_text: Option<String>,
    /// When true, cross-collection wikilinks render as
    /// "Restricted page" for viewers without read access, instead
    /// of leaking the page title.
    pub hide_titles_from_non_members: bool,
    pub workspace_id: i32,
    /// Fencing token from the per-document ownership claim (Phase 2
    /// affinity); see the note on `ArticleContent::fence_token`.
    pub fence_token: Option<i64>,
    /// When true, pages in this collection that have never been
    /// verified surface a "needs verification" prompt. Off by
    /// default: an unverified page is neutral, not unchecked.
    pub require_verification: bool,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct NewDocumentationCollection {
    pub uuid: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub is_system: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct DocumentationCollectionUpdate {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
    pub hide_titles_from_non_members: Option<bool>,
    pub require_verification: Option<bool>,
    pub description_text: Option<Option<String>>,
}

/// Yjs blob update issued by the collaboration handler when a
/// collection's description editor saves. Kept separate from
/// `DocumentationCollectionUpdate` so the metadata-edit surface
/// can't accidentally clobber the binary Yjs state.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct DocumentationCollectionDescriptionYjsUpdate {
    pub description_yjs: Option<Vec<u8>>,
    pub description_state_vector: Option<Vec<u8>>,
    pub updated_at: Option<NaiveDateTime>,
}

// Collection with visibility and page count
#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionWithDetails {
    #[serde(flatten)]
    pub collection: DocumentationCollection,
    pub visible_to_groups: Vec<Group>,
    pub visible_to_users: Vec<UserInfoWithAvatar>,
    pub is_public: bool,
    pub page_count: i64,
}

// Collection-Page junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::documentation_collection_pages)]
#[diesel(belongs_to(DocumentationCollection, foreign_key = collection_id))]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
#[diesel(primary_key(collection_id, page_id))]
pub struct DocumentationCollectionPage {
    pub collection_id: i32,
    pub page_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collection_pages)]
pub struct NewDocumentationCollectionPage {
    pub collection_id: i32,
    pub page_id: i32,
    pub created_by: Option<Uuid>,
}

// Collection-Group visibility junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::documentation_collection_visibility)]
#[diesel(belongs_to(DocumentationCollection, foreign_key = collection_id))]
#[diesel(primary_key(id))]
pub struct DocumentationCollectionVisibility {
    pub collection_id: i32,
    pub group_id: Option<i32>,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub id: i32,
    pub user_uuid: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collection_visibility)]
pub struct NewDocumentationCollectionVisibility {
    pub collection_id: i32,
    pub group_id: Option<i32>,
    pub created_by: Option<Uuid>,
    pub user_uuid: Option<Uuid>,
}

// ============================================================================
// Documentation Page <-> Ticket links
// ============================================================================
//
// Many-to-many between docs and tickets. `link_type` distinguishes
// "this doc resolved that ticket" (created from / answers it) from
// "this doc is referenced from that ticket" (relevant context, but
// the doc didn't originate from it).

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Associations, Clone)]
#[diesel(table_name = crate::schema::documentation_page_tickets)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
#[diesel(belongs_to(Ticket, foreign_key = ticket_id))]
#[diesel(primary_key(page_id, ticket_id))]
pub struct DocumentationPageTicket {
    pub page_id: i32,
    pub ticket_id: i32,
    pub link_type: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_page_tickets)]
pub struct NewDocumentationPageTicket {
    pub page_id: i32,
    pub ticket_id: i32,
    pub link_type: String,
    pub created_by: Option<Uuid>,
}

// ============================================================================
// Documentation Page Visibility - Page-level group access control
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::documentation_page_visibility)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
#[diesel(primary_key(id))]
pub struct DocumentationPageVisibility {
    pub page_id: i32,
    pub group_id: Option<i32>,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub id: i32,
    pub user_uuid: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_page_visibility)]
pub struct NewDocumentationPageVisibility {
    pub page_id: i32,
    pub group_id: Option<i32>,
    pub created_by: Option<Uuid>,
    pub user_uuid: Option<Uuid>,
}

// ============================================================================
// Documentation Page Embeddings - Tracks transclusion relationships
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Associations)]
#[diesel(table_name = crate::schema::documentation_page_embeddings)]
#[diesel(primary_key(source_page_id, target_page_id))]
#[diesel(belongs_to(DocumentationPage, foreign_key = source_page_id))]
pub struct DocumentationPageEmbedding {
    pub source_page_id: i32,
    pub target_page_id: i32,
    pub created_at: chrono::NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_page_embeddings)]
pub struct NewDocumentationPageEmbedding {
    pub source_page_id: i32,
    pub target_page_id: i32,
}

// ============================================================================
// Documentation Subscriptions
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::documentation_subscriptions)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
pub struct DocumentationSubscription {
    pub id: i32,
    pub user_uuid: Uuid,
    pub page_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_subscriptions)]
pub struct NewDocumentationSubscription {
    pub user_uuid: Uuid,
    pub page_id: i32,
}

// ============================================================================
// Documentation Starred Pages
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::documentation_starred_pages)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
pub struct DocumentationStarredPage {
    pub id: i32,
    pub user_uuid: Uuid,
    pub page_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_starred_pages)]
pub struct NewDocumentationStarredPage {
    pub user_uuid: Uuid,
    pub page_id: i32,
}

/// Info returned for starred pages (used by sidebar API)
#[derive(Debug, Serialize, Deserialize)]
pub struct StarredPageInfo {
    pub page_id: i32,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub starred_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Assignment Rules - Automatic Ticket Assignment
// ============================================================================

/// Assignment method enum - how tickets are assigned
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::AssignmentMethod)]
pub enum AssignmentMethod {
    #[serde(rename = "direct_user")]
    DirectUser,
    #[serde(rename = "group_round_robin")]
    GroupRoundRobin,
    #[serde(rename = "group_random")]
    GroupRandom,
    #[serde(rename = "group_queue")]
    GroupQueue,
}

impl ToSql<crate::schema::sql_types::AssignmentMethod, Pg> for AssignmentMethod {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            AssignmentMethod::DirectUser => "direct_user",
            AssignmentMethod::GroupRoundRobin => "group_round_robin",
            AssignmentMethod::GroupRandom => "group_random",
            AssignmentMethod::GroupQueue => "group_queue",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::AssignmentMethod, Pg> for AssignmentMethod {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"direct_user" => Ok(AssignmentMethod::DirectUser),
            b"group_round_robin" => Ok(AssignmentMethod::GroupRoundRobin),
            b"group_random" => Ok(AssignmentMethod::GroupRandom),
            b"group_queue" => Ok(AssignmentMethod::GroupQueue),
            _ => Err("Unrecognized assignment method".into()),
        }
    }
}

impl std::fmt::Display for AssignmentMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AssignmentMethod::DirectUser => "direct_user",
            AssignmentMethod::GroupRoundRobin => "group_round_robin",
            AssignmentMethod::GroupRandom => "group_random",
            AssignmentMethod::GroupQueue => "group_queue",
        };
        write!(f, "{s}")
    }
}

/// Core assignment rule configuration
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::assignment_rules)]
pub struct AssignmentRule {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub priority: i32,
    pub is_active: bool,
    pub method: AssignmentMethod,
    pub target_user_uuid: Option<Uuid>,
    pub target_group_id: Option<i32>,
    pub trigger_on_create: bool,
    pub trigger_on_category_change: bool,
    pub category_id: Option<i32>,
    pub conditions: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::assignment_rules)]
pub struct NewAssignmentRule {
    pub name: String,
    pub description: Option<String>,
    pub priority: i32,
    pub is_active: bool,
    pub method: AssignmentMethod,
    pub target_user_uuid: Option<Uuid>,
    pub target_group_id: Option<i32>,
    pub trigger_on_create: bool,
    pub trigger_on_category_change: bool,
    pub category_id: Option<i32>,
    pub conditions: Option<serde_json::Value>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::assignment_rules)]
pub struct AssignmentRuleUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    pub method: Option<AssignmentMethod>,
    pub target_user_uuid: Option<Option<Uuid>>,
    pub target_group_id: Option<Option<i32>>,
    pub trigger_on_create: Option<bool>,
    pub trigger_on_category_change: Option<bool>,
    pub category_id: Option<Option<i32>>,
    pub conditions: Option<serde_json::Value>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Round-robin and assignment state tracking
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::assignment_rule_state)]
#[diesel(belongs_to(AssignmentRule, foreign_key = rule_id))]
#[diesel(primary_key(rule_id))]
pub struct AssignmentRuleState {
    pub rule_id: i32,
    pub last_assigned_index: i32,
    pub total_assignments: i32,
    pub last_assigned_at: Option<NaiveDateTime>,
    pub last_assigned_user_uuid: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::assignment_rule_state)]
pub struct NewAssignmentRuleState {
    pub rule_id: i32,
    pub last_assigned_index: i32,
    pub total_assignments: i32,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::assignment_rule_state)]
pub struct AssignmentRuleStateUpdate {
    pub last_assigned_index: Option<i32>,
    pub total_assignments: Option<i32>,
    pub last_assigned_at: Option<NaiveDateTime>,
    pub last_assigned_user_uuid: Option<Uuid>,
}

/// Assignment audit log entry
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::assignment_log)]
#[diesel(belongs_to(AssignmentRule, foreign_key = rule_id))]
#[diesel(belongs_to(Ticket))]
pub struct AssignmentLog {
    pub id: i32,
    pub ticket_id: i32,
    pub rule_id: Option<i32>,
    pub trigger_type: String,
    pub previous_assignee_uuid: Option<Uuid>,
    pub new_assignee_uuid: Option<Uuid>,
    pub method: AssignmentMethod,
    pub context: Option<serde_json::Value>,
    pub assigned_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::assignment_log)]
pub struct NewAssignmentLog {
    pub ticket_id: i32,
    pub rule_id: Option<i32>,
    pub trigger_type: String,
    pub previous_assignee_uuid: Option<Uuid>,
    pub new_assignee_uuid: Option<Uuid>,
    pub method: AssignmentMethod,
    pub context: Option<serde_json::Value>,
}

/// Assignment rule with related data for API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct AssignmentRuleWithDetails {
    #[serde(flatten)]
    pub rule: AssignmentRule,
    pub target_user: Option<UserInfoWithAvatar>,
    pub target_group: Option<Group>,
    pub category: Option<TicketCategory>,
    pub state: Option<AssignmentRuleState>,
}

/// Trigger types for assignment evaluation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignmentTrigger {
    TicketCreated,
    CategoryChanged,
}

impl AssignmentTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssignmentTrigger::TicketCreated => "ticket_created",
            AssignmentTrigger::CategoryChanged => "category_changed",
        }
    }
}

/// Result of automatic assignment evaluation
#[derive(Debug, Clone)]
pub struct AssignmentResult {
    pub rule_id: i32,
    pub rule_name: String,
    pub assigned_user_uuid: Option<Uuid>,
    pub method: AssignmentMethod,
}

// ============================================================================
// Notification Models
// ============================================================================

/// Notification type definition
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::notification_types)]
pub struct NotificationType {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub default_channels: serde_json::Value,
    pub created_at: NaiveDateTime,
}

/// User notification preference
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notification_preferences)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct NotificationPreference {
    pub id: i32,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub channel: String,
    pub enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notification_preferences)]
pub struct NewNotificationPreference {
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub channel: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::notification_preferences)]
pub struct NotificationPreferenceUpdate {
    pub enabled: Option<bool>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Persistent notification record
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notifications)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct Notification {
    pub id: i32,
    pub uuid: Uuid,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub channels_delivered: serde_json::Value,
    pub is_read: bool,
    pub read_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notifications)]
pub struct NewNotification {
    pub uuid: Uuid,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub channels_delivered: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::notifications)]
pub struct NotificationUpdate {
    pub is_read: Option<bool>,
    pub read_at: Option<NaiveDateTime>,
    pub channels_delivered: Option<serde_json::Value>,
}

/// Rate limit tracking for email notifications
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notification_rate_limits)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct NotificationRateLimit {
    pub id: i32,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub last_notified_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notification_rate_limits)]
pub struct NewNotificationRateLimit {
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
}

/// API response for notification preferences (grouped by type)
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationPreferenceResponse {
    pub notification_type: String,
    pub notification_name: String,
    pub description: Option<String>,
    pub category: String,
    pub channels: std::collections::HashMap<String, bool>,
}

/// API response for a notification
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationResponse {
    pub id: i32,
    pub uuid: Uuid,
    pub notification_type: String,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub is_read: bool,
    pub created_at: NaiveDateTime,
}

// ===== WEBHOOK MODELS =====

/// Webhook configuration (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct Webhook {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<Option<String>>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: i32,
    pub disabled_reason: Option<String>,
    pub workspace_id: i32,
}

/// New webhook for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct NewWebhook {
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<Option<String>>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_by: Option<Uuid>,
}

/// Webhook update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct WebhookUpdate {
    pub name: Option<String>,
    pub url: Option<String>,
    pub secret: Option<String>,
    pub events: Option<Vec<Option<String>>>,
    pub enabled: Option<bool>,
    pub headers: Option<serde_json::Value>,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: Option<i32>,
    pub disabled_reason: Option<Option<String>>,
}

/// Webhook delivery record
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct WebhookDelivery {
    pub id: i32,
    pub uuid: Uuid,
    pub webhook_id: i32,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub request_headers: Option<serde_json::Value>,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub attempt_number: i32,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub next_retry_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
}

/// New webhook delivery for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct NewWebhookDelivery {
    pub webhook_id: i32,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub request_headers: Option<serde_json::Value>,
    pub attempt_number: i32,
}

/// Webhook delivery update
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct WebhookDeliveryUpdate {
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub next_retry_at: Option<Option<NaiveDateTime>>,
    pub attempt_number: Option<i32>,
}

// ===== WEBHOOK API TYPES =====

/// Request to create a webhook
#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
}

/// Request to update a webhook
#[derive(Debug, Deserialize)]
pub struct UpdateWebhookRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
    pub regenerate_secret: Option<bool>,
}

/// Webhook response (hides full secret)
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret_preview: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: i32,
    pub disabled_reason: Option<String>,
}

impl Webhook {
    /// Returns a preview of the secret (first 12 chars + "...")
    pub fn secret_preview(&self) -> String {
        format!("{}...", self.secret.chars().take(12).collect::<String>())
    }
}

impl From<Webhook> for WebhookResponse {
    fn from(w: Webhook) -> Self {
        // Compute secret_preview before any moves
        let secret_preview = w.secret_preview();
        WebhookResponse {
            uuid: w.uuid,
            name: w.name,
            url: w.url,
            secret_preview,
            events: w.events.into_iter().flatten().collect(),
            enabled: w.enabled,
            headers: w.headers,
            created_at: w.created_at,
            updated_at: w.updated_at,
            last_triggered_at: w.last_triggered_at,
            failure_count: w.failure_count,
            disabled_reason: w.disabled_reason,
        }
    }
}

/// Webhook created response (shows full secret once)
#[derive(Debug, Serialize)]
pub struct WebhookCreatedResponse {
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<String>,
}

/// Delivery history entry
#[derive(Debug, Serialize)]
pub struct WebhookDeliveryResponse {
    pub uuid: Uuid,
    pub event_type: String,
    pub response_status: Option<i32>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub attempt_number: i32,
}

// ===== PLUGIN SYSTEM TYPES =====

/// Plugin trust level
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PluginTrustLevel {
    Official,
    Verified,
    #[default]
    Community,
}

impl std::fmt::Display for PluginTrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginTrustLevel::Official => write!(f, "official"),
            PluginTrustLevel::Verified => write!(f, "verified"),
            PluginTrustLevel::Community => write!(f, "community"),
        }
    }
}

impl std::str::FromStr for PluginTrustLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "official" => Ok(PluginTrustLevel::Official),
            "verified" => Ok(PluginTrustLevel::Verified),
            "community" => Ok(PluginTrustLevel::Community),
            _ => Err(anyhow::anyhow!("Unknown trust level: {}", s)),
        }
    }
}

/// Installed plugin
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugins)]
pub struct Plugin {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
    /// Base64 Ed25519 pubkey that signed this bundle. The current
    /// install pipeline always populates this (every install path
    /// goes through signature verification), so production rows
    /// have `Some`; the column is `Option` only to tolerate
    /// pre-signing-system rows in upgraded databases.
    pub signer_pubkey: Option<String>,
    /// Which authority chain recognised this signer: `nosdesk-root`
    /// | `verified-publisher` | `community-publisher` | `local` |
    /// `dev`. See `services::plugins::signing::sources`.
    pub signer_source: Option<String>,
    /// Full signature envelope captured at install time for audit.
    pub signature_metadata: Option<serde_json::Value>,
    /// Validated `icon.svg` bytes extracted from the signed zip at
    /// install time. Served verbatim from `GET /api/plugins/{uuid}/icon`.
    pub icon_svg: Option<Vec<u8>>,
    /// Lifecycle state. Stringly-typed in the DB (VARCHAR with a
    /// CHECK constraint) but parsed into the typed `PluginState`
    /// enum on read; consumers match exhaustively, eliminating
    /// the typo class that the constants module was prone to.
    pub state: PluginState,
    /// Bundle bytes stored inline. Replaces the previous on-disk
    /// uploads-volume staging so install becomes a single
    /// transactional write (DB row + bundle bytes commit together
    /// or both roll back). NULL only on legacy rows installed
    /// before this column existed; reinstall populates it. Capped
    /// at `install::MAX_BUNDLE_SIZE` (500 KB).
    pub bundle_js: Option<Vec<u8>>,
    pub workspace_id: i32,
}

impl Plugin {
    /// True when the plugin is in the `installed` state (active +
    /// loaded). Replaces the old `enabled` boolean for callers that
    /// only need a yes/no view.
    pub fn is_active(&self) -> bool {
        matches!(self.state, PluginState::Installed)
    }
}

/// Lifecycle state of a plugin row. Stored as a `VARCHAR(32)` in
/// `plugins.state` with a CHECK constraint enforcing the allowlist;
/// the typed enum here is the canonical in-memory representation.
/// Custom Diesel `ToSql<Text>` / `FromSql<Text>` impls handle the
/// wire conversion. Adding a new variant means migrating the DB
/// CHECK constraint AND extending the exhaustive matches that
/// fall out elsewhere; the compiler points at every site.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::AsExpression,
    diesel::FromSqlRow,
    serde::Serialize,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Active. Bundle is served, components render, events dispatch.
    Installed,
    /// Admin paused. Bundle is NOT served, components don't render,
    /// but the row + plugin_data are intact and a flip back to
    /// `Installed` restores everything.
    Disabled,
    /// Trust-chain failure (signer revoked, signature mismatched on
    /// re-check). Refused for new use; existing data preserved for
    /// audit. Triggered by background revocation sweeps; never set
    /// by user action.
    Quarantined,
    /// Plugin was uninstalled via a manifest declaring
    /// `lifecycle.on_uninstall = preserve`. The row + plugin_data
    /// + collection rows are kept so a future reinstall of the same
    /// plugin name reattaches the data automatically. Bundle is
    /// removed from disk.
    Uninstalled,
}

impl PluginState {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            PluginState::Installed => "installed",
            PluginState::Disabled => "disabled",
            PluginState::Quarantined => "quarantined",
            PluginState::Uninstalled => "uninstalled",
        }
    }

    pub fn from_db_str(s: &str) -> Result<Self, String> {
        match s {
            "installed" => Ok(PluginState::Installed),
            "disabled" => Ok(PluginState::Disabled),
            "quarantined" => Ok(PluginState::Quarantined),
            "uninstalled" => Ok(PluginState::Uninstalled),
            other => Err(format!("unknown plugin state {other:?}")),
        }
    }
}

impl std::fmt::Display for PluginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

impl<'de> serde::Deserialize<'de> for PluginState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginState::from_db_str(&s).map_err(serde::de::Error::custom)
    }
}

impl diesel::serialize::ToSql<diesel::sql_types::Text, diesel::pg::Pg> for PluginState {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        <str as diesel::serialize::ToSql<diesel::sql_types::Text, diesel::pg::Pg>>::to_sql(
            self.as_db_str(),
            &mut out.reborrow(),
        )
    }
}

impl diesel::deserialize::FromSql<diesel::sql_types::Text, diesel::pg::Pg> for PluginState {
    fn from_sql(
        bytes: <diesel::pg::Pg as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        let s = <String as diesel::deserialize::FromSql<
            diesel::sql_types::Text,
            diesel::pg::Pg,
        >>::from_sql(bytes)?;
        PluginState::from_db_str(&s).map_err(|e| e.into())
    }
}

/// New plugin for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugins)]
pub struct NewPlugin {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    /// Initial lifecycle state, almost always `PluginState::Installed`.
    pub state: PluginState,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub source: String,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
    pub icon_svg: Option<Vec<u8>>,
}

/// Plugin update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginUpdate {
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub manifest: Option<serde_json::Value>,
    pub state: Option<PluginState>,
    pub trust_level: Option<String>,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
    /// `Some(Some(bytes))` writes the icon, `Some(None)` clears it,
    /// `None` leaves it alone. Distinct from the other signer
    /// fields' `Option<T>` because clearing-to-NULL on update is
    /// realistic here (a new plugin version might drop its icon).
    pub icon_svg: Option<Option<Vec<u8>>>,
}

/// Plugin bundle update changeset. `bundle_js` carries the raw
/// bytes; `bundle_hash`/`size`/`uploaded_at` are denormalised
/// metadata kept in sync. All four fields are written in the
/// same row update so they can't drift.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginBundleUpdate {
    pub bundle_js: Option<Vec<u8>>,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
}

/// Publisher whose Ed25519 pubkey is trusted to sign `verified` or
/// `community` tier plugins. Populated from the signed nosdesk.com
/// keylist; revocation is expressed by setting `revoked_at`.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_trusted_publishers)]
pub struct TrustedPublisher {
    pub id: i32,
    pub pubkey: String,
    pub display_name: String,
    pub tier: String,
    pub website: Option<String>,
    pub added_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_trusted_publishers)]
pub struct NewTrustedPublisher {
    pub pubkey: String,
    pub display_name: String,
    pub tier: String,
    pub website: Option<String>,
    pub revoked_at: Option<NaiveDateTime>,
}

/// Single-row table holding the instance's local Ed25519 signing
/// keypair. `encrypted_sk` is AES-256-GCM ciphertext under the same
/// key material as MFA secrets (see `utils::encryption`).
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::plugin_local_signing_key)]
pub struct LocalSigningKey {
    pub id: i32,
    pub pubkey: String,
    /// Framed AES-256-GCM blob (`utils::encryption::Keyring` shape).
    /// AAD = `b"nosdesk.plugin.local_sk.v1"` (singleton table; no row
    /// identity to bind beyond the domain tag).
    pub encrypted_sk: Vec<u8>,
    pub fingerprint: String,
    pub created_at: NaiveDateTime,
    pub encrypted_sk_kek_id: i16,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_local_signing_key)]
pub struct NewLocalSigningKey {
    pub id: i32,
    pub pubkey: String,
    pub encrypted_sk: Vec<u8>,
    pub encrypted_sk_kek_id: i16,
    pub fingerprint: String,
}

/// Single-row table that persists the anti-rollback counters from
/// the last registry snapshot the instance accepted. Durability
/// across restarts is load-bearing: without it, an attacker who
/// forces a restart could race the first boot fetch with an older
/// signed snapshot.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::plugin_registry_state)]
pub struct PluginRegistryState {
    pub id: i32,
    pub publishers_version: i64,
    pub index_version: i64,
    pub last_fetched_at: Option<NaiveDateTime>,
    pub last_fetch_error: Option<String>,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, AsChangeset, Default)]
#[diesel(table_name = crate::schema::plugin_registry_state)]
pub struct PluginRegistryStateUpdate {
    pub publishers_version: Option<i64>,
    pub index_version: Option<i64>,
    pub last_fetched_at: Option<Option<NaiveDateTime>>,
    pub last_fetch_error: Option<Option<String>>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Plugin data type - settings (admin-configured) or storage (plugin-managed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginDataType {
    Setting,
    Storage,
}

impl std::fmt::Display for PluginDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginDataType::Setting => write!(f, "setting"),
            PluginDataType::Storage => write!(f, "storage"),
        }
    }
}

/// Consolidated plugin data (settings and storage in one table)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct PluginData {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub data_type: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

impl PluginData {
    /// Check if this is a setting (admin-configured)
    pub fn is_setting(&self) -> bool {
        self.data_type == "setting"
    }

    /// Check if this is storage (plugin-managed)
    pub fn is_storage(&self) -> bool {
        self.data_type == "storage"
    }
}

/// New plugin data for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct NewPluginData {
    pub plugin_id: i32,
    pub data_type: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
}

/// Plugin data update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct PluginDataUpdate {
    pub value: Option<Option<serde_json::Value>>,
}

/// Plugin activity log entry
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_activity)]
pub struct PluginActivity {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// New plugin activity entry for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_activity)]
pub struct NewPluginActivity {
    pub plugin_id: i32,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
}

// ===== PLUGIN API TYPES =====

/// Plugin manifest structure (matches frontend manifest.json format).
///
/// `deny_unknown_fields` is load-bearing: every field a plugin
/// declares must be one this binary understands, otherwise we fail
/// closed at install. Combined with `manifest_version`, that lets
/// us evolve the schema without ambiguity. v2 plugins declare
/// `manifest_version: 2` and the parser dispatches to a different
/// struct; v1 plugins are forever interpreted by the rules below.
///
/// Trust-affecting fields (`name`, `permissions`, `engines`, etc.)
/// are part of the canonical archive digest because they live in
/// `manifest.json`, so the signer commits to all of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// MUST be 1 for the schema described here. Future bumps go to
    /// 2/3/etc. Validators dispatch on this.
    pub manifest_version: u32,

    /// Stable plugin identifier. Lowercase ASCII letters, digits,
    /// and hyphens. Used as the DB key and display URL slug.
    pub name: String,

    /// User-facing name. Free-form, locale-neutral.
    #[serde(rename = "displayName")]
    pub display_name: String,

    /// SemVer string (e.g. "2.1.0"). Compared between installs to
    /// detect upgrades.
    pub version: String,

    /// Short user-facing description. Free-form.
    pub description: Option<String>,

    /// SPDX license identifier (e.g. "MIT", "Apache-2.0",
    /// "BUSL-1.1"). Optional but strongly recommended.
    pub license: Option<String>,

    /// Author display name. For non-official plugins (verified /
    /// community tier), the install pipeline asserts this matches
    /// the publishers.json entry for the signing key. Local-tier
    /// installs skip the check.
    pub author: Option<String>,

    /// Source repository URL.
    pub repository: Option<String>,

    /// Plugin homepage / documentation URL.
    pub homepage: Option<String>,

    /// Issue tracker URL. Distinct from `repository` because some
    /// plugins host code on one host and bugs on another (e.g.
    /// Bugzilla, Linear, internal tracker).
    pub bugs: Option<String>,

    /// Support contact: email or URL. Surfaced on the registry
    /// browse UI so users know where to ask for help. Format
    /// validated lightly: must contain `@` or look like a URL.
    pub support_contact: Option<String>,

    /// Engine compatibility. Plugin will be refused if the
    /// instance doesn't satisfy these constraints.
    pub engines: PluginEngines,

    /// Other plugins this one depends on. Each value is a semver
    /// requirement against the dep's `version`. The install
    /// pipeline refuses if a declared dep isn't installed; it does
    /// NOT auto-install transitively (registry-driven install
    /// surfaces the prompt for the operator). Reserved shape for
    /// future inter-plugin APIs and ordering guarantees; even
    /// without those, having the declaration prevents silent
    /// "plugin assumes peer is present" footguns.
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, String>,

    /// Discovery taxonomy for the registry browse UI. Values are
    /// validated against an allowlist of known categories.
    #[serde(default)]
    pub categories: Vec<String>,

    /// Free-form discovery tags. No allowlist; the registry build
    /// can lowercase + dedupe but doesn't reject unknowns.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Paths inside the zip pointing at PNG/SVG screenshots for
    /// the registry browse UI. Validated at install.
    #[serde(default)]
    pub screenshots: Vec<String>,

    /// Capability grants the plugin requests. Parsed at manifest
    /// load time into typed `Permission` values; unknown or
    /// malformed entries fail deserialisation, so consumers past
    /// this point never see raw permission strings.
    #[serde(default)]
    pub permissions: Vec<crate::services::plugins::types::Permission>,

    /// Components this plugin contributes. Keyed by component name
    /// (used as the entry-point key in the bundle's default export).
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, PluginComponentConfig>,

    /// Events the plugin subscribes to. Validated against an
    /// allowlist; unknown events refused.
    #[serde(default)]
    pub events: Vec<String>,

    /// Plugin-defined settings rendered in the admin UI.
    #[serde(default)]
    pub settings: Vec<PluginSettingDefinition>,

    /// Plugin-owned collections. Each carries its own
    /// `schema_version` so future migrations can be expressed.
    #[serde(default)]
    pub collections: std::collections::BTreeMap<String, CollectionDefinition>,

    /// Declarative auth configuration: maps exact hostnames to
    /// auth strategies the proxy injects automatically. Wildcards
    /// are NOT permitted as auth keys (a future schema bump can
    /// loosen this if a real use case appears); each declared host
    /// must be covered by at least one `network:` permission.
    #[serde(default)]
    pub auth: std::collections::BTreeMap<crate::services::plugins::types::Host, PluginAuthConfig>,

    /// Lifecycle policy declarations. Default cascades plugin data
    /// on uninstall; plugins that store user-meaningful work
    /// should declare `on_uninstall: "preserve"`.
    #[serde(default)]
    pub lifecycle: PluginLifecyclePolicy,

    /// Palette-triggerable actions the plugin contributes. Reserved
    /// in v1: declared, validated, but the runtime palette is not
    /// yet implemented. Refused at install if non-empty until the
    /// dispatcher lands.
    #[serde(default)]
    pub commands: Vec<PluginCommandDefinition>,

    /// Menu contributions, keyed by menu identifier (e.g.
    /// `ticket-context`). Reserved in v1.
    #[serde(default)]
    pub menus: std::collections::BTreeMap<String, Vec<PluginMenuItem>>,

    /// URL-handler claims, e.g. `nosdesk://plugin/<plugin-name>/...`
    /// patterns this plugin owns. Reserved in v1.
    #[serde(default)]
    pub url_handlers: Vec<PluginUrlHandler>,

    /// Forward-compat bucket for typed inter-plugin exports.
    /// Modelled as a `BTreeMap<String, serde_json::Value>` so the
    /// same `is_empty()` predicate gates every reserved field;
    /// previously this was `serde_json::Value` with an
    /// `is_null()` check that let `{}` slip through. v1 refuses
    /// any non-empty value at install.
    #[serde(default)]
    pub extensions: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Engine compatibility constraints. Both values are required.
/// Refused at install when not satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEngines {
    /// SemVer requirement against the running Nosdesk version
    /// (e.g. ">=1.5.0", "^2.0", "1.4.x").
    pub nosdesk: String,

    /// Plugin runtime API major version the plugin was built
    /// against. Currently must be "1". The runtime exposes the
    /// supported version range to plugin code via `api.version`.
    pub plugin_api: String,
}

/// Declarative lifecycle policy. v1 honours `on_uninstall` only;
/// future fields here can land without breaking older manifests
/// because new defaults are added with `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PluginLifecyclePolicy {
    /// What happens to plugin-owned data when the plugin is
    /// uninstalled. `cascade` deletes all `plugin_data` and
    /// `plugin_collection_rows` for the plugin; `preserve` keeps
    /// them, supporting reinstall-without-data-loss flows.
    #[serde(default)]
    pub on_uninstall: PluginUninstallPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginUninstallPolicy {
    #[default]
    Cascade,
    Preserve,
}

/// Palette command contributed by a plugin. Reserved for the
/// future command-palette dispatcher; v1 install refuses non-empty
/// `commands` arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommandDefinition {
    /// Stable namespaced identifier, e.g. `github.sync`.
    pub id: String,
    /// User-facing label.
    pub title: String,
    /// Optional context filter (matches `KNOWN_CONTEXTS`).
    pub when: Option<String>,
}

/// Menu item contributed by a plugin. Reserved in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMenuItem {
    /// Command id this entry invokes.
    pub command: String,
    /// Optional grouping hint (e.g. `integrations`).
    pub group: Option<String>,
}

/// URL handler claim, e.g. `nosdesk://plugin/<plugin-name>/<pattern>`.
/// Reserved in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUrlHandler {
    /// Glob-like pattern under the plugin's namespace, e.g. `link/*`.
    pub pattern: String,
    /// Command id to invoke when matched.
    pub command: Option<String>,
}

/// Authentication configuration for a specific domain/host pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAuthConfig {
    /// Authorization: Bearer <secret_value>
    Bearer { secret: String },
    /// Authorization: Basic base64(username:password)
    Basic {
        username_secret: String,
        password_secret: String,
    },
    /// Custom header with secret value (e.g. X-API-Key)
    ApiKey { header: String, secret: String },
    /// OAuth2 Client Credentials flow: exchanges client_id + client_secret for a bearer token
    Oauth2ClientCredentials {
        token_url: String,
        client_id_secret: String,
        client_secret_secret: String,
    },
}

/// Plugin component configuration in manifest. The `kind` field
/// reserves space for future component shapes (settings tabs,
/// admin pages, background workers, webhook handlers); v1 only
/// implements `slot`-kind components, but the field is required
/// so future plugins can be expressed without a manifest version
/// bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponentConfig {
    /// What this component IS. Defaults to `slot` for backward
    /// readability; future kinds expand the allowed set.
    #[serde(default)]
    pub kind: PluginComponentKind,

    /// For `kind = slot`: the slot identifier (validated against
    /// allowlist). For other kinds, semantics differ.
    pub slot: String,

    /// Entry-point key inside the plugin's bundle default export.
    pub entry: String,

    /// Context types the component receives at render time
    /// (e.g. `["ticket"]`). Validated against allowlist.
    #[serde(default)]
    pub context: Vec<String>,

    pub label: Option<String>,
    pub icon: Option<String>,
    pub action: Option<PluginComponentAction>,
}

/// Component kind. Only `Slot` is implemented in v1; the others
/// are reserved enum variants so a future plugin declaring
/// `kind: "admin_page"` is parseable today (and rejected at
/// install with a clear "kind not yet supported" error rather
/// than a parse failure that looks like a bug).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentKind {
    #[default]
    Slot,
    /// Reserved: a settings panel rendered inside the plugin's
    /// settings dialog instead of the declarative settings form.
    Settings,
    /// Reserved: a full admin page mounted at /admin/plugins/<name>/...
    AdminPage,
    /// Reserved: a backend worker invoked on a schedule.
    Worker,
    /// Reserved: a webhook handler matching a registered path.
    Webhook,
}

impl PluginComponentKind {
    /// Wire-format string for this kind (matches the serde
    /// `rename_all = "snake_case"`). Used by validators when
    /// reporting "kind X is not supported" without depending on
    /// `serde_json` to round-trip.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Settings => "settings",
            Self::AdminPage => "admin_page",
            Self::Worker => "worker",
            Self::Webhook => "webhook",
        }
    }
}

/// Plugin component action for unified "+ Add" menu
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponentAction {
    pub label: String,
}

/// Plugin setting definition in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingDefinition {
    pub key: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub label: String,
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub default: Option<serde_json::Value>,
    /// Storage scope. `global` (default) means one value per
    /// instance; `user` means one value per logged-in user
    /// (e.g. each user's own GitHub PAT). Reserved in v1: the
    /// install validator refuses `user`-scoped settings until the
    /// per-user storage layer lands. Declaring the field now
    /// prevents the storage layout from being implicitly committed
    /// to "everything global" by the first wave of plugins.
    #[serde(default)]
    pub scope: PluginSettingScope,
    #[serde(default)]
    pub options: Option<Vec<PluginSettingOption>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingScope {
    #[default]
    Global,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingOption {
    pub value: String,
    pub label: String,
}

/// Request to toggle a plugin's lifecycle state. The endpoint
/// only honours the enabled-toggle (Installed <-> Disabled);
/// manifest edits used to be allowed here but were removed
/// because they bypassed signature reverification: an admin
/// could rewrite a verified plugin's stored manifest while the
/// signer fields kept claiming the original signer signed it.
/// Manifest changes now flow through the signed install paths
/// (zip upload, registry install) which re-verify end-to-end.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePluginRequest {
    pub enabled: Option<bool>,
}

/// Plugin response (for API)
#[derive(Debug, Serialize)]
pub struct PluginResponse {
    pub uuid: Uuid,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: PluginManifest,
    /// Lifecycle state. Serialises to one of `installed` /
    /// `disabled` / `quarantined` / `uninstalled` on the wire.
    /// The frontend toggles render rows where this is `installed`
    /// or `disabled`; the others are rendered as read-only audit
    /// entries.
    pub state: PluginState,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
    /// When non-null, the publisher that signed this plugin has been
    /// revoked from `plugin_trusted_publishers`. The plugin keeps
    /// running (we don't tear down installed rows on revocation),
    /// but the admin UI surfaces the state so operators can decide
    /// whether to uninstall or keep with the trust caveat. NULL for
    /// official-tier plugins (signed by the Nosdesk root) and
    /// local-tier plugins (signed by the instance key), since
    /// neither resolves through plugin_trusted_publishers.
    pub signer_revoked_at: Option<NaiveDateTime>,
}

impl Plugin {
    /// Parse the manifest JSON into a PluginManifest struct
    pub fn parse_manifest(&self) -> Result<PluginManifest, serde_json::Error> {
        serde_json::from_value(self.manifest.clone())
    }
}

impl TryFrom<Plugin> for PluginResponse {
    type Error = serde_json::Error;

    fn try_from(p: Plugin) -> Result<Self, Self::Error> {
        let manifest = p.parse_manifest()?;
        Ok(PluginResponse {
            uuid: p.uuid,
            name: p.name,
            display_name: p.display_name,
            version: p.version,
            description: p.description,
            manifest,
            state: p.state,
            trust_level: p.trust_level,
            installed_by: p.installed_by,
            installed_at: p.installed_at,
            updated_at: p.updated_at,
            bundle_hash: p.bundle_hash,
            bundle_size: p.bundle_size,
            bundle_uploaded_at: p.bundle_uploaded_at,
            source: p.source,
            // Default to None; handlers enrich via a separate
            // revocation map lookup so the conversion stays
            // dependency-free and the bulk list endpoint can resolve
            // every plugin's revocation in a single round-trip.
            signer_revoked_at: None,
        })
    }
}

/// Plugin setting response (hides secret values)
#[derive(Debug, Serialize)]
pub struct PluginSettingResponse {
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
}

impl From<PluginData> for PluginSettingResponse {
    fn from(d: PluginData) -> Self {
        PluginSettingResponse {
            key: d.key,
            // Hide secret values in response
            value: if d.is_secret { None } else { d.value },
            is_secret: d.is_secret,
        }
    }
}

/// Request to set a plugin setting or storage
#[derive(Debug, Deserialize)]
pub struct SetPluginDataRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// Plugin storage response
#[derive(Debug, Serialize)]
pub struct PluginStorageResponse {
    pub key: String,
    pub value: Option<serde_json::Value>,
}

impl From<PluginData> for PluginStorageResponse {
    fn from(d: PluginData) -> Self {
        PluginStorageResponse {
            key: d.key,
            value: d.value,
        }
    }
}

/// Plugin activity response
#[derive(Debug, Serialize)]
pub struct PluginActivityResponse {
    pub uuid: Uuid,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
    pub created_at: NaiveDateTime,
}

impl From<PluginActivity> for PluginActivityResponse {
    fn from(a: PluginActivity) -> Self {
        PluginActivityResponse {
            uuid: a.uuid,
            action: a.action,
            details: a.details,
            user_uuid: a.user_uuid,
            created_at: a.created_at,
        }
    }
}

/// Request for proxied external API calls
#[derive(Debug, Deserialize)]
pub struct PluginProxyRequest {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub body: Option<serde_json::Value>,
    /// Body encoding: "json" (default) or "form" (application/x-www-form-urlencoded)
    pub content_type: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// Response from proxied external API call
#[derive(Debug, Serialize)]
pub struct PluginProxyResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

// ===== PLUGIN COLLECTION TYPES =====

/// Collection field definition in plugin manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionFieldDefinition {
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub reference: Option<String>,
}

/// Collection definition in plugin manifest. `schema_version` is
/// required so future plugin versions can express migrations
/// (rename, drop, retype a field) without losing data. v1
/// recognises only schema_version 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDefinition {
    pub schema_version: u32,
    pub label: Option<String>,
    pub fields: std::collections::HashMap<String, CollectionFieldDefinition>,
}

/// Plugin collection schema (DB row)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
#[diesel(belongs_to(Plugin))]
pub struct PluginCollectionSchema {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// New collection schema for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
pub struct NewPluginCollectionSchema {
    pub plugin_id: i32,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
}

/// Collection schema update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
pub struct PluginCollectionSchemaUpdate {
    pub schema: Option<serde_json::Value>,
    pub version: Option<i32>,
}

/// Plugin collection row (DB row)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
#[diesel(belongs_to(PluginCollectionSchema, foreign_key = schema_id))]
pub struct PluginCollectionRow {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub schema_id: i32,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// New collection row for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
pub struct NewPluginCollectionRow {
    pub plugin_id: i32,
    pub schema_id: i32,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
}

/// Collection row update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
pub struct PluginCollectionRowUpdate {
    pub data: Option<serde_json::Value>,
}

// ===== COLLECTION API TYPES =====

/// Query params for listing collection rows
#[derive(Debug, Deserialize)]
pub struct CollectionQueryParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub filter: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

/// Request to create a collection row
#[derive(Debug, Deserialize)]
pub struct CreateCollectionRowRequest {
    pub data: serde_json::Value,
}

/// Request to update a collection row
#[derive(Debug, Deserialize)]
pub struct UpdateCollectionRowRequest {
    pub data: serde_json::Value,
}

/// Collection row API response
#[derive(Debug, Serialize)]
pub struct CollectionRowResponse {
    pub uuid: Uuid,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<PluginCollectionRow> for CollectionRowResponse {
    fn from(row: PluginCollectionRow) -> Self {
        CollectionRowResponse {
            uuid: row.uuid,
            data: row.data,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Paginated collection rows response
#[derive(Debug, Serialize)]
pub struct CollectionListResponse {
    pub rows: Vec<CollectionRowResponse>,
    pub total: i64,
}

/// Collection schema API response
#[derive(Debug, Serialize)]
pub struct CollectionSchemaResponse {
    pub uuid: Uuid,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
    pub row_count: i64,
}

// ============================================================================
// Channels — multi-channel message ingestion framework
// ============================================================================
//
// See services/channels/mod.rs for the adapter trait hierarchy and event
// shapes; these structs are the persisted representations. The tables
// model N channel instances from day one even though phase 1 ships a
// single-mailbox admin UI.

/// Direction of a [`ChannelMessage`]. Stored as a string in the DB so new
/// variants don't require schema churn; validated by a CHECK constraint.
pub const CHANNEL_DIRECTION_INBOUND: &str = "inbound";
pub const CHANNEL_DIRECTION_OUTBOUND: &str = "outbound";

/// Credential-type tags stored on [`ChannelCredential::credential_type`].
/// Not an enum because new providers (Slack, Teams, Discord) each bring
/// their own credential kinds — keeping this as a string keeps the schema
/// open for extension without migration.
pub const CRED_TYPE_IMAP_PASSWORD: &str = "imap_password";

/// `channels.provider` values for the two email ingestion paths. `email_imap`
/// polls a mailbox (self-host / niche providers); `email_forward` receives
/// mail the customer forwards to a generated `<token>@inbound.<domain>`
/// address (the hosted path). Both feed the same parse pipeline; only the
/// ingestion source differs.
pub const CHANNEL_PROVIDER_EMAIL_IMAP: &str = "email_imap";
pub const CHANNEL_PROVIDER_EMAIL_FORWARD: &str = "email_forward";

/// `inbound_addresses.status` values, in lockstep with the
/// `inbound_addresses_status_check` SQL constraint. `active` addresses route;
/// `retired` ones are kept on record but no longer resolve.
pub const INBOUND_ADDRESS_STATUS_ACTIVE: &str = "active";
pub const INBOUND_ADDRESS_STATUS_RETIRED: &str = "retired";

/// `inbound_dead_letters.reason` values. `unknown_token` is clean mail (scans
/// passed) that resolved to no active forwarding token.
pub const INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN: &str = "unknown_token";

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::channels)]
pub struct Channel {
    pub id: i32,
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub runtime_state: serde_json::Value,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_polled_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::channels)]
pub struct NewChannel {
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
}

/// Partial update to an existing channel. `Option<Option<T>>` fields use
/// `Some(None)` to explicitly clear; plain `None` means "don't change."
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::channels)]
pub struct ChannelUpdate {
    pub provider: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<serde_json::Value>,
    pub runtime_state: Option<serde_json::Value>,
    pub last_polled_at: Option<Option<NaiveDateTime>>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Encrypted secret associated with a channel. The plaintext value never
/// leaves `utils::encryption`; this struct carries only the ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::channel_credentials)]
#[diesel(belongs_to(Channel))]
pub struct ChannelCredential {
    pub id: i32,
    pub channel_id: i32,
    pub credential_type: String,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
    /// Framed AES-256-GCM blob (`utils::encryption::Keyring` shape).
    /// AAD = `channel_id.to_be_bytes() ‖ b":" ‖ credential_type.as_bytes()`.
    pub encrypted_value: Vec<u8>,
    pub encrypted_kek_id: i16,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::channel_credentials)]
pub struct NewChannelCredential {
    pub channel_id: i32,
    pub credential_type: String,
    pub encrypted_value: Vec<u8>,
    pub encrypted_kek_id: i16,
    pub expires_at: Option<NaiveDateTime>,
}

/// Per-workspace outbound email identity (one row per workspace).
///
/// Deliberately NOT `Serialize`: `encrypted_smtp_password` must never reach
/// a client. The admin handler builds a separate response DTO carrying a
/// `password_configured` flag instead of the ciphertext. The blob is a
/// framed AES-256-GCM value (`utils::encryption::Keyring` shape) with
/// AAD = `workspace_id.to_be_bytes() ‖ b".nosdesk.workspace.email.v1"`,
/// decrypted by the outbound resolver at send time.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = crate::schema::workspace_email_settings)]
pub struct WorkspaceEmailSettings {
    pub workspace_id: i32,
    pub enabled: bool,
    pub from_name: String,
    pub from_email: String,
    pub smtp_host: String,
    pub smtp_port: i32,
    pub smtp_security: String,
    pub smtp_username: String,
    pub encrypted_smtp_password: Option<Vec<u8>>,
    pub encrypted_kek_id: Option<i16>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    /// How this workspace sends: `fallback` (instance identity),
    /// `verified_domain` (DKIM-signed via the instance relay), or
    /// `smtp_relay` (the workspace's own relay, the `smtp_*` columns).
    /// See [`workspace_email_sending_mode`].
    pub sending_mode: String,
    /// The verified sending domain (the `From` domain), for `verified_domain`.
    pub sending_domain: Option<String>,
    /// DKIM selector (`<selector>._domainkey.<sending_domain>`).
    pub dkim_selector: Option<String>,
    /// `rsa` | `ed25519`.
    pub dkim_algorithm: Option<String>,
    /// KEK-encrypted DKIM private key (framed AES-256-GCM blob), AAD-bound to
    /// the workspace. Redacted from the audit log.
    pub encrypted_dkim_private_key: Option<Vec<u8>>,
    /// kek_id sidecar for `encrypted_dkim_private_key`.
    pub dkim_kek_id: Option<i16>,
    /// `unverified` | `pending` | `verified` | `failed`. Only `verified`
    /// permits sending from the workspace's domain.
    pub verification_status: String,
    pub verified_at: Option<NaiveDateTime>,
}

/// Sending-mode + verification-status constants, kept in lockstep with the
/// `workspace_email_settings_*_check` SQL constraints.
pub mod workspace_email_sending_mode {
    pub const FALLBACK: &str = "fallback";
    pub const VERIFIED_DOMAIN: &str = "verified_domain";
    pub const SMTP_RELAY: &str = "smtp_relay";
}

pub mod workspace_email_verification_status {
    pub const UNVERIFIED: &str = "unverified";
    pub const PENDING: &str = "pending";
    pub const VERIFIED: &str = "verified";
    pub const FAILED: &str = "failed";
}

/// Editable fields of [`WorkspaceEmailSettings`]. Omits `workspace_id` (the
/// RLS GUC fills it on insert), the password and DKIM columns (managed
/// separately by `set_password`/`clear_password` and `provision_dkim`), and
/// the timestamps. `sending_mode` chooses how the workspace sends; the
/// verified-domain fields are populated by `provision_dkim`.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::workspace_email_settings)]
pub struct UpsertWorkspaceEmailSettings {
    pub enabled: bool,
    pub from_name: String,
    pub from_email: String,
    pub smtp_host: String,
    pub smtp_port: i32,
    pub smtp_security: String,
    pub smtp_username: String,
    /// See [`workspace_email_sending_mode`].
    pub sending_mode: String,
}

/// Ledger row — one per inbound or outbound message through a channel.
/// Used for dedup (unique on `channel_id, external_id, direction`),
/// thread resolution (lookup by `external_id`), and audit.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::channel_messages)]
#[diesel(belongs_to(Channel))]
#[diesel(belongs_to(Ticket))]
pub struct ChannelMessage {
    pub id: i64,
    pub channel_id: i32,
    pub external_id: String,
    pub direction: String,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub in_reply_to: Option<String>,
    pub from_address: Option<String>,
    pub author_user_uuid: Option<Uuid>,
    pub raw_metadata: Option<serde_json::Value>,
    pub received_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::channel_messages)]
pub struct NewChannelMessage {
    pub channel_id: i32,
    pub external_id: String,
    pub direction: String,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub in_reply_to: Option<String>,
    pub from_address: Option<String>,
    pub author_user_uuid: Option<Uuid>,
    pub raw_metadata: Option<serde_json::Value>,
}

/// A forwarding address (`<token>@inbound.<domain>`) owned by an
/// `email_forward` channel. The `token` is the routing key the inbound
/// webhook resolves; see `repository::inbound_addresses` and the
/// `inbound_addresses` migration for the capability rationale.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::inbound_addresses)]
#[diesel(belongs_to(Channel))]
pub struct InboundAddress {
    pub id: i32,
    pub token: String,
    pub channel_id: i32,
    /// See [`INBOUND_ADDRESS_STATUS_ACTIVE`] / [`INBOUND_ADDRESS_STATUS_RETIRED`].
    pub status: String,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// Insert shape for a new forwarding address. `status` defaults to `active`,
/// `workspace_id` is filled from the RLS GUC, and the timestamp defaults at
/// the DB; the caller supplies only the channel and the generated token.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::inbound_addresses)]
pub struct NewInboundAddress {
    pub token: String,
    pub channel_id: i32,
}

/// A platform-level dead-letter row: clean inbound mail (spam/virus scans
/// passed) that resolved to no active forwarding token. Untenanted by design
/// (see the `inbound_dead_letters` migration) because an unknown token can't
/// be attributed to a workspace; surfaced to the operator so a misconfigured
/// forward is diagnosable rather than silently lost.
#[derive(Debug, Clone, Serialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::inbound_dead_letters)]
pub struct InboundDeadLetter {
    pub id: i64,
    pub envelope_recipient: String,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub s3_key: String,
    /// See [`INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN`].
    pub reason: String,
    pub received_at: NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::inbound_dead_letters)]
pub struct NewInboundDeadLetter {
    pub envelope_recipient: String,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub s3_key: String,
    pub reason: String,
}

// ---------- Canned responses ----------

/// Reusable reply template that techs can pull into the ticket
/// composer with one click. Shared across the team (not per-user);
/// `created_by` is informational.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct CannedResponse {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable, Deserialize)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct NewCannedResponse {
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
}

/// Partial-update payload. `Option<T>` fields leave the column
/// untouched when `None`.
#[derive(Debug, Default, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct CannedResponseUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

/// API shape for the admin list page: a canned response plus its
/// rolling 30-day insertion count. The composer picker doesn't read
/// `inserts_30d` but the field is cheap to include, so we ship one
/// list endpoint instead of two.
#[derive(Debug, Clone, Serialize)]
pub struct CannedResponseListItem {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
    /// Insertions in the last 30 days. `0` for templates that have
    /// never been used or were last used >30d ago.
    pub inserts_30d: i64,
}

impl CannedResponseListItem {
    pub fn from_parts(row: CannedResponse, inserts_30d: i64) -> Self {
        Self {
            id: row.id,
            title: row.title,
            body: row.body,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            workspace_id: row.workspace_id,
            inserts_30d,
        }
    }
}

/// Insertable for `canned_response_insertions`. One row per use of
/// a canned response in the composer. Append-only workspace-local
/// usage log; the admin list page rolls these into the 30-day
/// counter column.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::canned_response_insertions)]
pub struct NewCannedResponseInsertion {
    pub canned_response_id: i32,
    pub user_uuid: Option<Uuid>,
    pub ticket_id: Option<i32>,
    pub workspace_id: i32,
}

/// Read-only "starter template" served by the admin endpoint as a
/// browseable catalog. Selecting one pre-fills the editor; nothing
/// is persisted until the admin clicks Save.
#[derive(Debug, Clone, Serialize)]
pub struct CannedResponseStarter {
    /// Stable identifier used by the frontend to address one
    /// starter in the catalog. Not a database id.
    pub slug: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

// ============================================================================
// Search Query Log (Phase 2c of the docs/KB redesign)
// ============================================================================

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::search_query_log)]
pub struct NewSearchQueryLog {
    pub query_raw: String,
    pub query_norm: String,
    pub result_count: i32,
}

// ============================================================================
// Knowledge Gaps (Phase 2a of the docs/KB redesign)
// ============================================================================
//
// `knowledge_gaps` is the canonical entity (lifecycle, ranking,
// resolution); `knowledge_gap_signals` carries raw evidence with
// a polymorphic source reference. See the migration for the data
// model rationale; the short version is "every detection
// mechanism writes into the same shape so an LLM in Phase 3 can
// consume them uniformly."

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::knowledge_gaps)]
pub struct KnowledgeGap {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub assignee_uuid: Option<Uuid>,
    pub resolved_page_id: Option<i32>,
    pub evidence_count: i32,
    pub last_evidence_at: Option<NaiveDateTime>,
    pub impact_score: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub dismissed_at: Option<NaiveDateTime>,
    pub dismissed_by: Option<Uuid>,
    pub resolved_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::knowledge_gaps)]
pub struct NewKnowledgeGap {
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub created_by: Option<Uuid>,
    pub impact_score: i32,
    pub evidence_count: i32,
    pub last_evidence_at: Option<NaiveDateTime>,
}

#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::knowledge_gaps)]
pub struct KnowledgeGapUpdate {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub status: Option<String>,
    pub assignee_uuid: Option<Option<Uuid>>,
    pub resolved_page_id: Option<Option<i32>>,
    pub evidence_count: Option<i32>,
    pub last_evidence_at: Option<Option<NaiveDateTime>>,
    pub impact_score: Option<i32>,
    pub updated_at: Option<NaiveDateTime>,
    pub dismissed_at: Option<Option<NaiveDateTime>>,
    pub dismissed_by: Option<Option<Uuid>>,
    pub resolved_at: Option<Option<NaiveDateTime>>,
}

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Associations, Clone)]
#[diesel(table_name = crate::schema::knowledge_gap_signals)]
#[diesel(belongs_to(KnowledgeGap, foreign_key = gap_id))]
pub struct KnowledgeGapSignal {
    pub id: i64,
    pub gap_id: i64,
    pub signal_type: String,
    pub source_kind: String,
    pub source_ref: String,
    pub payload: serde_json::Value,
    pub confidence: i32,
    pub detected_by: Option<Uuid>,
    pub detected_at: NaiveDateTime,
    pub dismissed_at: Option<NaiveDateTime>,
    pub dismissed_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::knowledge_gap_signals)]
pub struct NewKnowledgeGapSignal {
    pub gap_id: i64,
    pub signal_type: String,
    pub source_kind: String,
    pub source_ref: String,
    pub payload: serde_json::Value,
    pub confidence: i32,
    pub detected_by: Option<Uuid>,
}

// ── CSP violation reports ─────────────────────────────────────
//
// Browser-submitted reports of Content-Security-Policy violations.
// See `repository/csp_reports.rs` for upsert semantics; identical
// reports increment `occurrence_count` rather than inserting new
// rows.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::csp_reports)]
pub struct CspReport {
    pub id: i64,
    pub dedup_hash: String,
    pub effective_directive: String,
    pub blocked_uri: Option<String>,
    pub source_file: Option<String>,
    pub line_number: Option<i32>,
    pub column_number: Option<i32>,
    pub document_uri: String,
    pub referrer: Option<String>,
    pub violated_directive: Option<String>,
    pub original_policy: Option<String>,
    pub disposition: String,
    pub user_agent: Option<String>,
    pub user_uuid: Option<Uuid>,
    pub occurrence_count: i32,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::csp_reports)]
pub struct NewCspReport {
    pub dedup_hash: String,
    pub effective_directive: String,
    pub blocked_uri: Option<String>,
    pub source_file: Option<String>,
    pub line_number: Option<i32>,
    pub column_number: Option<i32>,
    pub document_uri: String,
    pub referrer: Option<String>,
    pub violated_directive: Option<String>,
    pub original_policy: Option<String>,
    pub disposition: String,
    pub user_agent: Option<String>,
    pub user_uuid: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Bug reports
// ---------------------------------------------------------------------------
//
// User-submitted bug reports from the in-app "Report a problem" modal.
// One row per submission, workspace-scoped via RLS. See
// `repository/bug_reports.rs` and `handlers/bug_reports.rs`.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::bug_reports)]
pub struct BugReport {
    pub id: i64,
    pub workspace_id: i32,
    pub user_uuid: Option<Uuid>,
    pub session_id: Uuid,
    pub description: String,
    pub url: String,
    pub breadcrumbs: serde_json::Value,
    pub build_sha: String,
    pub user_agent: Option<String>,
    pub viewport: Option<serde_json::Value>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::bug_reports)]
pub struct NewBugReport {
    pub session_id: Uuid,
    pub user_uuid: Option<Uuid>,
    pub description: String,
    pub url: String,
    pub breadcrumbs: serde_json::Value,
    pub build_sha: String,
    pub user_agent: Option<String>,
    pub viewport: Option<serde_json::Value>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Audit log
// ---------------------------------------------------------------------------
//
// Read-only model. The Postgres trigger `audit_log_trigger()` writes rows
// from inside user-facing transactions; the application never inserts into
// this table directly. See `repository/audit_log.rs` for the read API and
// `migrations/2026-05-02-100000_sync_substrate/up.sql` for the trigger.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable)]
#[diesel(table_name = crate::schema::audit_log)]
pub struct AuditLogRow {
    pub id: i64,
    pub table_name: String,
    pub pk_text: String,
    /// One of 'I' / 'U' / 'D' (CHECK constraint enforced by Postgres).
    pub op: String,
    pub before_jsonb: Option<serde_json::Value>,
    pub after_jsonb: Option<serde_json::Value>,
    /// Set only on UPDATE; lists JSONB keys that changed.
    pub changed_cols: Option<Vec<Option<String>>>,
    pub actor_uuid: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

// ---------------------------------------------------------------------------
// Outbound email queue (Item J Pass 1)
// ---------------------------------------------------------------------------
//
// Durable, retryable replacement for the `tokio::spawn` fire-and-forget
// outbound path. Every external-channel send goes through this queue; a
// worker drains via SELECT FOR UPDATE SKIP LOCKED and dispatches to SMTP.
// See migrations/2026-05-11-300000_outbound_emails_queue and
// `services/email_queue/` for the worker.

/// One outbound email row. The `status` column is constrained at the
/// schema layer to `pending | sending | sent | failed | dead | suppressed`.
///
/// `QueryableByName` is needed alongside `Queryable` because the worker's
/// claim path uses a CTE-with-UPDATE pattern that Diesel's typed builder
/// can't express; the raw `sql_query(...).load::<OutboundEmail>()` shape
/// needs the by-name variant.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, QueryableByName, Identifiable)]
#[diesel(table_name = crate::schema::outbound_emails)]
pub struct OutboundEmail {
    pub id: i64,
    /// `Some(channel_id)` for ticket-reply rows that thread back into
    /// an inbound channel; `None` for transactional sends (password
    /// reset, invitation, notification) that don't belong to any
    /// channel. The worker skips the `channel_messages` book-keeping
    /// step when this is None.
    pub channel_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub recipient: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    /// Stamped at enqueue, persisted, reused on every retry. Receiving
    /// MTAs and customer MUAs dedupe on Message-ID — this is the
    /// primary defense against crash-mid-send duplicates.
    pub message_id: String,
    pub in_reply_to: Option<String>,
    /// Diesel renders `TEXT[]` columns as `Vec<Option<String>>`; nulls
    /// inside the array are unused but the type plumbing requires it.
    pub references_list: Vec<Option<String>>,
    pub headers_json: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub last_smtp_code: Option<i32>,
    pub next_attempt_at: chrono::DateTime<chrono::Utc>,
    pub lease_token: Option<Uuid>,
    pub lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub sent_at: Option<chrono::DateTime<chrono::Utc>>,
    pub failed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub correlation_id: Option<Uuid>,
    /// Stamped when an inbound DSN linked back to this row via
    /// the deterministic Message-ID. NULL for the normal happy
    /// path. See migration `2026-05-12-100000_outbound_email_bounce_fields`.
    pub bounced_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The address the remote MTA rejected. Usually matches
    /// `recipient`; differs when the original recipient was a
    /// distribution list / forwarder and the failure came from
    /// the downstream member.
    pub bounce_recipient: Option<String>,
    /// Raw RFC 3464 Diagnostic-Code or Status text from the
    /// DSN's `message/delivery-status` part. Verbatim so the
    /// admin queue UI can show the upstream reason without us
    /// having to guess at categorisation.
    pub bounce_diagnostic: Option<String>,
    /// Optional caller-supplied key for at-least-once → effectively-
    /// once enqueue. Two enqueues with the same key collapse to a
    /// single queue row (see `repository::outbound_emails::enqueue_idempotent`).
    /// Channel-reply rows leave it NULL — they're already deduped at
    /// the handler layer via stable Message-ID.
    pub idempotency_key: Option<String>,
    pub workspace_id: i32,
    /// The sending provider's own message id. **Always NULL under SMTP** (no
    /// provider id; the RFC `message_id` is the only identity). The worker still
    /// plumbs it through `mark_sent`, so it is ready for a future transport that
    /// returns one, but nothing populates it today.
    pub provider_message_id: Option<String>,
    /// Which sending identity the worker uses for this row (see
    /// [`outbound_email_sender_identity`]): `workspace` (the workspace's own
    /// SMTP identity, falling back to the instance identity) or `platform`
    /// (the instance identity, for auth mail that must not originate from a
    /// tenant relay). Decided at enqueue.
    pub sender_identity: String,
    /// Notification vs transactional (see [`outbound_email_mail_class`]).
    /// Drives deliverability headers (List-Unsubscribe on notification only).
    /// Last field so the column order matches the schema.
    pub mail_class: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::outbound_emails)]
pub struct NewOutboundEmail {
    /// `Some(channel_id)` for channel-mediated ticket replies. `None`
    /// for transactional sends that don't bind to any channel.
    pub channel_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub recipient: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references_list: Vec<Option<String>>,
    pub headers_json: serde_json::Value,
    pub correlation_id: Option<Uuid>,
    /// Idempotency key — see `OutboundEmail::idempotency_key`. Use
    /// `enqueue_idempotent` when this is `Some`; `enqueue` with a
    /// None key for fire-and-forget channel replies.
    pub idempotency_key: Option<String>,
    /// See [`outbound_email_sender_identity`]: `workspace` for conversation /
    /// notification mail, `platform` for password reset / invitation.
    pub sender_identity: String,
    /// See [`outbound_email_mail_class`]: `notification` (opt-out-able) or
    /// `transactional` (must-deliver). Set explicitly at enqueue.
    pub mail_class: String,
}

/// Status string constants. Centralised so Rust callers (worker, repo,
/// admin handlers) and SQL CHECK constraint stay in lockstep.
pub mod outbound_email_status {
    pub const PENDING: &str = "pending";
    pub const SENDING: &str = "sending";
    pub const SENT: &str = "sent";
    pub const FAILED: &str = "failed";
    pub const DEAD: &str = "dead";
    pub const SUPPRESSED: &str = "suppressed";
}

/// Sender-identity constants, kept in lockstep with the
/// `outbound_emails_sender_identity_check` SQL constraint.
///
/// `WORKSPACE` is tenant-content mail (notifications, the portal sign-in link):
/// it sends ONLY from the workspace's own verified sending domain and is
/// deferred (never sent from the platform) until one is configured, so
/// tenant-controlled content never leaves on the platform domain — a phishing
/// and deliverability-reputation boundary. `PLATFORM` is platform-own mail
/// (account/auth, billing) to the platform's own users: it pins the instance
/// identity and never originates from a tenant relay.
pub mod outbound_email_sender_identity {
    pub const WORKSPACE: &str = "workspace";
    pub const PLATFORM: &str = "platform";
}

/// Mail-class constants, kept in lockstep with the
/// `outbound_emails_mail_class_check` SQL constraint. `NOTIFICATION` is
/// opt-out-able mail (ticket-update notifications) that carries
/// List-Unsubscribe; `TRANSACTIONAL` is must-deliver mail (password reset,
/// invitation, the agent's reply, auto-ack) that never does. A distinct axis
/// from sender identity: a conversation reply is `workspace` + `transactional`.
pub mod outbound_email_mail_class {
    pub const TRANSACTIONAL: &str = "transactional";
    pub const NOTIFICATION: &str = "notification";
}

// === Email suppression list ==================================
//
// Addresses on this list are skipped by the outbound enqueue path.
// Auto-populated by hard-bounce detection (J Pass 2.2b) and
// manually managed by admins via the suppression admin view. See
// migration `2026-05-12-110000_email_suppressions`.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(primary_key(email))]
#[diesel(table_name = crate::schema::email_suppressions)]
pub struct EmailSuppression {
    pub email: String,
    /// Short identifier the admin UI groups on: `hard_bounce`,
    /// `manual`, `complaint`. Kept loose so future categories
    /// (`unsubscribe`, `gdpr_erase`) don't require a migration.
    pub reason: String,
    /// Verbatim upstream diagnostic from the most recent bounce.
    /// `NULL` for manually-added entries.
    pub bounce_diagnostic: Option<String>,
    /// Bumped each time the same address bounces again so admins
    /// can spot chronic vs one-off issues.
    pub bounce_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::email_suppressions)]
pub struct NewEmailSuppression {
    pub email: String,
    pub reason: String,
    pub bounce_diagnostic: Option<String>,
}

/// Suppression reason constants.
pub mod email_suppression_reason {
    pub const HARD_BOUNCE: &str = "hard_bounce";
    pub const MANUAL: &str = "manual";
    /// Recipient marked a message as spam (a feedback-loop complaint).
    /// Continuing to send to a complainer wrecks sender reputation.
    pub const COMPLAINT: &str = "complaint";
}

// =====================================================================
// Rules engine (docs/rules-and-actions-plan.md). Phase 1 ships the
// manual-trigger surface; the data model below is the unified shape
// Phase 2 (event triggers), Phase 3 (time-elapsed), and Phase 4
// (webhook actions) extend without schema changes.
// =====================================================================

/// Workflow state of a rule. `draft` is the editor's initial state,
/// `dry_run` writes shadow rule_applications rows the admin reviews
/// before flipping to `live`, `live` is in-flight, `archived` is a
/// soft-deleted row that the picker / engine ignore. See decisions
/// 12 + 32 in the plan: archived is the only state hard-delete is
/// permitted from.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::RuleState)]
pub enum RuleState {
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "dry_run")]
    DryRun,
    #[serde(rename = "live")]
    Live,
    #[serde(rename = "archived")]
    Archived,
}

impl RuleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::DryRun => "dry_run",
            Self::Live => "live",
            Self::Archived => "archived",
        }
    }
}

impl ToSql<crate::schema::sql_types::RuleState, Pg> for RuleState {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::RuleState, Pg> for RuleState {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"draft" => Ok(Self::Draft),
            b"dry_run" => Ok(Self::DryRun),
            b"live" => Ok(Self::Live),
            b"archived" => Ok(Self::Archived),
            other => Err(format!("unknown rule_state: {}", String::from_utf8_lossy(other)).into()),
        }
    }
}

/// What kind of event a rule reacts to. `manual` (Phase 1) is fired
/// from the agent Actions toolbar; the rest (Phase 2+) are fired by
/// the engine subscribing to the existing `sync_actions` stream
/// (ticket_created / ticket_updated / ticket_replied) or the
/// per-ticket due_at queue (time_elapsed).
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::RuleTriggerKind)]
pub enum RuleTriggerKind {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "ticket_created")]
    TicketCreated,
    #[serde(rename = "ticket_updated")]
    TicketUpdated,
    #[serde(rename = "ticket_replied")]
    TicketReplied,
    #[serde(rename = "time_elapsed")]
    TimeElapsed,
}

impl RuleTriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::TicketCreated => "ticket_created",
            Self::TicketUpdated => "ticket_updated",
            Self::TicketReplied => "ticket_replied",
            Self::TimeElapsed => "time_elapsed",
        }
    }
}

impl ToSql<crate::schema::sql_types::RuleTriggerKind, Pg> for RuleTriggerKind {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::RuleTriggerKind, Pg> for RuleTriggerKind {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"manual" => Ok(Self::Manual),
            b"ticket_created" => Ok(Self::TicketCreated),
            b"ticket_updated" => Ok(Self::TicketUpdated),
            b"ticket_replied" => Ok(Self::TicketReplied),
            b"time_elapsed" => Ok(Self::TimeElapsed),
            other => Err(format!(
                "unknown rule_trigger_kind: {}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

/// Outcome of one fire attempt. Captured on every `rule_applications`
/// row so the admin log can answer "why didn't this rule fire?"
/// without log-tailing. See plan §4.3.
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::RuleApplicationStatus)]
pub enum RuleApplicationStatus {
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "dry_run")]
    DryRun,
    #[serde(rename = "skipped_preflight")]
    SkippedPreflight,
    #[serde(rename = "skipped_condition_unmet")]
    SkippedConditionUnmet,
    #[serde(rename = "suppressed_recursion_budget")]
    SuppressedRecursionBudget,
    #[serde(rename = "suppressed_loop_guard")]
    SuppressedLoopGuard,
    #[serde(rename = "failed")]
    Failed,
}

impl RuleApplicationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::DryRun => "dry_run",
            Self::SkippedPreflight => "skipped_preflight",
            Self::SkippedConditionUnmet => "skipped_condition_unmet",
            Self::SuppressedRecursionBudget => "suppressed_recursion_budget",
            Self::SuppressedLoopGuard => "suppressed_loop_guard",
            Self::Failed => "failed",
        }
    }
}

impl ToSql<crate::schema::sql_types::RuleApplicationStatus, Pg> for RuleApplicationStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::RuleApplicationStatus, Pg> for RuleApplicationStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"succeeded" => Ok(Self::Succeeded),
            b"dry_run" => Ok(Self::DryRun),
            b"skipped_preflight" => Ok(Self::SkippedPreflight),
            b"skipped_condition_unmet" => Ok(Self::SkippedConditionUnmet),
            b"suppressed_recursion_budget" => Ok(Self::SuppressedRecursionBudget),
            b"suppressed_loop_guard" => Ok(Self::SuppressedLoopGuard),
            b"failed" => Ok(Self::Failed),
            other => Err(format!(
                "unknown rule_application_status: {}",
                String::from_utf8_lossy(other)
            )
            .into()),
        }
    }
}

/// One rule. Read view; INSERTs go via `NewRule` and UPDATEs via
/// `RuleUpdate`. `reads_set` and `writes_set` are derived from
/// `conditions` and `actions` at save time by the repository helper.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::rules)]
pub struct Rule {
    pub id: i32,
    pub workspace_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: serde_json::Value,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    // Postgres TEXT[] elements are nullable by default. The repository
    // helpers only ever insert non-null values; reads expose the
    // Option<String> shape Diesel's schema requires. Convert at the
    // boundary in the API layer if a flat Vec<String> is needed.
    pub reads_set: Vec<Option<String>>,
    pub writes_set: Vec<Option<String>>,
    pub state: RuleState,
    pub priority: i32,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub fire_count: i32,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

/// INSERT row for `rules`. The repository populates `reads_set` and
/// `writes_set` from the conditions / actions trees before passing
/// this in, so the engine's skip-on-no-reads-changed query path and
/// the self-referential save linter both work off durable columns.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::rules)]
pub struct NewRule {
    pub workspace_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: serde_json::Value,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    pub reads_set: Vec<Option<String>>,
    pub writes_set: Vec<Option<String>>,
    pub state: RuleState,
    pub priority: i32,
    pub created_by: Option<Uuid>,
}

/// PATCH row for `rules`. Every field is `Option`-wrapped so the
/// editor's partial updates round-trip without clobbering unsent
/// fields. The repository's update path recomputes `reads_set` /
/// `writes_set` when conditions or actions move.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::rules)]
pub struct RuleUpdate {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub trigger_kind: Option<RuleTriggerKind>,
    pub trigger_config: Option<serde_json::Value>,
    pub conditions: Option<serde_json::Value>,
    pub actions: Option<serde_json::Value>,
    pub reads_set: Option<Vec<Option<String>>>,
    pub writes_set: Option<Vec<Option<String>>>,
    pub state: Option<RuleState>,
    pub priority: Option<i32>,
    pub last_fired_at: Option<Option<DateTime<Utc>>>,
    pub fire_count: Option<i32>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
}

/// One immutable snapshot of a rule's fields at save time. The
/// repository never INSERTs into this table directly; the
/// `rules_version_on_insert` and `rules_version_on_update` triggers
/// in the migration do.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::rule_versions)]
pub struct RuleVersion {
    pub id: i32,
    pub rule_id: i32,
    pub workspace_id: i32,
    pub version: i32,
    pub name: String,
    pub description: Option<String>,
    pub trigger_kind: RuleTriggerKind,
    pub trigger_config: serde_json::Value,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    pub state: RuleState,
    pub priority: i32,
    pub saved_by: Option<Uuid>,
    pub saved_at: DateTime<Utc>,
}

/// One row per fire attempt. See plan §4.3 for the shape: succeeded
/// rows are tight (no payloads); dry_run + failed + suppressed_* +
/// skipped_* rows carry condition / action snapshots for the
/// inspector to render.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::rule_applications)]
pub struct RuleApplication {
    pub id: i64,
    pub workspace_id: i32,
    pub rule_id: i32,
    pub rule_version: i32,
    pub ticket_id: i32,
    pub status: RuleApplicationStatus,
    pub correlation_id: Option<Uuid>,
    pub actor_uuid: Option<Uuid>,
    pub actor_kind: String,
    pub originating_event_id: Option<Uuid>,
    pub originating_event_kind: Option<String>,
    pub condition_evaluation: Option<serde_json::Value>,
    pub actions_taken: Option<serde_json::Value>,
    pub actions_skipped: Option<serde_json::Value>,
    pub failure_reason: Option<String>,
    pub applied_at: DateTime<Utc>,
}

/// INSERT row for `rule_applications`. Built by the engine inside
/// the apply transaction so the correlation_id stitches with the
/// audit_log + sync_actions rows the same apply produced.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::rule_applications)]
pub struct NewRuleApplication {
    pub workspace_id: i32,
    pub rule_id: i32,
    pub rule_version: i32,
    pub ticket_id: i32,
    pub status: RuleApplicationStatus,
    pub correlation_id: Option<Uuid>,
    pub actor_uuid: Option<Uuid>,
    pub actor_kind: String,
    pub originating_event_id: Option<Uuid>,
    pub originating_event_kind: Option<String>,
    pub condition_evaluation: Option<serde_json::Value>,
    pub actions_taken: Option<serde_json::Value>,
    pub actions_skipped: Option<serde_json::Value>,
    pub failure_reason: Option<String>,
}

/// Per-event recursion budget row. The engine attempts an INSERT
/// for every fire candidate; ON CONFLICT means the budget is
/// consumed and the fire is suppressed. Phase 2 substrate (no
/// active writer in Phase 1).
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Insertable)]
#[diesel(table_name = crate::schema::ticket_rule_runs, primary_key(event_id, ticket_id, rule_id))]
pub struct TicketRuleRun {
    pub event_id: Uuid,
    pub ticket_id: i32,
    pub rule_id: i32,
    pub fired_at: DateTime<Utc>,
}
