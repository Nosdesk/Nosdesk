ALTER TABLE tickets
    DROP CONSTRAINT IF EXISTS tickets_triage_state_check,
    DROP COLUMN IF EXISTS triage_state;
DROP INDEX IF EXISTS tickets_untriaged_idx;
DROP TABLE IF EXISTS cycle_tickets;
DROP TABLE IF EXISTS cycles;
