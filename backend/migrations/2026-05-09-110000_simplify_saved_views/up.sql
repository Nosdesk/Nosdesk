-- Simplify saved_views by dropping is_default and archived_at.
--
-- Why: both columns existed to support features that never grew a
-- user-facing surface. is_default introduced a second authoritative
-- "default" notion competing with the resolution-chain fallback in
-- useTicketsViewResolution, with no admin UI to manage it (and a
-- partial unique index whose maintenance cost outweighed the value).
-- archived_at was a soft-delete with no archived-views browser, no
-- restore flow, and no second consumer of archived rows. Both
-- removed in favour of: hard DELETE for "delete a view," and the
-- single built-in MY_OPEN_VIEW fallback for "what shows by default."
--
-- Indexes touched:
-- - `saved_views_default_per_scope` (partial unique on is_default) — dropped.
-- - `saved_views_scope_idx` (partial on archived_at IS NULL) — dropped and
--   recreated without the archived_at predicate.

DROP INDEX IF EXISTS saved_views_default_per_scope;
DROP INDEX IF EXISTS saved_views_scope_idx;

ALTER TABLE saved_views DROP COLUMN IF EXISTS is_default;
ALTER TABLE saved_views DROP COLUMN IF EXISTS archived_at;

CREATE INDEX saved_views_scope_idx
    ON saved_views (scope, scope_id);
