-- A membership row outlives the membership (`removed_at`, 2026-09-09), so a
-- person who has left stays one of the workspace's people and keeps rendering
-- by name on what they left behind. Members removed before then lost the row
-- outright. Give every user that a workspace's own rows still name (any user
-- foreign key on a table with a workspace_id, the set the workspace export
-- collects) and who has no membership row there a removed one, so they are
-- one of its people again. Role `member` grants nothing, and the seat trigger
-- ignores both member and removed rows.
--
-- workspace_members is audited (tr_audit_workspace_members), and that trigger
-- needs app.workspace_id, which a migration session doesn't set (NDX01), so it
-- is off for the backfill, as in other data backfills.
ALTER TABLE public.workspace_members DISABLE TRIGGER tr_audit_workspace_members;

DO $$
DECLARE
    ref RECORD;
BEGIN
    FOR ref IN
        SELECT con.conrelid::regclass AS child_table,
               child_col.attname AS child_column
        FROM pg_constraint con
        JOIN pg_attribute child_col
          ON child_col.attrelid = con.conrelid AND child_col.attnum = con.conkey[1]
        JOIN pg_attribute parent_col
          ON parent_col.attrelid = con.confrelid AND parent_col.attnum = con.confkey[1]
        WHERE con.contype = 'f'
          AND con.conparentid = 0
          AND con.connamespace = 'public'::regnamespace
          AND con.confrelid = 'public.users'::regclass
          AND con.conrelid <> 'public.workspace_members'::regclass
          AND parent_col.attname = 'uuid'
          AND array_length(con.conkey, 1) = 1
          AND EXISTS (
              SELECT 1 FROM pg_attribute wa
              WHERE wa.attrelid = con.conrelid
                AND wa.attname = 'workspace_id'
                AND NOT wa.attisdropped
          )
    LOOP
        EXECUTE format(
            'INSERT INTO public.workspace_members (workspace_id, user_uuid, role, removed_at) '
            'SELECT DISTINCT r.workspace_id, r.%2$I, ''member'', now() '
            'FROM %1$s r '
            'JOIN public.workspaces w ON w.id = r.workspace_id '
            'WHERE r.%2$I IS NOT NULL '
            'ON CONFLICT (workspace_id, user_uuid) DO NOTHING',
            ref.child_table, ref.child_column
        );
    END LOOP;
END
$$;

ALTER TABLE public.workspace_members ENABLE TRIGGER tr_audit_workspace_members;

-- The people definition (repository::directory) reads each source by
-- (workspace_id, user column): a list scans only the workspace's own index
-- entries, and a lookup is one probe. workspace_members and user_profiles have
-- that as their primary key already.
CREATE INDEX IF NOT EXISTS tickets_workspace_requester_idx
    ON public.tickets (workspace_id, requester_uuid) WHERE requester_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS tickets_workspace_assignee_idx
    ON public.tickets (workspace_id, assignee_uuid) WHERE assignee_uuid IS NOT NULL;
CREATE INDEX IF NOT EXISTS ticket_watchers_workspace_user_idx
    ON public.ticket_watchers (workspace_id, user_uuid);
CREATE INDEX IF NOT EXISTS comments_workspace_user_idx
    ON public.comments (workspace_id, user_uuid);
