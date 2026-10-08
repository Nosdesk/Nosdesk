-- Merge notes are internal: they name every merged ticket, whoever asked for
-- it. Notes written before that are public; make them internal, and their
-- comment rows in sync_actions with them, so the portal, the activity feed and
-- a sync replay all leave them out for a requester. comments has no audit
-- trigger (set_updated_at bumps updated_at); sync_actions has insert triggers
-- only, and occurred_at is untouched, so no row changes partition.
UPDATE comments
SET is_internal = true
WHERE channel_metadata->>'kind' = 'merge_marker'
  AND NOT is_internal;

UPDATE sync_actions s
SET data = jsonb_set(s.data, '{is_internal}', 'true'::jsonb)
WHERE s.aggregate = 'comment'
  AND s.data->>'is_internal' = 'false'
  AND s.aggregate_id IN (
      SELECT c.id::text FROM comments c
      WHERE c.channel_metadata->>'kind' = 'merge_marker'
  );

-- The agent's merge reason is for the team. ticket.merged no longer carries
-- it (merge history reads ticket_merges); take it off the rows already sent.
UPDATE sync_actions
SET data = data - 'reason'
WHERE event_type = 'ticket.merged'
  AND data ? 'reason';
