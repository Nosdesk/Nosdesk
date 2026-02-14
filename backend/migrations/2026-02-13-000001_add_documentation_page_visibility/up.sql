CREATE TABLE documentation_page_visibility (
    page_id INT NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (page_id, group_id)
);

CREATE INDEX idx_page_visibility_page ON documentation_page_visibility(page_id);
CREATE INDEX idx_page_visibility_group ON documentation_page_visibility(group_id);
