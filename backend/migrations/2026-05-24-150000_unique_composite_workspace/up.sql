-- Phase 3h.8 — close UNIQUE-constraint cross-tenant existence leaks
-- on tenant tables whose user-facing identifier columns
-- (slug / name) were UNIQUE globally rather than per-workspace.
--
-- Threat shape (Bytebase RLS footgun #8):
--   BEGIN; SET ROLE nosdesk_app;
--   SELECT set_config('app.workspace_id','1',true);
--   INSERT INTO tags (name, workspace_id) VALUES ('leakprobe', 1);
--   SELECT set_config('app.workspace_id','2',true);
--   INSERT INTO tags (name, workspace_id) VALUES ('leakprobe', 2);
--   -> ERROR: duplicate key value violates unique constraint "tags_name_unique"
--
-- ws2 user learns from the error message that ws1 has a tag named
-- 'leakprobe' — RLS doesn't catch this because UNIQUE is enforced
-- before the policy filter runs (and the error text is the side
-- channel anyway).
--
-- Fix: replace the global-UNIQUE constraints with composite
-- (workspace_id, <col>) constraints on the columns that are
-- conceptually per-workspace identifiers. Six constraints
-- across six tables.
--
-- Audit decisions for other UNIQUE constraints (kept as-is):
--   * Anywhere `uuid` is UNIQUE — UUIDs are globally unique by
--     construction; no leak possible.
--   * Anywhere a UNIQUE includes an FK to a workspace-scoped
--     parent (e.g. channel_messages on channel_id, plugin_data
--     on plugin_id, documentation_revisions on page_id) — the
--     parent's FK already scopes the constraint per-workspace.
--   * `tickets.guest_lookup_token` — opaque UUID by design.
--   * `users.email`, `user_emails.email` — global identity by
--     product decision (one email -> one user across all
--     workspaces); not changing here.
--   * `notification_preferences.(user_uuid, type_id, channel)` —
--     a user might have prefs across multiple workspaces; this
--     should arguably include workspace_id too, but it's a
--     correctness concern beyond the leak and lives in a
--     separate follow-up.

-- 1. asset_kinds — per-workspace slug ("laptop", "desktop", ...)
ALTER TABLE asset_kinds DROP CONSTRAINT asset_kinds_slug_key;
ALTER TABLE asset_kinds ADD CONSTRAINT asset_kinds_workspace_slug_key
    UNIQUE (workspace_id, slug);

-- 2. documentation_collections — per-workspace URL slug
ALTER TABLE documentation_collections DROP CONSTRAINT documentation_collections_slug_key;
ALTER TABLE documentation_collections ADD CONSTRAINT documentation_collections_workspace_slug_key
    UNIQUE (workspace_id, slug);

-- 3. documentation_pages — per-workspace URL slug
ALTER TABLE documentation_pages DROP CONSTRAINT documentation_pages_slug_key;
ALTER TABLE documentation_pages ADD CONSTRAINT documentation_pages_workspace_slug_key
    UNIQUE (workspace_id, slug);

-- 4. plugins — per-workspace plugin install name. Two tenants
--    can have the same plugin name installed without collision.
ALTER TABLE plugins DROP CONSTRAINT plugins_name_key;
ALTER TABLE plugins ADD CONSTRAINT plugins_workspace_name_key
    UNIQUE (workspace_id, name);

-- 5. tags — per-workspace label
ALTER TABLE tags DROP CONSTRAINT tags_name_unique;
ALTER TABLE tags ADD CONSTRAINT tags_workspace_name_unique
    UNIQUE (workspace_id, name);

-- 6. ticket_categories — per-workspace category label.
--    The seeding upsert in repository::categories::seed_defaults
--    uses ON CONFLICT (name); the corresponding handler change
--    ships in the same commit.
ALTER TABLE ticket_categories DROP CONSTRAINT ticket_categories_name_unique;
ALTER TABLE ticket_categories ADD CONSTRAINT ticket_categories_workspace_name_unique
    UNIQUE (workspace_id, name);
