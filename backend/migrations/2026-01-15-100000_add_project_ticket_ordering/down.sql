-- Remove display_order column and index
DROP INDEX IF EXISTS idx_project_tickets_order;
ALTER TABLE project_tickets DROP COLUMN display_order;
