-- Remove is_delta column from sync_history table
ALTER TABLE sync_history DROP COLUMN is_delta;
