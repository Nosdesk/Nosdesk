DROP INDEX IF EXISTS tickets_due_date_idx;
ALTER TABLE tickets DROP COLUMN IF EXISTS due_date;
