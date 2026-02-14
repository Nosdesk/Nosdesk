ALTER TABLE documentation_collections ADD COLUMN display_order INTEGER NOT NULL DEFAULT 0;

-- Initialize display_order based on current name ordering so existing order is preserved
WITH ordered AS (
    SELECT id, ROW_NUMBER() OVER (ORDER BY name ASC) - 1 AS rn
    FROM documentation_collections
)
UPDATE documentation_collections
SET display_order = ordered.rn
FROM ordered
WHERE documentation_collections.id = ordered.id;
