-- Add is_delta column to sync_history table
-- Defaults to true since delta sync is the default/recommended mode
ALTER TABLE sync_history ADD COLUMN is_delta BOOLEAN NOT NULL DEFAULT true;
