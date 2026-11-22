-- Asset records jobs and their saved matches can't exist under the old
-- constraints. The records an import wrote stay. The audit triggers need a
-- workspace context a migration doesn't have.
ALTER TABLE public.import_value_answers DISABLE TRIGGER tr_audit_import_value_answers;
DELETE FROM public.import_value_answers WHERE job_type = 'asset_records';
ALTER TABLE public.import_value_answers ENABLE TRIGGER tr_audit_import_value_answers;

ALTER TABLE public.import_jobs DISABLE TRIGGER tr_audit_import_jobs;
DELETE FROM public.import_jobs WHERE job_type = 'asset_records';
ALTER TABLE public.import_jobs ENABLE TRIGGER tr_audit_import_jobs;

ALTER TABLE public.import_value_answers DROP CONSTRAINT import_value_answers_job_type_check;
ALTER TABLE public.import_value_answers ADD CONSTRAINT import_value_answers_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets'));

ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_job_type_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets', 'documentation'));
