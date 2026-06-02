-- Restore `users.role` and reverse-derive its value from
-- `users.platform_role` + `workspace_members.role` for the
-- bootstrap workspace (id=1). Owner/admin workspace memberships
-- map back to 'admin', agents to 'technician', everything else to
-- 'user'; a `platform_admin` platform_role overrides to 'admin'.
ALTER TABLE users
    ADD COLUMN role VARCHAR(32) NOT NULL DEFAULT 'user';

UPDATE users u
    SET role = CASE
        WHEN u.platform_role = 'platform_admin' THEN 'admin'
        WHEN wm.role IN ('owner', 'admin') THEN 'admin'
        WHEN wm.role = 'agent' THEN 'technician'
        ELSE 'user'
    END
    FROM workspace_members wm
    WHERE wm.workspace_id = 1
      AND wm.user_uuid = u.uuid;

ALTER TABLE users
    ADD CONSTRAINT users_role_check
    CHECK (role IN ('admin', 'technician', 'user', 'audit_reviewer'));
