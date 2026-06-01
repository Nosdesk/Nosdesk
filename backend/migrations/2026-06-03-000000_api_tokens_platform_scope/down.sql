DROP INDEX IF EXISTS api_tokens_platform_scoped_idx;
ALTER TABLE api_tokens DROP COLUMN is_platform_scoped;
