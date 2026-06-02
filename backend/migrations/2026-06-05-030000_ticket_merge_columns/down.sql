ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_merge_complete;

DROP INDEX IF EXISTS tickets_merged_into_idx;

ALTER TABLE tickets
    DROP COLUMN IF EXISTS merged_into_ticket_id,
    DROP COLUMN IF EXISTS merged_at,
    DROP COLUMN IF EXISTS merged_by_user_uuid,
    DROP COLUMN IF EXISTS merge_reason;
