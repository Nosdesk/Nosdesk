-- The assignment tables come back empty. The rules the migration made from
-- them stay in `rules`, with their applications.

ALTER TABLE public.ticket_rule_runs DROP COLUMN IF EXISTS stopped;
ALTER TABLE public.tickets DROP COLUMN IF EXISTS rules_released_at;
ALTER TABLE public.tickets DROP COLUMN IF EXISTS rules_held_at;

CREATE TYPE public.assignment_method AS ENUM (
    'direct_user',
    'group_round_robin',
    'group_random',
    'group_queue'
);
ALTER TYPE public.assignment_method OWNER TO nosdesk_admin;

CREATE TABLE public.assignment_rules (
    id serial PRIMARY KEY,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL UNIQUE,
    name character varying(255) NOT NULL,
    description text,
    priority integer DEFAULT 100 NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    method public.assignment_method NOT NULL,
    target_user_uuid uuid REFERENCES public.users(uuid) ON DELETE SET NULL,
    target_group_id integer,
    trigger_on_create boolean DEFAULT true NOT NULL,
    trigger_on_category_change boolean DEFAULT true NOT NULL,
    category_id integer,
    conditions jsonb DEFAULT '{}'::jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid REFERENCES public.users(uuid) ON DELETE SET NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer NOT NULL
        REFERENCES public.workspaces(id)
);
ALTER TABLE public.assignment_rules OWNER TO nosdesk_admin;
CREATE UNIQUE INDEX assignment_rules_workspace_id_id_key ON public.assignment_rules (workspace_id, id);
CREATE INDEX idx_assignment_rules_category ON public.assignment_rules (category_id) WHERE is_active = true;
CREATE INDEX idx_assignment_rules_priority ON public.assignment_rules (priority) WHERE is_active = true;
ALTER TABLE public.assignment_rules
    ADD CONSTRAINT assignment_rules_category_id_fkey FOREIGN KEY (workspace_id, category_id)
        REFERENCES public.ticket_categories (workspace_id, id) ON DELETE SET NULL (category_id),
    ADD CONSTRAINT assignment_rules_target_group_id_fkey FOREIGN KEY (workspace_id, target_group_id)
        REFERENCES public.groups (workspace_id, id) ON DELETE SET NULL (target_group_id);

CREATE TABLE public.assignment_rule_state (
    rule_id integer PRIMARY KEY,
    last_assigned_index integer DEFAULT 0 NOT NULL,
    total_assignments integer DEFAULT 0 NOT NULL,
    last_assigned_at timestamp with time zone,
    last_assigned_user_uuid uuid REFERENCES public.users(uuid) ON DELETE SET NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer NOT NULL
        REFERENCES public.workspaces(id),
    CONSTRAINT assignment_rule_state_rule_id_fkey FOREIGN KEY (workspace_id, rule_id)
        REFERENCES public.assignment_rules (workspace_id, id) ON DELETE CASCADE
);
ALTER TABLE public.assignment_rule_state OWNER TO nosdesk_admin;

CREATE TABLE public.assignment_log (
    id serial PRIMARY KEY,
    ticket_id integer NOT NULL,
    rule_id integer,
    trigger_type character varying(50) NOT NULL,
    previous_assignee_uuid uuid,
    new_assignee_uuid uuid,
    method public.assignment_method NOT NULL,
    context jsonb DEFAULT '{}'::jsonb,
    assigned_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer NOT NULL
        REFERENCES public.workspaces(id),
    CONSTRAINT assignment_log_rule_id_fkey FOREIGN KEY (workspace_id, rule_id)
        REFERENCES public.assignment_rules (workspace_id, id) ON DELETE SET NULL (rule_id),
    CONSTRAINT assignment_log_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id)
        REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE
);
ALTER TABLE public.assignment_log OWNER TO nosdesk_admin;
CREATE INDEX idx_assignment_log_assigned_at ON public.assignment_log (assigned_at);
CREATE INDEX idx_assignment_log_ticket ON public.assignment_log (ticket_id);

ALTER TABLE public.assignment_rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.assignment_rules FORCE ROW LEVEL SECURITY;
ALTER TABLE public.assignment_rule_state ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.assignment_rule_state FORCE ROW LEVEL SECURITY;
ALTER TABLE public.assignment_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.assignment_log FORCE ROW LEVEL SECURITY;
CREATE POLICY assignment_rules_workspace_isolation ON public.assignment_rules
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
CREATE POLICY assignment_rule_state_workspace_isolation ON public.assignment_rule_state
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
CREATE POLICY assignment_log_workspace_isolation ON public.assignment_log
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

CREATE TRIGGER tr_audit_assignment_rule_state AFTER INSERT OR DELETE OR UPDATE ON public.assignment_rule_state
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('rule_id');
CREATE TRIGGER tr_audit_assignment_rules AFTER INSERT OR DELETE OR UPDATE ON public.assignment_rules
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.assignment_rules TO nosdesk_app;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.assignment_rule_state TO nosdesk_app;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.assignment_log TO nosdesk_app;
GRANT ALL ON SEQUENCE public.assignment_rules_id_seq TO nosdesk_app;
GRANT ALL ON SEQUENCE public.assignment_log_id_seq TO nosdesk_app;
