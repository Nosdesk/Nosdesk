-- Email-ready PNG copies of the workspace's logos (utils::email_logo): the
-- URL, the display size, and whether the logo reads on the letter's light
-- and dark paper. NULL until made: at upload, or by the startup pass for a
-- logo uploaded before this.
ALTER TABLE public.site_settings
    ADD COLUMN email_logo jsonb,
    ADD COLUMN email_logo_light jsonb;
