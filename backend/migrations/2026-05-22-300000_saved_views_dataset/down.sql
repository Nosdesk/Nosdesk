DROP INDEX IF EXISTS idx_saved_views_user_dataset;
ALTER TABLE saved_views DROP COLUMN IF EXISTS dataset;
