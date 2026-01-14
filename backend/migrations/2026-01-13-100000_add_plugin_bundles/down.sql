-- Remove bundle storage columns from plugins table
ALTER TABLE plugins DROP COLUMN IF EXISTS bundle_hash;
ALTER TABLE plugins DROP COLUMN IF EXISTS bundle_size;
ALTER TABLE plugins DROP COLUMN IF EXISTS bundle_uploaded_at;
