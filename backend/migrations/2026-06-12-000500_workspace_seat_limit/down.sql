DROP TRIGGER IF EXISTS tr_enforce_workspace_seat_limit ON workspace_members;
DROP FUNCTION IF EXISTS public.enforce_workspace_seat_limit();
ALTER TABLE workspaces DROP COLUMN seat_limit;
