ALTER TABLE public.site_settings
    DROP COLUMN IF EXISTS email_logo_light,
    DROP COLUMN IF EXISTS email_logo;
