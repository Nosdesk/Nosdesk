-- Best-effort reverse: collapse a `categories` set back to the legacy
-- `status` bucket by precedence (closed > in-progress > open). A set that
-- spans buckets can't round-trip exactly, and a `categories` condition
-- authored natively (not via the up migration) would also be folded here,
-- so this is lossy by nature. Rows without a `categories` key are left
-- untouched.
UPDATE assignment_rules
SET conditions = (conditions - 'categories') || jsonb_build_object(
    'status',
    CASE
        WHEN conditions->'categories' @> '["done"]'::jsonb
          OR conditions->'categories' @> '["cancelled"]'::jsonb
          OR conditions->'categories' @> '["merged"]'::jsonb THEN 'closed'
        WHEN conditions->'categories' @> '["active"]'::jsonb
          OR conditions->'categories' @> '["in_review"]'::jsonb THEN 'in-progress'
        ELSE 'open'
    END
)
WHERE conditions ? 'categories';
