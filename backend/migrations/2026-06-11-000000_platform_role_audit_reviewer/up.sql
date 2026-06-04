-- Give the audit-reading capability a durable home on the platform
-- role. Pre-W2 it lived on the legacy `users.role = 'audit_reviewer'`
-- projection (UserRole::AuditReviewer); that column is gone and the
-- enum is being removed, so the capability moves onto
-- `users.platform_role` as a first-class value alongside
-- `platform_admin` and `user`.
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_platform_role_check;
ALTER TABLE users
    ADD CONSTRAINT users_platform_role_check
    CHECK (platform_role IN ('platform_admin', 'audit_reviewer', 'user'));
