-- No-op: the cleared labels were derived from user_agent, which is still
-- stored, so nothing is lost and nothing needs restoring. Re-running the old
-- parser is not desirable at any point.
SELECT 1;
