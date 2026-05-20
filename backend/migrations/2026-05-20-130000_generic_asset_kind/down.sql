ALTER TABLE assets ALTER COLUMN kind SET DEFAULT 'device';

DELETE FROM asset_kinds WHERE slug = 'generic';
