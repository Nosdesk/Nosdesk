-- workspace_members shipped in the squashed initial schema granted only
-- SELECT,INSERT to nosdesk_app, unlike every other tenant table (which
-- gets SELECT,INSERT,DELETE,UPDATE). The W2 role-change path runs
--   UPDATE workspace_members SET role = ... WHERE workspace_id = ... AND user_uuid = ...
-- as the RLS-enforced nosdesk_app role, so it failed with
-- "permission denied for table workspace_members". DELETE is granted
-- alongside UPDATE so member removal/demotion matches the peer pattern
-- and does not hit the same wall next.
GRANT UPDATE, DELETE ON TABLE public.workspace_members TO nosdesk_app;
