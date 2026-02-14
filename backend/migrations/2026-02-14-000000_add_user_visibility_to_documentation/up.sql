-- Add user-level visibility to documentation collections and pages.
-- Both tables get: id SERIAL PK, group_id becomes nullable, new user_uuid column.
-- CHECK constraint: exactly one of group_id or user_uuid must be non-null.

-- ============================================================================
-- documentation_collection_visibility
-- ============================================================================

-- Drop the existing composite PK and add an id column
ALTER TABLE documentation_collection_visibility
    DROP CONSTRAINT documentation_collection_visibility_pkey;

ALTER TABLE documentation_collection_visibility
    ADD COLUMN id SERIAL PRIMARY KEY;

-- Make group_id nullable
ALTER TABLE documentation_collection_visibility
    ALTER COLUMN group_id DROP NOT NULL;

-- Add user_uuid column
ALTER TABLE documentation_collection_visibility
    ADD COLUMN user_uuid UUID REFERENCES users(uuid) ON DELETE CASCADE;

-- CHECK: exactly one of group_id or user_uuid must be set
ALTER TABLE documentation_collection_visibility
    ADD CONSTRAINT chk_collection_vis_one_principal
    CHECK (
        (group_id IS NOT NULL AND user_uuid IS NULL)
        OR (group_id IS NULL AND user_uuid IS NOT NULL)
    );

-- Partial unique indexes for dedup
CREATE UNIQUE INDEX idx_collection_vis_group
    ON documentation_collection_visibility (collection_id, group_id)
    WHERE group_id IS NOT NULL;

CREATE UNIQUE INDEX idx_collection_vis_user
    ON documentation_collection_visibility (collection_id, user_uuid)
    WHERE user_uuid IS NOT NULL;

-- ============================================================================
-- documentation_page_visibility
-- ============================================================================

ALTER TABLE documentation_page_visibility
    DROP CONSTRAINT documentation_page_visibility_pkey;

ALTER TABLE documentation_page_visibility
    ADD COLUMN id SERIAL PRIMARY KEY;

ALTER TABLE documentation_page_visibility
    ALTER COLUMN group_id DROP NOT NULL;

ALTER TABLE documentation_page_visibility
    ADD COLUMN user_uuid UUID REFERENCES users(uuid) ON DELETE CASCADE;

ALTER TABLE documentation_page_visibility
    ADD CONSTRAINT chk_page_vis_one_principal
    CHECK (
        (group_id IS NOT NULL AND user_uuid IS NULL)
        OR (group_id IS NULL AND user_uuid IS NOT NULL)
    );

CREATE UNIQUE INDEX idx_page_vis_group
    ON documentation_page_visibility (page_id, group_id)
    WHERE group_id IS NOT NULL;

CREATE UNIQUE INDEX idx_page_vis_user
    ON documentation_page_visibility (page_id, user_uuid)
    WHERE user_uuid IS NOT NULL;
