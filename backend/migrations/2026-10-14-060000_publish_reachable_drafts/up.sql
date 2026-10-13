-- A draft page is now open to staff only (agents, admins, owners). So that
-- the upgrade hides nothing someone outside staff can read today, every
-- draft such a reader can open is published:
--
-- - one published to guests (is_public);
-- - one with no rules of its own, in no collection or an unrestricted one;
-- - one whose rules (its own when restricted, else its collection's) name a
--   requester, directly or through a group they are in or one that includes
--   it.
--
-- A requester is a current member with the member role who isn't a platform
-- admin. Drafts open to staff only stay drafts. The table isn't audited; its
-- updated_at trigger is held off so the change doesn't read as an edit.
ALTER TABLE public.documentation_pages DISABLE TRIGGER set_updated_at;

WITH requester AS (
    SELECT m.workspace_id, m.user_uuid
    FROM public.workspace_members m
    JOIN public.users u ON u.uuid = m.user_uuid
    WHERE m.role = 'member'
      AND m.removed_at IS NULL
      AND u.platform_role <> 'platform_admin'
),
requester_group AS (
    SELECT ug.group_id
    FROM public.user_groups ug
    JOIN requester r ON r.user_uuid = ug.user_uuid AND r.workspace_id = ug.workspace_id
    UNION
    SELECT gi.parent_group_id
    FROM public.group_includes gi
    JOIN public.user_groups ug ON ug.group_id = gi.child_group_id
    JOIN requester r ON r.user_uuid = ug.user_uuid AND r.workspace_id = ug.workspace_id
),
reachable AS (
    SELECT p.id
    FROM public.documentation_pages p
    WHERE p.status = 'draft'
      AND (
        p.is_public
        OR (
            p.restricted
            AND EXISTS (
                SELECT 1 FROM public.documentation_page_visibility v
                WHERE v.page_id = p.id
                  AND (
                    v.group_id IN (SELECT group_id FROM requester_group)
                    OR EXISTS (
                        SELECT 1 FROM requester r
                        WHERE r.user_uuid = v.user_uuid AND r.workspace_id = p.workspace_id
                    )
                  )
            )
        )
        OR (
            NOT p.restricted
            AND (
                NOT EXISTS (
                    SELECT 1 FROM public.documentation_collection_pages cp
                    WHERE cp.page_id = p.id
                )
                OR EXISTS (
                    SELECT 1
                    FROM public.documentation_collection_pages cp
                    JOIN public.documentation_collections c ON c.id = cp.collection_id
                    WHERE cp.page_id = p.id
                      AND (
                        NOT c.restricted
                        OR EXISTS (
                            SELECT 1 FROM public.documentation_collection_visibility v
                            WHERE v.collection_id = c.id
                              AND (
                                v.group_id IN (SELECT group_id FROM requester_group)
                                OR EXISTS (
                                    SELECT 1 FROM requester r
                                    WHERE r.user_uuid = v.user_uuid
                                      AND r.workspace_id = p.workspace_id
                                )
                              )
                        )
                      )
                )
            )
        )
      )
)
UPDATE public.documentation_pages p
SET status = 'published'
FROM reachable
WHERE p.id = reachable.id;

ALTER TABLE public.documentation_pages ENABLE TRIGGER set_updated_at;
