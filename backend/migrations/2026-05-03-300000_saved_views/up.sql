-- Saved views: per-user / per-project / workspace-wide presets that
-- bundle a ViewShape + FilterState. The architecture doc § 9
-- specifies the shape; this is the v1 single-workspace cut, no
-- workspace_id column (the SOT for multi-workspace lives in the
-- deployment-model decision and lands as a non-destructive ALTER
-- when the second workspace ships).

CREATE TABLE saved_views (
    id          SERIAL PRIMARY KEY,
    uuid        UUID NOT NULL DEFAULT uuidv7() UNIQUE,
    -- 'workspace' | 'project' | 'private'
    scope       VARCHAR(20) NOT NULL,
    -- For scope='project': the project id as text.
    -- For scope='private': the user uuid as text.
    -- For scope='workspace': NULL.
    scope_id    TEXT,
    name        VARCHAR(120) NOT NULL,
    -- ViewShape JSON. Validated client-side; the server treats it
    -- as opaque (forward-compat for plugin-defined view types).
    shape       JSONB NOT NULL,
    -- FilterState JSON. Same opacity contract as `shape`.
    filter      JSONB NOT NULL,
    created_by  UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    is_default  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at TIMESTAMPTZ,
    CONSTRAINT saved_views_scope_check CHECK (scope IN ('workspace', 'project', 'private')),
    CONSTRAINT saved_views_scope_id_shape CHECK (
        (scope = 'workspace' AND scope_id IS NULL)
        OR (scope IN ('project', 'private') AND scope_id IS NOT NULL)
    )
);

-- Per-scope active set lookup — the most common query is "list
-- saved views I can see for this scope," which is a partial index
-- by scope + scope_id excluding archived rows.
CREATE INDEX saved_views_scope_idx
    ON saved_views (scope, scope_id) WHERE archived_at IS NULL;

-- Exactly one default per scope (so the route-default lookup is a
-- single-row read with no tie-breaking). Partial unique index lets
-- archived defaults coexist with a live default in the same scope.
CREATE UNIQUE INDEX saved_views_default_per_scope
    ON saved_views (scope, scope_id) WHERE is_default = TRUE AND archived_at IS NULL;
