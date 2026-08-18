-- Restore the columns as they were. Both were dead, so there is no data to
-- recover: is_current comes back defaulted and security_events.session_id
-- comes back NULL everywhere.
ALTER TABLE active_sessions
    ADD COLUMN is_current BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE security_events
    ADD COLUMN session_id INTEGER
    REFERENCES active_sessions(id) ON DELETE SET NULL;
