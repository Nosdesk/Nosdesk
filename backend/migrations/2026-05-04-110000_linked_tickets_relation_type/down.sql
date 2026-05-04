-- Reverse the rename + check. Legacy strings are restored so a
-- rollback into pre-rename code keeps reading the same values.

DROP INDEX IF EXISTS idx_linked_tickets_relation_type;

ALTER TABLE linked_tickets DROP CONSTRAINT IF EXISTS linked_tickets_relation_type_check;

ALTER TABLE linked_tickets ALTER COLUMN relation_type DROP DEFAULT;

UPDATE linked_tickets
SET relation_type = CASE relation_type
    WHEN 'related' THEN 'relates_to'
    WHEN 'duplicate_of' THEN 'duplicates'
    ELSE relation_type
END;

ALTER TABLE linked_tickets RENAME COLUMN relation_type TO link_type;
ALTER TABLE linked_tickets ALTER COLUMN link_type SET DEFAULT 'relates_to';

CREATE INDEX idx_linked_tickets_link_type ON linked_tickets(link_type);
