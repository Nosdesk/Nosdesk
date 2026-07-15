-- Notification preferences: per-cell delivery FREQUENCY, replacing the binary
-- `enabled` toggle.
--
--   instant  deliver immediately on this channel  (== legacy enabled = true)
--   digest   batch into a periodic summary (email only; the digest batcher
--            consumes these — a later step)
--   off      never deliver on this channel        (== legacy enabled = false)
--
-- Expand step of an expand/contract migration. `frequency` is added NULLABLE
-- with NO backfill UPDATE — mirroring 2026-07-11 notification_engagement_state,
-- which avoided a backfill precisely because notification_preferences carries
-- an audit trigger that needs `app.workspace_id`; a bulk UPDATE here would fire
-- it outside any workspace context and crash-loop. Instead the app coalesces:
-- effective = frequency, else (enabled ? 'instant' : 'off'). New writes set
-- BOTH columns (dual-write) so an old instance mid-rollout still reads a
-- consistent row. A later contract migration drops `enabled` + tightens NOT NULL.
ALTER TABLE notification_preferences
    ADD COLUMN frequency text
        CHECK (frequency IS NULL OR frequency IN ('instant', 'digest', 'off'));
