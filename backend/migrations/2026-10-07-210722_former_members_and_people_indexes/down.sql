DROP INDEX IF EXISTS public.comments_workspace_user_idx;
DROP INDEX IF EXISTS public.ticket_watchers_workspace_user_idx;
DROP INDEX IF EXISTS public.tickets_workspace_assignee_idx;
DROP INDEX IF EXISTS public.tickets_workspace_requester_idx;

-- The removed memberships the backfill added stay: a removed row grants
-- nothing, and they can't be told from rows removal wrote since.
