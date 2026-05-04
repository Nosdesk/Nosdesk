-- Phase 8 spec calls for a `relation_type` enum on linked_tickets
-- with the canonical four values blocks / blocked_by / related /
-- duplicate_of. The original column was `link_type VARCHAR(50)`
-- with legacy strings (`relates_to`, `duplicates`) — this rename
-- maps those to the new canonical values, then locks the set with
-- a CHECK constraint so future inserts can't drift.
--
-- Stored as VARCHAR + CHECK rather than a Postgres ENUM type so a
-- future fifth relation kind is a single ALTER (drop + recreate
-- the constraint) rather than the two-table-rewrite ENUM dance.

-- 1. Map legacy values onto the canonical set. Any unrecognised
-- value becomes 'related' so the subsequent CHECK doesn't reject
-- pre-existing data.
UPDATE linked_tickets
SET link_type = CASE link_type
    WHEN 'blocks' THEN 'blocks'
    WHEN 'blocked_by' THEN 'blocked_by'
    WHEN 'relates_to' THEN 'related'
    WHEN 'related' THEN 'related'
    WHEN 'duplicates' THEN 'duplicate_of'
    WHEN 'duplicate_of' THEN 'duplicate_of'
    ELSE 'related'
END;

-- 2. Rename the column.
ALTER TABLE linked_tickets RENAME COLUMN link_type TO relation_type;

-- 3. New default matches the canonical "related" rather than the
-- old `relates_to`.
ALTER TABLE linked_tickets ALTER COLUMN relation_type SET DEFAULT 'related';

-- 4. Lock the set.
ALTER TABLE linked_tickets ADD CONSTRAINT linked_tickets_relation_type_check
    CHECK (relation_type IN ('blocks', 'blocked_by', 'related', 'duplicate_of'));

-- 5. Index follows the rename.
DROP INDEX IF EXISTS idx_linked_tickets_link_type;
CREATE INDEX idx_linked_tickets_relation_type ON linked_tickets(relation_type);
