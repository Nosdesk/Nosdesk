ALTER TABLE documentation_pages DROP CONSTRAINT IF EXISTS documentation_pages_slug_unique;
ALTER TABLE documentation_pages ALTER COLUMN slug DROP NOT NULL;
