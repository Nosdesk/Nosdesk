DROP POLICY IF EXISTS ticket_tags_workspace_isolation ON ticket_tags;
ALTER TABLE ticket_tags NO FORCE ROW LEVEL SECURITY;
ALTER TABLE ticket_tags DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tags_workspace_isolation ON tags;
ALTER TABLE tags NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tags DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS category_group_visibility_workspace_isolation ON category_group_visibility;
ALTER TABLE category_group_visibility NO FORCE ROW LEVEL SECURITY;
ALTER TABLE category_group_visibility DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS ticket_categories_workspace_isolation ON ticket_categories;
ALTER TABLE ticket_categories NO FORCE ROW LEVEL SECURITY;
ALTER TABLE ticket_categories DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS workflow_states_workspace_isolation ON workflow_states;
ALTER TABLE workflow_states NO FORCE ROW LEVEL SECURITY;
ALTER TABLE workflow_states DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS project_tickets_workspace_isolation ON project_tickets;
ALTER TABLE project_tickets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE project_tickets DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS projects_workspace_isolation ON projects;
ALTER TABLE projects NO FORCE ROW LEVEL SECURITY;
ALTER TABLE projects DISABLE ROW LEVEL SECURITY;
