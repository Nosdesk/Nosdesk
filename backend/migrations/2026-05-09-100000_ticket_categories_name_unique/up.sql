-- Categories are presented to admins as a unique-by-name list (the
-- ticket-creation dropdown shows one entry per name). The seeder
-- introduced in 83fc722 inserts a defaults set on first-run admin
-- setup; without a UNIQUE constraint two simultaneous setups (or a
-- buggy retry) could produce duplicate "Support" rows that confuse
-- the dropdown UI. Lock that out at the schema level and let the
-- seeder use ON CONFLICT DO NOTHING.
--
-- Defensive dedup before the constraint: if a dev DB has already
-- accumulated duplicates from running setup multiple times against
-- the buggy current seeder, drop everything but the lowest-id row
-- per name. Production DBs that ran setup once will see this as a
-- no-op.

DELETE FROM ticket_categories t1
USING ticket_categories t2
WHERE t1.id > t2.id
  AND t1.name = t2.name;

ALTER TABLE ticket_categories
    ADD CONSTRAINT ticket_categories_name_unique UNIQUE (name);
