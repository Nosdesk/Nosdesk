//! Assignment Rules Repository
//!
//! CRUD operations and queries for assignment rules, state, and logs.

use chrono::Utc;
use diesel::prelude::*;
use diesel::result::Error;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// ============================================================================
// Assignment Rules CRUD
// ============================================================================

/// Get all assignment rules
pub fn get_all_rules(conn: &mut DbConnection) -> QueryResult<Vec<AssignmentRule>> {
    assignment_rules::table
        .order(assignment_rules::priority.asc())
        .load(conn)
}

/// Get active rules ordered by priority
#[allow(dead_code)]
pub fn get_active_rules_by_priority(conn: &mut DbConnection) -> QueryResult<Vec<AssignmentRule>> {
    assignment_rules::table
        .filter(assignment_rules::is_active.eq(true))
        .order(assignment_rules::priority.asc())
        .load(conn)
}

/// Get a rule by ID
pub fn get_rule_by_id(conn: &mut DbConnection, rule_id: i32) -> QueryResult<AssignmentRule> {
    assignment_rules::table.find(rule_id).first(conn)
}

/// Get a rule with all related details (user, group, category, state)
pub fn get_rule_with_details(conn: &mut DbConnection, rule_id: i32) -> Result<AssignmentRuleWithDetails, Error> {
    let rule = assignment_rules::table.find(rule_id).first::<AssignmentRule>(conn)?;

    // Get target user if set
    let target_user = if let Some(user_uuid) = rule.target_user_uuid {
        crate::repository::get_user_by_uuid(&user_uuid, conn)
            .ok()
            .map(UserInfoWithAvatar::from)
    } else {
        None
    };

    // Get target group if set
    let target_group = if let Some(group_id) = rule.target_group_id {
        crate::repository::groups::get_group_by_id(conn, group_id).ok()
    } else {
        None
    };

    // Get category if set
    let category = if let Some(category_id) = rule.category_id {
        crate::repository::categories::get_category_by_id(conn, category_id).ok()
    } else {
        None
    };

    // Get state
    let state = get_rule_state(conn, rule_id).ok();

    Ok(AssignmentRuleWithDetails {
        rule,
        target_user,
        target_group,
        category,
        state,
    })
}

/// Get all rules with details
pub fn get_all_rules_with_details(conn: &mut DbConnection) -> Result<Vec<AssignmentRuleWithDetails>, Error> {
    let rules = get_all_rules(conn)?;
    let mut result = Vec::new();

    for rule in rules {
        let details = get_rule_with_details(conn, rule.id)?;
        result.push(details);
    }

    Ok(result)
}

/// Create a new assignment rule
pub fn create_rule(conn: &mut DbConnection, new_rule: NewAssignmentRule) -> QueryResult<AssignmentRule> {
    diesel::insert_into(assignment_rules::table)
        .values(&new_rule)
        .get_result(conn)
}

/// Update an assignment rule
pub fn update_rule(
    conn: &mut DbConnection,
    rule_id: i32,
    mut rule_update: AssignmentRuleUpdate,
) -> QueryResult<AssignmentRule> {
    // Set updated_at if not provided
    if rule_update.updated_at.is_none() {
        rule_update.updated_at = Some(Utc::now().naive_utc());
    }

    diesel::update(assignment_rules::table.find(rule_id))
        .set(&rule_update)
        .get_result(conn)
}

/// Delete an assignment rule
pub fn delete_rule(conn: &mut DbConnection, rule_id: i32) -> QueryResult<usize> {
    diesel::delete(assignment_rules::table.find(rule_id)).execute(conn)
}

/// Reorder rules by setting their priorities
pub fn reorder_rules(conn: &mut DbConnection, orders: Vec<(i32, i32)>) -> Result<(), Error> {
    for (rule_id, new_priority) in orders {
        diesel::update(assignment_rules::table.find(rule_id))
            .set((
                assignment_rules::priority.eq(new_priority),
                assignment_rules::updated_at.eq(Utc::now().naive_utc()),
            ))
            .execute(conn)?;
    }
    Ok(())
}

// ============================================================================
// Rule State Operations
// ============================================================================

/// Get rule state
pub fn get_rule_state(conn: &mut DbConnection, rule_id: i32) -> QueryResult<AssignmentRuleState> {
    assignment_rule_state::table.find(rule_id).first(conn)
}

/// Get or create rule state
#[allow(dead_code)]
pub fn get_or_create_state(conn: &mut DbConnection, rule_id: i32) -> QueryResult<AssignmentRuleState> {
    // Try to get existing
    if let Ok(state) = get_rule_state(conn, rule_id) {
        return Ok(state);
    }

    // Create new
    let new_state = NewAssignmentRuleState {
        rule_id,
        last_assigned_index: 0,
        total_assignments: 0,
    };

    diesel::insert_into(assignment_rule_state::table)
        .values(&new_state)
        .get_result(conn)
}

/// Update rule state
#[allow(dead_code)]
pub fn update_state(
    conn: &mut DbConnection,
    rule_id: i32,
    update: AssignmentRuleStateUpdate,
) -> QueryResult<AssignmentRuleState> {
    diesel::update(assignment_rule_state::table.find(rule_id))
        .set(&update)
        .get_result(conn)
}

// ============================================================================
// Assignment Log Operations
// ============================================================================

/// Log an assignment
#[allow(dead_code)]
pub fn log_assignment(conn: &mut DbConnection, new_log: NewAssignmentLog) -> QueryResult<AssignmentLog> {
    diesel::insert_into(assignment_log::table)
        .values(&new_log)
        .get_result(conn)
}

/// Get recent assignment logs (for monitoring/debugging)
pub fn get_recent_logs(conn: &mut DbConnection, limit: i64) -> QueryResult<Vec<AssignmentLog>> {
    assignment_log::table
        .order(assignment_log::assigned_at.desc())
        .limit(limit)
        .load(conn)
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Get the next available priority (for new rules)
pub fn get_next_priority(conn: &mut DbConnection) -> QueryResult<i32> {
    use diesel::dsl::max;

    let max_priority: Option<i32> = assignment_rules::table
        .select(max(assignment_rules::priority))
        .first(conn)?;

    Ok(max_priority.unwrap_or(0) + 10)
}

/// Check if a rule name already exists
pub fn rule_name_exists(conn: &mut DbConnection, name: &str, exclude_id: Option<i32>) -> QueryResult<bool> {
    let mut query = assignment_rules::table
        .filter(assignment_rules::name.eq(name))
        .into_boxed();

    if let Some(id) = exclude_id {
        query = query.filter(assignment_rules::id.ne(id));
    }

    let count: i64 = query.count().get_result(conn)?;
    Ok(count > 0)
}
