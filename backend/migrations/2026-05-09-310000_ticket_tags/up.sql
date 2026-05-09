-- Tags: free-form, multi-valued labels on tickets.
--
-- Sits alongside the fixed `category_id` (one per ticket) as a
-- flexible second axis. Categories are admin-curated and gate
-- workflow / SLA logic; tags are user-applied and serve discovery
-- (filtering, search, ad-hoc grouping). Every other modern
-- helpdesk ships both — Linear has Labels, Zendesk has Tags,
-- Help Scout has Tags, Jira SM has Components + Labels.
--
-- Workspace-scoped namespace. Single-workspace deployment today
-- so no `workspace_id` column; multi-tenant migration adds one
-- alongside the same migration that promotes other workspace-
-- scoped tables.

CREATE TABLE tags (
    id              SERIAL PRIMARY KEY,
    name            VARCHAR(64) NOT NULL,
    -- Display colour token. Same vocabulary the workflow_state
    -- picker uses (slate / gray / blue / purple / green / amber /
    -- rose / subtle) — keeps the chip rendering uniform across
    -- the app. NULL means "use the neutral default".
    color           VARCHAR(32),
    -- Free-text description shown in the tag manager + the picker
    -- tooltip. Optional; tags are usually self-evident.
    description     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at     TIMESTAMPTZ,
    CONSTRAINT tags_name_unique UNIQUE (name)
);

-- Ticket ↔ tag join. Composite primary key prevents the same tag
-- being assigned twice; ON DELETE CASCADE on both sides cleans up
-- when either parent disappears.
CREATE TABLE ticket_tags (
    ticket_id       INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    tag_id          INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    -- Who attached this tag, for the activity timeline. NULL for
    -- system-applied tags (e.g. assignment-rule actions in a
    -- future iteration).
    created_by      UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (ticket_id, tag_id)
);

-- Lookup index for "which tickets have tag X" — drives tag-based
-- list filters. The reverse direction (tag list for a ticket) is
-- already covered by the primary key.
CREATE INDEX ticket_tags_tag_idx ON ticket_tags (tag_id);
