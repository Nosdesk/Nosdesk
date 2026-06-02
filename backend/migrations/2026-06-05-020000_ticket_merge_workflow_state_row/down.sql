-- Remove the seeded Merged state. Suppress the audit trigger for the
-- same reason the up migration does.
SET LOCAL session_replication_role = 'replica';

DELETE FROM workflow_states WHERE category = 'merged' AND name = 'Merged';

SET LOCAL session_replication_role = 'origin';
