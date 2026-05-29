-- Per-row recurrence flag for working-calendar holidays.
--   none   : single date (the default; what every existing row meant
--            implicitly before this column existed)
--   annual : the MM-DD repeats every year. The engine expands these
--            into concrete dates at load time, so the SLA arithmetic
--            keeps working with a flat HashSet<NaiveDate>.
--
-- VARCHAR rather than an enum so the set can grow (monthly,
-- nth-weekday) without an enum-rename migration; we validate it on
-- the application side.
ALTER TABLE working_calendar_holidays
    ADD COLUMN recurrence VARCHAR(20) NOT NULL DEFAULT 'none';
