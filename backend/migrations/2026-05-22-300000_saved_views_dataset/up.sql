-- Extend saved_views beyond tickets. The table originally
-- stored only ticket-list views; adding a `dataset` column lets
-- the same table back saved views for assets, users, and any
-- future list view that wants the "save my filters + display
-- config" affordance. Existing rows are tickets by construction;
-- the DEFAULT covers the backfill.
--
-- Workspace and project scopes stay tickets-only because their
-- permission model (admin / project-member) is tied to ticket
-- access tiers. Asset and user saved views are private to the
-- creator. The handler enforces this; no DB-level check because
-- a future "asset workspace view" feature might want to drop
-- the constraint without a migration.

ALTER TABLE saved_views
    ADD COLUMN dataset VARCHAR(20) NOT NULL DEFAULT 'tickets';

ALTER TABLE saved_views ALTER COLUMN dataset DROP DEFAULT;

-- Private-scope lookup by (created_by, dataset) is the dominant
-- query for asset and user views. The existing (scope, scope_id)
-- index covers workspace/project ticket views; this one covers
-- the per-user private path.
CREATE INDEX idx_saved_views_user_dataset
    ON saved_views (created_by, dataset)
    WHERE scope = 'private';
