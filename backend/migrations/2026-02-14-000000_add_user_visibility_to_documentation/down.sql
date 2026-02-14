-- Revert user-level visibility changes

-- ============================================================================
-- documentation_page_visibility
-- ============================================================================

DROP INDEX IF EXISTS idx_page_vis_user;
DROP INDEX IF EXISTS idx_page_vis_group;

ALTER TABLE documentation_page_visibility
    DROP CONSTRAINT IF EXISTS chk_page_vis_one_principal;

ALTER TABLE documentation_page_visibility
    DROP COLUMN IF EXISTS user_uuid;

-- Remove rows that have NULL group_id (shouldn't exist after revert, but safety)
DELETE FROM documentation_page_visibility WHERE group_id IS NULL;

ALTER TABLE documentation_page_visibility
    ALTER COLUMN group_id SET NOT NULL;

ALTER TABLE documentation_page_visibility
    DROP CONSTRAINT documentation_page_visibility_pkey;

ALTER TABLE documentation_page_visibility
    DROP COLUMN id;

ALTER TABLE documentation_page_visibility
    ADD PRIMARY KEY (page_id, group_id);

-- ============================================================================
-- documentation_collection_visibility
-- ============================================================================

DROP INDEX IF EXISTS idx_collection_vis_user;
DROP INDEX IF EXISTS idx_collection_vis_group;

ALTER TABLE documentation_collection_visibility
    DROP CONSTRAINT IF EXISTS chk_collection_vis_one_principal;

ALTER TABLE documentation_collection_visibility
    DROP COLUMN IF EXISTS user_uuid;

DELETE FROM documentation_collection_visibility WHERE group_id IS NULL;

ALTER TABLE documentation_collection_visibility
    ALTER COLUMN group_id SET NOT NULL;

ALTER TABLE documentation_collection_visibility
    DROP CONSTRAINT documentation_collection_visibility_pkey;

ALTER TABLE documentation_collection_visibility
    DROP COLUMN id;

ALTER TABLE documentation_collection_visibility
    ADD PRIMARY KEY (collection_id, group_id);
