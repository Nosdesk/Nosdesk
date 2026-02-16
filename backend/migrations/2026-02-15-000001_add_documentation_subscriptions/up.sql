CREATE TABLE documentation_subscriptions (
    id SERIAL PRIMARY KEY,
    user_uuid UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    page_id INT NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_uuid, page_id)
);
CREATE INDEX idx_doc_subscriptions_user ON documentation_subscriptions(user_uuid);
CREATE INDEX idx_doc_subscriptions_page ON documentation_subscriptions(page_id);

INSERT INTO notification_types (code, name, description, category, default_channels)
VALUES ('doc_page_updated', 'Documentation Page Updated', 'When a documentation page you subscribe to is modified', 'documentation', '["in_app"]');
