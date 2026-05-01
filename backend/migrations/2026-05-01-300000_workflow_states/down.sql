-- Best-effort rollback. Lossy if customers have added named states inside a
-- category since the migration ran: those states all collapse back to the
-- single matching enum value. Recoverable for hours, not weeks.

CREATE TYPE ticket_status AS ENUM ('open', 'in-progress', 'closed');

ALTER TABLE tickets
    ADD COLUMN status ticket_status;

UPDATE tickets t
   SET status = CASE ws.category
                   WHEN 'triage'    THEN 'open'::ticket_status
                   WHEN 'backlog'   THEN 'open'::ticket_status
                   WHEN 'active'    THEN 'in-progress'::ticket_status
                   WHEN 'in_review' THEN 'in-progress'::ticket_status
                   WHEN 'done'      THEN 'closed'::ticket_status
                   WHEN 'cancelled' THEN 'closed'::ticket_status
                END
  FROM workflow_states ws
 WHERE t.workflow_state_id = ws.id;

ALTER TABLE tickets ALTER COLUMN status SET NOT NULL;
ALTER TABLE tickets ALTER COLUMN status SET DEFAULT 'open';

DROP INDEX IF EXISTS tickets_workflow_state;
ALTER TABLE tickets DROP COLUMN workflow_state_id;

DROP INDEX IF EXISTS workflow_states_category;
DROP INDEX IF EXISTS workflow_states_category_position;
DROP INDEX IF EXISTS workflow_states_default_unique;
DROP TABLE workflow_states;
DROP TYPE workflow_state_category;
