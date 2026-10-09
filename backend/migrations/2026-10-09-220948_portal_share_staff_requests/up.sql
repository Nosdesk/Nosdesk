-- With organisation sharing on (portal_share_by_domain), requests that staff
-- raised under their own name are shared only when this is on. Off by
-- default. Adding a column with a constant default rewrites no rows, so the
-- site_settings audit trigger doesn't fire.
ALTER TABLE public.site_settings
    ADD COLUMN portal_share_staff_requests boolean NOT NULL DEFAULT false;
