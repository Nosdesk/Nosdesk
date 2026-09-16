use super::assets::Asset;
use super::comments::CommentWithAttachments;
use super::projects::Project;
use super::serialize_optional_uuid_as_string;
use super::users::UserInfoWithAvatar;
use super::workflow_states::WorkflowState;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

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
    /// ("not spam").
    pub spam_suspected: bool,
    /// Optional planning start for the gantt timeline. NULL means
    /// unplanned; the gantt falls back to created_at for a bar's left
    /// edge. A planning field, distinct from the factual created_at.
    pub start_date: Option<NaiveDateTime>,
    /// P2 SLA clock: the effective start of the SLA window (also pushed forward
    /// by paused business time, so pausing subtracts). NULL = the clock has
    /// never started; for an `activated`-clock policy that means `not_started`
    /// (no pill). Set when the ticket first enters a non-pausing state.
    pub sla_clock_started_at: Option<NaiveDateTime>,
    /// When the current SLA pause began, so a resume can add the paused business
    /// time back onto `sla_clock_started_at`. NULL = not currently paused.
    pub sla_paused_at: Option<NaiveDateTime>,
    /// Per-ticket SLA override: `"auto"` (normal policy resolution) or `"none"`
    /// (this ticket has no SLA regardless of matching policies — the manual
    /// escape hatch, short-circuited at the top of `compute_pill`). Must stay the
    /// LAST field to match `schema.rs` column order (positional Queryable).
    pub sla_override: String,
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
    pub start_date: Option<NaiveDateTime>,
    pub recurrence_rule: Option<String>,
    pub recurrence_template_id: Option<i32>,
    pub resolution_notes: Option<String>,
    /// Defaults false; set true by the inbound pipeline when the source
    /// message was flagged as spam.
    pub spam_suspected: bool,
}

impl NewTicket {
    /// Keep only what a non-staff caller is allowed to set, taking every other
    /// column from the ticket as it stands.
    ///
    /// `PUT /api/tickets/{id}` overwrites the whole row: `NewTicket` is an
    /// `AsChangeset`, so every field in the body lands. Its only gate is
    /// `TicketAccess`, which is a *read* gate by its own module doc, so a
    /// requester or watcher who can merely see a ticket could set the columns
    /// that belong to staff — close or reopen it via `workflow_state_id`,
    /// assign it, change its priority or category, flip `triage_state`,
    /// `verification_state` or `spam_suspected`, hand it to someone else via
    /// `requester_uuid`, or mint a `guest_lookup_token`.
    ///
    /// Staff are unaffected. For everyone else the answer is "you may retitle
    /// your own ticket", which is deliberately narrow: this endpoint has no
    /// client (the apps use PATCH), so the cost of being strict is close to
    /// zero, and widening it later is a decision someone can make on purpose.
    ///
    /// Category is not handled here even though it is staff-controlled, because
    /// it needs a visibility lookup rather than a comparison; the handler runs
    /// the same `can_user_see_category` check its PATCH sibling already does.
    #[must_use]
    pub fn redact_for(self, can_handle_tickets: bool, existing: &Ticket) -> Self {
        if can_handle_tickets {
            return self;
        }
        Self {
            // The one field a requester owns.
            title: self.title,
            // Everything else is whatever it already was. Written field by
            // field rather than with `..existing` so that adding a column to
            // `NewTicket` fails to compile here, forcing a decision about who
            // may set it instead of defaulting it open.
            workflow_state_id: existing.workflow_state_id,
            priority: existing.priority,
            requester_uuid: existing.requester_uuid,
            assignee_uuid: existing.assignee_uuid,
            category_id: existing.category_id,
            submitted_via: existing.submitted_via.clone(),
            guest_lookup_token: existing.guest_lookup_token,
            verification_state: existing.verification_state.clone(),
            origin_channel_id: existing.origin_channel_id,
            triage_state: existing.triage_state.clone(),
            due_date: existing.due_date,
            start_date: existing.start_date,
            recurrence_rule: existing.recurrence_rule.clone(),
            recurrence_template_id: existing.recurrence_template_id,
            resolution_notes: existing.resolution_notes.clone(),
            spam_suspected: existing.spam_suspected,
        }
    }
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
    pub start_date: Option<Option<NaiveDateTime>>,
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
    /// Per-ticket SLA override: `"auto"` / `"none"`. Outer `None` leaves it
    /// unchanged; `Some(_)` sets it. A change recomputes the pill (see
    /// `pill_affecting` in `update_ticket_partial`).
    pub sla_override: Option<String>,
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

/// A comment's reference to another ticket (`comment_ticket_references`).
/// The source ticket is `comments.ticket_id`; no copy is kept here because a
/// merge moves comments between tickets.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::comment_ticket_references)]
pub struct NewCommentTicketReference {
    pub comment_id: i32,
    pub referenced_ticket_id: i32,
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

#[cfg(test)]
mod new_ticket_redaction_tests {
    use super::*;

