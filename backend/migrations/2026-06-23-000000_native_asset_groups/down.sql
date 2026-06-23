-- Reverse native asset groups, then restore the directory-membership junction
-- to its original `asset_groups` name.

DROP TABLE IF EXISTS public.asset_group_assignments;

DROP TABLE IF EXISTS public.asset_groups;
DROP SEQUENCE IF EXISTS public.asset_groups_id_seq;

ALTER INDEX idx_asset_directory_memberships_external RENAME TO idx_asset_groups_external;
ALTER INDEX idx_asset_directory_memberships_group RENAME TO idx_asset_groups_group;
ALTER INDEX idx_asset_directory_memberships_asset RENAME TO idx_asset_groups_asset;
ALTER POLICY asset_directory_memberships_workspace_isolation
    ON public.asset_directory_memberships RENAME TO asset_groups_workspace_isolation;
ALTER TABLE public.asset_directory_memberships RENAME TO asset_groups;
