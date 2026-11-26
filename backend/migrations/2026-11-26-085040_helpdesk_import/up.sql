-- Helpdesk import: bring tickets, conversations, people and files across from
-- another helpdesk (Zendesk first). Design:
-- docs/plans/onboarding-1.2/02-peppermint-review-and-framework.md §2.
--
-- Three pieces:
--   1. `helpdesk` joins the import job types.
--   2. `import_source_ids` maps every source record to the row it became, so
--      a run can resume, a rerun skips what landed, and comments find their
--      ticket.
--   3. Rows written under an `import:<job>` actor reference stay quiet: the
--      notification, rule and webhook outboxes skip them. The sync pool, SSE
--      feed and search still see them.

-- ---------------------------------------------------------------------------
-- 1. Job type
-- ---------------------------------------------------------------------------

ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_job_type_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets', 'documentation', 'asset_records', 'helpdesk'));

-- ---------------------------------------------------------------------------
-- 2. Id map
-- ---------------------------------------------------------------------------

CREATE TABLE public.import_source_ids (
    workspace_id      integer      NOT NULL
                      DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                      REFERENCES public.workspaces (id) ON DELETE CASCADE,
    -- 'zendesk', 'freshdesk', ...
    source            varchar(32)  NOT NULL,
    -- The source instance, e.g. the Zendesk subdomain, so two Zendesk
    -- accounts imported into one workspace never share ids.
    source_scope      text         NOT NULL,
    -- person, ticket, comment, attachment, category, ...
    entity            varchar(16)  NOT NULL,
    source_id         text         NOT NULL,
    -- The Nosdesk id: an integer id or a uuid, as text.
    target_id         text         NOT NULL,
    -- The job that wrote it. Deliberately no foreign key: the map outlives
    -- the job's lifecycle, and a deleted job must not take the map with it
    -- (a rerun would then duplicate everything).
    job_id            uuid         NOT NULL,
    source_updated_at timestamptz,
    written_at        timestamptz  NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, source, source_scope, entity, source_id)
);

ALTER TABLE public.import_source_ids OWNER TO nosdesk_admin;

-- Undo and "imported from" lookups go from the Nosdesk side.
CREATE INDEX import_source_ids_target_idx
    ON public.import_source_ids (workspace_id, entity, target_id);
CREATE INDEX import_source_ids_job_idx
    ON public.import_source_ids (workspace_id, job_id);

ALTER TABLE public.import_source_ids ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.import_source_ids FORCE ROW LEVEL SECURITY;
CREATE POLICY import_source_ids_workspace_isolation ON public.import_source_ids
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.import_source_ids TO nosdesk_app;

-- No audit trigger: this is bookkeeping, one row per imported record, and a
-- large import writes hundreds of thousands. The records themselves are
-- audited as they are written; the job row records who ran the import.

-- ---------------------------------------------------------------------------
-- 3. Quiet imports
-- ---------------------------------------------------------------------------
-- Each body below is the latest definition, with one early return added:
--   notification_outbox_enqueue  2026-09-28-000005_ticket_approvals
--   rule_event_outbox_enqueue    2026-11-18-022437_rules_plugin_event_outbox
--   webhook_outbox_enqueue       2026-06-25-020000_squash_post_v1011
-- The two that ran without a fixed search_path get one, as a SECURITY
-- DEFINER function should; down.sql restores them as they were.

CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path = public, pg_temp
    AS $$
BEGIN
    -- Imported history notifies nobody.
    IF NEW.actor_ref LIKE 'import:%' THEN
        RETURN NEW;
    END IF;
    IF NEW.event_type = 'comment.created'
       OR NEW.event_type = 'ticket_reference.added'
       OR NEW.event_type IN ('ticket.approval_requested', 'ticket.approval_decided')
       OR (NEW.event_type = 'ticket.created' AND NEW.data->>'requester_uuid' IS NOT NULL)
       OR (NEW.data ? 'previous_assignee_uuid'
           AND NEW.data->>'assignee_uuid' IS DISTINCT FROM NEW.data->>'previous_assignee_uuid')
       OR (NEW.data ? 'previous_workflow_state_id'
           AND NEW.data->>'workflow_state_id' IS DISTINCT FROM NEW.data->>'previous_workflow_state_id')
    THEN
        INSERT INTO notification_outbox (sync_id) VALUES (NEW.sync_id)
        ON CONFLICT (sync_id) DO NOTHING;
        PERFORM pg_notify('notification_outbox_new', '');
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION public.rule_event_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path = public, pg_temp
    AS $$
BEGIN
    -- Imported history fires no rules.
    IF NEW.actor_ref LIKE 'import:%' THEN
        RETURN NEW;
    END IF;
    IF NEW.event_type IN (
           'ticket.created',
           'ticket.updated',
           'ticket.workflow_state_changed',
           'ticket.assignee_changed',
           'ticket.priority_changed',
           'ticket.title_changed',
           'ticket.category_changed',
           'ticket.verification_changed',
           'ticket.resolution_notes_changed',
           'ticket.tags_changed')
       OR (NEW.event_type = 'comment.created' AND NEW.data->>'is_internal' = 'false')
    THEN
        IF EXISTS (
            SELECT 1 FROM public.rules
            WHERE workspace_id = NEW.workspace_id
              AND archived_at IS NULL
              AND state IN ('live', 'dry_run')
              AND trigger_kind <> 'manual'
              AND trigger_kind <> 'plugin_event'
        ) THEN
            INSERT INTO public.rule_event_outbox (sync_id, workspace_id)
            VALUES (NEW.sync_id, NEW.workspace_id)
            ON CONFLICT (sync_id) DO NOTHING;
            PERFORM pg_notify('rule_event_outbox_new', '');
        END IF;
    ELSIF NEW.aggregate = 'plugin'
          AND NEW.actor_kind = 'plugin'
          AND jsonb_typeof(NEW.data->'event'->'ticket_id') = 'number'
    THEN
        IF EXISTS (
            SELECT 1 FROM public.rules
            WHERE workspace_id = NEW.workspace_id
              AND archived_at IS NULL
              AND state IN ('live', 'dry_run')
              AND trigger_kind = 'plugin_event'
              AND trigger_config->>'plugin_uuid' = NEW.aggregate_id
              AND trigger_config->>'event_type' = NEW.event_type
        ) THEN
            INSERT INTO public.rule_event_outbox (sync_id, workspace_id)
            VALUES (NEW.sync_id, NEW.workspace_id)
            ON CONFLICT (sync_id) DO NOTHING;
            PERFORM pg_notify('rule_event_outbox_new', '');
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.rule_event_outbox_enqueue() OWNER TO nosdesk_admin;

CREATE OR REPLACE FUNCTION public.webhook_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path = public, pg_temp
    AS $$
BEGIN
    -- Imported history is not news to a webhook receiver.
    IF NEW.actor_ref LIKE 'import:%' THEN
        RETURN NEW;
    END IF;
    INSERT INTO webhook_outbox (sync_id) VALUES (NEW.sync_id)
    ON CONFLICT (sync_id) DO NOTHING;
    RETURN NEW;
END;
$$;
