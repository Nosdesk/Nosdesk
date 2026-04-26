-- Collection-first-class redesign.
--
-- A collection now owns its own description (a Yjs-backed rich
-- document) instead of pointing at a special "main page" via
-- root_page_id. Removes the lifecycle-coupling bug where deleting
-- the root page silently orphaned the collection.
--
-- Membership remains via the documentation_collection_pages
-- junction, but a UNIQUE(page_id) constraint enforces that a page
-- belongs to exactly one collection. Cross-collection visibility
-- is intentionally handled via wikilinks + permissions (not
-- multi-membership).

-- 1. Collection now owns rich description content directly.
ALTER TABLE documentation_collections
    ADD COLUMN description_yjs BYTEA,
    ADD COLUMN description_state_vector BYTEA,
    ADD COLUMN description_text TEXT;

-- 2. Per-collection "hide titles from non-members" toggle for
--    sensitive collections (HR, security incidents). Renders
--    cross-collection wikilinks as "Restricted page" instead of
--    leaking the title to viewers without read access.
ALTER TABLE documentation_collections
    ADD COLUMN hide_titles_from_non_members BOOLEAN NOT NULL DEFAULT FALSE;

-- 3. Backfill description_yjs from each collection's existing
--    root page Yjs document, then soft-delete the page so it
--    disappears from the tree but remains restorable for one
--    release as a safety net.
UPDATE documentation_collections c
SET
    description_yjs = p.yjs_document,
    description_state_vector = p.yjs_state_vector
FROM documentation_pages p
WHERE c.root_page_id IS NOT NULL
  AND c.root_page_id = p.id
  AND c.description_yjs IS NULL;

UPDATE documentation_pages
SET status = 'deleted',
    archived_at = NOW(),
    updated_at = NOW()
WHERE id IN (SELECT root_page_id FROM documentation_collections WHERE root_page_id IS NOT NULL);

DELETE FROM documentation_collection_pages
WHERE page_id IN (SELECT root_page_id FROM documentation_collections WHERE root_page_id IS NOT NULL);

-- 4. Drop the FK and column.
ALTER TABLE documentation_collections
    DROP CONSTRAINT IF EXISTS documentation_collections_root_page_id_fkey;
ALTER TABLE documentation_collections
    DROP COLUMN root_page_id;

-- 5. Re-parent any page whose parent_id points outside its
--    own collection set. After this no parent_id crosses a
--    collection boundary. (The same-collection invariant is
--    enforced in handlers from here on; this is the one-shot
--    cleanup for legacy data.)
UPDATE documentation_pages p
SET parent_id = NULL
WHERE p.parent_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1 FROM documentation_collection_pages cp1
      JOIN documentation_collection_pages cp2
        ON cp1.collection_id = cp2.collection_id
      WHERE cp1.page_id = p.id
        AND cp2.page_id = p.parent_id
  );

-- 6. Collapse multi-collection memberships down to one. Pre-launch
--    so there shouldn't be any, but this guarantees the upcoming
--    UNIQUE(page_id) doesn't error on legacy data. Keep the oldest
--    membership row (lowest created_at) as the surviving one.
DELETE FROM documentation_collection_pages cp
USING (
    SELECT page_id, MIN(created_at) AS keep_created_at
    FROM documentation_collection_pages
    GROUP BY page_id
) keepers
WHERE cp.page_id = keepers.page_id
  AND cp.created_at <> keepers.keep_created_at;

-- 7. Enforce single-collection membership.
ALTER TABLE documentation_collection_pages
    ADD CONSTRAINT documentation_collection_pages_page_id_key UNIQUE (page_id);

-- 8. Indexes for the new query patterns.
--    parent_id: tree walks within a collection.
--    (collection_id, page_id): junction lookups (already covered
--    by the table's PK on (collection_id, page_id), so no add).
CREATE INDEX IF NOT EXISTS idx_documentation_pages_parent_id
    ON documentation_pages(parent_id) WHERE parent_id IS NOT NULL;
