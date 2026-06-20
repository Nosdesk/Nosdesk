-- Re-add the columns to tickets, copy the data back, restore the check, drop
-- the satellite.
ALTER TABLE public.tickets
    ADD COLUMN merged_into_ticket_id integer,
    ADD COLUMN merged_at timestamp with time zone,
    ADD COLUMN merged_by_user_uuid uuid,
    ADD COLUMN merge_reason text;

UPDATE public.tickets t
SET merged_into_ticket_id = m.merged_into_ticket_id,
    merged_at = m.merged_at,
    merged_by_user_uuid = m.merged_by_user_uuid,
    merge_reason = m.merge_reason
FROM public.ticket_merges m
WHERE t.id = m.ticket_id;

ALTER TABLE public.tickets
    ADD CONSTRAINT tickets_merge_complete CHECK (
        ((merged_into_ticket_id IS NULL) AND (merged_at IS NULL) AND (merged_by_user_uuid IS NULL))
        OR ((merged_into_ticket_id IS NOT NULL) AND (merged_at IS NOT NULL) AND (merged_by_user_uuid IS NOT NULL))
    );

DROP TABLE public.ticket_merges;
