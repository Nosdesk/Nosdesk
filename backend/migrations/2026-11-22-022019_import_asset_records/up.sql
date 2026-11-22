-- Asset records import as their own type: past work done on assets, from a
-- spreadsheet of service history.
ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_job_type_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets', 'documentation', 'asset_records'));

ALTER TABLE public.import_value_answers DROP CONSTRAINT import_value_answers_job_type_check;
ALTER TABLE public.import_value_answers ADD CONSTRAINT import_value_answers_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets', 'asset_records'));
