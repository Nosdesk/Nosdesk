-- A draft page is now open to staff only (agents, admins, owners). So that
-- the upgrade hides nothing someone outside staff can read today, every
-- draft such a reader can open is published:
--
-- - one published to guests (is_public);
-- - in a workspace with a requester, one with no rules of its own, in no
--   collection or an unrestricted one;
-- - one whose rules (its own when restricted, else its collection's) name a
--   requester, directly or through a group they are in or one that includes
--   it. Includes are followed one level, as the app resolves a reader's
--   groups (repository::groups::get_group_ids_for_user).
--
-- A requester is a current member with the member role who isn't a platform
-- admin. Drafts open to staff only stay drafts. The table isn't audited; its
-- updated_at trigger is held off so the change doesn't read as an edit.
--
-- Publishing a page resolves the knowledge gaps drafting on it, as
-- repository::knowledge_gaps::on_page_status_changed and resolve_gap do:
-- every ticket a gap's live signals cite is linked to the page as resolving
-- it, and the gap is resolved with the page.
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
                (
                    NOT EXISTS (
                        SELECT 1 FROM public.documentation_collection_pages cp
                        WHERE cp.page_id = p.id
                    )
                    AND EXISTS (SELECT 1 FROM requester r WHERE r.workspace_id = p.workspace_id)
                )
                OR EXISTS (
                    SELECT 1
                    FROM public.documentation_collection_pages cp
                    JOIN public.documentation_collections c ON c.id = cp.collection_id
                    WHERE cp.page_id = p.id
                      AND (
                        (
                            NOT c.restricted
                            AND EXISTS (
                                SELECT 1 FROM requester r WHERE r.workspace_id = p.workspace_id
                            )
                        )
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
),
published AS (
    UPDATE public.documentation_pages p
    SET status = 'published'
    FROM reachable
    WHERE p.id = reachable.id
    RETURNING p.id, p.workspace_id
),
resolving AS (
    SELECT g.id AS gap_id, g.workspace_id, pub.id AS page_id
    FROM public.knowledge_gaps g
    JOIN published pub ON pub.id = g.draft_page_id AND pub.workspace_id = g.workspace_id
    WHERE g.status = 'drafting'
),
-- What each live signal cites: a cluster's payload ticket_ids, or the
-- ticket a ticket-sourced signal names.
cited_ref AS (
    SELECT r.workspace_id, r.page_id, e.value::text AS ref
    FROM resolving r
    JOIN public.knowledge_gap_signals s ON s.gap_id = r.gap_id AND s.dismissed_at IS NULL
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(s.payload -> 'ticket_ids') = 'array'
             THEN s.payload -> 'ticket_ids' ELSE '[]'::jsonb END
    ) AS e(value)
    WHERE s.signal_type = 'ticket_cluster'
      AND jsonb_typeof(e.value) = 'number'
    UNION
    SELECT r.workspace_id, r.page_id, s.source_ref
    FROM resolving r
    JOIN public.knowledge_gap_signals s ON s.gap_id = r.gap_id AND s.dismissed_at IS NULL
    WHERE s.signal_type <> 'ticket_cluster'
      AND s.source_kind = 'ticket'
),
-- Only whole numbers that fit a ticket id, as the app parses them.
cited AS (
    SELECT workspace_id, page_id,
           CASE WHEN ref ~ '^[+-]?[0-9]{1,10}$' THEN ref::bigint END AS ticket_id
    FROM cited_ref
),
-- A cited ticket that no longer exists is skipped, as the app skips a link
-- that fails.
links AS (
    INSERT INTO public.documentation_page_tickets (workspace_id, page_id, ticket_id, link_type)
    SELECT DISTINCT c.workspace_id, c.page_id, t.id, 'resolves'
    FROM cited c
    JOIN public.tickets t ON t.workspace_id = c.workspace_id AND t.id = c.ticket_id
    WHERE c.ticket_id BETWEEN -2147483648 AND 2147483647
    ON CONFLICT (page_id, ticket_id) DO UPDATE SET link_type = 'resolves'
    RETURNING 1
)
UPDATE public.knowledge_gaps g
SET status = 'resolved',
    resolved_page_id = r.page_id,
    resolved_at = now(),
    draft_page_id = NULL,
    updated_at = now()
FROM resolving r
WHERE g.id = r.gap_id;

ALTER TABLE public.documentation_pages ENABLE TRIGGER set_updated_at;
