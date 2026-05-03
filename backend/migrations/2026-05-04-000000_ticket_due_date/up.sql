-- Tickets gain an optional due_date for the calendar view.
-- TIMESTAMPTZ rather than DATE because most "due" semantics in
-- helpdesks are end-of-business in some timezone, and storing
-- the timezone on the column is the cleanest way to round-trip
-- without ambiguity. Calendar renderers display the local-day
-- bucket; reports that care about precise breach windows read
-- the timestamptz directly.
--
-- Partial index on the "due in the future" set so the calendar
-- range query stays cheap as the workspace grows.

ALTER TABLE tickets ADD COLUMN due_date TIMESTAMPTZ;

CREATE INDEX tickets_due_date_idx
    ON tickets (due_date)
    WHERE due_date IS NOT NULL;
