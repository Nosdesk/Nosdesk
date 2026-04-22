-- Canned responses: reusable reply templates that techs can insert
-- into the ticket comment composer with one click.
--
-- Supported template variables: {{ticket_id}}, {{ticket_title}},
-- {{customer_name}}, {{tech_name}}, {{app_name}}. Substitution
-- happens at insert-into-composer time (frontend) for the simple
-- values and at save time for anything needing backend context.
--
-- Not per-user: canned responses are shared across the team.
-- `created_by` is informational — deleting a user doesn't cascade-
-- delete their responses, they stay owned by the workspace.

CREATE TABLE canned_responses (
    id SERIAL PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    body TEXT NOT NULL,
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Title is searched in the composer's picker; keep it indexed for
-- case-insensitive prefix matching when we add search later.
CREATE INDEX canned_responses_title_idx ON canned_responses (lower(title));
