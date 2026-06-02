DROP INDEX IF EXISTS saved_views_viz_pickable_idx;

ALTER TABLE saved_views
    DROP COLUMN IF EXISTS viz_config,
    DROP COLUMN IF EXISTS viz_type;
