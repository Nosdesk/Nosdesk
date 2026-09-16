use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;

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
    /// A comment mentioning another ticket (`comment_ticket_references`).
    /// Composite key `referenced_ticket_id:comment_id`, grouped on the
    /// referenced ticket. Not pool-materialised: the referenced ticket's
    /// activity feed and the notification deriver read the event.
    #[serde(rename = "ticket_reference")]
    TicketReference,
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
    /// Device loan ledger row (`asset_loans`). A loan span: borrower,
    /// optional due-back, optional ticket. Op Insert on issue, Update on
    /// return or due-date edit.
    #[serde(rename = "asset_loan")]
    AssetLoan,
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
            Self::TicketReference => "ticket_reference",
            Self::AssetUsage => "asset_usage",
            Self::AssetAudit => "asset_audit",
            Self::AssetLoan => "asset_loan",
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
            b"ticket_reference" => Ok(Self::TicketReference),
            b"asset_usage" => Ok(Self::AssetUsage),
            b"asset_audit" => Ok(Self::AssetAudit),
            b"asset_loan" => Ok(Self::AssetLoan),
            other => {
                Err(format!("unknown sync_aggregate: {}", String::from_utf8_lossy(other)).into())
            }
        }
    }
}
