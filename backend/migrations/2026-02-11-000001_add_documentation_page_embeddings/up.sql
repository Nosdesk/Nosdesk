-- Tracks which documentation pages embed other pages
-- Used for: cache invalidation, cycle detection during export, "referenced by" display
CREATE TABLE documentation_page_embeddings (
    source_page_id INTEGER NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    target_page_id INTEGER NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (source_page_id, target_page_id)
);

CREATE INDEX idx_doc_embeddings_target ON documentation_page_embeddings(target_page_id);
