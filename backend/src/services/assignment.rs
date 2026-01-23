//! Assignment Engine Service
//!
//! Handles automatic ticket assignment based on configurable rules.
//! Supports multiple assignment methods: direct user, round-robin, random, and group queue.

use chrono::Utc;
use diesel::prelude::*;
use rand::seq::SliceRandom;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

/// Assignment Engine for automatic ticket routing
pub struct AssignmentEngine;

impl AssignmentEngine {
    /// Evaluate all active rules for a ticket and return the first matching assignment
    ///
    /// Rules are evaluated in priority order (lower priority number = higher priority).
    /// The first matching rule wins.
    pub fn evaluate_rules(
        conn: &mut DbConnection,
        ticket: &Ticket,
        trigger: AssignmentTrigger,
    ) -> Option<AssignmentResult> {
        // Get active rules ordered by priority
        let rules = match Self::get_active_rules_by_priority(conn) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to get assignment rules: {e:?}");
                return None;
            }
        };

        for rule in rules {
            // Check if rule applies to this trigger
            if !Self::matches_trigger(&rule, &trigger) {
                continue;
            }

            // Check if rule applies to this category
            if !Self::matches_category(&rule, ticket.category_id) {
                continue;
            }

            // Check extended conditions (JSON-based)
            if !Self::evaluate_conditions(&rule, ticket) {
                continue;
            }

            // Execute the assignment strategy
            if let Some(assigned_user) = Self::execute_strategy(conn, &rule) {
                // Log the assignment
                let _ = Self::log_assignment(
                    conn,
                    ticket.id,
                    &rule,
                    &trigger,
                    ticket.assignee_uuid,
                    assigned_user,
                );

                return Some(AssignmentResult {
                    rule_id: rule.id,
                    rule_name: rule.name.clone(),
                    assigned_user_uuid: assigned_user,
                    method: rule.method,
                });
            }
        }

        None
    }

    /// Check if the rule applies to the given trigger type
    fn matches_trigger(rule: &AssignmentRule, trigger: &AssignmentTrigger) -> bool {
        match trigger {
            AssignmentTrigger::TicketCreated => rule.trigger_on_create,
            AssignmentTrigger::CategoryChanged => rule.trigger_on_category_change,
        }
    }

    /// Check if the rule applies to the ticket's category
    fn matches_category(rule: &AssignmentRule, ticket_category_id: Option<i32>) -> bool {
        match rule.category_id {
            // Rule has no category filter - applies to all
            None => true,
            // Rule has category filter - must match
            Some(rule_cat_id) => ticket_category_id == Some(rule_cat_id),
        }
    }

    /// Evaluate extended JSON conditions
    ///
    /// Currently supports:
    /// - priority: "low" | "medium" | "high"
    /// - status: "open" | "in-progress" | "closed"
    /// - title_contains: "string to match"
    ///
    /// All conditions must match (AND logic).
    fn evaluate_conditions(rule: &AssignmentRule, ticket: &Ticket) -> bool {
        let conditions = match &rule.conditions {
            Some(c) if !c.is_null() && c.as_object().is_some_and(|o| !o.is_empty()) => c,
            _ => return true, // No conditions = always match
        };

        let obj = match conditions.as_object() {
            Some(o) => o,
            None => return true,
        };

        // Check priority condition
        if let Some(priority_val) = obj.get("priority") {
            if let Some(priority_str) = priority_val.as_str() {
                let ticket_priority = format!("{:?}", ticket.priority).to_lowercase();
                if ticket_priority != priority_str {
                    return false;
                }
            }
        }

        // Check status condition
        if let Some(status_val) = obj.get("status") {
            if let Some(status_str) = status_val.as_str() {
                let ticket_status = match ticket.status {
                    TicketStatus::Open => "open",
                    TicketStatus::InProgress => "in-progress",
                    TicketStatus::Closed => "closed",
                };
                if ticket_status != status_str {
                    return false;
                }
            }
        }

        // Check title_contains condition
        if let Some(title_val) = obj.get("title_contains") {
            if let Some(search_str) = title_val.as_str() {
                if !ticket.title.to_lowercase().contains(&search_str.to_lowercase()) {
                    return false;
                }
            }
        }

        true
    }

    /// Execute the assignment strategy and return the assigned user UUID
    fn execute_strategy(conn: &mut DbConnection, rule: &AssignmentRule) -> Option<Option<Uuid>> {
        match rule.method {
            AssignmentMethod::DirectUser => {
                // Assign to the specific user
                Some(rule.target_user_uuid)
            }
            AssignmentMethod::GroupRoundRobin => {
                Self::round_robin_assignment(conn, rule)
            }
            AssignmentMethod::GroupRandom => {
                Self::random_assignment(conn, rule)
            }
            AssignmentMethod::GroupQueue => {
                // Queue assignment: no specific user, just mark for the group
                // Return None to indicate "assign to group" (no specific user)
                Some(None)
            }
        }
    }

    /// Round-robin assignment from group members
    fn round_robin_assignment(conn: &mut DbConnection, rule: &AssignmentRule) -> Option<Option<Uuid>> {
        let group_id = rule.target_group_id?;

        // Get group members ordered consistently
        let members = match crate::repository::groups::get_users_in_group(conn, group_id) {
            Ok(m) if !m.is_empty() => m,
            Ok(_) => {
                log::warn!("Group {group_id} has no members for round-robin");
                return None;
            }
            Err(e) => {
                log::error!("Failed to get group members: {e:?}");
                return None;
            }
        };

        // Get or create state for this rule
        let state = Self::get_or_create_state(conn, rule.id);
        let current_index = state.map(|s| s.last_assigned_index).unwrap_or(0);

        // Calculate next index
        let next_index = (current_index + 1) % (members.len() as i32);
        let selected_user = &members[next_index as usize];

        // Update state
        let _ = Self::update_state(conn, rule.id, next_index, Some(selected_user.uuid));

        Some(Some(selected_user.uuid))
    }

    /// Random assignment from group members
    fn random_assignment(conn: &mut DbConnection, rule: &AssignmentRule) -> Option<Option<Uuid>> {
        let group_id = rule.target_group_id?;

        // Get group members
        let members = match crate::repository::groups::get_users_in_group(conn, group_id) {
            Ok(m) if !m.is_empty() => m,
            Ok(_) => {
                log::warn!("Group {group_id} has no members for random assignment");
                return None;
            }
            Err(e) => {
                log::error!("Failed to get group members: {e:?}");
                return None;
            }
        };

        // Select random member
        let mut rng = rand::thread_rng();
        let selected_user = members.choose(&mut rng)?;

        // Update state for tracking
        let _ = Self::update_state(conn, rule.id, 0, Some(selected_user.uuid));

        Some(Some(selected_user.uuid))
    }

    /// Get active rules ordered by priority (lower number = higher priority)
    fn get_active_rules_by_priority(conn: &mut DbConnection) -> diesel::QueryResult<Vec<AssignmentRule>> {
        assignment_rules::table
            .filter(assignment_rules::is_active.eq(true))
            .order(assignment_rules::priority.asc())
            .load(conn)
    }

    /// Get or create state for a rule
    fn get_or_create_state(conn: &mut DbConnection, rule_id: i32) -> Option<AssignmentRuleState> {
        // Try to get existing state
        let existing = assignment_rule_state::table
            .find(rule_id)
            .first::<AssignmentRuleState>(conn);

        if let Ok(state) = existing {
            return Some(state);
        }

        // Create new state
        let new_state = NewAssignmentRuleState {
            rule_id,
            last_assigned_index: 0,
            total_assignments: 0,
        };

        diesel::insert_into(assignment_rule_state::table)
            .values(&new_state)
            .get_result(conn)
            .ok()
    }

    /// Update the state after an assignment
    fn update_state(
        conn: &mut DbConnection,
        rule_id: i32,
        new_index: i32,
        assigned_user: Option<Uuid>,
    ) -> diesel::QueryResult<AssignmentRuleState> {
        // Ensure state exists
        Self::get_or_create_state(conn, rule_id);

        diesel::update(assignment_rule_state::table.find(rule_id))
            .set((
                assignment_rule_state::last_assigned_index.eq(new_index),
                assignment_rule_state::total_assignments.eq(assignment_rule_state::total_assignments + 1),
                assignment_rule_state::last_assigned_at.eq(Utc::now().naive_utc()),
                assignment_rule_state::last_assigned_user_uuid.eq(assigned_user),
            ))
            .get_result(conn)
    }

    /// Log an assignment for audit purposes
    fn log_assignment(
        conn: &mut DbConnection,
        ticket_id: i32,
        rule: &AssignmentRule,
        trigger: &AssignmentTrigger,
        previous_assignee: Option<Uuid>,
        new_assignee: Option<Uuid>,
    ) -> diesel::QueryResult<AssignmentLog> {
        let context = json!({
            "rule_name": rule.name,
            "rule_priority": rule.priority,
        });

        let new_log = NewAssignmentLog {
            ticket_id,
            rule_id: Some(rule.id),
            trigger_type: trigger.as_str().to_string(),
            previous_assignee_uuid: previous_assignee,
            new_assignee_uuid: new_assignee,
            method: rule.method,
            context: Some(context),
        };

        diesel::insert_into(assignment_log::table)
            .values(&new_log)
            .get_result(conn)
    }
}
