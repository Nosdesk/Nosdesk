-- Postgres doesn't support DROP VALUE on an enum type. Reverting
-- this migration would require recreating the enum without the new
-- variant and updating every dependent table — an O(n) rewrite of
-- the sync_actions partitions. Accepted as a lossy down: the variant
-- stays in the enum, and downstream code that no longer recognises
-- it would error on a row read. In practice rolling back past this
-- point requires restoring from a backup snapshot.

-- Intentional no-op.
SELECT 1;
