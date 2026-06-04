-- Rewrite stored assignment-rule conditions from the legacy three-bucket
-- `status` key to the workflow-state `categories` set the evaluator now
-- reads: open -> triage+backlog, in-progress -> active+in_review,
-- closed -> done+cancelled+merged. Rows without a `status` key are left
-- untouched. The frontend never exposed a status-condition editor, so in
-- practice this touches at most hand-seeded / API-authored rows; it runs
-- as defence so no rule silently stops matching after the legacy bucket
-- helper is removed.
UPDATE assignment_rules
SET conditions = (conditions - 'status') || jsonb_build_object(
    'categories',
    CASE conditions->>'status'
        WHEN 'open'        THEN '["triage","backlog"]'::jsonb
        WHEN 'in-progress' THEN '["active","in_review"]'::jsonb
        WHEN 'closed'      THEN '["done","cancelled","merged"]'::jsonb
        ELSE '[]'::jsonb
    END
)
WHERE conditions ? 'status';
