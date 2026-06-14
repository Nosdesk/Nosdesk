DROP POLICY IF EXISTS workspace_members_workspace_isolation ON public.workspace_members;
ALTER TABLE public.workspace_members NO FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_members DISABLE ROW LEVEL SECURITY;
