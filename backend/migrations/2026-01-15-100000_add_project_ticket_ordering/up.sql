-- Add display_order column for custom ticket ordering within project kanban columns
ALTER TABLE project_tickets ADD COLUMN display_order INTEGER NOT NULL DEFAULT 0;

-- Create index for efficient ordering queries
CREATE INDEX idx_project_tickets_order ON project_tickets (project_id, display_order);
