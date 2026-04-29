-- Reverse of up.sql. The ticket_id column is restored as a single
-- pointer; we backfill it from the most recent 'resolves' row per
-- page (a best-effort restore, since the column couldn't represent
-- multiple links anyway).

ALTER TABLE documentation_pages
    DROP COLUMN verified_by,
    DROP COLUMN verified_at,
    DROP COLUMN verify_interval_days;

ALTER TABLE documentation_pages
    ADD COLUMN ticket_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL;

UPDATE documentation_pages p
SET ticket_id = sub.ticket_id
FROM (
    SELECT DISTINCT ON (page_id) page_id, ticket_id
    FROM documentation_page_tickets
    WHERE link_type = 'resolves'
    ORDER BY page_id, created_at DESC
) sub
WHERE p.id = sub.page_id;

DROP TABLE documentation_page_tickets;
