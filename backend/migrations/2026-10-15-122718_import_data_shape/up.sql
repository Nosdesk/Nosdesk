-- What an import job keeps beyond its rows. `answers` holds the admin's
-- answer per distinct value (`{field: {context: {value: answer}}}`), and
-- `record_ids` the records the commit created and updated
-- (`{created: [...], updated: [...]}`), kept for the life of the workspace
-- after the per-row results are pruned.
ALTER TABLE public.import_jobs
    ADD COLUMN answers JSONB,
    ADD COLUMN record_ids JSONB;

-- Answers carry cell values and the id lists can be long, so the audit log
-- keeps only whether they changed, like the other large columns.
DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE OR UPDATE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'id', 'summary', 'source', 'mapping', 'options', 'row_results', 'answers', 'record_ids'
    );

-- Lets a row of another workspace table reference a job in its own
-- workspace only.
CREATE UNIQUE INDEX import_jobs_workspace_id_id_key ON public.import_jobs (workspace_id, id);

-- Saved matches: the answer an admin gave for a source value, keyed by
-- import type, field, context (the manufacturer answer for models, the kind
-- for enum attributes, else empty) and the value as normalised for matching.
-- The next import of that type fills the answer in.
CREATE TABLE public.import_value_answers (
    id               uuid        PRIMARY KEY DEFAULT uuidv7(),
    workspace_id     integer     NOT NULL
                     DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                     REFERENCES public.workspaces(id) ON DELETE CASCADE,
    job_type         varchar(16) NOT NULL
                     CHECK (job_type IN ('assets', 'users', 'tickets')),
    field            text        NOT NULL,
    context          text        NOT NULL DEFAULT '',
    normalized_value text        NOT NULL,
    answer           jsonb       NOT NULL,
    answered_by      uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    job_id           uuid,
    updated_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT import_value_answers_key
        UNIQUE (workspace_id, job_type, field, context, normalized_value),
    CONSTRAINT import_value_answers_job_id_fkey FOREIGN KEY (workspace_id, job_id)
        REFERENCES public.import_jobs (workspace_id, id) ON DELETE SET NULL (job_id)
);
CREATE INDEX idx_import_value_answers_answered_by
    ON public.import_value_answers (answered_by) WHERE answered_by IS NOT NULL;
CREATE INDEX idx_import_value_answers_job_id
    ON public.import_value_answers (job_id) WHERE job_id IS NOT NULL;
ALTER TABLE public.import_value_answers OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.import_value_answers TO nosdesk_app;
ALTER TABLE public.import_value_answers ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.import_value_answers FORCE ROW LEVEL SECURITY;
CREATE POLICY import_value_answers_workspace_isolation ON public.import_value_answers
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
-- Source values can be people's emails, so the audit log keeps only whether
-- the value and the answer changed.
CREATE TRIGGER tr_audit_import_value_answers AFTER INSERT OR DELETE OR UPDATE ON public.import_value_answers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'normalized_value', 'answer');
