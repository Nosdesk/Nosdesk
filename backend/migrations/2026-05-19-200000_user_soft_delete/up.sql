-- Soft-delete users with a restore window before purge.
--
-- The single `delete_user` flow used to hard-delete in a
-- destructive cascade: it deleted the user's comments outright
-- and NULLed every ticket FK that pointed at them. Once the
-- transaction committed there was no recovery. Worse, the batch
-- delete on the user list view skipped the per-action MFA check
-- the single delete required, so the "safer" UX was actually the
-- less protected one.
--
-- Industry pattern (Salesforce: no hard delete at all; Google:
-- 20-day restore window; Atlassian: deactivate-first) is to mark
-- the row as deleted, hide it from active surfaces, and let a
-- retention worker purge after a configurable grace window. That
-- gives:
--
--   * Reversible delete (admin can restore from a "Deleted users"
--     view during the window).
--   * Comment / ticket history stays intact during the window;
--     the eventual purge is still destructive but predictable.
--   * Batch and single delete share the same primitive, so the
--     UX inconsistency goes away without needing to gate batch
--     deletes with MFA.
--
-- `deleted_at` defaults to NULL, so every existing user row is
-- "active" by definition. No backfill required.
ALTER TABLE users
  ADD COLUMN deleted_at TIMESTAMPTZ NULL;

-- Partial index on the small subset of rows that are soft-deleted.
-- The retention worker's daily query reads "rows with deleted_at
-- older than N days"; without this index it would scan the whole
-- users table. Partial WHERE keeps the index small (only the
-- pending-purge subset) so it pays for itself even at low write
-- volume to the soft-delete column.
CREATE INDEX idx_users_deleted_at_pending
    ON users (deleted_at)
    WHERE deleted_at IS NOT NULL;
