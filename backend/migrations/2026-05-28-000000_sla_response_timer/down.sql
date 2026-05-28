DROP INDEX IF EXISTS tickets_first_response_at_idx;

ALTER TABLE tickets
    DROP COLUMN IF EXISTS first_response_at;
