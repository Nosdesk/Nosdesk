DROP POLICY IF EXISTS article_content_revisions_workspace_isolation ON article_content_revisions;
ALTER TABLE article_content_revisions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE article_content_revisions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS article_contents_workspace_isolation ON article_contents;
ALTER TABLE article_contents NO FORCE ROW LEVEL SECURITY;
ALTER TABLE article_contents DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_subscriptions_workspace_isolation ON documentation_subscriptions;
ALTER TABLE documentation_subscriptions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_subscriptions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_starred_pages_workspace_isolation ON documentation_starred_pages;
ALTER TABLE documentation_starred_pages NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_starred_pages DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_revisions_workspace_isolation ON documentation_revisions;
ALTER TABLE documentation_revisions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_revisions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_page_visibility_workspace_isolation ON documentation_page_visibility;
ALTER TABLE documentation_page_visibility NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_page_visibility DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_page_tickets_workspace_isolation ON documentation_page_tickets;
ALTER TABLE documentation_page_tickets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_page_tickets DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_page_embeddings_workspace_isolation ON documentation_page_embeddings;
ALTER TABLE documentation_page_embeddings NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_page_embeddings DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_collection_visibility_workspace_isolation ON documentation_collection_visibility;
ALTER TABLE documentation_collection_visibility NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_collection_visibility DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_collection_pages_workspace_isolation ON documentation_collection_pages;
ALTER TABLE documentation_collection_pages NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_collection_pages DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_collections_workspace_isolation ON documentation_collections;
ALTER TABLE documentation_collections NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_collections DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS documentation_pages_workspace_isolation ON documentation_pages;
ALTER TABLE documentation_pages NO FORCE ROW LEVEL SECURITY;
ALTER TABLE documentation_pages DISABLE ROW LEVEL SECURITY;
