-- Drop indexes
DROP INDEX IF EXISTS idx_assignment_log_assigned_at;
DROP INDEX IF EXISTS idx_assignment_log_ticket;
DROP INDEX IF EXISTS idx_assignment_rules_category;
DROP INDEX IF EXISTS idx_assignment_rules_priority;

-- Drop tables in reverse order
DROP TABLE IF EXISTS assignment_log;
DROP TABLE IF EXISTS assignment_rule_state;
DROP TABLE IF EXISTS assignment_rules;

-- Drop the enum type
DROP TYPE IF EXISTS assignment_method;
