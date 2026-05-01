-- Replaces the rigid ticket_status enum with a configurable workflow_states
-- table. The system reasons in fixed categories; workspace-level state names
-- inside each category are user-configurable.

CREATE TYPE workflow_state_category AS ENUM (
    'triage',
    'backlog',
    'active',
    'in_review',
    'done',
    'cancelled'
);

CREATE TABLE workflow_states (
    id           SERIAL PRIMARY KEY,
    name         VARCHAR(64) NOT NULL,
    category     workflow_state_category NOT NULL,
    color        VARCHAR(20) NOT NULL,
    position     INT NOT NULL,
    is_default   BOOLEAN NOT NULL DEFAULT FALSE,
    archived_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by   UUID REFERENCES users(uuid) ON DELETE SET NULL
);

CREATE UNIQUE INDEX workflow_states_default_unique
    ON workflow_states (is_default) WHERE is_default = TRUE;

CREATE UNIQUE INDEX workflow_states_category_position
    ON workflow_states (category, position) WHERE archived_at IS NULL;

CREATE INDEX workflow_states_category
    ON workflow_states (category) WHERE archived_at IS NULL;

-- Seed the default Linear-style six-state workflow.
INSERT INTO workflow_states (name, category, color, position, is_default) VALUES
    ('Triage',      'triage',    'slate',  0, FALSE),
    ('Backlog',     'backlog',   'gray',   0, TRUE),
    ('In Progress', 'active',    'blue',   0, FALSE),
    ('In Review',   'in_review', 'purple', 0, FALSE),
    ('Done',        'done',      'green',  0, FALSE),
    ('Cancelled',   'cancelled', 'subtle', 0, FALSE);

-- Tickets gain a nullable workflow_state_id, then we backfill from the old
-- enum, then we make it NOT NULL and drop the old column. All in one
-- migration so the schema is never in a half-migrated state at rest.
ALTER TABLE tickets
    ADD COLUMN workflow_state_id INT REFERENCES workflow_states(id);

UPDATE tickets
   SET workflow_state_id = (SELECT id FROM workflow_states WHERE name = 'Backlog')
 WHERE status = 'open';

UPDATE tickets
   SET workflow_state_id = (SELECT id FROM workflow_states WHERE name = 'In Progress')
 WHERE status = 'in-progress';

UPDATE tickets
   SET workflow_state_id = (SELECT id FROM workflow_states WHERE name = 'Done')
 WHERE status = 'closed';

ALTER TABLE tickets ALTER COLUMN workflow_state_id SET NOT NULL;
CREATE INDEX tickets_workflow_state ON tickets (workflow_state_id);

ALTER TABLE tickets DROP COLUMN status;
DROP TYPE ticket_status;
