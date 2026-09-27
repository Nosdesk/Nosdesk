-- Email timing (docs/plans/requester-phase3.md, item 3): a reply or status
-- email to someone who has the portal open live is held a few minutes and
-- dropped if they look at the request in the meantime.
ALTER TABLE public.outbound_emails ADD COLUMN skip_if_seen_by uuid;
