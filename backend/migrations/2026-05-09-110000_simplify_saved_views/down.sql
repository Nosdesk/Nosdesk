DROP INDEX IF EXISTS saved_views_scope_idx;

ALTER TABLE saved_views ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE saved_views ADD COLUMN archived_at TIMESTAMPTZ;

CREATE INDEX saved_views_scope_idx
    ON saved_views (scope, scope_id) WHERE archived_at IS NULL;

CREATE UNIQUE INDEX saved_views_default_per_scope
    ON saved_views (scope, scope_id) WHERE is_default = TRUE AND archived_at IS NULL;
