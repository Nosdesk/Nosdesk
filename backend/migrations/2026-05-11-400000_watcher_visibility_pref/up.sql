-- Per-watch preference for internal-note visibility.
--
-- Staff watchers occasionally want to follow a ticket for status
-- and public-reply context without being pinged for every working
-- note the assigned tech adds. Unwatching is too coarse (they
-- still want to know when the requester responds). This column
-- gives them a per-ticket toggle: leave it on (default) to keep
-- the current behaviour, flip it off to mute internal-note
-- notifications on this specific watch.
--
-- The dispatcher checks the column when comment.is_internal is
-- true and drops opted-out watchers from the CommentAdded fan-out.
-- Mentions (@user) still notify because they're explicit pings,
-- not implicit fan-out.

ALTER TABLE ticket_watchers
    ADD COLUMN notify_on_internal_notes BOOLEAN NOT NULL DEFAULT TRUE;
