ALTER TYPE documentation_status ADD VALUE IF NOT EXISTS 'deleted';
ALTER TABLE documentation_pages ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ DEFAULT NULL;
CREATE INDEX IF NOT EXISTS idx_documentation_pages_status ON documentation_pages(status);
CREATE INDEX IF NOT EXISTS idx_documentation_pages_deleted_at ON documentation_pages(deleted_at);
