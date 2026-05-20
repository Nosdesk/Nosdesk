-- Add a workspace-neutral default kind and seed it before
-- flipping the column default away from 'device'.
--
-- Background: Phase A seeded `kind = 'device'` as the column's
-- DB DEFAULT so every legacy /api/devices POST landed on the
-- IT-desk view unchanged. That was the right migration move
-- at the time, but it makes the workspace experience IT-
-- flavoured by default. For a workspace that uses Nosdesk for
-- non-IT inventory (the plumbing-business use case), the
-- neutral landing kind should be 'generic' instead.
--
-- This migration:
-- - Inserts the 'generic' builtin kind.
-- - Flips the DB DEFAULT to 'generic' so new inserts that omit
--   the column land neutrally.
-- - Leaves every existing row at its current kind. Rows
--   created before this migration are still 'device' (the old
--   default) because they came in through the IT-desk POST
--   path; rewriting them would change asset semantics, which
--   we don't want.

INSERT INTO asset_kinds (slug, label, description, icon, sort_order, is_builtin) VALUES
    ('generic', 'Generic asset', 'A workspace-neutral asset. Use for anything that does not fit a more specific kind.', 'asset', 5, TRUE);

ALTER TABLE assets ALTER COLUMN kind SET DEFAULT 'generic';
