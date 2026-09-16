use chrono::{DateTime, Utc};
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

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
