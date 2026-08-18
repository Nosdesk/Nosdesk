-- Drop two columns nothing reads.
--
-- active_sessions.is_current was written `true` for every session at insert
-- and never read back. "Current" is a property of the request, not of the row:
-- the sessions endpoint compares each row's session_id against the caller's
-- `sid` claim, which is the only way to get it right when one user holds
-- several sessions.
--
-- security_events.session_id was a FK to active_sessions(id) that no call site
-- ever populated (every SecurityEventInput passes None). It could not have
-- carried a durable trail anyway: sessions are hard-deleted on logout and
-- revoke, so ON DELETE SET NULL would erase the link exactly when an
-- investigation wanted it. A replacement should be a plain uuid column that
-- outlives the session row.
--
-- Neither column is written by any live path, so there is nothing to back up
-- and no audit trigger to disable (active_sessions is sync-audit-only).
ALTER TABLE active_sessions DROP COLUMN is_current;
ALTER TABLE security_events DROP COLUMN session_id;
