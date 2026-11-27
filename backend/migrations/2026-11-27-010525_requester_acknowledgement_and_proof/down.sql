ALTER TABLE public.user_emails DROP COLUMN IF EXISTS proven_at;
ALTER TABLE public.site_settings
    DROP COLUMN IF EXISTS requester_ack_template,
    DROP COLUMN IF EXISTS requester_ack_enabled;
