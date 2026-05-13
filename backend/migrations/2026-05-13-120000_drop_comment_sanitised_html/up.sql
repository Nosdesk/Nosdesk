-- Drop the redundant sanitised_html column added in
-- 2026-05-13-110000.
--
-- The column was speculative: it stored the full sanitised body
-- alongside the post-split `new_content` + `quoted_content`
-- columns. After Pass 2 reordered the pipeline to sanitise BEFORE
-- splitting, those two columns concatenated already equal what
-- `sanitised_html` carried. No call site reads it; keeping it
-- doubles HTML storage per comment for no value, and a re-
-- sanitise backfill flow re-reads the raw `body_html`, not this
-- column. Drop and reclaim the space.

ALTER TABLE comments
    DROP COLUMN sanitised_html;
