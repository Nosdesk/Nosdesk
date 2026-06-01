-- =====================================================================
-- Workspace role overhaul (multi-tenant Phase 4 W2).
--
-- Splits the one-dimensional `users.role` model into two distinct
-- role surfaces:
--
--   * `users.platform_role`  — platform-wide privileges
--     (`platform_admin` / `user`). Almost everyone is `user`; the
--     bootstrap admin is `platform_admin`. Drives instance-level
--     gating (workspace lifecycle, hosted billing, control-plane
--     federation). NEW COLUMN, added in this migration.
--
--   * `workspace_members.role` — per-workspace privileges
--     (`owner` / `admin` / `agent` / `member`). Drives every
--     workspace-scoped endpoint. ALREADY EXISTS; this migration
--     expands the CHECK to include `agent` and backfills it for
--     existing `users.role = 'technician'` rows.
--
-- The old `users.role` column stays in place during this
-- migration so the application code keeps compiling while a
-- follow-up sweep updates callers. A subsequent migration
-- (2026-06-XX-YYYY_drop_users_role) will drop the column once the
-- sweep lands.
--
-- Mapping (this migration is the source of truth for the backfill):
--
--   users.role = 'admin'      → users.platform_role = 'platform_admin'
--   users.role = 'technician' → users.platform_role = 'user' AND
--                               workspace_members.role bumped to 'agent'
--   users.role = 'user'       → users.platform_role = 'user'
--
-- =====================================================================

-- 1. Add platform_role column. Default 'user' lets existing rows
--    backfill via the UPDATE below; new rows minted by application
--    code post-deploy inherit the same default until callers start
--    setting platform_role explicitly.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS platform_role VARCHAR(32) NOT NULL DEFAULT 'user';

-- 2. Backfill from the existing users.role column.
UPDATE users
    SET platform_role = CASE
        WHEN role = 'admin' THEN 'platform_admin'
        ELSE 'user'
    END
    WHERE platform_role = 'user'; -- only fresh rows; safe re-run

-- 3. Lock the platform_role vocabulary at the DB layer. Two values
--    only; the application enum mirrors this.
ALTER TABLE users
    ADD CONSTRAINT users_platform_role_check
    CHECK (platform_role IN ('platform_admin', 'user'));

-- 4. Expand workspace_members.role to include 'agent'. Existing
--    CHECK was `IN ('owner', 'admin', 'member')`; replace it.
ALTER TABLE workspace_members
    DROP CONSTRAINT IF EXISTS workspace_members_role_check;
ALTER TABLE workspace_members
    ADD CONSTRAINT workspace_members_role_check
    CHECK (role IN ('owner', 'admin', 'agent', 'member'));

-- 5. Bump workspace_members.role from 'member' to 'agent' for every
--    user who was a technician under the old model. Owner / admin
--    workspace memberships stay as they were; only the technician
--    -> agent mapping touches workspace_members.
--
--    Per W2 plan + the locked decision (3a in the role-overhaul
--    sync): preserve ticket-handling permissions for existing
--    technicians by giving them the per-workspace agent role.
UPDATE workspace_members wm
    SET role = 'agent'
    FROM users u
    WHERE wm.user_uuid = u.uuid
      AND u.role = 'technician'
      AND wm.role = 'member'; -- don't downgrade an owner/admin membership
