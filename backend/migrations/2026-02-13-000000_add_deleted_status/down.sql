DROP INDEX IF EXISTS idx_documentation_pages_deleted_at;
DROP INDEX IF EXISTS idx_documentation_pages_status;
ALTER TABLE documentation_pages DROP COLUMN IF EXISTS deleted_at;
-- PostgreSQL cannot remove enum values; 'deleted' remains but is harmless
