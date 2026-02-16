CREATE TABLE documentation_starred_pages (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    page_id INT NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_uuid, page_id)
);
CREATE INDEX idx_doc_starred_user ON documentation_starred_pages(user_uuid);
CREATE INDEX idx_doc_starred_page ON documentation_starred_pages(page_id);
