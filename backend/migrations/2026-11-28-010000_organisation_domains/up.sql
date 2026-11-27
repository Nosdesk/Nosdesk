-- The workspace's organisation email domains, listed by an admin without
-- setting up requester sign-in. Inbound mail copied from someone at one of
-- these domains to a colleague at the same domain counts as the team's choice,
-- as it does for the sign-in provider's domains. Adding a column with a
-- constant default rewrites no rows, so the site_settings audit trigger
-- doesn't fire.
ALTER TABLE public.site_settings
    ADD COLUMN organisation_domains text[] NOT NULL DEFAULT '{}';
