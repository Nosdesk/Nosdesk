ALTER TABLE documentation_collections
  ADD COLUMN root_page_id INTEGER REFERENCES documentation_pages(id) ON DELETE SET NULL;
