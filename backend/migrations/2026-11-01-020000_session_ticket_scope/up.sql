-- A portal session opened from a "View request" or approval link reaches
-- only that ticket (and what it was merged into). The grant lives on the
-- session, so a refreshed token keeps it and no token can widen it. NULL is
-- an ordinary session.
ALTER TABLE public.active_sessions ADD COLUMN scope_ticket_id integer;
