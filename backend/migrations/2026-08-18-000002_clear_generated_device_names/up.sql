-- Clear the device labels the old server-side user-agent parser wrote.
--
-- `parse_device_name` produced a fixed set of hardcoded English strings, two of
-- them wrong ("Android Asset", "Unknown Asset", from a Device/Asset rename
-- sweep). It has been removed: `device_name` is now only ever a name a native
-- client sent for itself, and the session list derives a translated label from
-- the user-agent when the column is NULL.
--
-- Without this, every session that predates the change keeps its generated
-- label until it expires, so users would still be reading "Unknown Asset" for
-- up to a week after deploy.
--
-- Matching the exact generated set is safe: before this change no client could
-- supply a name at all, so any row holding one of these values was written by
-- the parser. active_sessions is sync-audit-only, so there is no audit trigger
-- to disable.
UPDATE active_sessions
SET device_name = NULL
WHERE device_name IN (
    'iPhone', 'iPad', 'Android Asset', 'Mac', 'Windows PC', 'Linux', 'Unknown Asset'
);
