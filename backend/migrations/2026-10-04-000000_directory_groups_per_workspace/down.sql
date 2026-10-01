-- Fails if two workspaces hold a group with the same directory id.
DROP INDEX public.idx_groups_external_id;

CREATE UNIQUE INDEX idx_groups_external_id
    ON public.groups USING btree (external_id)
    WHERE (external_id IS NOT NULL);
