-- Add source column to track where plugins come from
-- 'provisioned' = loaded from /app/plugins/ directory on startup
-- 'uploaded' = installed via UI zip upload
ALTER TABLE plugins ADD COLUMN source VARCHAR(20) NOT NULL DEFAULT 'uploaded';

-- Add index for filtering by source
CREATE INDEX idx_plugins_source ON plugins(source);
