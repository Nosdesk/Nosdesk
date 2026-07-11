-- Engagement-state columns for notifications: mutually-inclusive
-- lifecycle axes beyond the binary is_read. All nullable/additive, so
-- there is no backfill (and thus no audit-trigger crash-loop hazard),
-- and existing rows read as unseen/active/never-snoozed.
--
--   seen_at       set when the recipient opens the panel/inbox; the
--                 badge counts UNSEEN (seen_at IS NULL), distinct from
--                 read (a glance clears the badge without marking every
--                 item read).
--   archived_at   reversible triage, independent of read; replaces the
--                 destructive hard-delete-on-dismiss.
--   snoozed_until hide until a time; auto-unsnooze on new entity
--                 activity (enforced in the app layer).
ALTER TABLE notifications
    ADD COLUMN seen_at timestamptz,
    ADD COLUMN archived_at timestamptz,
    ADD COLUMN snoozed_until timestamptz;

-- Badge query: unseen + active (not archived) per recipient. Partial
-- index keeps it cheap as the table grows, mirroring the existing
-- idx_notifications_user_unread shape (RLS supplies the workspace_id
-- predicate at query time).
CREATE INDEX idx_notifications_user_unseen
    ON notifications (user_uuid)
    WHERE seen_at IS NULL AND archived_at IS NULL;
