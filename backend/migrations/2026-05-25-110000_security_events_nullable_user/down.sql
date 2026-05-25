-- Reverse C/W2: restore NOT NULL on security_events.user_uuid.
-- Only succeeds if no anonymous-attempt rows (user_uuid IS NULL)
-- exist; delete them first if reverting.
DELETE FROM security_events WHERE user_uuid IS NULL;
ALTER TABLE security_events ALTER COLUMN user_uuid SET NOT NULL;
