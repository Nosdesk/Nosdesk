-- Remove category_id from tickets
ALTER TABLE tickets DROP COLUMN IF EXISTS category_id;

-- Drop junction tables first (they reference the main tables)
DROP TABLE IF EXISTS category_group_visibility;
DROP TABLE IF EXISTS user_groups;

-- Drop main tables
DROP TABLE IF EXISTS ticket_categories;
DROP TABLE IF EXISTS groups;
