-- Add bundle storage columns to plugins table
ALTER TABLE plugins ADD COLUMN bundle_hash VARCHAR(64);
ALTER TABLE plugins ADD COLUMN bundle_size INTEGER;
ALTER TABLE plugins ADD COLUMN bundle_uploaded_at TIMESTAMPTZ;
