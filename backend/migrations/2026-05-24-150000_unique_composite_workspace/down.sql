-- Reverse the composite-UNIQUE conversions: restore the
-- global-UNIQUE constraints under their original names.
-- Note this only succeeds if no cross-workspace duplicate
-- values have been written since the up.sql migration applied
-- (which would now be allowed under the composite shape).

ALTER TABLE ticket_categories DROP CONSTRAINT ticket_categories_workspace_name_unique;
ALTER TABLE ticket_categories ADD CONSTRAINT ticket_categories_name_unique UNIQUE (name);

ALTER TABLE tags DROP CONSTRAINT tags_workspace_name_unique;
ALTER TABLE tags ADD CONSTRAINT tags_name_unique UNIQUE (name);

ALTER TABLE plugins DROP CONSTRAINT plugins_workspace_name_key;
ALTER TABLE plugins ADD CONSTRAINT plugins_name_key UNIQUE (name);

ALTER TABLE documentation_pages DROP CONSTRAINT documentation_pages_workspace_slug_key;
ALTER TABLE documentation_pages ADD CONSTRAINT documentation_pages_slug_key UNIQUE (slug);

ALTER TABLE documentation_collections DROP CONSTRAINT documentation_collections_workspace_slug_key;
ALTER TABLE documentation_collections ADD CONSTRAINT documentation_collections_slug_key UNIQUE (slug);

ALTER TABLE asset_kinds DROP CONSTRAINT asset_kinds_workspace_slug_key;
ALTER TABLE asset_kinds ADD CONSTRAINT asset_kinds_slug_key UNIQUE (slug);
