-- Request types (docs/plans/v1.1-requester-experience.md, P2.3): a category an
-- admin marks `requester_visible` is offered to requesters as the type of a new
-- request (portal and guest form). Off by default, so nothing appears until an
-- admin chooses which categories to show and how to describe them.
ALTER TABLE public.ticket_categories
    ADD COLUMN requester_visible boolean NOT NULL DEFAULT false;
