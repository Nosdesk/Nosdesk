-- The page a gap is being written as while it is `drafting`. Publishing that
-- page resolves the gap; deleting it sends the gap back to `open`. Hard-deleting
-- the page (workspace purge) just clears the pointer.
ALTER TABLE knowledge_gaps
    ADD COLUMN draft_page_id INTEGER REFERENCES documentation_pages(id) ON DELETE SET NULL;

CREATE INDEX idx_knowledge_gaps_draft_page_id
    ON knowledge_gaps (draft_page_id)
    WHERE draft_page_id IS NOT NULL;
