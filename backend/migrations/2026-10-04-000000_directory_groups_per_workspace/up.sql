-- A directory group's id is unique within its workspace, not across the
-- server, so two workspaces can sync the same directory.
DROP INDEX public.idx_groups_external_id;

CREATE UNIQUE INDEX idx_groups_external_id
    ON public.groups USING btree (workspace_id, external_id)
    WHERE (external_id IS NOT NULL);
