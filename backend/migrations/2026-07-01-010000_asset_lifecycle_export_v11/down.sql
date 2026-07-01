DROP TABLE IF EXISTS asset_disposals;
ALTER TABLE assets DROP COLUMN IF EXISTS managed_by_user_uuid;
