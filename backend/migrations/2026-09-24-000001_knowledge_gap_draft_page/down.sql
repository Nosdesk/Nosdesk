DROP INDEX IF EXISTS idx_knowledge_gaps_draft_page_id;
ALTER TABLE knowledge_gaps DROP COLUMN IF EXISTS draft_page_id;
