-- Irreversible data backfill: the original NULLs can't be told apart
-- from rows legitimately stamped after this migration, so reverting
-- would corrupt the accept state. No-op.
SELECT 1;
