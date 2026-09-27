-- Organisation visibility (docs/plans/v1.1-requester-experience.md, P2.6): when
-- on, a requester's portal also lists requests from people at the same
-- verified email domain (view only). Off by default; free-mail domains never
-- share.
ALTER TABLE public.site_settings
    ADD COLUMN portal_share_by_domain boolean NOT NULL DEFAULT false;
