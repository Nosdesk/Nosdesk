-- A token made for someone else records the roles it was made for. The auth
-- path refuses it once its holder's role is above them, or once its maker is
-- no longer an admin of its workspace. A token made for oneself records none
-- and follows its holder's role.
ALTER TABLE api_tokens
    ADD COLUMN role_ceiling VARCHAR(32),
    ADD COLUMN platform_role_ceiling VARCHAR(32);

-- Existing tokens made for someone else: the holder's current workspace role,
-- capped at the maker's (as minting caps it), and the holder's platform role
-- when the maker holds the same one. A token whose holder still has the role
-- it acts with today keeps working. (No audit trigger on api_tokens.)
UPDATE api_tokens t
SET role_ceiling = (
        SELECT CASE LEAST(
                   CASE holder.role WHEN 'owner' THEN 3 WHEN 'admin' THEN 2 WHEN 'agent' THEN 1 ELSE 0 END,
                   COALESCE(
                       CASE maker.role WHEN 'owner' THEN 3 WHEN 'admin' THEN 2 WHEN 'agent' THEN 1 ELSE 0 END,
                       3
                   )
               )
               WHEN 3 THEN 'owner' WHEN 2 THEN 'admin' WHEN 1 THEN 'agent' ELSE 'member'
               END
        FROM workspace_members holder
        LEFT JOIN workspace_members maker
               ON maker.workspace_id = t.workspace_id
              AND maker.user_uuid = t.created_by
              AND maker.removed_at IS NULL
        WHERE holder.workspace_id = t.workspace_id
          AND holder.user_uuid = t.user_uuid
          AND holder.removed_at IS NULL
    ),
    platform_role_ceiling = (
        SELECT CASE WHEN holder.platform_role = maker.platform_role
                    THEN holder.platform_role ELSE 'user' END
        FROM users holder, users maker
        WHERE holder.uuid = t.user_uuid AND maker.uuid = t.created_by
    )
WHERE t.created_by <> t.user_uuid;
