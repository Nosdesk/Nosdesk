DROP INDEX IF EXISTS tickets_recurrence_rule_idx;
ALTER TABLE tickets
    DROP COLUMN IF EXISTS recurrence_template_id,
    DROP COLUMN IF EXISTS recurrence_rule;
