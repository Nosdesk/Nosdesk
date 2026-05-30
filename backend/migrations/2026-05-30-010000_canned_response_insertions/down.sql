DROP POLICY IF EXISTS canned_response_insertions_workspace_isolation
    ON canned_response_insertions;
DROP INDEX IF EXISTS canned_response_insertions_response_time_idx;
DROP TABLE IF EXISTS canned_response_insertions;
