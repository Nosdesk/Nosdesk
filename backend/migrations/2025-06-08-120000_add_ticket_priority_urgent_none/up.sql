-- Extend ticket_priority so the sync layer can persist the UI's
-- five priority tiers (urgent / high / medium / low / none).
ALTER TYPE ticket_priority ADD VALUE IF NOT EXISTS 'urgent' BEFORE 'high';
ALTER TYPE ticket_priority ADD VALUE IF NOT EXISTS 'none' BEFORE 'low';
