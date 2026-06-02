-- =====================================================================
-- Drop the legacy `users.role` column (multi-tenant Phase 4 W2 cleanup).
--
-- The W2 overhaul (2026-06-04-010000_workspace_role_overhaul) split
-- the one-dimensional role model into `users.platform_role` and
-- `workspace_members.role`, and the W2 sweep moved every gate /
-- handler / extractor onto those two sources of truth. This
-- migration retires the original column.
--
-- Drop is unconditional because every reader has already moved.
-- The User and NewUser structs lose their `role` field in the same
-- commit so a stale caller can't quietly reintroduce the dependency.
-- =====================================================================

ALTER TABLE users
    DROP COLUMN IF EXISTS role;
