-- Collapse audit reviewers back to plain users before narrowing the
-- CHECK, otherwise re-adding the two-value constraint would fail
-- against any audit_reviewer row.
UPDATE users SET platform_role = 'user' WHERE platform_role = 'audit_reviewer';
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_platform_role_check;
ALTER TABLE users
    ADD CONSTRAINT users_platform_role_check
    CHECK (platform_role IN ('platform_admin', 'user'));
