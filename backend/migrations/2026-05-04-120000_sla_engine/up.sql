-- SLA engine + working calendars (architecture doc § 6 / roadmap
-- Tier 2). Three tables: working_calendars define business hours,
-- holidays carve exceptions, sla_policies bundle the response /
-- resolution targets that apply to a ticket.
--
-- Per-ticket pill is derived on read (see services/sla.rs); the
-- ticket payload carries the pill on the sync bootstrap so every
-- view shape can render it without a per-row SQL fetch.
--
-- v1 keeps the table shape minimal: one workspace-default
-- calendar, one workspace-default policy, no per-priority
-- specialisation yet. The shape leaves room for policies that
-- match on workflow_state.category, priority, or category_id once
-- the admin UI lands.

CREATE TABLE working_calendars (
    id           SERIAL PRIMARY KEY,
    name         VARCHAR(120) NOT NULL,
    timezone     VARCHAR(64) NOT NULL DEFAULT 'UTC',
    -- Weekly schedule as JSONB:
    --   { "mon": [["09:00","17:00"]], "tue": [["09:00","17:00"]], ... }
    -- Empty array for a day means non-working. Multiple ranges
    -- support split shifts (rare for IT helpdesks, common in
    -- managed-service contracts so it's worth the shape).
    schedule     JSONB NOT NULL,
    is_default   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by   UUID REFERENCES users(uuid) ON DELETE SET NULL
);

-- Exactly one default calendar at a time.
CREATE UNIQUE INDEX working_calendars_one_default
    ON working_calendars (is_default) WHERE is_default = TRUE;

CREATE TABLE working_calendar_holidays (
    id           SERIAL PRIMARY KEY,
    calendar_id  INTEGER NOT NULL REFERENCES working_calendars(id) ON DELETE CASCADE,
    date         DATE NOT NULL,
    label        VARCHAR(120),
    UNIQUE (calendar_id, date)
);

CREATE INDEX working_calendar_holidays_idx
    ON working_calendar_holidays (calendar_id, date);

CREATE TABLE sla_policies (
    id                          SERIAL PRIMARY KEY,
    name                        VARCHAR(120) NOT NULL,
    -- Targets in business minutes (working hours, not wall clock).
    -- NULL means the policy doesn't track that target.
    target_response_minutes     INTEGER,
    target_resolution_minutes   INTEGER,
    working_calendar_id         INTEGER REFERENCES working_calendars(id) ON DELETE SET NULL,
    -- Match criteria; all NULL means "applies to every ticket"
    -- (the workspace-default policy). When more than one policy
    -- could match, the highest-id policy wins (last-write applies)
    -- — explicit ordering can replace this when an admin UI ships.
    priority_filter             VARCHAR(20),
    category_id_filter          INTEGER REFERENCES ticket_categories(id) ON DELETE SET NULL,
    is_default                  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by                  UUID REFERENCES users(uuid) ON DELETE SET NULL
);

CREATE UNIQUE INDEX sla_policies_one_default
    ON sla_policies (is_default) WHERE is_default = TRUE;

CREATE INDEX sla_policies_filters_idx
    ON sla_policies (priority_filter, category_id_filter);

-- Seed a Mon-Fri 9-5 UTC default calendar so the SLA engine has
-- something to compute against on a fresh install.
INSERT INTO working_calendars (name, timezone, schedule, is_default)
VALUES (
    'Default 9-5',
    'UTC',
    '{
        "mon": [["09:00","17:00"]],
        "tue": [["09:00","17:00"]],
        "wed": [["09:00","17:00"]],
        "thu": [["09:00","17:00"]],
        "fri": [["09:00","17:00"]],
        "sat": [],
        "sun": []
    }'::jsonb,
    TRUE
);

-- Seed a default workspace-level SLA policy: 4-hour first response,
-- 24-hour resolution. Both targets are in business minutes against
-- the default calendar above. Admins replace these via the SLA
-- admin surface (lands later) without touching this seed.
INSERT INTO sla_policies (
    name,
    target_response_minutes,
    target_resolution_minutes,
    working_calendar_id,
    is_default
)
SELECT
    'Default',
    4 * 60,
    24 * 60,
    id,
    TRUE
FROM working_calendars
WHERE is_default = TRUE
LIMIT 1;