    /// A ticket with every staff-controlled column set to a recognisable
    /// value, so a field that leaks through redaction shows up as the
    /// attacker's value rather than this one.
    fn existing_ticket() -> Ticket {
        let now = chrono::NaiveDateTime::UNIX_EPOCH;
        Ticket {
            id: 1,
            title: "As filed".into(),
            priority: TicketPriority::Low,
            requester_uuid: Some(Uuid::from_u128(1)),
            assignee_uuid: Some(Uuid::from_u128(2)),
            created_at: now,
            updated_at: now,
            created_by: Some(Uuid::from_u128(1)),
            closed_at: None,
            closed_by: None,
            category_id: Some(10),
            submitted_via: Some("email".into()),
            guest_lookup_token: Some(Uuid::from_u128(3)),
            verification_state: Some("verified".into()),
            origin_channel_id: Some(20),
            workflow_state_id: 30,
            triage_state: Some("triaged".into()),
            due_date: Some(now),
            recurrence_rule: Some("FREQ=DAILY".into()),
            recurrence_template_id: Some(40),
            resolution_notes: Some("resolved by staff".into()),
            workspace_id: 1,
            first_response_at: None,
            sla_response_target_at: None,
            sla_response_breached_at: None,
            sla_resolution_target_at: None,
            sla_resolution_breached_at: None,
            uuid: Uuid::from_u128(4),
            spam_suspected: false,
            start_date: Some(now),
            sla_clock_started_at: None,
            sla_paused_at: None,
            sla_override: "none".into(),
        }
    }

    /// What a requester might PUT to take over their own ticket: every
    /// staff-controlled column set to something of their choosing.
    fn hostile_body() -> NewTicket {
        NewTicket {
            title: "Retitled by the requester".into(),
            workflow_state_id: 99,
            priority: TicketPriority::High,
            requester_uuid: Some(Uuid::from_u128(999)),
            assignee_uuid: Some(Uuid::from_u128(998)),
            category_id: Some(997),
            submitted_via: Some("forged".into()),
            guest_lookup_token: Some(Uuid::from_u128(996)),
            verification_state: Some("forged".into()),
            origin_channel_id: Some(995),
            triage_state: Some("forged".into()),
            due_date: None,
            start_date: None,
            recurrence_rule: Some("FREQ=HOURLY".into()),
            recurrence_template_id: Some(994),
            resolution_notes: Some("forged".into()),
            spam_suspected: true,
        }
    }

    #[test]
    fn a_requester_may_retitle_and_nothing_else() {
        let existing = existing_ticket();
        let out = hostile_body().redact_for(false, &existing);

        assert_eq!(
            out.title, "Retitled by the requester",
            "the one field a requester owns"
        );

        // Everything a requester could otherwise have seized. Listed one by one
        // rather than compared structurally, so a failure names the column.
        assert_eq!(
            out.workflow_state_id, existing.workflow_state_id,
            "cannot close or reopen"
        );
        assert_eq!(out.priority, existing.priority, "cannot re-prioritise");
        assert_eq!(
            out.requester_uuid, existing.requester_uuid,
            "cannot hand the ticket away"
        );
        assert_eq!(out.assignee_uuid, existing.assignee_uuid, "cannot assign");
        assert_eq!(
            out.category_id, existing.category_id,
            "cannot move category"
        );
        assert_eq!(out.submitted_via, existing.submitted_via);
        assert_eq!(
            out.guest_lookup_token, existing.guest_lookup_token,
            "cannot mint an unauthenticated lookup handle"
        );
        assert_eq!(out.verification_state, existing.verification_state);
        assert_eq!(out.origin_channel_id, existing.origin_channel_id);
        assert_eq!(
            out.triage_state, existing.triage_state,
            "cannot self-triage"
        );
        assert_eq!(out.due_date, existing.due_date);
        assert_eq!(out.start_date, existing.start_date);
        assert_eq!(out.recurrence_rule, existing.recurrence_rule);
        assert_eq!(out.recurrence_template_id, existing.recurrence_template_id);
        assert_eq!(out.resolution_notes, existing.resolution_notes);
        assert_eq!(
            out.spam_suspected, existing.spam_suspected,
            "cannot clear a spam flag"
        );
    }

    #[test]
    fn staff_are_unaffected() {
        let existing = existing_ticket();
        let submitted = hostile_body();
        let expected = hostile_body();
        let out = submitted.redact_for(true, &existing);

        // Staff get exactly what they sent; this is the ordinary edit path and
        // redaction must not narrow it.
        assert_eq!(out.title, expected.title);
        assert_eq!(out.workflow_state_id, expected.workflow_state_id);
        assert_eq!(out.assignee_uuid, expected.assignee_uuid);
        assert_eq!(out.requester_uuid, expected.requester_uuid);
        assert_eq!(out.priority, expected.priority);
        assert_eq!(out.spam_suspected, expected.spam_suspected);
        assert_eq!(out.guest_lookup_token, expected.guest_lookup_token);
    }

    /// A requester resubmitting the row they were shown must not be refused;
    /// redaction has to be idempotent on an unchanged body, or the endpoint
    /// silently rejects legitimate retitles.
    #[test]
    fn resubmitting_the_current_values_changes_nothing() {
        let existing = existing_ticket();
        let faithful = NewTicket {
            title: existing.title.clone(),
            workflow_state_id: existing.workflow_state_id,
            priority: existing.priority,
            requester_uuid: existing.requester_uuid,
            assignee_uuid: existing.assignee_uuid,
            category_id: existing.category_id,
            submitted_via: existing.submitted_via.clone(),
            guest_lookup_token: existing.guest_lookup_token,
            verification_state: existing.verification_state.clone(),
            origin_channel_id: existing.origin_channel_id,
            triage_state: existing.triage_state.clone(),
            due_date: existing.due_date,
            start_date: existing.start_date,
            recurrence_rule: existing.recurrence_rule.clone(),
            recurrence_template_id: existing.recurrence_template_id,
            resolution_notes: existing.resolution_notes.clone(),
            spam_suspected: existing.spam_suspected,
        };
        let out = faithful.redact_for(false, &existing);
        assert_eq!(out.title, existing.title);
        assert_eq!(out.workflow_state_id, existing.workflow_state_id);
        assert_eq!(out.category_id, existing.category_id);
    }
}
