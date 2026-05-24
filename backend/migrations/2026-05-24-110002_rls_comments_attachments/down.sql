DROP POLICY IF EXISTS ticket_watchers_workspace_isolation ON ticket_watchers;
ALTER TABLE ticket_watchers NO FORCE ROW LEVEL SECURITY;
ALTER TABLE ticket_watchers DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS linked_tickets_workspace_isolation ON linked_tickets;
ALTER TABLE linked_tickets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE linked_tickets DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS attachments_workspace_isolation ON attachments;
ALTER TABLE attachments NO FORCE ROW LEVEL SECURITY;
ALTER TABLE attachments DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS comments_workspace_isolation ON comments;
ALTER TABLE comments NO FORCE ROW LEVEL SECURITY;
ALTER TABLE comments DISABLE ROW LEVEL SECURITY;
