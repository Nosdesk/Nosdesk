-- Replace the `enabled` boolean with a `state` enum that captures
-- the full plugin lifecycle: installed (active), disabled (admin
-- paused), quarantined (signature/publisher revoked), and
-- uninstalled (manifest declared on_uninstall = preserve, so the
-- row remains as a data anchor for future reinstall but the
-- plugin is otherwise gone).
ALTER TABLE plugins
    ADD COLUMN state VARCHAR(32) NOT NULL DEFAULT 'installed';

-- Backfill: enabled=true -> 'installed', enabled=false -> 'disabled'.
UPDATE plugins
SET state = CASE WHEN enabled THEN 'installed' ELSE 'disabled' END;

ALTER TABLE plugins
    ADD CONSTRAINT plugins_state_check
        CHECK (state IN ('installed', 'disabled', 'quarantined', 'uninstalled'));

ALTER TABLE plugins
    DROP COLUMN enabled;

CREATE INDEX idx_plugins_state ON plugins(state);
