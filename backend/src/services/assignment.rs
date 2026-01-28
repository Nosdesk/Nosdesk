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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    /// Build a minimal `AssignmentRule` for testing pure logic functions.
    fn make_rule(overrides: impl FnOnce(&mut AssignmentRule)) -> AssignmentRule {
        let now = Utc::now().naive_utc();
        let mut rule = AssignmentRule {
            id: 1,
            uuid: Uuid::new_v4(),
            name: "test-rule".into(),
            description: None,
            priority: 10,
            is_active: true,
            method: AssignmentMethod::DirectUser,
            target_user_uuid: None,
            target_group_id: None,
            trigger_on_create: false,
            trigger_on_category_change: false,
            category_id: None,
            conditions: None,
            created_at: now,
            updated_at: now,
            created_by: None,
        };
        overrides(&mut rule);
        rule
    }

    /// Build a minimal `Ticket` for testing pure logic functions.
    fn make_ticket(overrides: impl FnOnce(&mut Ticket)) -> Ticket {
        let now = Utc::now().naive_utc();
        let mut ticket = Ticket {
            id: 1,
            title: "Test ticket".into(),
            status: TicketStatus::Open,
            priority: TicketPriority::Medium,
            requester_uuid: None,
            assignee_uuid: None,
            created_at: now,
            updated_at: now,
            created_by: None,
            closed_at: None,
            closed_by: None,
            category_id: None,
        };
        overrides(&mut ticket);
        ticket
    }

    // ── matches_trigger ──────────────────────────────────────────────

    #[test]
    fn trigger_on_create_matches_ticket_created() {
        let rule = make_rule(|r| r.trigger_on_create = true);
        assert!(AssignmentEngine::matches_trigger(&rule, &AssignmentTrigger::TicketCreated));
    }

    #[test]
    fn trigger_on_create_false_does_not_match_ticket_created() {
        let rule = make_rule(|r| r.trigger_on_create = false);
        assert!(!AssignmentEngine::matches_trigger(&rule, &AssignmentTrigger::TicketCreated));
    }

    #[test]
    fn trigger_on_category_change_matches() {
        let rule = make_rule(|r| r.trigger_on_category_change = true);
        assert!(AssignmentEngine::matches_trigger(&rule, &AssignmentTrigger::CategoryChanged));
    }

    #[test]
    fn trigger_on_category_change_false_does_not_match() {
        let rule = make_rule(|r| r.trigger_on_category_change = false);
        assert!(!AssignmentEngine::matches_trigger(&rule, &AssignmentTrigger::CategoryChanged));
    }

    // ── matches_category ─────────────────────────────────────────────

    #[test]
    fn no_category_filter_matches_any_ticket() {
        let rule = make_rule(|r| r.category_id = None);
        assert!(AssignmentEngine::matches_category(&rule, Some(42)));
        assert!(AssignmentEngine::matches_category(&rule, None));
    }

    #[test]
    fn category_filter_matches_same_category() {
        let rule = make_rule(|r| r.category_id = Some(1));
        assert!(AssignmentEngine::matches_category(&rule, Some(1)));
    }

    #[test]
    fn category_filter_does_not_match_different_category() {
        let rule = make_rule(|r| r.category_id = Some(1));
        assert!(!AssignmentEngine::matches_category(&rule, Some(2)));
    }

    #[test]
    fn category_filter_does_not_match_none() {
        let rule = make_rule(|r| r.category_id = Some(1));
        assert!(!AssignmentEngine::matches_category(&rule, None));
    }

    // ── evaluate_conditions ──────────────────────────────────────────

    #[test]
    fn no_conditions_always_matches() {
        let rule = make_rule(|r| r.conditions = None);
        let ticket = make_ticket(|_| {});
        assert!(AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn empty_object_conditions_matches() {
        let rule = make_rule(|r| r.conditions = Some(json!({})));
        let ticket = make_ticket(|_| {});
        assert!(AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn priority_condition_matches() {
        let rule = make_rule(|r| r.conditions = Some(json!({"priority": "high"})));
        let ticket = make_ticket(|t| t.priority = TicketPriority::High);
        assert!(AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn priority_condition_mismatches() {
        let rule = make_rule(|r| r.conditions = Some(json!({"priority": "high"})));
        let ticket = make_ticket(|t| t.priority = TicketPriority::Low);
        assert!(!AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn status_condition_matches() {
        let rule = make_rule(|r| r.conditions = Some(json!({"status": "in-progress"})));
        let ticket = make_ticket(|t| t.status = TicketStatus::InProgress);
        assert!(AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn status_condition_mismatches() {
        let rule = make_rule(|r| r.conditions = Some(json!({"status": "closed"})));
        let ticket = make_ticket(|t| t.status = TicketStatus::Open);
        assert!(!AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn title_contains_condition_matches() {
        let rule = make_rule(|r| r.conditions = Some(json!({"title_contains": "urgent"})));
        let ticket = make_ticket(|t| t.title = "URGENT: server down".into());
        assert!(AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn title_contains_condition_mismatches() {
        let rule = make_rule(|r| r.conditions = Some(json!({"title_contains": "urgent"})));
        let ticket = make_ticket(|t| t.title = "Routine maintenance".into());
        assert!(!AssignmentEngine::evaluate_conditions(&rule, &ticket));
    }

    #[test]
    fn multiple_conditions_all_must_match() {
        let rule = make_rule(|r| {
            r.conditions = Some(json!({
                "priority": "high",
                "status": "open",
                "title_contains": "fire"
            }));
        });

        // All match
        let ticket = make_ticket(|t| {
            t.priority = TicketPriority::High;
            t.status = TicketStatus::Open;
            t.title = "The server is on fire".into();
        });
        assert!(AssignmentEngine::evaluate_conditions(&rule, &ticket));

        // One doesn't match
        let ticket2 = make_ticket(|t| {
            t.priority = TicketPriority::Low; // mismatch
            t.status = TicketStatus::Open;
            t.title = "The server is on fire".into();
        });
        assert!(!AssignmentEngine::evaluate_conditions(&rule, &ticket2));
    }
}
