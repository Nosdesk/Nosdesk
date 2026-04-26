-- Restore root_page_id and the auto-create flow. Pre-launch
-- rollback only — running this on a system that has been live
-- under the new model loses any pages added at "true root" since
-- the up migration ran.

ALTER TABLE documentation_collection_pages
    DROP CONSTRAINT IF EXISTS documentation_collection_pages_page_id_key;

DROP INDEX IF EXISTS idx_documentation_pages_parent_id;

ALTER TABLE documentation_collections
    ADD COLUMN root_page_id INTEGER REFERENCES documentation_pages(id) ON DELETE SET NULL;

ALTER TABLE documentation_collections
    DROP COLUMN IF EXISTS hide_titles_from_non_members,
    DROP COLUMN IF EXISTS description_text,
    DROP COLUMN IF EXISTS description_state_vector,
    DROP COLUMN IF EXISTS description_yjs;
