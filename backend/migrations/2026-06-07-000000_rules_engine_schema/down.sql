-- Reverse of up.sql. RLS policies, ownership, and grants disappear
-- with the tables. We only need to drop the triggers/functions
-- explicitly since they aren't owned by the parent table.

DROP TRIGGER IF EXISTS rules_version_on_update ON rules;
DROP TRIGGER IF EXISTS rules_version_on_insert ON rules;
DROP FUNCTION IF EXISTS rules_write_update_version();
DROP FUNCTION IF EXISTS rules_write_initial_version();

DROP TABLE IF EXISTS ticket_rule_runs;
DROP TABLE IF EXISTS rule_applications;
DROP TABLE IF EXISTS rule_versions;
DROP TABLE IF EXISTS rules;

DROP TYPE IF EXISTS rule_application_status;
DROP TYPE IF EXISTS rule_trigger_kind;
DROP TYPE IF EXISTS rule_state;
