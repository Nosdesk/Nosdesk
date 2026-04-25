ALTER TABLE plugins
    ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT TRUE;

UPDATE plugins SET enabled = (state = 'installed');

DROP INDEX IF EXISTS idx_plugins_state;

ALTER TABLE plugins
    DROP CONSTRAINT IF EXISTS plugins_state_check;

ALTER TABLE plugins
    DROP COLUMN state;
