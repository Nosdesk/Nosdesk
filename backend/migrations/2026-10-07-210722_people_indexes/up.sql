-- Indexes for the people definition (`repository::directory`): each source is
-- read by (workspace_id, user column), so listing a workspace's people reads
-- only that workspace's index entries and checking one person is one probe
-- per source. workspace_members and user_profiles have this as their primary
-- key already.
--
-- Built inside the migration's transaction, so writes to these tables wait
-- until it commits: seconds at self-hosted sizes.
CREATE INDEX IF NOT EXISTS tickets_workspace_requester_idx
    ON public.tickets (workspace_id, requester_uuid) WHERE requester_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS tickets_workspace_assignee_idx
    ON public.tickets (workspace_id, assignee_uuid) WHERE assignee_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS tickets_workspace_created_by_idx
    ON public.tickets (workspace_id, created_by) WHERE created_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS tickets_workspace_closed_by_idx
    ON public.tickets (workspace_id, closed_by) WHERE closed_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS ticket_watchers_workspace_user_idx
    ON public.ticket_watchers (workspace_id, user_uuid);
CREATE INDEX IF NOT EXISTS comments_workspace_user_idx
    ON public.comments (workspace_id, user_uuid);
CREATE INDEX IF NOT EXISTS documentation_pages_workspace_created_by_idx
    ON public.documentation_pages (workspace_id, created_by);
CREATE INDEX IF NOT EXISTS documentation_pages_workspace_last_edited_by_idx
    ON public.documentation_pages (workspace_id, last_edited_by);
CREATE INDEX IF NOT EXISTS documentation_pages_workspace_verified_by_idx
    ON public.documentation_pages (workspace_id, verified_by) WHERE verified_by IS NOT NULL;
CREATE INDEX IF NOT EXISTS documentation_revisions_workspace_created_by_idx
    ON public.documentation_revisions (workspace_id, created_by);
CREATE INDEX IF NOT EXISTS assets_workspace_primary_user_idx
    ON public.assets (workspace_id, primary_user_uuid) WHERE primary_user_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS assets_workspace_managed_by_idx
    ON public.assets (workspace_id, managed_by_user_uuid) WHERE managed_by_user_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS asset_loans_workspace_borrower_idx
    ON public.asset_loans (workspace_id, borrower_user_uuid);
