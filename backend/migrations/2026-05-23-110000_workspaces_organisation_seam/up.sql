-- Organisation-of-workspaces seam.
--
-- Adds a nullable `organisation_id INTEGER` column to the
-- workspaces table. The column exists; the `organisations`
-- table does not. The seam preserves the architectural option
-- to introduce an org-as-parent-of-workspaces tier later (MSPs
-- managing many customer workspaces; enterprises grouping
-- divisional workspaces under one billing umbrella) without
-- re-migrating every existing workspace's identity at that
-- point.
--
-- No FK constraint yet because there's no table to reference.
-- When the org tier ships:
--   1. CREATE TABLE organisations (...)
--   2. CREATE TABLE organisation_members (...)
--   3. ALTER TABLE workspaces ADD CONSTRAINT
--      workspaces_organisation_id_fkey FOREIGN KEY
--      (organisation_id) REFERENCES organisations(id);
--   4. Decide subdomain routing semantics (D1 revisit:
--      org.nosdesk.com/{workspace}? or stay
--      {workspace}.nosdesk.com with org-as-billing-only?).
--
-- For now, NULL on every workspace means "independent
-- workspace, no parent org" — the only model the current
-- product surface supports.

ALTER TABLE workspaces
    ADD COLUMN IF NOT EXISTS organisation_id INTEGER;
