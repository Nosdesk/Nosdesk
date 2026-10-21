ALTER TABLE public.workspaces
    DROP COLUMN billing_as_of,
    DROP COLUMN billing_cancel_at,
    DROP COLUMN billing_trial_end,
    DROP COLUMN billing_status;
