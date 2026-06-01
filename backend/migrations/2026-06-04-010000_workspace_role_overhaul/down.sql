-- Reverse the W2 role overhaul.
--
-- Reverts workspace_members.role 'agent' back to 'member' (since
-- the old CHECK didn't allow 'agent'), restores the old CHECK
-- vocabulary, then drops the platform_role column. The old
-- `users.role` was never touched, so reverting the platform_role
-- side is sufficient.

UPDATE workspace_members SET role = 'member' WHERE role = 'agent';

ALTER TABLE workspace_members
    DROP CONSTRAINT IF EXISTS workspace_members_role_check;
ALTER TABLE workspace_members
    ADD CONSTRAINT workspace_members_role_check
    CHECK (role IN ('owner', 'admin', 'member'));

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_platform_role_check;
ALTER TABLE users
    DROP COLUMN IF EXISTS platform_role;
