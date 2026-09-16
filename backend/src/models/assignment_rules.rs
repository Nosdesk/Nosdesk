use super::categories::TicketCategory;
use super::groups::Group;
use super::tickets::Ticket;
use super::users::UserInfoWithAvatar;
use chrono::NaiveDateTime;
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

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
