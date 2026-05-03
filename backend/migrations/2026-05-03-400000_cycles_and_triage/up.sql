-- Cycles + triage_state. Architecture doc § 10 phase 6.
--
-- Cycles are time-boxed buckets a ticket can belong to. They live
-- under a project and carry a TSTZRANGE so range-overlap queries
-- (calendar, gantt, "in this iteration") become a single GIST
-- lookup. cycle_tickets is intentionally a separate table rather
-- than a tickets.cycle_id column so a future "ticket spans two
-- cycles" change is a non-destructive ALTER. v1 still gates
-- ticket → cycle uniqueness (one cycle per ticket at a time)
-- through a partial unique index until a use case for the
-- multi-cycle shape surfaces.
--
-- triage_state is a per-ticket flag carried independently of
-- workflow_state. The Triage saved view filters on
-- (cycle = NULL AND triage_state = 'untriaged'). Stored as the
-- existing pattern: VARCHAR + CHECK so adding a new triage state
-- is a single ALTER TABLE rather than a coordinated enum migration.

CREATE TABLE cycles (
    id          SERIAL PRIMARY KEY,
    uuid        UUID NOT NULL DEFAULT uuidv7() UNIQUE,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        VARCHAR(120) NOT NULL,
    -- Inclusive on start, exclusive on end (matches TSTZRANGE
    -- '[start, end)' semantics if we ever migrate to a real
    -- range type for GIST overlap queries). NULL endpoints
    -- are valid for "open-ended" cycles (e.g. a continuous
    -- backlog that converts to a closed cycle once the team
    -- commits to a duration).
    start_at    TIMESTAMPTZ,
    end_at      TIMESTAMPTZ,
    -- 'planned' before start, 'active' inside span, 'completed'
    -- once frozen. completed_at is NULL while planned/active.
    state       VARCHAR(20) NOT NULL DEFAULT 'planned',
    -- Snapshot of cycle stats taken at completion. Frozen here so
    -- post-completion ticket edits don't retroactively rewrite
    -- the cycle's burndown. Shape is { tickets, completed,
    -- carried_over, scope_changes } per the architecture spec.
    completion_snapshot JSONB,
    completed_at        TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at TIMESTAMPTZ,
    created_by  UUID REFERENCES users(uuid) ON DELETE SET NULL,
    CONSTRAINT cycles_state_check CHECK (state IN ('planned', 'active', 'completed')),
    -- A completed cycle must have its snapshot frozen.
    CONSTRAINT cycles_completed_snapshot CHECK (
        (state = 'completed' AND completion_snapshot IS NOT NULL AND completed_at IS NOT NULL)
        OR (state <> 'completed' AND completed_at IS NULL)
    )
);

-- B-tree on (start_at, end_at) covers "what's the active cycle
-- right now" and "find all cycles ending after X" calendar
-- queries. If overlap-style queries become hot we'll add a real
-- TSTZRANGE column with a GIST index alongside.
CREATE INDEX cycles_span_idx ON cycles (start_at, end_at);

-- Per-project active cycle lookup is the most-common query.
CREATE INDEX cycles_project_state_idx
    ON cycles (project_id, state) WHERE archived_at IS NULL;

-- Exactly one active cycle per project at any time. A partial
-- UNIQUE index does this without pulling in btree_gist; EXCLUDE
-- GIST would be necessary only if we wanted the constraint to
-- forbid span overlap, which we don't (planned + active spans
-- routinely brush up against each other at the cycle boundary).
CREATE UNIQUE INDEX cycles_one_active_per_project
    ON cycles (project_id) WHERE state = 'active' AND archived_at IS NULL;

CREATE TABLE cycle_tickets (
    cycle_id   INTEGER NOT NULL REFERENCES cycles(id) ON DELETE CASCADE,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    -- When the ticket was added to this cycle. Drives "scope
    -- changes after start" for burndown.
    added_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    added_by   UUID REFERENCES users(uuid) ON DELETE SET NULL,
    PRIMARY KEY (cycle_id, ticket_id)
);

-- Reverse-direction index: "which cycle is this ticket in?"
CREATE INDEX cycle_tickets_ticket_idx ON cycle_tickets (ticket_id);

-- Enforce one cycle per ticket at a time. Drop this when the
-- multi-cycle shape arrives.
CREATE UNIQUE INDEX cycle_tickets_one_per_ticket ON cycle_tickets (ticket_id);

ALTER TABLE tickets
    ADD COLUMN triage_state VARCHAR(20),
    ADD CONSTRAINT tickets_triage_state_check
        CHECK (triage_state IS NULL OR triage_state IN ('untriaged', 'triaged', 'rejected'));

-- Triage view's hot path: WHERE triage_state = 'untriaged' AND
-- ticket is not in any cycle. The partial index keeps the work
-- on the active set only.
CREATE INDEX tickets_untriaged_idx
    ON tickets (id) WHERE triage_state = 'untriaged';
